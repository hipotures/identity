use glib::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};

use crate::application::Application;
use crate::picture::Picture;
use crate::scale_request::ScaleRequest;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::marker::PhantomData;
    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::Instant;

    use adw::subclass::prelude::*;
    use gettextrs::gettext;
    use glib::subclass::Signal;
    use glib::{clone, ControlFlow, Properties};
    use gst::bus::BusWatchGuard;
    use gst::prelude::*;
    use gtk::prelude::*;
    use gtk::{gdk, CompositeTemplate};
    use tracing::{instrument, Span};

    use super::*;
    use crate::application::VaDisplayState;
    use crate::texture_paintable::TexturePaintable;
    use crate::utils::gettext_f;

    const VA_CONTEXT_TYPE: &str = "gst.va.display.handle";

    #[derive(Debug, Default, CompositeTemplate, Properties)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/page.ui")]
    #[properties(wrapper_type = super::Page)]
    pub struct Page {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        picture: TemplateChild<Picture>,
        #[template_child]
        scrolled_window: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        title_label: TemplateChild<gtk::Label>,
        #[template_child]
        context_menu: TemplateChild<gtk::PopoverMenu>,
        #[template_child]
        click_menu_gesture: TemplateChild<gtk::GestureClick>,
        #[template_child]
        press_menu_gesture: TemplateChild<gtk::GestureLongPress>,

        #[property(get, set, construct_only)]
        application: OnceCell<Application>,
        #[property(get, set, construct_only)]
        file: OnceCell<gio::File>,
        #[property(get = Self::display_path)]
        display_path: PhantomData<Option<String>>,
        // I like single lines and rustfmt ignores this attribute so I declare this one as allowed.
        #[property(get = Self::scale_request, set = Self::set_scale_request, explicit_notify, minimum = 0., maximum = 10.)]
        scale_request: PhantomData<ScaleRequest>,
        #[property(get = Self::scale)]
        scale: PhantomData<f64>,
        #[property(get = Self::h_scroll_pos, set = Self::set_h_scroll_pos, explicit_notify)]
        h_scroll_pos: PhantomData<f64>,
        #[property(get = Self::v_scroll_pos, set = Self::set_v_scroll_pos, explicit_notify)]
        v_scroll_pos: PhantomData<f64>,

        // Page can be backed either by a GStreamer playbin, or by glycin, in which case it will
        // have a GdkTexture available. That is to say, either `playbin` or `texture` will be
        // `Some`, but not both.
        #[property(get)]
        playbin: RefCell<Option<gst::Element>>,
        #[property(get)]
        texture: RefCell<Option<gdk::Texture>>,

        // This can be a OnceCell<String>, but then #[property] assumes it's not nullable.
        #[property(get = Self::display_name)]
        display_name: RefCell<Option<glib::GString>>,
        #[property(get, default_value = true)]
        is_loading: Cell<bool>,
        #[property(get)]
        is_error: Cell<bool>,
        #[property(get, minimum = 0.)]
        framerate: Cell<f32>,
        #[property(get)]
        video_codec: RefCell<Option<String>>,
        #[property(get)]
        container_format: RefCell<Option<String>>,
        #[property(get = Self::resolution)]
        resolution: PhantomData<String>,
        #[property(get, set)]
        show_overlay: Cell<bool>,
        #[property(set = Self::set_menu_model)]
        menu_model: PhantomData<Option<gio::MenuModel>>,

        constructed_at: OnceCell<Instant>,
        bus_watch_guard: RefCell<Option<BusWatchGuard>>,
        preroll_span: RefCell<Option<Span>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Page {
        const NAME: &'static str = "IdPage";
        type Type = super::Page;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("id-page");

            // Copied from gtkbutton.c.
            const ACTIVATE_KEYS: [gdk::Key; 5] = [
                gdk::Key::space,
                gdk::Key::KP_Space,
                gdk::Key::Return,
                gdk::Key::ISO_Enter,
                gdk::Key::KP_Enter,
            ];
            for key in ACTIVATE_KEYS {
                klass.add_binding_signal(key, gdk::ModifierType::empty(), "activate");
            }

            klass.bind_template();
            klass.bind_template_callbacks();
            klass.bind_template_instance_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Page {
        fn properties() -> &'static [glib::ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            self.derived_set_property(id, value, pspec)
        }

        fn property(&self, id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            self.derived_property(id, pspec)
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("activate").action().build(),
                    Signal::builder("stop-kinetic-scrolling")
                        .param_types([super::Picture::static_type()])
                        .build(),
                    Signal::builder("setup-menu")
                        .param_types([bool::static_type()])
                        .action()
                        .build(),
                ]
            })
        }

        fn constructed(&self) {
            let obj = self.obj();
            self.parent_constructed();

            self.constructed_at
                .set(Instant::now())
                .expect("unexpected set `constructed_at`");

            self.is_loading.set(true);

            glib::MainContext::default().spawn_local(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.retrieve_display_name().await;
                }
            ));

            // Bind this here instead of the .blp because the .blp binding seems to happen before
            // `file` is set, and adding manual `file` setter that notifies `display-path` correctly
            // is a little more involved.
            obj.bind_property("display-path", &*self.title_label, "tooltip-text")
                .sync_create()
                .build();
            // Same as above, but for the case when retrieving the display name fails (e.g. the file
            // does not exist) in which case it should use file's URI.
            obj.bind_property("display-name", &*self.title_label, "label")
                .sync_create()
                .build();

            // For border-radius.
            obj.set_overflow(gtk::Overflow::Hidden);

            // Click to activate.
            let gesture = gtk::GestureClick::new();
            gesture.connect_released(clone!(
                #[weak]
                obj,
                move |gesture, _, _, _| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    obj.emit_by_name::<()>("activate", &[]);
                }
            ));
            obj.add_controller(gesture);

            // Click to open the context menu.
            self.click_menu_gesture.connect_pressed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |gesture, _n_clicks, x, y| {
                    if gesture.current_event().unwrap().triggers_context_menu() {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        imp.show_context_menu(x as i32, y as i32);
                    }
                }
            ));

            // Touch long press to open the context menu.
            self.press_menu_gesture.connect_pressed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |gesture, x, y| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    imp.show_context_menu(x as i32, y as i32);
                }
            ));

            // Notification that the context menu closed.
            self.context_menu.connect_notify_local(
                Some("visible"),
                clone!(
                    #[weak]
                    obj,
                    move |menu, _| {
                        if !menu.is_visible() {
                            // Run in an idle because otherwise it happens before the actual action.
                            glib::idle_add_local_once(move || {
                                obj.emit_by_name::<()>("setup-menu", &[&false]);
                            });
                        }
                    }
                ),
            );

            glib::MainContext::default().spawn_local(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.load_file().await;
                }
            ));
        }

        fn dispose(&self) {
            debug!("Page::dispose");
            if let Some(playbin) = self.playbin.take() {
                debug!("setting to Null and dropping playbin and bus");

                // Do it synchronously so that the main thread doesn't exit before
                // gtk4paintablesink's Paintable is dropped.
                if let Err(err) =
                    info_span!("set state").in_scope(|| playbin.set_state(gst::State::Null))
                {
                    warn!("error setting playbin state to Null: {err:?}");
                }

                drop(self.bus_watch_guard.take());
            }
        }
    }

    impl WidgetImpl for Page {}
    impl BinImpl for Page {}

    #[gtk::template_callbacks]
    impl Page {
        fn display_path(&self) -> Option<String> {
            self.file.get().map(|file| {
                file.path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file.uri().into())
            })
        }

        fn scale_request(&self) -> ScaleRequest {
            self.picture.scale_request()
        }

        fn set_scale_request(&self, val: ScaleRequest) {
            self.picture.set_scale_request(val)
        }

        fn scale(&self) -> f64 {
            self.picture.scale()
        }

        fn h_scroll_pos(&self) -> f64 {
            self.picture.h_scroll_pos()
        }

        fn set_h_scroll_pos(&self, val: f64) {
            self.picture.set_h_scroll_pos(val);
        }

        fn v_scroll_pos(&self) -> f64 {
            self.picture.v_scroll_pos()
        }

        fn set_v_scroll_pos(&self, val: f64) {
            self.picture.set_v_scroll_pos(val);
        }

        fn display_name(&self) -> Option<glib::GString> {
            if let Some(display_name) = &*self.display_name.borrow() {
                Some(display_name.clone())
            } else {
                self.file.get().map(|file| file.uri())
            }
        }

        pub fn set_error(&self) {
            if self.is_error.get() {
                return;
            }

            let obj = self.obj();
            let _guard = obj.freeze_notify();

            if self.is_loading.get() {
                self.is_loading.set(false);
                obj.notify_is_loading();
            }

            self.is_error.set(true);
            obj.notify_is_error();

            self.stack.set_visible_child_name("error");

            if let Some(playbin) = self.playbin.take() {
                if let Some(parent) = playbin.parent() {
                    // Remove it from the parent pipeline.
                    debug!("playbin parent is {parent:?}, removing playbin from it");
                    parent
                        .downcast::<gst::Bin>()
                        .unwrap()
                        .remove(&playbin)
                        .unwrap();
                }

                debug!("setting to Null and dropping playbin and bus");
                if let Err(err) =
                    info_span!("set state").in_scope(|| playbin.set_state(gst::State::Null))
                {
                    warn!("error setting playbin state to Null: {err:?}");
                }

                drop(self.bus_watch_guard.take());
            }
            obj.notify_playbin();
        }

        fn resolution(&self) -> String {
            self.picture
                .paintable()
                .and_then(|p| {
                    let width = p.intrinsic_width();
                    let height = p.intrinsic_height();
                    if width != 0 && height != 0 {
                        // Translators: Pixel-resolution format string for the media properties
                        // window. `{}` are replaced with pixel width and height. For example,
                        // 1920 × 1080.
                        Some(gettext_f(
                            "{} × {}",
                            [width.to_string(), height.to_string()],
                        ))
                    } else {
                        None
                    }
                })
                // Translators: "Not applicable" string for the media properties dialog when a
                // given property is unknown or missing (e.g. images don't have frame rate).
                .unwrap_or_else(|| gettext("N/A"))
        }

        fn set_menu_model(&self, val: Option<&gio::MenuModel>) {
            self.context_menu.set_menu_model(val);
        }

        pub fn reset_kinetic_scrolling(&self, except_picture: Option<&Picture>) {
            if except_picture == Some(&*self.picture) {
                return;
            }

            self.scrolled_window.set_kinetic_scrolling(false);
            self.scrolled_window.set_kinetic_scrolling(true);

            // Resetting kinetic scrolling breaks touchscreen pan gesture starting for horizontal
            // pans. Reallocating the scrolled window seems to fix that. Don't ask me why.
            self.scrolled_window.queue_allocate();
        }

        fn show_context_menu(&self, x: i32, y: i32) {
            self.obj().emit_by_name::<()>("setup-menu", &[&true]);

            let context_menu = &self.context_menu;
            let rect = gdk::Rectangle::new(x, y, 0, 0);
            context_menu.set_pointing_to(Some(&rect));

            context_menu.popup();
        }

        pub fn grab_focus_(&self) {
            self.scrolled_window.grab_focus();
        }

        #[instrument("Page::retrieve_display_name", fields(file = self.display_path().unwrap()), skip_all)]
        async fn retrieve_display_name(&self) {
            let file = self.file.get().expect("unexpected unset `file`");

            // glib::timeout_future_seconds(1).await;

            let info = file
                .query_info_future(
                    "standard::display-name",
                    gio::FileQueryInfoFlags::NONE,
                    glib::Priority::DEFAULT,
                )
                .await;

            let name = match info {
                Ok(info) => info.display_name(),
                Err(err) => {
                    warn!("error retrieving file display name: {err:?}");
                    return;
                }
            };

            assert_eq!(self.display_name.replace(Some(name)), None);
            self.obj().notify_display_name();
        }

        #[template_callback]
        fn get_content_provider(&self) -> gdk::ContentProvider {
            let file_list = gdk::FileList::from_array(&[self.file.get().unwrap().clone()]);
            gdk::ContentProvider::for_value(&file_list.to_value())
        }

        #[instrument("Page::load_file", skip_all)]
        async fn load_file(&self) {
            let file = self.file.get().expect("unexpected unset `file`");

            // Try to load the file with glycin first, and only after it fails, try with GStreamer.
            // This is unfortunate because it means that on network mounts the loading time becomes
            // much longer. However, due to GStreamer being prone to crashing (e.g. simply loading
            // a 16-bit PNG crashes the whole application at the time of this writing), we cannot
            // load the playbin in parallel. Oh well...
            let image = match glycin::Loader::new(file.clone()).load().await {
                Ok(image) => image,
                Err(err) => {
                    if err.is_out_of_memory() {
                        warn!("glycin reported out of memory; aborting loading");
                        self.set_error();
                        return;
                    }

                    let elapsed = self
                        .constructed_at
                        .get()
                        .expect("unexpected unset `constructed_at`")
                        .elapsed();
                    debug!(
                        "glycin failed to load format {:?} after {elapsed:?}, trying GStreamer",
                        err.unsupported_format(),
                    );

                    self.prepare_playbin(file);
                    return;
                }
            };

            let frame = match image.next_frame().await {
                Ok(frame) => frame,
                Err(err) => {
                    warn!("error loading frame with glycin: {err}");
                    self.set_error();
                    return;
                }
            };

            debug!(
                "glycin ready in {:?}",
                self.constructed_at
                    .get()
                    .expect("unexpected unset `constructed_at`")
                    .elapsed()
            );

            let obj = self.obj();
            let _guard = obj.freeze_notify();

            let texture = frame.texture();
            let paintable = TexturePaintable::new(&texture);
            self.picture.set_paintable(Some(paintable));
            assert_eq!(self.texture.replace(Some(texture)), None);

            self.is_loading.set(false);
            obj.notify_is_loading();

            obj.notify_resolution();

            if let Some(format_name) = image.format_name() {
                self.container_format.replace(Some(format_name));
                self.obj().notify_container_format();
            }

            self.stack.set_visible_child_name("content");
        }

        /// Prepares the playbin for the file.
        #[instrument("Page::prepare_playbin", skip_all)]
        fn prepare_playbin(&self, file: &gio::File) {
            let obj = self.obj();

            let sink = gst::ElementFactory::make("gtk4paintablesink")
                .build()
                .expect("could not create a `gtk4paintablesink` GStreamer element");
            let paintable = sink.property::<gdk::Paintable>("paintable");

            let sink = if paintable
                .property::<Option<gdk::GLContext>>("gl-context")
                .is_some()
            {
                debug!("paintable has gl-context, creating a glsinkbin");

                match gst::ElementFactory::make("glsinkbin")
                    .property("sink", &sink)
                    .build()
                {
                    Ok(glsinkbin) => glsinkbin,
                    Err(err) => {
                        warn!(
                            "could not create a `glsinkbin` GStreamer element, \
                            using sink as is: {err:?}"
                        );
                        sink
                    }
                }
            } else {
                debug!("paintable does not have gl-context, using sink as is");

                sink
            };

            paintable.set_property("use-scaling-filter", true);

            paintable.connect_invalidate_size(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.notify_resolution();
                }
            ));
            self.picture.set_paintable(Some(paintable));

            let playbin = gst::ElementFactory::make("playbin3")
                .build()
                .expect("could not create a `playbin3` GStreamer element");
            playbin.set_property("video-sink", &sink);
            playbin.set_property("uri", file.uri());

            // Disable audio. Do not use mute or volume properties because they change the global
            // application volume.
            let flags: glib::Value = playbin.property("flags");
            let flags_class =
                glib::FlagsClass::with_type(flags.type_()).expect("could not create `FlagsClass`");
            let flags = flags_class
                .builder_with_value(flags)
                .expect("could not create `FlagsBuilder`")
                .unset_by_nick("audio")
                .unset_by_nick("deinterlace")
                .build()
                .expect("could not create flags `Value`");
            playbin.set_property("flags", flags);

            // Set the playbin property so it can be set to Null on the main thread on dispose.
            assert!(self.playbin.replace(Some(playbin.clone())).is_none());

            // Create a bus message stream.
            let bus = playbin.bus().unwrap();
            let guard = bus
                .add_watch_local(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[upgrade_or]
                    ControlFlow::Break,
                    move |_, msg| {
                        imp.on_bus_message(msg);
                        ControlFlow::Continue
                    }
                ))
                .unwrap();
            assert!(self.bus_watch_guard.replace(Some(guard)).is_none());

            let app = self.application.get().unwrap();
            let va_state = app.va_state();
            bus.set_sync_handler(move |_bus, msg| on_context_msg(&va_state, msg));

            obj.notify_playbin();

            // Pre-roll the playbin by trying to get it to the Paused state.
            let span = info_span!(parent: None, "preroll");
            span.follows_from(Span::current());
            assert!(self.preroll_span.replace(Some(span.clone())).is_none());

            playbin.call_async(|playbin| {
                if let Err(err) = playbin.set_state(gst::State::Paused) {
                    // Can fail when the file is inaccessible.
                    warn!("error setting playbin state to Paused: {err:?}");

                    // We'll get an error on the bus after this where we'll handle it.
                }
            });
        }

        fn on_bus_message(&self, msg: &gst::Message) {
            let Some(playbin) = self.playbin.borrow().clone() else {
                return;
            };
            let obj = self.obj();

            use gst::MessageView;
            match msg.view() {
                MessageView::Error(err) => {
                    // Can fail on missing codecs.
                    warn!(
                        "playbin bus: Error from {:?}: {} ({:?})",
                        err.src(),
                        err.error(),
                        err.debug(),
                    );

                    drop(self.preroll_span.take());

                    // The error does not necessarily bubble up all the way to the playbin
                    // itself, so we must exit unconditionally.
                    self.set_error();
                }
                MessageView::StateChanged(state_changed)
                    if state_changed.src() == Some(playbin.upcast_ref()) =>
                {
                    debug!(
                        "playbin StateChanged old: {:?}, current: {:?}, pending: {:?}",
                        state_changed.old(),
                        state_changed.current(),
                        state_changed.pending(),
                    );

                    if state_changed.current() == gst::State::Paused
                        && state_changed.pending() == gst::State::VoidPending
                    {
                        // Pre-rolled and ready to show.
                        //
                        // Sometimes a missing codec error may arrive a little later (looking at
                        // you, AV1), but due to the multithreaded nature, it's not really possible
                        // to predict. Even spawning the code below into an idle isn't always enough
                        // (the error sometimes arrives even later). The best we can do is keep
                        // listening to this bus to catch the error.
                        if self.is_loading.get() {
                            let _guard = obj.freeze_notify();

                            drop(self.preroll_span.take());

                            debug!(
                                "playbin ready in {:?}",
                                self.constructed_at
                                    .get()
                                    .expect("unexpected unset `constructed_at`")
                                    .elapsed()
                            );

                            self.is_loading.set(false);
                            obj.notify_is_loading();

                            playbin
                                .downcast_ref::<gst::Bin>()
                                .unwrap()
                                .debug_to_dot_file(
                                    gst::DebugGraphDetails::ALL,
                                    format!(
                                        "identity-{}",
                                        self.file
                                            .get()
                                            .unwrap()
                                            .basename()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                    ),
                                );

                            self.refresh_caps_data(&playbin.property("video-sink"));

                            self.stack.set_visible_child_name("content");
                        }
                    }
                }
                MessageView::Tag(tag) => {
                    let tags = tag.tags();
                    debug!("tags: {tags:?}");

                    for (name, value) in tags.iter() {
                        match name.as_str() {
                            "video-codec" => match value.get() {
                                Ok(value) => {
                                    self.video_codec.replace(Some(value));
                                    self.obj().notify_video_codec();
                                }
                                Err(err) => warn!("error retrieving tag value: {err:?}"),
                            },
                            "container-format" => match value.get() {
                                Ok(value) => {
                                    self.container_format.replace(Some(value));
                                    self.obj().notify_container_format();
                                }
                                Err(err) => warn!("error retrieving tag value: {err:?}"),
                            },
                            _ => (),
                        }
                    }
                }
                _ => (),
            }
        }

        fn refresh_caps_data(&self, sink: &gst::Element) {
            let Some(sink_pad) = sink.static_pad("sink") else {
                warn!("unexpected missing sink pad");
                return;
            };

            let Some(caps) = sink_pad.current_caps() else {
                warn!("missing caps on the sink pad");
                return;
            };

            debug!("caps: {caps:?}");

            let size = caps.size();
            if size != 1 {
                warn!("unexpected caps size: {size}");
                return;
            }

            let Some(structure) = caps.structure(0) else {
                warn!("unexpected missing structure at index 0");
                return;
            };

            match structure.get::<gst::Fraction>("framerate") {
                Ok(framerate) => {
                    if framerate.numer() != 0 && framerate.denom() != 0 {
                        self.framerate
                            .set(framerate.numer() as f32 / framerate.denom() as f32);
                        self.obj().notify_framerate();
                    }
                }
                Err(err) => warn!("error getting framerate cap: {err:?}"),
            }
        }
    }

    fn on_context_msg(
        va_state: &(Mutex<VaDisplayState>, Condvar),
        msg: &gst::Message,
    ) -> gst::BusSyncReply {
        use gst::MessageView;
        match msg.view() {
            MessageView::NeedContext(need_context) => {
                let context_type = need_context.context_type();

                if context_type == VA_CONTEXT_TYPE {
                    let display = {
                        let (m, c) = va_state;
                        let mut state = m.lock().unwrap();

                        match &*state {
                            VaDisplayState::Empty => {
                                debug!(
                                    "playbin bus: need VA context; \
                                     this element will create the context"
                                );

                                *state = VaDisplayState::Creating {
                                    creator: msg.src().unwrap().clone(),
                                };
                                return gst::BusSyncReply::Drop;
                            }
                            VaDisplayState::Creating { .. } => loop {
                                debug!(
                                    "playbin bus: need VA context; \
                                     waiting for the context"
                                );

                                state = c.wait(state).unwrap();
                                if let VaDisplayState::Ready { display } = &*state {
                                    break display.clone();
                                }
                            },
                            VaDisplayState::Ready { display } => {
                                debug!(
                                    "playbin bus: need VA context; \
                                     the context is ready"
                                );

                                display.clone()
                            }
                        }
                    };

                    if let Some(display) = display {
                        let mut context = gst::Context::new(context_type, true);
                        {
                            let context = context.get_mut().unwrap();
                            context.structure_mut().set("gst-display", display);
                        }

                        let element = msg.src().unwrap();
                        let element = element.downcast_ref::<gst::Element>().unwrap();
                        element.set_context(&context);
                    }

                    return gst::BusSyncReply::Drop;
                }
            }
            MessageView::HaveContext(have_context) => {
                let context = have_context.context();
                let context_type = context.context_type();

                if context_type == VA_CONTEXT_TYPE {
                    let display = context
                        .structure()
                        .get_optional::<gst::Object>("gst-display")
                        .unwrap();

                    let name = display.as_ref().map(|d| d.name());
                    debug!("playbin bus: have VA context: {name:?}");

                    {
                        let (m, c) = va_state;
                        let mut state = m.lock().unwrap();

                        if let VaDisplayState::Creating { creator } = &*state {
                            if Some(creator) == msg.src() {
                                debug!("this was the creator, notifying others");

                                *state = VaDisplayState::Ready {
                                    display: display.clone(),
                                };
                                c.notify_all();
                            }
                        }
                    }

                    return gst::BusSyncReply::Drop;
                }
            }
            MessageView::Error(error) => {
                let (m, c) = va_state;
                let mut state = m.lock().unwrap();

                if let VaDisplayState::Creating { creator } = &*state {
                    if Some(creator) == msg.src() {
                        warn!(
                            "got error from the VA creator element, notifying others: {} ({:?})",
                            error.error(),
                            error.debug(),
                        );

                        *state = VaDisplayState::Ready { display: None };
                        c.notify_all();
                    }
                }
            }
            _ => (),
        }

        gst::BusSyncReply::Pass
    }
}

