use std::{cell::Cell, cell::RefCell, rc::Rc};

use gettextrs::gettext;
use gio::prelude::*;
use gst::prelude::*;
use gstreamer as gst;
use gtk::prelude::*;
use libhandy as hdy;

use crate::config::{LOG_DOMAIN, PROFILE};

pub struct Window {
    pub window: hdy::ApplicationWindow,
    stack_media: gtk::Stack,
    pipeline: gst::Pipeline,
    pipeline_playing: Cell<bool>,
    label_current_time: gtk::Label,
    adjustment_position: gtk::Adjustment,
    adjustment_position_value_changed: glib::SignalHandlerId,
    stack_title: gtk::Stack,
    button_play_pause_image: gtk::Image,
    revealer_controls: gtk::Revealer,
}

impl Window {
    pub fn new() -> Rc<Self> {
        let builder = gtk::Builder::from_resource("/org/gnome/gitlab/YaLTeR/Identity/window.ui");
        let window: hdy::ApplicationWindow = builder.get_object("window").unwrap();
        let stack_media: gtk::Stack = builder.get_object("stack_media").unwrap();
        let stack_switcher_media: gtk::StackSwitcher =
            builder.get_object("stack_switcher_media").unwrap();
        let button_open: gtk::Button = builder.get_object("button_open").unwrap();
        let button_play_pause: gtk::Button = builder.get_object("button_play_pause").unwrap();
        let button_play_pause_image: gtk::Image =
            builder.get_object("button_play_pause_image").unwrap();
        let label_current_time: gtk::Label = builder.get_object("label_current_time").unwrap();
        let adjustment_position: gtk::Adjustment =
            builder.get_object("adjustment_position").unwrap();
        let stack_title: gtk::Stack = builder.get_object("stack_title").unwrap();
        let revealer_controls: gtk::Revealer = builder.get_object("revealer_controls").unwrap();

        // Devel Profile
        if PROFILE == "Devel" {
            window.get_style_context().add_class("devel");
        }

        let pipeline = gst::Pipeline::new(None);

        let adjustment_position_value_changed = adjustment_position.connect_value_changed({
            let pipeline = pipeline.downgrade();
            move |adjustment_position| {
                let pipeline = pipeline.upgrade().unwrap();
                if let Some(duration) = pipeline.query_duration::<gst::ClockTime>() {
                    let value = adjustment_position.get_value();

                    g_debug!(LOG_DOMAIN, "seeking, value: {}", value);

                    let time = gst::ClockTime::from_nseconds(
                        (value * duration.nseconds().unwrap() as f64) as u64,
                    );
                    pipeline.seek_simple(gst::SeekFlags::FLUSH, time).unwrap();
                }
            }
        });

        let self_ = Rc::new(Window {
            window,
            stack_media,
            pipeline: pipeline.clone(),
            pipeline_playing: Cell::new(false),
            label_current_time,
            adjustment_position,
            adjustment_position_value_changed,
            stack_title,
            button_play_pause_image,
            revealer_controls,
        });

        button_play_pause.connect_clicked({
            let self_ = Rc::downgrade(&self_);
            move |_| {
                let self_ = self_.upgrade().unwrap();
                self_.play_pause();
            }
        });

        // Handle GStreamer messages.
        let bus = pipeline.get_bus().unwrap();
        bus.add_watch_local({
            let self_ = Rc::downgrade(&self_);
            move |_, msg| {
                let self_ = self_.upgrade().unwrap();
                self_.on_bus_message(msg)
            }
        })
        .unwrap();

        pipeline.set_state(gst::State::Paused).unwrap();

        self_.window.connect_delete_event({
            move |_, _| {
                pipeline.set_state(gst::State::Null).unwrap();

                // This returns Err if called multiple times.
                let _ = bus.remove_watch();

                Inhibit(false)
            }
        });

        button_open.connect_clicked({
            let self_ = Rc::downgrade(&self_);
            move |_| {
                let self_ = self_.upgrade().unwrap();
                self_.show_open_dialog();
            }
        });

        glib::timeout_add_local(100, {
            let self_ = Rc::downgrade(&self_);
            move || {
                if let Some(self_) = self_.upgrade() {
                    self_.refresh_ui();
                    glib::Continue(true)
                } else {
                    glib::Continue(false)
                }
            }
        });

        // Add ellipsizing to the stack switcher button labels so long filenames don't cause big
        // window width requirements.
        stack_switcher_media.connect_add(|_, radio_button| {
            // These two downcasts don't fail for me, but this is a GTK implementation detail, so
            // let's err on the safe side.
            if let Some(radio_button) = radio_button.downcast_ref::<gtk::RadioButton>() {
                radio_button.connect_add(|_, label| {
                    if let Some(label) = label.downcast_ref::<gtk::Label>() {
                        label.set_ellipsize(pango::EllipsizeMode::Middle);
                    }
                });
            }
        });

        self_
    }

    pub fn set_visible_child(&self, child: u8) {
        self.stack_media.set_visible_child_name(&child.to_string());
    }

    pub fn show_open_dialog(self: Rc<Self>) {
        let file_chooser = gtk::FileChooserNativeBuilder::new()
            .transient_for(&self.window)
            .modal(true)
            .action(gtk::FileChooserAction::Open)
            .select_multiple(true)
            // Translators: file chooser dialog title.
            .title(&gettext("Open image or video"))
            .build();

        file_chooser.connect_response({
            let file_chooser = RefCell::new(Some(file_chooser.clone()));
            move |_, response| {
                let file_chooser = file_chooser.borrow_mut().take().unwrap();

                if response == gtk::ResponseType::Accept {
                    for file in file_chooser.get_files() {
                        self.add_file(&file);
                    }
                }
            }
        });

        file_chooser.show();
    }

