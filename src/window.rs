use std::cell::RefCell;

use gettextrs::gettext;
use glib::{debug, warn};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio};

use crate::G_LOG_DOMAIN;

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

mod imp {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::time::Duration;

    use adw::subclass::prelude::*;
    use glib::{clone, closure, error, SignalHandlerId, SourceId};
    use gst::prelude::*;
    use gtk::gdk::{self, Key, ModifierType};
    use gtk::subclass::prelude::*;
    use gtk::{glib, CompositeTemplate};
    use once_cell::unsync::OnceCell;

    use super::*;
    use crate::application::Application;
    use crate::config;
    use crate::page::Page;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/window.ui")]
    pub struct Window {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        tab_view: TemplateChild<adw::TabView>,
        #[template_child]
        controls_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        play_pause_button: TemplateChild<gtk::Button>,
        #[template_child]
        time_label: TemplateChild<gtk::Label>,
        #[template_child]
        time_adjustment: TemplateChild<gtk::Adjustment>,
        time_adjustment_value_changed: OnceCell<SignalHandlerId>,

        pipeline: OnceCell<gst::Pipeline>,

        is_playing: Cell<bool>,
        is_backward: Cell<bool>,

        page_is_loading_notify_id: RefCell<HashMap<Page, SignalHandlerId>>,
        switch_to_content_source_id: RefCell<Option<SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "IdWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            Self::Type::bind_template_callbacks(klass);

            klass.install_action("win.play-pause", None, |obj, _, _| obj.imp().play_pause());
            klass.add_binding_action(
                Key::space,
                ModifierType::CONTROL_MASK,
                "win.play-pause",
                None,
            );
            klass.add_binding_action(Key::p, ModifierType::empty(), "win.play-pause", None);

            klass.install_action("win.open", None, |obj, _, _| obj.on_open_clicked());
            klass.add_binding_action(Key::o, ModifierType::CONTROL_MASK, "win.open", None);

            klass.install_action("win.paste", None, |obj, _, _| {
                glib::MainContext::default()
                    .spawn_local(clone!(@weak obj => async move { obj.paste().await; }));
            });
            klass.add_binding_action(Key::v, ModifierType::CONTROL_MASK, "win.paste", None);

            klass.install_action("win.close-tab", None, |obj, _, _| obj.imp().close_tab());
            klass.add_binding_action(Key::w, ModifierType::CONTROL_MASK, "win.close-tab", None);

            klass.install_action("win.step-forward", None, |obj, _, _| {
                obj.imp().step_forward()
            });
            klass.add_binding_action(Key::period, ModifierType::empty(), "win.step-forward", None);

            klass.install_action("win.step-back", None, |obj, _, _| obj.imp().step_back());
            klass.add_binding_action(Key::comma, ModifierType::empty(), "win.step-back", None);

            klass.install_action(
                "win.focus-tab",
                Some(i32::static_variant_type().as_str()),
                |obj, _, param| {
                    let index = param
                        .expect("missing parameter")
                        .get()
                        .expect("wrong parameter type");
                    obj.imp().focus_tab(index);
                },
            );

            for i in 0..10 {
                klass.add_binding_action(
                    Key::from_name(&format!("{i}")).unwrap(),
                    ModifierType::empty(),
                    "win.focus-tab",
                    Some(&((i + 9) % 10).to_variant()),
                );
                klass.add_binding_action(
                    Key::from_name(&format!("KP_{i}")).unwrap(),
                    ModifierType::empty(),
                    "win.focus-tab",
                    Some(&((i + 9) % 10).to_variant()),
                );
            }