glib::wrapper! {
    pub struct Page(ObjectSubclass<imp::Page>) @extends adw::Bin, gtk::Widget;
}

#[gtk::template_callbacks]
impl Page {
    pub fn new(application: &Application, file: &gio::File) -> Self {
        glib::Object::builder()
            .property("application", application)
            .property("file", file)
            .build()
    }

    pub fn set_error(&self) {
        self.imp().set_error();
    }

    #[template_callback]
    fn on_scale_request_changed(&self) {
        self.notify_scale_request();
    }

    #[template_callback]
    fn on_scale_changed(&self) {
        self.notify_scale();
    }

    #[template_callback]
    fn on_h_scroll_pos_notify(&self) {
        self.notify_h_scroll_pos();
    }

    #[template_callback]
    fn on_v_scroll_pos_notify(&self) {
        self.notify_v_scroll_pos();
    }

    #[template_callback]
    fn on_stop_kinetic_scrolling(&self, except_picture: Option<Picture>) {
        self.emit_by_name::<()>("stop-kinetic-scrolling", &[&except_picture]);
    }

    pub fn reset_kinetic_scrolling(&self, except_picture: Option<&Picture>) {
        self.imp().reset_kinetic_scrolling(except_picture);
    }

    pub fn grab_focus_(&self) {
        self.imp().grab_focus_();
    }
}
