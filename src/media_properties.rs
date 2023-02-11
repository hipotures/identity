use gtk::{gio, glib};

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

        #[property(
            get = |_| self.stack.visible_child_name().unwrap() == "empty",
            set = |_, val: bool| {
                let name = if val { "empty" } else { "content" };
                self.stack.set_visible_child_name(name);
            },
        )]
        show_empty_state: PhantomData<bool>,
        #[property(
            get = |_| self.file_name_row.subtitle(),
            set = |_, val: Option<&str>| self.file_name_row.set_subtitle(val.unwrap_or("")),
        )]
        file_name: PhantomData<Option<glib::GString>>,
        #[property(
            get = |_| self.file_location_row.subtitle(),
            set = |_, val: Option<&str>| self.file_location_row.set_subtitle(val.unwrap_or("")),
        )]
        file_location: PhantomData<Option<glib::GString>>,
        #[property(
            get = |_| self.resolution_row.subtitle(),
            set = |_, val: Option<&str>| self.resolution_row.set_subtitle(val.unwrap_or("")),
        )]
        resolution: PhantomData<Option<glib::GString>>,
        #[property(
            get = |_| self.frame_rate_row.subtitle(),
            set = |_, val: Option<&str>| self.frame_rate_row.set_subtitle(val.unwrap_or("")),
        )]
        frame_rate: PhantomData<Option<glib::GString>>,
        #[property(
            get = |_| self.codec_row.subtitle(),
            set = |_, val: Option<&str>| self.codec_row.set_subtitle(val.unwrap_or("")),
        )]
        codec: PhantomData<Option<glib::GString>>,
        #[property(
            get = |_| self.container_row.subtitle(),
            set = |_, val: Option<&str>| self.container_row.set_subtitle(val.unwrap_or("")),
        )]
        container: PhantomData<Option<glib::GString>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MediaProperties {
        const NAME: &'static str = "IdMediaProperties";
        type Type = super::MediaProperties;
        type ParentType = adw::Window;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();

            klass.add_binding_action(Key::Escape, ModifierType::empty(), "window.close", None);
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
}

glib::wrapper! {
    pub struct MediaProperties(ObjectSubclass<imp::MediaProperties>)
        @extends adw::Window, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap;
}
