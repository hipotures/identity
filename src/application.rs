use gtk::prelude::*;
use gtk::{gio, glib};

use crate::config;
use crate::window::Window;

mod imp {
    use adw::prelude::AdwApplicationExt;
    use adw::subclass::prelude::*;
    use glib::{clone, debug};
    use gtk::subclass::prelude::*;

    use super::*;
    use crate::G_LOG_DOMAIN;

    #[derive(Default)]
    pub struct Application {}

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "IdApplication";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {}

    impl ApplicationImpl for Application {
        fn activate(&self, obj: &Self::Type) {
            debug!("activate");
            self.parent_activate(obj);
            obj.open_new_window();
        }

        fn open(&self, obj: &Self::Type, files: &[gio::File], _hint: &str) {
            debug!(
                "open: {:?}",
                files
                    .iter()
                    .map(|x| x.uri().into())
                    .collect::<Vec<String>>()
            );

            let window = obj.create_new_window();

            for file in files {
                window.open_file(file);
            }
        }

        fn startup(&self, obj: &Self::Type) {
            self.parent_startup(obj);

            obj.style_manager()
                .set_color_scheme(adw::ColorScheme::PreferDark);

            let action = gio::SimpleAction::new("quit", None);
            action.connect_activate(clone!(@weak obj => move |_, _| obj.quit()));
            obj.add_action(&action);
            obj.set_accels_for_action("app.quit", &["<primary>q"]);

            let action = gio::SimpleAction::new("new-window", None);
            action.connect_activate(clone!(@weak obj => move |_, _| { obj.open_new_window(); }));
            obj.add_action(&action);
            obj.set_accels_for_action("app.new-window", &["<primary>n"]);
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}
}

glib::wrapper! {
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Application {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        glib::Object::new(&[
            ("application-id", &config::APP_ID),
            ("flags", &gio::ApplicationFlags::HANDLES_OPEN),
            ("resource-base-path", &"/org/gnome/gitlab/YaLTeR/Identity"),
        ])
        .expect("could not create `Application`")
    }

    pub fn create_new_window(&self) -> Window {
        let window = Window::new(self);

        // Put it in a new window group so modal dialogs don't block other windows.
        let group = gtk::WindowGroup::new();
        group.add_window(&window);

        window
    }

    pub fn open_new_window(&self) -> Window {
        let window = self.create_new_window();

        window.present();

        window
    }
}
