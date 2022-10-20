use gtk::{gio, glib};

mod imp {
    use adw::subclass::prelude::*;
    use gtk::prelude::*;
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
        file_name_label: TemplateChild<gtk::Label>,
        #[template_child]
        file_location_label: TemplateChild<gtk::Label>,
        #[template_child]
        resolution_label: TemplateChild<gtk::Label>,
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
            static PROPERTIES: Lazy<[glib::ParamSpec; 4]> = Lazy::new(|| {
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
                "file-name" => self.file_name_label.set_text(value.get().unwrap_or("")),
                "file-location" => self.file_location_label.set_text(value.get().unwrap_or("")),
                "resolution" => self.resolution_label.set_text(value.get().unwrap_or("")),
                _ => unimplemented!(),
            }
        }

        fn property(&self, _obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "show-empty-state" => {
                    (self.stack.visible_child_name().unwrap() == "empty").to_value()
                }
                "file-name" => self.file_name_label.text().to_value(),
                "file-location" => self.file_location_label.text().to_value(),
                "resolution" => self.resolution_label.text().to_value(),
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
