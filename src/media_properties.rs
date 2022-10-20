use gtk::{gio, glib};

mod imp {
    use adw::prelude::*;
    use adw::subclass::prelude::*;
    use gtk::CompositeTemplate;
    use once_cell::sync::Lazy;

    use super::*;
    use crate::config;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/media_properties.ui")]
    pub struct MediaProperties {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        file_name_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        file_location_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        resolution_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        frame_rate_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        codec_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        container_row: TemplateChild<adw::ActionRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MediaProperties {
        const NAME: &'static str = "IdMediaProperties";
        type Type = super::MediaProperties;
        type ParentType = adw::Window;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MediaProperties {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            if config::PROFILE == "Devel" {
                obj.add_css_class("devel");
            }
        }

        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<[glib::ParamSpec; 7]> = Lazy::new(|| {
                [
                    glib::ParamSpecBoolean::new(
                        "show-empty-state",
                        "",
                        "",
                        true,
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "file-name",
                        "",
                        "",
                        Some(""),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "file-location",
                        "",
                        "",
                        Some(""),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "resolution",
                        "",
                        "",
                        Some(""),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "frame-rate",
                        "",
                        "",
                        Some(""),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "codec",
                        "",
                        "",
                        Some(""),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "container",
                        "",
                        "",
                        Some(""),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
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
                "show-empty-state" => {
                    let value: bool = value.get().unwrap();
                    let name = if value { "empty" } else { "content" };
                    self.stack.set_visible_child_name(name);
                }
                "file-name" => self.file_name_row.set_subtitle(value.get().unwrap_or("")),
                "file-location" => self
                    .file_location_row
                    .set_subtitle(value.get().unwrap_or("")),
                "resolution" => self.resolution_row.set_subtitle(value.get().unwrap_or("")),
                "frame-rate" => self.frame_rate_row.set_subtitle(value.get().unwrap_or("")),
                "codec" => self.codec_row.set_subtitle(value.get().unwrap_or("")),
                "container" => self.container_row.set_subtitle(value.get().unwrap_or("")),
                _ => unimplemented!(),
            }
        }

        fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "show-empty-state" => {
                    (self.stack.visible_child_name().unwrap() == "empty").to_value()
                }
                "file-name" => self.file_name_row.subtitle().to_value(),
                "file-location" => self.file_location_row.subtitle().to_value(),
                "resolution" => self.resolution_row.subtitle().to_value(),
                "frame-rate" => self.frame_rate_row.subtitle().to_value(),
                "codec" => self.codec_row.subtitle().to_value(),
                "container" => self.container_row.subtitle().to_value(),
                _ => unimplemented!(),
            }
        }
    }

    impl WidgetImpl for MediaProperties {}
    impl WindowImpl for MediaProperties {}
    impl AdwWindowImpl for MediaProperties {}
}

glib::wrapper! {
    pub struct MediaProperties(ObjectSubclass<imp::MediaProperties>)
        @extends adw::Window, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap;
}