            klass.install_action("win.about", None, |window, _, _| {
                gtk::AboutDialog::builder()
                    .transient_for(window)
                    .modal(true)
                    .logo_icon_name(config::APP_ID)
                    .version(config::VERSION)
                    .license_type(gtk::License::Gpl30)
                    .authors(vec!["Ivan Molodetskikh".to_owned()])
                    .website("https://gitlab.gnome.org/YaLTeR/identity")
                    // Translators: shown in the About dialog, put your name here.
                    .translator_credits(&gettext("translator-credits"))
                    .build()
                    .show();
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Window {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            if config::PROFILE == "Devel" {
                obj.add_css_class("devel");
            }

            // FIXME: Remove when https://github.com/gtk-rs/gtk4-rs/issues/934 is fixed.
            self.tab_view.connect_page_detached(
                clone!(@weak obj => move |_, tab_page, _| obj.imp().on_page_detached(tab_page)),
            );

            let drop_target =
                gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
            drop_target.connect_drop(
                clone!(@weak obj => @default-return false, move |_, data, _, _| {
                    if let Ok(file_list) = data.get::<gdk::FileList>() {
                        for file in file_list.files().into_iter() {
                            obj.open_file(&file);
                        }

                        return true;
                    }

                    false
                }),
            );
            self.stack.add_controller(&drop_target);

            let pipeline = gst::Pipeline::new(None);
            self.pipeline.set(pipeline.clone()).unwrap();

            let bus = pipeline.bus().unwrap();
            bus.add_watch_local(
                clone!(@weak obj => @default-return glib::Continue(false), move |_, msg| {
                    obj.imp().on_bus_message(msg);
                    glib::Continue(true)
                }),
            )
            .expect("could not add pipeline bus watch");

            pipeline
                .set_state(gst::State::Paused)
                .expect("error setting pipeline state to Paused");

            self.time_adjustment_value_changed
                .set(self.time_adjustment.connect_value_changed(
                    clone!(@weak obj => move |adj| obj.imp().seek(adj.value())),
                ))
                .unwrap();

            glib::timeout_add_local(
                Duration::from_millis(100),
                clone!(@weak obj => @default-return glib::Continue(false), move || {
                    obj.imp().refresh_controls();
                    glib::Continue(true)
                }),
            );
        }

        fn dispose(&self, _obj: &Self::Type) {
            if let Some(pipeline) = self.pipeline.get() {
                // I got this to return Err once by opening a file GStreamer couldn't play and a
                // regular video file.
                if let Err(err) = pipeline.set_state(gst::State::Null) {
                    warn!("error setting pipeline state to Null: {err:?}");
                }

                // This returns Err if called multiple times.
                if let Err(err) = pipeline.bus().unwrap().remove_watch() {
                    warn!("error removing pipeline bus watch: {err:?}");
                }
            } else {
                warn!("unexpected unset `pipeline`");
            }
        }
    }

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}

    #[gtk::template_callbacks]
    impl Window {
        fn on_bus_message(&self, msg: &gst::Message) {
            let pipeline = match self.pipeline.get() {
                Some(x) => x,
                None => {
                    error!("received bus message {msg:?} without `pipeline` set");
                    return;
                }
            };

            use gst::MessageView;
            match msg.view() {
                MessageView::StateChanged(state_changed)
                    if state_changed.src().as_ref() == Some(pipeline.upcast_ref()) =>
                {
                    debug!(
                        "bus: StateChanged old: {:?}, current: {:?}, pending: {:?}",
                        state_changed.old(),
                        state_changed.current(),
                        state_changed.pending(),
                    );

                    use gst::State::*;
                    match (state_changed.current(), state_changed.pending()) {
                        (Playing, VoidPending) => {
                            self.is_playing.set(true);
                            self.play_pause_button
                                .set_icon_name("media-playback-pause-symbolic");
                        }
                        (Paused, VoidPending) => {
                            self.is_playing.set(false);
                            self.play_pause_button
                                .set_icon_name("media-playback-start-symbolic");
                        }
                        (_, _) => (),
                    }
                }
                MessageView::Eos(_) => {
                    debug!("bus: Eos");

                    self.seek(0.);
                }
                MessageView::Error(err) => {
                    warn!(
                        "bus: Error from {:?}: {} ({:?})",
                        err.src(),
                        err.error(),
                        err.debug(),
                    );

                    // Upon getting an error, find the playbin the error originates from and remove
                    // it from the pipeline. Note that find_immediate_child() can fail if an element
                    // throws multiple errors at once since playbin will be removed from the
                    // pipeline the first time around.
                    if let Some(playbin) = err.src().and_then(|obj| self.find_immediate_child(obj))
                    {
                        let playbin = playbin
                            .downcast::<gst::Element>()
                            .expect("immediate child didn't downcast to `gst::Element`");

                        if let Err(err) = pipeline.remove(&playbin) {
                            warn!("error removing playbin from pipeline: {err:?}");
                        }

                        if let Some(page) = self.find_page_for_playbin(&playbin) {
                            page.set_error();
                        } else {
                            error!("couldn't find page for playbin");
                        }
                    }
                }
                _ => (),
            }
        }

        fn find_immediate_child(&self, mut object: gst::Object) -> Option<gst::Object> {
            let pipeline = match self.pipeline.get() {
                Some(x) => x,
                None => {
                    error!("find_immediate_child: unexpected unset `pipeline`");
                    return None;
                }
            };

            loop {
                let parent = object.parent()?;
                if &parent == pipeline {
                    return Some(object);
                }

                object = parent;
            }
        }

