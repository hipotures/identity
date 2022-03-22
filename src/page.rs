use gtk::subclass::prelude::*;
use gtk::{gio, glib};

mod imp {
    use std::cell::Cell;
    use std::time::Instant;

    use adw::subclass::prelude::*;
    use futures_util::StreamExt;
    use glib::prelude::*;
    use glib::{clone, debug, error, warn};
    use gst::prelude::*;
    use gst_video::VideoOrientationMethod;
    use gtk::prelude::*;
    use gtk::{gdk, CompositeTemplate};
    use once_cell::sync::Lazy;
    use once_cell::unsync::OnceCell;

    use super::*;
    use crate::G_LOG_DOMAIN;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/page.ui")]
    pub struct Page {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        picture: TemplateChild<gtk::Picture>,

        file: OnceCell<gio::File>,
        playbin: OnceCell<gst::Element>,
        display_name: OnceCell<String>,
        is_loading: Cell<bool>,
        is_error: Cell<bool>,

        constructed_at: OnceCell<Instant>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Page {
        const NAME: &'static str = "IdPage";
        type Type = super::Page;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Page {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<[glib::ParamSpec; 5]> = Lazy::new(|| {
                [
                    glib::ParamSpecObject::new(
                        "file",
                        "file",
                        "file",
                        gio::File::static_type(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
                    ),
                    glib::ParamSpecString::new(
                        "display-name",
                        "display-name",
                        "display-name",
                        None,
                        glib::ParamFlags::READABLE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecBoolean::new(
                        "is-loading",
                        "is-loading",
                        "is-loading",
                        true,
                        glib::ParamFlags::READABLE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecBoolean::new(
                        "is-error",
                        "is-error",
                        "is-error",
                        false,
                        glib::ParamFlags::READABLE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecObject::new(
                        "playbin",
                        "playbin",
                        "playbin",
                        gst::Element::static_type(),
                        glib::ParamFlags::READABLE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                ]
            });

            PROPERTIES.as_ref()
        }

        fn set_property(
            &self,
            _obj: &Self::Type,
            _id: usize,
            value: &glib::Value,
            pspec: &glib::ParamSpec,
        ) {
            match pspec.name() {
                "file" => {
                    let file: gio::File = value
                        .get()
                        .expect("tried to set `file` property to an invalid value");
                    self.file
                        .set(file)
                        .expect("tried to set `file` more than once");
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
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
                "is-loading" => self.is_loading.get().to_value(),
                "is-error" => self.is_error().to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            self.constructed_at
                .set(Instant::now())
                .expect("unexpected set `constructed_at`");

            self.is_loading.set(true);

            glib::MainContext::default().spawn_local(
                clone!(@strong obj => async move { obj.imp().retrieve_display_name(&obj).await; }),
            );

            glib::MainContext::default().spawn_local(
                clone!(@strong obj => async move { obj.imp().prepare_playbin(&obj).await; }),
            );
        }

        fn dispose(&self, _obj: &Self::Type) {
            if let Some(playbin) = self.playbin.get() {
                let _ = playbin.set_state(gst::State::Null);
            }
        }
    }

    impl WidgetImpl for Page {}
    impl BinImpl for Page {}

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

            let obj = self.instance();
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

        async fn retrieve_display_name(&self, obj: &super::Page) {
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
            obj.notify("display-name");
        }

        async fn prepare_playbin(&self, obj: &super::Page) {
            let file = self.file.get().expect("unexpected unset `file`");

            // glib::timeout_future_seconds(1).await;

            let sink = gst::ElementFactory::make("gtk4paintablesink", None)
                .expect("could not create a `gtk4paintablesink` GStreamer element");
            let paintable = sink.property::<gdk::Paintable>("paintable");
            self.picture.set_paintable(Some(&paintable));

            let playbin = gst::ElementFactory::make("playbin3", None)
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
                .build()
                .expect("could not create flags `Value`");
            playbin.set_property("flags", flags);

            // videoflip takes care of applying the rotation tag.
            match gst::ElementFactory::make("videoflip", None) {
                Ok(videoflip) => {
                    videoflip.set_property("video-direction", &VideoOrientationMethod::Auto);
                    playbin.set_property("video-filter", &videoflip);
                }
                Err(err) => warn!("could not create a `videoflip` GStreamer element: {err:?}"),
            }

            if preroll(&playbin).await {
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

                self.stack.set_visible_child(&*self.picture);
            } else {
                let _guard = obj.freeze_notify();

                self.is_loading.set(false);
                obj.notify("is-loading");

                self.is_error.set(true);
                obj.notify("is-error");

                self.stack.set_visible_child_name("error");
            }
        }
    }

    /// Pre-rolls the `playbin`.
    ///
    /// Returns `true` when the `playbin` has been successfully pre-rolled and put in the paused
    /// state, and `false` on error.
    async fn preroll(playbin: &gst::Element) -> bool {
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
                _ => (),
            }
        }
    }
}

glib::wrapper! {
    pub struct Page(ObjectSubclass<imp::Page>) @extends adw::Bin, gtk::Widget;
}

impl Page {
    pub fn new(file: &gio::File) -> Self {
        glib::Object::new(&[("file", file)]).expect("could not create a `Page`")
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
}
