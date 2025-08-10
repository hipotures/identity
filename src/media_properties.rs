use gtk::glib;

mod imp {
    use std::marker::PhantomData;

    use adw::prelude::*;
    use adw::subclass::prelude::*;
    use glib::Properties;
    use gtk::gdk::{Key, ModifierType};
    use gtk::CompositeTemplate;

    use super::*;
    use crate::config;

    #[derive(Debug, Default, CompositeTemplate, Properties)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/media_properties.ui")]
    #[properties(wrapper_type = super::MediaProperties)]
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

        #[property(get = Self::show_empty_state, set = Self::set_show_empty_state)]
        show_empty_state: PhantomData<bool>,
        #[property(get = Self::file_name, set = Self::set_file_name)]
        file_name: PhantomData<Option<glib::GString>>,
        #[property(get = Self::file_location, set = Self::set_file_location)]
        file_location: PhantomData<Option<glib::GString>>,
        #[property(get = Self::resolution, set = Self::set_resolution)]
        resolution: PhantomData<Option<glib::GString>>,
        #[property(get = Self::frame_rate, set = Self::set_frame_rate)]
        frame_rate: PhantomData<Option<glib::GString>>,
        #[property(get = Self::codec, set = Self::set_codec)]
        codec: PhantomData<Option<glib::GString>>,
        #[property(get = Self::container, set = Self::set_container)]
        container: PhantomData<Option<glib::GString>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MediaProperties {
        const NAME: &'static str = "IdMediaProperties";
        type Type = super::MediaProperties;
        type ParentType = adw::Window;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();

            klass.add_binding_action(Key::Escape, ModifierType::empty(), "window.close");
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MediaProperties {
        fn constructed(&self) {
            self.parent_constructed();

            if config::PROFILE == "Devel" {
                self.obj().add_css_class("devel");
            }
        }

        fn properties() -> &'static [glib::ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            self.derived_set_property(id, value, pspec);
        }

        fn property(&self, id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            self.derived_property(id, pspec)
        }
    }

    impl WidgetImpl for MediaProperties {}
    impl WindowImpl for MediaProperties {}
    impl AdwWindowImpl for MediaProperties {}

    impl MediaProperties {
        fn show_empty_state(&self) -> bool {
            self.stack.visible_child_name().unwrap() == "empty"
        }

        fn set_show_empty_state(&self, val: bool) {
            let name = if val { "empty" } else { "content" };
            self.stack.set_visible_child_name(name);
        }

        fn file_name(&self) -> Option<glib::GString> {
            self.file_name_row.subtitle()
        }

        fn set_file_name(&self, val: Option<&str>) {
            self.file_name_row.set_subtitle(val.unwrap_or(""))
        }

        fn file_location(&self) -> Option<glib::GString> {
            self.file_location_row.subtitle()
        }

        fn set_file_location(&self, val: Option<&str>) {
            self.file_location_row.set_subtitle(val.unwrap_or(""))
        }

        fn resolution(&self) -> Option<glib::GString> {
            self.resolution_row.subtitle()
        }

        fn set_resolution(&self, val: Option<&str>) {
            self.resolution_row.set_subtitle(val.unwrap_or(""))
        }

        fn frame_rate(&self) -> Option<glib::GString> {
            self.frame_rate_row.subtitle()
        }

        fn set_frame_rate(&self, val: Option<&str>) {
            self.frame_rate_row.set_subtitle(val.unwrap_or(""))
        }

        fn codec(&self) -> Option<glib::GString> {
            self.codec_row.subtitle()
        }

        fn set_codec(&self, val: Option<&str>) {
            self.codec_row.set_subtitle(val.unwrap_or(""))
        }

        fn container(&self) -> Option<glib::GString> {
            self.container_row.subtitle()
        }

        fn set_container(&self, val: Option<&str>) {
            self.container_row.set_subtitle(val.unwrap_or(""))
        }
    }
}

glib::wrapper! {
    pub struct MediaProperties(ObjectSubclass<imp::MediaProperties>)
        @extends adw::Window, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
            gtk::Native, gtk::Root, gtk::ShortcutManager;
}
