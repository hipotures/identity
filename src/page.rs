use glib::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};

use crate::picture::Picture;
use crate::scale_request::ScaleRequest;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::marker::PhantomData;
    use std::time::Instant;

    use adw::subclass::prelude::*;
    use futures_util::StreamExt;
    use gettextrs::gettext;
    use glib::subclass::Signal;
    use glib::{clone, Properties};
    use gst::prelude::*;
    use gst_video::VideoOrientationMethod;
    use gtk::prelude::*;
    use gtk::{gdk, CompositeTemplate};
    use once_cell::sync::Lazy;

    use super::*;

    #[derive(Debug, Default, CompositeTemplate, Properties)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/page.ui")]
    #[properties(wrapper_type = super::Page)]
    pub struct Page {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        spinner: TemplateChild<gtk::Spinner>,
        #[template_child]
        picture: TemplateChild<Picture>,
        #[template_child]
        scrolled_window: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        title_label: TemplateChild<gtk::Label>,

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
        // This can be a OnceCell<gst::Element>, but then #[property] assumes it's not nullable.
        #[property(get)]
        playbin: RefCell<Option<gst::Element>>,
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

        constructed_at: OnceCell<Instant>,
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
                klass.add_binding_signal(key, gdk::ModifierType::empty(), "activate", None);
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
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![
                    Signal::builder("activate").action().build(),
                    Signal::builder("stop-kinetic-scrolling")
                        .param_types([super::Picture::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            let obj = self.obj();
            self.parent_constructed();

            self.constructed_at
                .set(Instant::now())
                .expect("unexpected set `constructed_at`");

            self.is_loading.set(true);

            glib::MainContext::default().spawn_local(
                clone!(@to-owned self as imp => async move { imp.retrieve_display_name().await; }),
            );

            glib::MainContext::default().spawn_local(
                clone!(@to-owned self as imp => async move { imp.prepare_playbin().await; }),
            );

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
            gesture.connect_released(clone!(@weak obj => move |gesture, _, _, _| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                obj.emit_by_name::<()>("activate", &[]);
            }));
            obj.add_controller(gesture);
        }

        fn dispose(&self) {
            debug!("Page::dispose");
            if let Some(playbin) = &*self.playbin.borrow() {
                let _ = playbin.set_state(gst::State::Null);
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
            debug!("set_error");

            if self.is_loading.get() {
                error!("set_error: is-loading is true, called too early?");
            }

            let obj = self.obj();
            let _guard = obj.freeze_notify();

            self.is_error.set(true);
            obj.notify_is_error();

            self.stack.set_visible_child_name("error");
            self.spinner.set_spinning(false);

            if let Some(playbin) = &*self.playbin.borrow() {
                if let Err(err) = playbin.set_state(gst::State::Null) {
                    warn!("error setting playbin state to Null: {err:?}");
                }
            }
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
                        Some(gettext!("{} × {}", width, height))
                    } else {
                        None
                    }
                })
                // Translators: "Not applicable" string for the media properties dialog when a
                // given property is unknown or missing (e.g. images don't have frame rate).
                .unwrap_or_else(|| gettext("N/A"))
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

        #[instrument("Page::prepare_playbin", fields(file = self.display_path().unwrap()), skip_all)]
        async fn prepare_playbin(&self) {
            let obj = self.obj();

            let file = self.file.get().expect("unexpected unset `file`");

            // glib::timeout_future_seconds(1).await;

            let sink = gst::ElementFactory::make("gtk4paintablesink")
                .build()
                .expect("could not create a `gtk4paintablesink` GStreamer element");
            let paintable = sink.property::<gdk::Paintable>("paintable");

            let mut glvideoflip = None;
            let sink = if paintable
                .property::<Option<gdk::GLContext>>("gl-context")
                .is_some()
            {
                debug!("paintable has gl-context, creating a glsinkbin");

                match gst::ElementFactory::make("glsinkbin")
                    .property("sink", &sink)
                    .build()
                {
                    Ok(glsinkbin) => {
                        (|| {
                            let mut filter = match gst::ElementFactory::make("glvideoflip")
                                .property("video-direction", VideoOrientationMethod::Auto)
                                .build()
                            {
                                Ok(flip) => flip,
                                Err(err) => {
                                    warn!(
                                        "could not create a `glvideoflip` GStreamer element, \
                                         using regular `videoflip`: {err:?}"
                                    );
                                    return;
                                }
                            };

                            match gst::ElementFactory::make("glshader")
                                .property("fragment", include_str!("premultiply.frag"))
                                .build()
                            {
                                Ok(shader) => {
                                    // Link glvideoflip and glshader together in a bin.
                                    let bin = gst::Bin::new();
                                    bin.add_many(&[&filter, &shader]).unwrap();
                                    gst::Element::link_many(&[&filter, &shader]).unwrap();

                                    let sink_pad = gst::GhostPad::with_target(
                                        &filter.static_pad("sink").unwrap(),
                                    )
                                    .unwrap();
                                    let src_pad = gst::GhostPad::with_target(
                                        &shader.static_pad("src").unwrap(),
                                    )
                                    .unwrap();
                                    bin.add_pad(&sink_pad).unwrap();
                                    bin.add_pad(&src_pad).unwrap();

                                    filter = bin.upcast();
                                }
                                Err(err) => warn!(
                                    "could not create a `glshader` GStreamer element, \
                                     semitransparent media might not display properly: {err:?}"
                                ),
                            }

                            match gst::ElementFactory::make("glfilterbin")
                                .property("filter", filter)
                                .build()
                            {
                                Ok(filter) => glvideoflip = Some(filter),
                                Err(err) => warn!(
                                    "could not create a `glfilterbin` GStreamer element, \
                                     using regular `videoflip`: {err:?}"
                                ),
                            }
                        })();

                        glsinkbin
                    }
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

            paintable.connect_invalidate_size(clone!(@weak obj => move |_| {
                obj.notify_resolution();
            }));
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

            // videoflip takes care of applying the rotation tag.
            let make_videoflip = || match gst::ElementFactory::make("videoflip")
                .property("video-direction", VideoOrientationMethod::Auto)
                .build()
            {
                Ok(flip) => Some(flip),
                Err(err) => {
                    warn!("could not create a `videoflip` GStreamer element: {err:?}");
                    None
                }
            };
            if let Some(videoflip) = glvideoflip.or_else(make_videoflip) {
                playbin.set_property("video-filter", &videoflip);
            }

            if self.preroll(&playbin).await {
                let _guard = obj.freeze_notify();

                debug!(
                    "ready in {:?}",
                    self.constructed_at
                        .get()
                        .expect("unexpected unset `constructed_at`")
                        .elapsed()
                );

                assert_eq!(self.playbin.replace(Some(playbin)), None);
                obj.notify_playbin();

                self.is_loading.set(false);
                obj.notify_is_loading();

                if let Some(sink_pad) = sink.static_pad("sink") {
                    if let Some(caps) = sink_pad.current_caps() {
                        debug!("caps: {caps:?}");

                        let size = caps.size();
                        if size == 1 {
                            if let Some(structure) = caps.structure(0) {
                                match structure.get::<gst::Fraction>("framerate") {
                                    Ok(framerate) => {
                                        if framerate.numer() != 0 && framerate.denom() != 0 {
                                            self.framerate.set(
                                                framerate.numer() as f32 / framerate.denom() as f32,
                                            );
                                            obj.notify_framerate();
                                        }
                                    }
                                    Err(err) => warn!("error getting framerate cap: {err:?}"),
                                }
                            } else {
                                warn!("unexpected missing structure at index 0");
                            }
                        } else {
                            warn!("unexpected caps size: {size}");
                        }
                    } else {
                        warn!("missing caps on the sink pad");
                    }
                } else {
                    warn!("unexpected missing sink pad");
                }

                self.stack.set_visible_child_name("content");
                self.spinner.set_spinning(false);
            } else {
                let _guard = obj.freeze_notify();

                self.is_loading.set(false);
                obj.notify_is_loading();

                self.is_error.set(true);
                obj.notify_is_error();

                self.stack.set_visible_child_name("error");
                self.spinner.set_spinning(false);
            }
        }

        /// Pre-rolls the `playbin`.
        ///
        /// Returns `true` when the `playbin` has been successfully pre-rolled and put in the paused
        /// state, and `false` on error.
        #[instrument("Page::preroll", fields(file = self.display_path().unwrap()), skip_all)]
        async fn preroll(&self, playbin: &gst::Element) -> bool {
            let bus = playbin.bus().unwrap();

            // Create the stream first to not miss any messages.
            let mut stream = bus.stream();

            if let Err(err) = playbin
                .call_async_future(|playbin| playbin.set_state(gst::State::Paused))
                .await
            {
                // Can fail when the file is inaccessible.
                warn!("preroll: error setting playbin state: {err:?}");
                playbin.call_async(|playbin| {
                    let _ = playbin.set_state(gst::State::Null);
                });
                return false;
            }

            loop {
                match stream.next().await.unwrap().view() {
                    gst::MessageView::Error(err) => {
                        // Can fail on missing codecs.
                        warn!("preroll: playbin Error: {err:?}");
                        playbin.call_async(|playbin| {
                            let _ = playbin.set_state(gst::State::Null);
                        });
                        break false;
                    }
                    gst::MessageView::StateChanged(state_changed)
                        if state_changed.src() == Some(playbin.upcast_ref()) =>
                    {
                        debug!(
                            "preroll: playbin StateChanged old: {:?}, current: {:?}, pending: {:?}",
                            state_changed.old(),
                            state_changed.current(),
                            state_changed.pending(),
                        );

                        if state_changed.current() == gst::State::Paused
                            && state_changed.pending() == gst::State::VoidPending
                        {
                            // Pre-rolled and ready to show.
                            break true;
                        }
                    }
                    gst::MessageView::Tag(tag) => {
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
        }
    }
}

glib::wrapper! {
    pub struct Page(ObjectSubclass<imp::Page>) @extends adw::Bin, gtk::Widget;
}

#[gtk::template_callbacks]
impl Page {
    pub fn new(file: &gio::File) -> Self {
        glib::Object::builder().property("file", file).build()
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