        fn find_page_for_playbin(&self, playbin: &gst::Element) -> Option<Page> {
            for i in 0..self.tab_view.n_pages() {
                let page = self.tab_view.nth_page(i).child();
                let page = page
                    .downcast::<Page>()
                    .expect("unexpected widget type in tab view");
                if page.playbin() == Some(playbin) {
                    return Some(page);
                }
            }

            None
        }

        #[template_callback]
        fn play_pause(&self) {
            let pipeline = match self.pipeline.get() {
                Some(x) => x,
                None => {
                    error!("play_pause: unexpected unset `pipeline`");
                    return;
                }
            };

            let target_state = if self.is_playing.get() {
                gst::State::Paused
            } else {
                gst::State::Playing
            };

            debug!(
                "play_pause: target_state: {:?}, is_backward: {}",
                target_state,
                self.is_backward.get()
            );

            if target_state == gst::State::Playing && self.is_backward.get() {
                let position = pipeline.query_position::<gst::ClockTime>();
                if position.is_none() {
                    return;
                }

                if let Err(err) = pipeline.seek(
                    1.,
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::SeekType::Set,
                    position,
                    gst::SeekType::End,
                    Some(gst::ClockTime::ZERO),
                ) {
                    warn!("error seeking: {err:?}");
                }

                self.is_backward.set(false);
            }

            pipeline.set_state(target_state).unwrap();
        }

        fn seek(&self, value: f64) {
            debug!("seek({value:.02})");

            let pipeline = match self.pipeline.get() {
                Some(x) => x,
                None => {
                    error!("seek: unexpected unset `pipeline`");
                    return;
                }
            };

            if let Some(duration) = pipeline.query_duration::<gst::ClockTime>() {
                let time =
                    gst::ClockTime::from_nseconds((value * duration.nseconds() as f64) as u64);

                if let Err(err) = pipeline.seek_simple(gst::SeekFlags::FLUSH, time) {
                    // This can happen if there's a broken playbin in the pipeline that nevertheless
                    // hasn't sent an error to the bus yet.
                    warn!("error seeking: {err:?}");
                }

                self.is_backward.set(false);
            }
        }

        /// Steps one frame into the current playback direction.
        fn step_frame(&self) {
            debug!("step_frame()");

            let pipeline = match self.pipeline.get() {
                Some(x) => x,
                None => {
                    error!("step_frame: unexpected unset `pipeline`");
                    return;
                }
            };

            pipeline.send_event(gst::event::Step::new(
                gst::format::Buffers(1),
                1.,
                true,
                false,
            ));
        }

        fn step_forward(&self) {
            if self.is_playing.get() {
                // Only step while paused.
                return;
            }

            debug!("step_forward: is_backward: {}", self.is_backward.get());

            if self.is_backward.get() {
                let pipeline = match self.pipeline.get() {
                    Some(x) => x,
                    None => {
                        error!("step_forward: unexpected unset `pipeline`");
                        return;
                    }
                };

                if let Some(position) = pipeline.query_position::<gst::ClockTime>() {
                    // Reversing playback direction already steps 1 frame in most cases.
                    // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/20
                    if let Err(err) = pipeline.seek(
                        1.,
                        gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                        gst::SeekType::Set,
                        position,
                        gst::SeekType::End,
                        gst::ClockTime::ZERO,
                    ) {
                        warn!("step_forward: error seeking: {err:?}");
                    }

                    self.is_backward.set(false);
                }
            } else {
                self.step_frame();
            }
        }

        fn step_back(&self) {
            if self.is_playing.get() {
                // Only step while paused.
                return;
            }

            debug!("step_back: is_backward: {}", self.is_backward.get());

            if self.is_backward.get() {
                self.step_frame();
            } else {
                let pipeline = match self.pipeline.get() {
                    Some(x) => x,
                    None => {
                        error!("step_back: unexpected unset `pipeline`");
                        return;
                    }
                };

                if let Some(position) = pipeline.query_position::<gst::ClockTime>() {
                    // Reversing playback direction already steps 1 frame in most cases.
                    // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/20
                    if let Err(err) = pipeline.seek(
                        -1.,
                        gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                        gst::SeekType::Set,
                        gst::ClockTime::ZERO,
                        gst::SeekType::Set,
                        position,
                    ) {
                        warn!("step_back: error seeking: {err:?}");
                    }

                    self.is_backward.set(true);
                }
            }
        }