    pub fn add_file(&self, file: &gio::File) {
        g_debug!(LOG_DOMAIN, "add_file(), uri: {}", &file.get_uri());

        let gtkglsink = gst::ElementFactory::make("gtkglsink", None).unwrap();
        let glsinkbin = gst::ElementFactory::make("glsinkbin", None).unwrap();
        let playbin = gst::ElementFactory::make("playbin3", None).unwrap();

        glsinkbin
            .set_property("sink", &gtkglsink.to_value())
            .unwrap();

        playbin
            .set_property("video-sink", &glsinkbin.to_value())
            .unwrap();
        playbin.set_property("mute", &true).unwrap();
        playbin.set_property("uri", &file.get_uri()).unwrap();

        // Add the video widget to the UI.
        let widget = gtkglsink
            .get_property("widget")
            .unwrap()
            .get::<gtk::Widget>()
            .unwrap()
            .unwrap();
        widget.show();

        let index = self.stack_media.get_children().len() + 1;

        if index == 1 {
            // This is the first file.
            self.stack_title
                .set_visible_child_name("page_stack_switcher");
        }

        self.stack_media.add_titled(
            &widget,
            &index.to_string(),
            &file
                .query_info(
                    "standard::display-name",
                    gio::FileQueryInfoFlags::NONE,
                    None::<&gio::Cancellable>,
                )
                .ok()
                .and_then(|info| info.get_display_name())
                .unwrap_or_else(|| format!("File {}", index).into()),
        );

        self.pipeline.add(&playbin).unwrap();
        playbin.sync_state_with_parent().unwrap();
    }

    pub fn play_pause(&self) {
        let target_state = if self.pipeline_playing.get() {
            gst::State::Paused
        } else {
            gst::State::Playing
        };

        g_debug!(LOG_DOMAIN, "play_pause(), target_state: {:?}", target_state);

        self.pipeline.set_state(target_state).unwrap();
    }

    pub fn step_forward(&self) {
        g_debug!(LOG_DOMAIN, "step_forward()");

        self.pipeline.send_event(gst::event::Step::new(
            gst::format::Buffers(Some(1)),
            1.,
            true,
            false,
        ));
    }

    fn refresh_ui(&self) {
        if let Some(position) = self.pipeline.query_position::<gst::ClockTime>() {
            let nanoseconds = position.nanoseconds().unwrap();
            let mut seconds = nanoseconds / 1_000_000_000;
            let mut minutes = seconds / 60;
            let hours = minutes / 60;
            seconds %= 60;
            minutes %= 60;

            let time = if hours == 0 {
                format!("{}:{:02}", minutes, seconds)
            } else {
                format!("{}:{:02}:{:02}", hours, minutes, seconds)
            };

            self.label_current_time
                .set_markup(&format!("<span font_features=\"tnum\">{}</span>", time));

            if let Some(duration) = self.pipeline.query_duration::<gst::ClockTime>() {
                // There's a DurationChanged message, however it is delivered before the first
                // AsyncDone, which means it's possible that query_duration won't work yet. For
                // instance, with GST_DEBUG=5 querying the duration upon receiving DurationChanged
                // returns None all of the time.
                //
                // Hence, update the duration from here; this callback is called on a timer as well
                // as upon receiving AsyncDone.

                let value =
                    position.nanoseconds().unwrap() as f64 / duration.nanoseconds().unwrap() as f64;

                self.adjustment_position
                    .block_signal(&self.adjustment_position_value_changed);
                self.adjustment_position.set_value(value);
                self.adjustment_position
                    .unblock_signal(&self.adjustment_position_value_changed);
            }
        }
    }

    fn on_bus_message(&self, msg: &gst::Message) -> glib::Continue {
        use gst::MessageView;
        match msg.view() {
            MessageView::StateChanged(state_changed) => {
                // g_debug!(LOG_DOMAIN, "StateChanged {:?}", state_changed.get_current());

                if state_changed.get_current() == gst::State::Playing {
                    self.pipeline_playing.set(true);
                    self.button_play_pause_image
                        .set_property_icon_name(Some("media-playback-pause-symbolic"));
                } else {
                    self.pipeline_playing.set(false);
                    self.button_play_pause_image
                        .set_property_icon_name(Some("media-playback-start-symbolic"));
                }
            }
            MessageView::DurationChanged(_) => {
                g_debug!(LOG_DOMAIN, "DurationChanged");
            }
            MessageView::Eos(_) => {
                g_debug!(LOG_DOMAIN, "Eos");

                self.pipeline
                    .seek_simple(
                        gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                        gst::ClockTime::from_seconds(0),
                    )
                    .unwrap();
            }
            MessageView::AsyncDone(_) => {
                g_debug!(LOG_DOMAIN, "AsyncDone");

                // If we've opened something with a duration, show the controls.
                if self.pipeline.query_duration::<gst::ClockTime>().is_some() {
                    self.revealer_controls.set_reveal_child(true);
                }
            }
            MessageView::Error(err) => {
                g_warning!(
                    LOG_DOMAIN,
                    "Error from {:?}: {} ({:?})",
                    err.get_src().map(|s| s.get_path_string()),
                    err.get_error(),
                    err.get_debug()
                );
            }
            _ => (),
        }

        glib::Continue(true)
    }
}
