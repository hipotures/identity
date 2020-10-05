use std::{cell::Cell, cell::RefCell, convert::TryInto, rc::Rc, time::Duration};

use futures_util::{pin_mut, select_biased, FusedFuture, FutureExt, StreamExt};
use gettextrs::gettext;
use gio::prelude::*;
use gst::prelude::*;
use gstreamer as gst;
use gtk::prelude::*;
use libhandy as hdy;
use once_cell::unsync::OnceCell;

use crate::config::{LOG_DOMAIN, PROFILE};

/// Show a loading state when a file takes longer than this to load.
const TIMEOUT: Duration = Duration::from_millis(300);

/// MIME types for the file chooser filter.
const MIME_TYPES: &[&str] = &[
    "image/bmp",
    "image/jpeg",
    "image/jpg",
    "image/pjpeg",
    "image/png",
    "image/tiff",
    "image/x-bmp",
    "image/x-gray",
    "image/x-icb",
    "image/x-ico",
    "image/x-png",
    "image/x-portable-anymap",
    "image/x-portable-bitmap",
    "image/x-portable-graymap",
    "image/x-portable-pixmap",
    "image/x-xbitmap",
    "image/x-xpixmap",
    "image/x-pcx",
    "image/svg+xml",
    "image/svg+xml-compressed",
    "image/vnd.wap.wbmp",
    "image/x-icns",
    "video/3gp",
    "video/3gpp",
    "video/3gpp2",
    "video/dv",
    "video/divx",
    "video/fli",
    "video/flv",
    "video/mp2t",
    "video/mp4",
    "video/mp4v-es",
    "video/mpeg",
    "video/mpeg-system",
    "video/msvideo",
    "video/ogg",
    "video/quicktime",
    "video/vivo",
    "video/vnd.divx",
    "video/vnd.mpegurl",
    "video/vnd.rn-realvideo",
    "video/vnd.vivo",
    "video/webm",
    "video/x-anim",
    "video/x-avi",
    "video/x-flc",
    "video/x-fli",
    "video/x-flic",
    "video/x-flv",
    "video/x-m4v",
    "video/x-matroska",
    "video/x-mjpeg",
    "video/x-mpeg",
    "video/x-mpeg2",
    "video/x-ms-asf",
    "video/x-ms-asf-plugin",
    "video/x-ms-asx",
    "video/x-msvideo",
    "video/x-ms-wm",
    "video/x-ms-wmv",
    "video/x-ms-wvx",
    "video/x-nsv",
    "video/x-ogm+ogg",
    "video/x-theora",
    "video/x-theora+ogg",
];

struct Page {
    playbin: gst::Element,
    stack: gtk::Stack,
}

pub struct Window {
    pub window: hdy::ApplicationWindow,
    stack_media: gtk::Stack,
    pipeline: gst::Pipeline,
    pipeline_playing: Cell<bool>,
    forward: Cell<bool>,
    label_current_time: gtk::Label,
    adjustment_position: gtk::Adjustment,
    adjustment_position_value_changed: OnceCell<glib::SignalHandlerId>,
    stack_title: gtk::Stack,
    button_play_pause_image: gtk::Image,
    revealer_controls: gtk::Revealer,
    pages: RefCell<Vec<Page>>,
    stack_main: gtk::Stack,
    paused_senders: RefCell<Vec<futures_channel::oneshot::Sender<()>>>,
}

impl Window {
    pub fn new() -> Rc<Self> {
        let builder = gtk::Builder::from_resource("/org/gnome/gitlab/YaLTeR/Identity/window.ui");
        let window: hdy::ApplicationWindow = builder.get_object("window").unwrap();
        let stack_main: gtk::Stack = builder.get_object("stack_main").unwrap();
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

        stack_main.set_visible_child_name("page_empty");

        // Devel Profile
        if PROFILE == "Devel" {
            window.get_style_context().add_class("devel");
        }

        let pipeline = gst::Pipeline::new(None);

        let self_ = Rc::new(Window {
            window,
            stack_main,
            stack_media,
            pipeline: pipeline.clone(),
            pipeline_playing: Cell::new(false),
            forward: Cell::new(true),
            label_current_time,
            adjustment_position: adjustment_position.clone(),
            adjustment_position_value_changed: OnceCell::new(),
            stack_title,
            button_play_pause_image,
            revealer_controls,
            pages: RefCell::new(Vec::new()),
            paused_senders: RefCell::new(Vec::new()),
        });

        self_
            .adjustment_position_value_changed
            .set(adjustment_position.connect_value_changed({
                let self_ = Rc::downgrade(&self_);
                move |adjustment_position| {
                    let self_ = self_.upgrade().unwrap();
                    self_.seek(adjustment_position.get_value());
                }
            }))
            .unwrap();

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
                // I got this to return Err once by opening a file GStreamer couldn't play and a
                // regular video file.
                let _ = pipeline.set_state(gst::State::Null);

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

        // Register the window as a DnD destination.
        self_
            .window
            .drag_dest_set(gtk::DestDefaults::ALL, &[], gdk::DragAction::COPY);
        self_.window.drag_dest_add_uri_targets();
        self_.window.connect_drag_data_received({
            let self_ = Rc::downgrade(&self_);
            move |_, context, _, _, data, _, time| {
                let self_ = self_.upgrade().unwrap();
                let uris = data.get_uris();

                for uri in uris {
                    self_.add_file(gio::File::new_for_uri(&uri));
                }

                context.drag_finish(true, false, time);
            }
        });

        self_
    }