        fn refresh_controls(&self) {
            let pipeline = match self.pipeline.get() {
                Some(x) => x,
                None => {
                    error!("refresh_controls: unexpected unset `pipeline`");
                    return;
                }
            };

            let duration = pipeline.query_duration::<gst::ClockTime>();

            // If we've opened something with a duration, show the controls.
            //
            // Do this outside the position check because when attaching a new playbin to the
            // pipeline the position query might still return `None` even though the duration is
            // already set. We rely on `refresh_controls` to reveal the controls as soon as possible
            // for smooth animation.
            self.controls_revealer.set_reveal_child(duration.is_some());

            if let Some(position) = pipeline.query_position::<gst::ClockTime>() {
                let nanoseconds = position.nseconds();
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

                self.time_label.set_label(&time);

                if let Some(duration) = duration {
                    let value = position.nseconds() as f64 / duration.nseconds() as f64;

                    let value_changed = self
                        .time_adjustment_value_changed
                        .get()
                        .expect("unexpected unset `time_adjustment_value_changed`");
                    self.time_adjustment.block_signal(value_changed);
                    self.time_adjustment.set_value(value);
                    self.time_adjustment.unblock_signal(value_changed);
                }
            }
        }

        pub fn open_file(&self, file: &gio::File) {
            debug!("open_file(\"{}\")", file.uri());

            let page = Page::new(file);
            let tab_page = self.tab_view.append(&page);

            page.bind_property("display-name", &tab_page, "title")
                .flags(glib::BindingFlags::SYNC_CREATE)
                .build();
            page.bind_property("is-loading", &tab_page, "loading")
                .flags(glib::BindingFlags::SYNC_CREATE)
                .build();

            page.property_expression("is-error")
                .chain_closure::<Option<gio::Icon>>(closure!(
                    |_: Option<glib::Object>, is_error: bool| {
                        if is_error {
                            Some(gio::ThemedIcon::new("error-symbolic"))
                        } else {
                            None
                        }
                    }
                ))
                .bind(&tab_page, "icon", None::<&Page>);
        }

        fn attach_playbin(&self, playbin: &gst::Element) {
            let pipeline = match self.pipeline.get() {
                Some(x) => x,
                None => {
                    error!("attach_playbin: unexpected unset `pipeline`");
                    return;
                }
            };

            if let Err(err) = pipeline.add(playbin) {
                error!("error adding playbin to pipeline: {err:?}");
                return;
            }

            if let Err(err) = playbin.sync_state_with_parent() {
                warn!("error syncing playbin state with parent: {err:?}");
            }

            self.seek(self.time_adjustment.value());

            self.refresh_controls();
            self.stack.set_visible_child_name("content");
            self.instance().present_if_not_visible();
        }

        fn detach_playbin(&self, playbin: &gst::Element) {
            let pipeline = match self.pipeline.get() {
                Some(x) => x,
                None => {
                    error!("detach_playbin: unexpected unset `pipeline`");
                    return;
                }
            };

            if let Err(err) = pipeline.remove(playbin) {
                error!("error removing playbin from pipeline: {err:?}");
            }

            if let Err(err) = playbin.set_state(gst::State::Paused) {
                warn!("error setting playbin state to Paused: {err:?}");
            }
        }

        #[template_callback]
        fn on_page_attached(&self, tab_page: &adw::TabPage) {
            debug!("page-attached");

            self.switch_to_content_after_timeout();

            let page: Page = tab_page
                .child()
                .downcast()
                .expect("tab page child has wrong type");

            if page.is_error() {
                self.stack.set_visible_child_name("content");
                self.instance().present_if_not_visible();
            } else if let Some(playbin) = page.playbin() {
                self.attach_playbin(playbin);
            } else {
                let obj = self.instance();
                let id = page.connect_notify_local(
                    Some("is-loading"),
                    clone!(@weak obj => move |page, _| {
                        if let Some(playbin) = page.playbin() {
                            obj.imp().attach_playbin(playbin);
                        } else {
                            // attach_playbin does this for us in the good case.
                            obj.imp().stack.set_visible_child_name("content");
                            obj.present_if_not_visible();
                        }
                    }),
                );

                if self
                    .page_is_loading_notify_id
                    .borrow_mut()
                    .insert(page, id)
                    .is_some()
                {
                    error!("`page_playbin_notify_id` already had an entry for this page");
                };
            }
        }

