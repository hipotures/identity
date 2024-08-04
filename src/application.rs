use std::sync::{Arc, Condvar, Mutex};

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};

use crate::config;
use crate::window::Window;

#[derive(Default)]
pub enum VaDisplayState {
    #[default]
    Empty,
    /// An element is attempting to create a VA display.
    Creating { creator: gst::Object },
    /// The VA display creation had been attempted.
    Ready { display: Option<gst::Object> },
}

mod imp {
    use adw::prelude::AdwApplicationExt;
    use adw::subclass::prelude::*;

    use super::*;

    #[derive(Default)]
    pub struct Application {
        // GstVaDisplay shared among all playbins.
        va_state: Arc<(Mutex<VaDisplayState>, Condvar)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "IdApplication";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {}

    impl ApplicationImpl for Application {
        fn activate(&self) {
            debug!("activate");
            self.parent_activate();
            self.obj().open_new_window();
        }

        fn open(&self, files: &[gio::File], _hint: &str) {
            debug!(
                "open: {:?}",
                files
                    .iter()
                    .map(|x| x.uri().into())
                    .collect::<Vec<String>>()
            );

            let window = self.obj().create_new_window();

            for file in files {
                window.open_file(file);
            }
        }

        fn startup(&self) {
            self.parent_startup();

            let obj = self.obj();
            obj.style_manager()
                .set_color_scheme(adw::ColorScheme::PreferDark);

            obj.add_action_entries([
                gio::ActionEntry::builder("quit")
                    .activate(|obj: &Self::Type, _, _| obj.quit())
                    .build(),
                gio::ActionEntry::builder("new-window")
                    .activate(|obj: &Self::Type, _, _| drop(obj.open_new_window()))
                    .build(),
            ]);

            obj.set_accels_for_action("app.quit", &["<primary>q"]);
            obj.set_accels_for_action("app.new-window", &["<primary>n"]);
            obj.set_accels_for_action("win.play-pause", &["<primary>space"]);
            obj.set_accels_for_action("win.open", &["<primary>o"]);
            obj.set_accels_for_action("win.close-tab", &["<primary>w"]);
            obj.set_accels_for_action("win.zoom-in", &["<primary>plus"]);
            obj.set_accels_for_action("win.zoom-out", &["<primary>minus"]);
            obj.set_accels_for_action("win.set-scale-request(1.)", &["<primary>0"]);
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}

    impl Application {
        pub fn va_state(&self) -> Arc<(Mutex<VaDisplayState>, Condvar)> {
            self.va_state.clone()
        }
    }
}

glib::wrapper! {
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Application {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", config::APP_ID)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .property("resource-base-path", "/org/gnome/gitlab/YaLTeR/Identity")
            .build()
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

    pub fn va_state(&self) -> Arc<(Mutex<VaDisplayState>, Condvar)> {
        self.imp().va_state()
    }
}
