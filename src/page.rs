use glib::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};

use crate::picture::ScaleRequest;

mod imp {
    use std::cell::{Cell, RefCell};
    use std::time::Instant;

    use adw::subclass::prelude::*;
    use futures_util::StreamExt;
    use gettextrs::gettext;
    use glib::{clone, debug, error, warn};
    use gst::prelude::*;
    use gst_video::VideoOrientationMethod;
    use gtk::prelude::*;
    use gtk::{gdk, CompositeTemplate};
    use once_cell::sync::Lazy;
    use once_cell::unsync::OnceCell;

    use super::*;
    use crate::picture::Picture;
    use crate::G_LOG_DOMAIN;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/page.ui")]
    pub struct Page {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        picture: TemplateChild<Picture>,
        #[template_child]
        scrolled_window: TemplateChild<gtk::ScrolledWindow>,

        file: OnceCell<gio::File>,
        playbin: OnceCell<gst::Element>,
        display_name: OnceCell<String>,
        is_loading: Cell<bool>,
        is_error: Cell<bool>,
        framerate: Cell<f32>,
        video_codec: RefCell<Option<String>>,
        container_format: RefCell<Option<String>>,

        constructed_at: OnceCell<Instant>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Page {
        const NAME: &'static str = "IdPage";
        type Type = super::Page;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
            Self::Type::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Page {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<[glib::ParamSpec; 14]> = Lazy::new(|| {
                [
                    glib::ParamSpecObject::builder::<gio::File>("file")
                        .construct_only()
                        .build(),
                    glib::ParamSpecString::builder("display-name")
                        .read_only()
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecString::builder("path")
                        .read_only()
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecBoolean::builder("is-loading")
                        .default_value(true)
                        .read_only()
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecBoolean::builder("is-error")
                        .read_only()
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecObject::builder::<gst::Element>("playbin")
                        .read_only()
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecDouble::builder("scale-request")
                        .minimum(0.)
                        .maximum(10.)
                        .build(),
                    glib::ParamSpecDouble::builder("scale")
                        .minimum(0.)
                        .read_only()
                        .build(),
                    glib::ParamSpecDouble::builder("h-scroll-pos")
                        .minimum(0.)
                        .maximum(1.)
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecDouble::builder("v-scroll-pos")
                        .minimum(0.)
                        .maximum(1.)
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecString::builder("resolution")
                        .read_only()
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecFloat::builder("framerate")
                        .minimum(0.)
                        .read_only()
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecString::builder("video-codec")
                        .read_only()
                        .explicit_notify()
                        .build(),
                    glib::ParamSpecString::builder("container-format")
                        .read_only()
                        .explicit_notify()
                        .build(),
                ]
            });

            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "file" => {
                    let file: gio::File = value
                        .get()
                        .expect("tried to set `file` property to an invalid value");
                    self.file
                        .set(file)
                        .expect("tried to set `file` more than once");
                }
                "scale-request" => self.picture.set_property("scale-request", value),
                "h-scroll-pos" => {
                    let value: f64 = value.get().expect("invalid `h-scroll-pos` value type");
                    self.set_h_scroll_pos(value);
                }
                "v-scroll-pos" => {
                    let value: f64 = value.get().expect("invalid `v-scroll-pos` value type");
                    self.set_v_scroll_pos(value);
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "file" => self.file.get().to_value(),
                "playbin" => self.playbin().to_value(),
                "display-name" => {
                    if let Some(display_name) = self.display_name.get() {
                        display_name.to_value()
                    } else {
                        self.file.get().map(|file| file.uri()).to_value()
                    }
                }
                "path" => self
                    .file
                    .get()
                    .map(|file| {
                        file.path()
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_else(|| file.uri().into())
                    })
                    .to_value(),
                "is-loading" => self.is_loading.get().to_value(),
                "is-error" => self.is_error().to_value(),
                "scale-request" => self.picture.property("scale-request"),
                "scale" => self.picture.property("scale"),
                "h-scroll-pos" => self.h_scroll_pos().to_value(),
                "v-scroll-pos" => self.v_scroll_pos().to_value(),
                "resolution" => self
                    .picture
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
                    .to_value(),
                "framerate" => self.framerate.get().to_value(),
                "video-codec" => self.video_codec.borrow().to_value(),
                "container-format" => self.container_format.borrow().to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self) {
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
        }

        fn dispose(&self) {
            if let Some(playbin) = self.playbin.get() {
                let _ = playbin.set_state(gst::State::Null);
            }
        }
    }

    impl WidgetImpl for Page {}
    impl BinImpl for Page {}

    #[gtk::template_callbacks]
    impl Page {
        pub fn playbin(&self) -> Option<&gst::Element> {
            self.playbin.get()
        }

        pub fn is_error(&self) -> bool {
            self.is_error.get()
        }

        pub fn set_error(&self) {
            debug!("set_error");

            if self.is_loading.get() {
                error!("set_error: is-loading is true, called too early?");
            }

            let obj = self.obj();
            let _guard = obj.freeze_notify();

            self.is_error.set(true);
            obj.notify("is-error");

            self.stack.set_visible_child_name("error");

            if let Some(playbin) = self.playbin.get() {
                if let Err(err) = playbin.set_state(gst::State::Null) {
                    warn!("error setting playbin state to Null: {err:?}");
                }
            }
        }

        pub fn scale_request(&self) -> ScaleRequest {
            self.picture.scale_request()
        }

        pub fn set_scale_request(&self, scale_request: ScaleRequest) {
            self.picture.set_scale_request(scale_request);
        }

        pub fn scale(&self) -> f64 {
            self.picture.scale()
        }

        pub fn h_scroll_pos(&self) -> f64 {
            self.picture.h_scroll_pos()
        }

        pub fn set_h_scroll_pos(&self, value: f64) {
            self.picture.set_h_scroll_pos(value);
        }

        pub fn v_scroll_pos(&self) -> f64 {
            self.picture.v_scroll_pos()
        }

        pub fn set_v_scroll_pos(&self, value: f64) {
            self.picture.set_v_scroll_pos(value);
        }

        pub fn reset_kinetic_scrolling(&self) {
            self.scrolled_window.set_kinetic_scrolling(false);
            self.scrolled_window.set_kinetic_scrolling(true);
        }

        pub fn grab_focus_(&self) {
            self.scrolled_window.grab_focus();
        }

        async fn retrieve_display_name(&self) {
            let file = self.file.get().expect("unexpected unset `file`");

            // glib::timeout_future_seconds(1).await;

            let info = file
                .query_info_future(
                    "standard::display-name",
                    gio::FileQueryInfoFlags::NONE,
                    glib::PRIORITY_DEFAULT,
                )
                .await;

            let name = match info {
                Ok(info) => info.display_name(),
                Err(err) => {
                    warn!("error retrieving file display name: {err:?}");
                    return;
                }
            };

            self.display_name
                .set(name.into())
                .expect("`display_name` set more than once");
            self.obj().notify("display-name");
        }

        async fn prepare_playbin(&self) {
            let obj = self.obj();

            let file = self.file.get().expect("unexpected unset `file`");

            // glib::timeout_future_seconds(1).await;

            let sink = gst::ElementFactory::make("gtk4paintablesink")
                .build()
                .expect("could not create a `gtk4paintablesink` GStreamer element");
            let paintable = sink.property::<gdk::Paintable>("paintable");
            paintable.connect_invalidate_size(clone!(@weak obj => move |_| {
                obj.notify("resolution");
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
                glib::FlagsClass::new(flags.type_()).expect("could not create `FlagsClass`");
            let flags = flags_class
                .builder_with_value(flags)
                .expect("could not create `FlagsBuilder`")
                .unset_by_nick("audio")
                .unset_by_nick("deinterlace")
                .build()
                .expect("could not create flags `Value`");
            playbin.set_property("flags", flags);

            // videoflip takes care of applying the rotation tag.
            match gst::ElementFactory::make("videoflip").build() {
                Ok(videoflip) => {
                    videoflip.set_property("video-direction", &VideoOrientationMethod::Auto);
                    playbin.set_property("video-filter", &videoflip);
                }
                Err(err) => warn!("could not create a `videoflip` GStreamer element: {err:?}"),
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

                self.playbin
                    .set(playbin)
                    .expect("trying to set `playbin` more than once");
                obj.notify("playbin");

                self.is_loading.set(false);
                obj.notify("is-loading");

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
                                            obj.notify("framerate");
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
            } else {
                let _guard = obj.freeze_notify();

                self.is_loading.set(false);
                obj.notify("is-loading");

                self.is_error.set(true);
                obj.notify("is-error");

                self.stack.set_visible_child_name("error");
            }
        }

        /// Pre-rolls the `playbin`.
        ///
        /// Returns `true` when the `playbin` has been successfully pre-rolled and put in the paused
        /// state, and `false` on error.
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
                        if state_changed.src().as_ref() == Some(playbin.upcast_ref()) =>
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
                            match name {
                                "video-codec" => match value.get() {
                                    Ok(value) => {
                                        self.video_codec.replace(Some(value));
                                        self.obj().notify("video-codec");
                                    }
                                    Err(err) => warn!("error retrieving tag value: {err:?}"),
                                },
                                "container-format" => match value.get() {
                                    Ok(value) => {
                                        self.container_format.replace(Some(value));
                                        self.obj().notify("container-format");
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

    pub fn playbin(&self) -> Option<&gst::Element> {
        self.imp().playbin()
    }

    pub fn is_error(&self) -> bool {
        self.imp().is_error()
    }

    pub fn set_error(&self) {
        self.imp().set_error();
    }

    pub fn scale_request(&self) -> ScaleRequest {
        self.imp().scale_request()
    }

    pub fn set_scale_request(&self, scale_request: ScaleRequest) {
        self.imp().set_scale_request(scale_request);
    }

    pub fn scale(&self) -> f64 {
        self.imp().scale()
    }

    #[template_callback]
    fn on_scale_request_changed(&self) {
        self.notify("scale-request");
    }

    #[template_callback]
    fn on_scale_changed(&self) {
        self.notify("scale");
    }

    pub fn h_scroll_pos(&self) -> f64 {
        self.imp().h_scroll_pos()
    }

    pub fn set_h_scroll_pos(&self, value: f64) {
        self.imp().set_h_scroll_pos(value);
    }

    #[template_callback]
    fn on_h_scroll_pos_notify(&self) {
        self.notify("h-scroll-pos");
    }

    pub fn v_scroll_pos(&self) -> f64 {
        self.imp().v_scroll_pos()
    }

    pub fn set_v_scroll_pos(&self, value: f64) {
        self.imp().set_v_scroll_pos(value);
    }

    #[template_callback]
    fn on_v_scroll_pos_notify(&self) {
        self.notify("v-scroll-pos");
    }

    pub fn reset_kinetic_scrolling(&self) {
        self.imp().reset_kinetic_scrolling();
    }

    pub fn grab_focus_(&self) {
        self.imp().grab_focus_();
    }
}