        #[template_callback]
        fn on_page_detached(&self, tab_page: &adw::TabPage) {
            debug!("page-detached");

            if self.tab_view.n_pages() == 0 {
                self.stack.set_visible_child_name("empty");
            }

            let page: Page = tab_page
                .child()
                .downcast()
                .expect("tab page child has wrong type");

            if let Some(playbin) = page.playbin() {
                self.detach_playbin(playbin);
            } else if let Some(id) = self.page_is_loading_notify_id.borrow_mut().remove(&page) {
                page.disconnect(id);
            }
        }

        #[template_callback]
        fn on_create_window(&self) -> adw::TabView {
            debug!("create-window");

            let application: Application = self
                .instance()
                .application()
                .expect("application was not set")
                .downcast()
                .expect("application has wrong type");
            let new_window = application.create_new_window();
            new_window.imp().tab_view.clone()
        }

        pub fn close_tab(&self) {
            if let Some(page) = self.tab_view.selected_page() {
                self.tab_view.close_page(&page);
            } else {
                self.instance().close();
            }
        }

        pub fn focus_tab(&self, index: i32) {
            if index < self.tab_view.n_pages() {
                let page = self.tab_view.nth_page(index);
                self.tab_view.set_selected_page(&page);
            }
        }

        fn switch_to_content_after_timeout(&self) {
            let mut source_id = self.switch_to_content_source_id.borrow_mut();
            if source_id.is_some() {
                return;
            }

            let obj = self.instance();
            *source_id = Some(glib::timeout_add_local_once(
                Duration::from_millis(300),
                clone!(@weak obj => move || {
                    debug!("switch to content timeout callback");

                    let self_ = obj.imp();
                    let _ = self_.switch_to_content_source_id.take();

                    // The user could've closed the loading tab before the timeout fired or was
                    // cancelled. So check again here and only switch if there are open tabs.
                    if self_.tab_view.n_pages() > 0 {
                        self_.stack.set_visible_child_name("content");
                    }

                    obj.present_if_not_visible();
                }),
            ));
        }

        #[template_callback]
        fn on_visible_child_notify(&self) {
            if let Some(source_id) = self.switch_to_content_source_id.take() {
                source_id.remove();
            }
        }
    }
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap;
}

#[gtk::template_callbacks]
impl Window {
    pub fn new(app: &impl IsA<gtk::Application>) -> Self {
        glib::Object::new(&[("application", app)]).expect("could not create `Window`")
    }

    pub fn open_file(&self, file: &gio::File) {
        self.imp().open_file(file);
    }

    fn present_if_not_visible(&self) {
        if !self.is_visible() {
            debug!("present_if_not_visible: presenting");

            self.present();
        }
    }

    async fn paste(&self) {
        let value = match self
            .clipboard()
            .read_value_future(gdk::FileList::static_type(), glib::PRIORITY_DEFAULT)
            .await
        {
            Ok(x) => x,
            Err(err) => {
                warn!("could not read clipboard contents: {err:?}");
                return;
            }
        };

        let file_list: gdk::FileList = match value.get() {
            Ok(x) => x,
            Err(err) => {
                warn!("could not convert value to `FileList`: {err:?}");
                return;
            }
        };

        for file in file_list.files() {
            self.open_file(&file);
        }
    }

    #[template_callback]
    fn on_open_clicked(&self) {
        let filter = gtk::FileFilter::new();
        // Translators: file chooser file filter name.
        filter.set_name(Some(&gettext("Videos and images")));
        for mime_type in MIME_TYPES {
            filter.add_mime_type(mime_type);
        }

        let file_chooser = gtk::FileChooserNative::builder()
            .transient_for(self)
            .modal(true)
            .action(gtk::FileChooserAction::Open)
            .select_multiple(true)
            // Translators: file chooser dialog title.
            .title(&gettext("Open videos or images to compare"))
            .build();

        file_chooser.add_filter(&filter);

        file_chooser.connect_response({
            let obj = self.downgrade();
            let file_chooser = RefCell::new(Some(file_chooser.clone()));
            move |_, response| {
                if let Some(obj) = obj.upgrade() {
                    if let Some(file_chooser) = file_chooser.take() {
                        if response == gtk::ResponseType::Accept {
                            for file in file_chooser.files().snapshot().into_iter() {
                                let file: gio::File = file
                                    .downcast()
                                    .expect("unexpected type returned from file chooser");
                                obj.open_file(&file);
                            }
                        }
                    } else {
                        warn!("got file chooser response more than once");
                    }
                } else {
                    warn!("got file chooser response after window was freed");
                }
            }
        });

        file_chooser.show();
    }
}