    pub fn set_visible_child(&self, child: u8) {
        self.stack_media.set_visible_child_name(&child.to_string());
    }

    pub fn show_open_dialog(self: Rc<Self>) {
        let filter = gtk::FileFilter::new();
        // Translators: file chooser file filter name.
        filter.set_name(Some(&gettext("Videos and images")));
        for mime_type in MIME_TYPES {
            filter.add_mime_type(mime_type);
        }

        let file_chooser = gtk::FileChooserNativeBuilder::new()
            .transient_for(&self.window)
            .modal(true)
            .action(gtk::FileChooserAction::Open)
            .select_multiple(true)
            // Translators: file chooser dialog title.
            .title(&gettext("Open videos or images to compare"))
            .build();

        file_chooser.add_filter(&filter);

        file_chooser.connect_response({
            let file_chooser = RefCell::new(Some(file_chooser.clone()));
            move |_, response| {
                let file_chooser = file_chooser.borrow_mut().take().unwrap();

                if response == gtk::ResponseType::Accept {
                    for file in file_chooser.get_files() {
                        self.add_file(file);
                    }
                }
            }
        });

        file_chooser.show();
    }

    pub fn add_file(self: &Rc<Self>, file: gio::File) {
        g_debug!(LOG_DOMAIN, "add_file(), uri: {}", &file.get_uri());

        let (playbin, widget) = create_player(&file.get_uri());

        let index = self.stack_media.get_children().len() + 1;

        if index == 1 {
            // This is the first file.
            self.stack_title
                .set_visible_child_name("page_stack_switcher");
        }

        let builder =
            gtk::Builder::from_resource("/org/gnome/gitlab/YaLTeR/Identity/media_page.ui");
        let stack: gtk::Stack = builder.get_object("stack_main").unwrap();

        // Set up the media page.
        let box_media: gtk::Box = builder.get_object("box_media").unwrap();
        box_media.pack_start(&widget, true, true, 0);

        // Show the loading spinner by default.
        stack.set_visible_child_name("page_loading");

        self.pages.borrow_mut().push(Page {
            playbin: playbin.clone(),
            stack: stack.clone(),
        });

        self.stack_media
            // Translators: placeholder shown in the headerbar for new files before their display
            // name is available (for example, when loading a file from a network mount).
            .add_titled(&stack, &index.to_string(), &gettext("Loading…"));

        let self_ = Rc::clone(self);
        let stack_ = stack.clone();
        let get_name_and_show_page = async move {
            let info_future = file.query_info_async_future(
                "standard::display-name",
                gio::FileQueryInfoFlags::NONE,
                glib::PRIORITY_DEFAULT,
            );

            let info = add_timeout_action(info_future.fuse(), TIMEOUT, || {
                // Show the page with a temporary name.
                stack_.show_all();
            })
            .await;

            let title = info
                .ok()
                .and_then(|info| info.get_display_name())
                .unwrap_or_else(|| file.get_uri());
            self_.stack_media.set_child_title(&stack_, Some(&title));

            stack_.show_all();
        };
        glib::MainContext::default().spawn_local(get_name_and_show_page);

        let self_ = Rc::clone(self);
        let start_playback = async move {
            let start_future = preroll(&playbin);

            let success = add_timeout_action(start_future.fuse(), TIMEOUT, || {
                // After a 300 ms timeout, show the loading spinner and the window, if it hasn't
                // opened yet.
                if self_.stack_main.get_visible_child_name().unwrap() == "page_empty" {
                    self_
                        .stack_main
                        .set_transition_type(gtk::StackTransitionType::Crossfade);
                    self_.stack_main.set_visible_child_name("page_loading");
                    self_
                        .stack_main
                        .set_transition_type(gtk::StackTransitionType::OverDownUp);
                }

                self_.window.show_all();
            })
            .await;

            stack.show_all();

            // Query duration now that the playbin is pre-rolled, before potentially changing its
            // state.
            let has_duration = playbin.query_duration::<gst::ClockTime>().is_some();

            if success {
                // To synchronize the playbin and the pipeline position, perform a seek on the
                // pipeline after adding the playbin.

                // First, pause the pipeline.
                let playing = self_.pipeline_playing.get();
                if playing {
                    let (tx, rx) = futures_channel::oneshot::channel();
                    self_.paused_senders.borrow_mut().push(tx);
                    self_.pipeline.set_state(gst::State::Paused).unwrap();
                    // Wait until it's paused.
                    let _ = rx.await;
                }

                self_.pipeline.add(&playbin).unwrap();

                self_.seek(self_.adjustment_position.get_value());

                // Finally, resume the pipeline if it has been playing.
                if playing {
                    self_.pipeline.set_state(gst::State::Playing).unwrap();
                }

                stack.set_visible_child_name("page_media");
            } else {
                stack.set_visible_child_name("page_error");
            }

            // Change the main stack visible child _after_ the media stack, so that the media stack
            // transition isn't run unnecessarily.
            if self_.stack_main.get_visible_child_name().unwrap() != "page_media" {
                // This is the first file being loaded. Display the revealer if needed and then set
                // its transition type. This way when the main stack first reveals a video, it will
                // already show the controls without an extra animation looking weird.
                if has_duration {
                    self_.revealer_controls.set_reveal_child(true);
                }

                self_
                    .revealer_controls
                    .set_transition_type(gtk::RevealerTransitionType::SlideUp);

                self_.stack_main.set_visible_child_name("page_media");
            }

            self_.window.show_all();
            self_
                .stack_main
                .set_transition_type(gtk::StackTransitionType::OverDownUp);
        };
        glib::MainContext::default().spawn_local(start_playback);
    }

    pub fn play_pause(&self) {
        let target_state = if self.pipeline_playing.get() {
            gst::State::Paused
        } else {
            gst::State::Playing
        };

        let forward = self.forward.get();

        g_debug!(
            LOG_DOMAIN,
            "play_pause(), target_state: {:?}, forward: {}",
            target_state,
            forward
        );

        if target_state == gst::State::Playing && !forward {
            if let Some(position) = self.pipeline.query_position::<gst::ClockTime>() {
                if position.is_none() {
                    return;
                }

                let _ = self.pipeline.seek(
                    1.,
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::SeekType::Set,
                    position,
                    gst::SeekType::End,
                    0.into(),
                );

                self.forward.set(true);
            }
        }

        self.pipeline.set_state(target_state).unwrap();
    }

    fn seek(&self, value: f64) {
        if let Some(duration) = self.pipeline.query_duration::<gst::ClockTime>() {
            g_debug!(LOG_DOMAIN, "seeking, value: {}", value);

            let time =
                gst::ClockTime::from_nseconds((value * duration.nseconds().unwrap() as f64) as u64);
            let _ = self.pipeline.seek_simple(gst::SeekFlags::FLUSH, time);

            self.forward.set(true);
        }
    }

    /// Steps one frame into the current playback direction.
    fn step_frame(&self) {
        self.pipeline.send_event(gst::event::Step::new(
            gst::format::Buffers(Some(1)),
            1.,
            true,
            false,
        ));
    }

    pub fn step_forward(&self) {
        if self.pipeline_playing.get() {
            // Only step while paused.
            return;
        }

        let forward = self.forward.get();

        g_debug!(LOG_DOMAIN, "step_forward(), forward: {}", forward);

        if forward {
            self.step_frame();
        } else {
            if let Some(position) = self.pipeline.query_position::<gst::ClockTime>() {
                if position.is_none() {
                    return;
                }

                // Reversing playback direction already steps 1 frame in most cases.
                // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/20
                let _ = self.pipeline.seek(
                    1.,
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::SeekType::Set,
                    position,
                    gst::SeekType::End,
                    0.into(),
                );

                self.forward.set(true);
            }
        }
    }

    pub fn step_back(&self) {
        if self.pipeline_playing.get() {
            // Only step while paused.
            return;
        }

        let forward = self.forward.get();

        g_debug!(LOG_DOMAIN, "step_back(), forward: {}", forward);

        if forward {
            if let Some(position) = self.pipeline.query_position::<gst::ClockTime>() {
                if position.is_none() {
                    return;
                }

                // Reversing playback direction already steps 1 frame in most cases.
                // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/20
                let _ = self.pipeline.seek(
                    -1.,
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::SeekType::Set,
                    0.into(),
                    gst::SeekType::Set,
                    position,
                );

                self.forward.set(false);
            }
        } else {
            self.step_frame();
        }
    }

    pub fn close_selected_file(&self) {
        let selected = match self.stack_media.get_visible_child() {
            Some(child) => child,
            None => return,
        };

        let mut pages = self.pages.borrow_mut();
        let index = match pages.iter().position(|page| page.stack == selected) {
            Some(index) => index,
            None => return,
        };

        let page = pages.remove(index);
        self.stack_media.remove(&page.stack);

        // Update names of pages past the removed one so our shortcuts still work.
        for (i, page) in pages.iter().skip(index).enumerate() {
            self.stack_media
                .set_child_name(&page.stack, Some(&(i + index + 1).to_string()));
        }

        let _ = self.pipeline.remove(&page.playbin);
        let _ = page.playbin.set_state(gst::State::Null);

        if self.stack_media.get_children().is_empty() {
            // No elements left, go back to the empty state.
            self.stack_main.set_visible_child_name("page_empty");
            self.stack_title.set_visible_child_name("page_title");

            // Reset the pipeline position and state.
            self.adjustment_position.set_value(0.); // This is used to seek in add_file().
            self.pipeline.set_state(gst::State::Paused).unwrap();
        }
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
                let value =
                    position.nanoseconds().unwrap() as f64 / duration.nanoseconds().unwrap() as f64;

                let value_changed = self.adjustment_position_value_changed.get().unwrap();
                self.adjustment_position.block_signal(value_changed);
                self.adjustment_position.set_value(value);
                self.adjustment_position.unblock_signal(value_changed);

                // If we've opened something with a duration, show the controls.
                if self.pipeline.query_duration::<gst::ClockTime>().is_some() {
                    self.revealer_controls.set_reveal_child(true);
                }
            }
        }
    }

    fn on_bus_message(&self, msg: &gst::Message) -> glib::Continue {
        use gst::MessageView;
        match msg.view() {
            MessageView::StateChanged(state_changed)
                if state_changed.get_src().as_ref()
                    == Some(self.pipeline.upcast_ref::<gst::Object>()) =>
            {
                g_debug!(
                    LOG_DOMAIN,
                    "StateChanged old: {:?}, current: {:?}, pending: {:?}",
                    state_changed.get_old(),
                    state_changed.get_current(),
                    state_changed.get_pending()
                );

                use gst::State::*;
                match (state_changed.get_current(), state_changed.get_pending()) {
                    (Playing, VoidPending) => {
                        self.pipeline_playing.set(true);
                        self.button_play_pause_image
                            .set_property_icon_name(Some("media-playback-pause-symbolic"));
                    }
                    (Paused, VoidPending) => {
                        self.pipeline_playing.set(false);
                        self.button_play_pause_image
                            .set_property_icon_name(Some("media-playback-start-symbolic"));

                        for sender in self.paused_senders.borrow_mut().drain(..) {
                            let _ = sender.send(());
                        }
                    }
                    (_, _) => (),
                }
            }
            MessageView::DurationChanged(_) => {
                g_debug!(LOG_DOMAIN, "DurationChanged");
            }
            MessageView::Eos(_) => {
                g_debug!(LOG_DOMAIN, "Eos");

                self.seek(0.);
            }
            MessageView::AsyncDone(_) => {
                g_debug!(LOG_DOMAIN, "AsyncDone");
            }
            MessageView::Error(err) => {
                g_warning!(
                    LOG_DOMAIN,
                    "Error from {:?}: {} ({:?})",
                    err.get_src(),
                    err.get_error(),
                    err.get_debug()
                );

                // Upon getting an error, find the playbin the error originates from and remove it
                // from the pipeline. Note that find_immediate_child() can fail if an element
                // throws multiple errors at once since playbin will be removed from the pipeline
                // the first time around.
                if let Some(playbin) = err.get_src().and_then(|obj| self.find_immediate_child(obj))
                {
                    let playbin = playbin.downcast::<gst::Element>().unwrap();
                    self.on_playbin_error(playbin);
                }
            }
            _ => (),
        }

        glib::Continue(true)
    }

    fn find_immediate_child(&self, mut object: gst::Object) -> Option<gst::Object> {
        loop {
            let parent = object.get_parent()?;
            if parent == self.pipeline {
                return Some(object);
            }

            object = parent;
        }
    }

    fn on_playbin_error(&self, playbin: gst::Element) {
        let pages = self.pages.borrow();
        let (i, page) = pages
            .iter()
            .enumerate()
            .find(|(_, page)| page.playbin == playbin)
            .unwrap();

        g_warning!(LOG_DOMAIN, "Hiding media on page {} due to error", i + 1);
        page.stack.set_visible_child_name("page_error");

        let _ = self.pipeline.remove(&playbin); // We can call this before adding playbin.
        let _ = playbin.set_state(gst::State::Null);
    }
}

/// Creates and returns a new playbin with a sink GTK widget.
///
/// # Panics
///
/// Panics if the creation of `gtksink` and `playbin3` elements fails.
fn create_player(uri: &glib::GString) -> (gst::Element, gtk::Widget) {
    // Using gtksink instead of gtkglsink due to instability.
    //
    // Issues I've hit:
    // - https://gitlab.freedesktop.org/mesa/mesa/-/issues/3029
    // - https://gitlab.gnome.org/GNOME/gtk/-/issues/3208
    //
    // Besides, with gtksink I can use alpha.
    let gtksink = gst::ElementFactory::make("gtksink", None).unwrap();
    let playbin = gst::ElementFactory::make("playbin3", None).unwrap();

    gtksink.set_property("ignore-alpha", &false).unwrap();

    playbin
        .set_property("video-sink", &gtksink.to_value())
        .unwrap();
    playbin.set_property("mute", &true).unwrap();
    playbin.set_property("uri", uri).unwrap();

    // Add the video widget to the UI.
    let widget = gtksink
        .get_property("widget")
        .unwrap()
        .get::<gtk::Widget>()
        .unwrap()
        .unwrap();

    (playbin, widget)
}

/// Adds a timeout action to the future.
///
/// If the future doesn't complete within `timeout`, `on_timeout` is called.
///
/// The value of `timeout` is floored down to a millisecond.
///
/// # Panics
///
/// Panics if the number of milliseconds in `timeout` doesn't fit into a `u32`.
async fn add_timeout_action<F: FusedFuture, O: FnOnce()>(
    future: F,
    timeout: Duration,
    on_timeout: O,
) -> F::Output {
    let timeout_ms = timeout.as_millis().try_into().unwrap();
    let mut timeout = glib::timeout_future(timeout_ms).fuse();
    pin_mut!(future);

    // Use biased select as a main loop stall can result in both the timeout and the target future
    // complete at the same time. In this case we want to prioritize the target future.
    select_biased! {
        result = future => result,
        _ = timeout => {
            on_timeout();
            future.await
        }
    }
}

/// Pre-rolls the `playbin`.
///
/// Returns `true` when the `playbin` has been successfully pre-rolled and put in the paused state,
/// and `false` on error.
async fn preroll(playbin: &gst::Element) -> bool {
    // Create the stream first to not miss any messages.
    let mut stream = playbin.get_bus().unwrap().stream();

    if let Err(err) = playbin
        .call_async_future(|playbin| playbin.set_state(gst::State::Paused))
        .await
    {
        // Can fail when the file is inaccessible.
        g_warning!(LOG_DOMAIN, "Error setting playbin state: {:?}", err);
        playbin.call_async(|playbin| {
            let _ = playbin.set_state(gst::State::Null);
        });
        return false;
    }

    loop {
        match stream.next().await.unwrap().view() {
            gst::MessageView::Error(err) => {
                // Can fail on missing codecs.
                g_warning!(LOG_DOMAIN, "playbin Error: {:?}", err);
                playbin.call_async(|playbin| {
                    let _ = playbin.set_state(gst::State::Null);
                });
                break false;
            }
            gst::MessageView::StateChanged(state_changed)
                if state_changed.get_src().as_ref()
                    == Some(playbin.upcast_ref::<gst::Object>()) =>
            {
                g_debug!(
                    LOG_DOMAIN,
                    "playbin StateChanged old: {:?}, current: {:?}, pending: {:?}",
                    state_changed.get_old(),
                    state_changed.get_current(),
                    state_changed.get_pending()
                );

                if state_changed.get_current() == gst::State::Paused
                    && state_changed.get_pending() == gst::State::VoidPending
                {
                    // Pre-rolled and ready to show.
                    break true;
                }
            }
            _ => (),
        }
    }
}
