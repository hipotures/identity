use gio::prelude::*;
use gtk::prelude::*;
use libhandy as hdy;
use std::{env, rc::Rc};

use crate::config;
use crate::window::Window;

pub struct Application {
    app: gtk::Application,
    window: Rc<Window>,
}

impl Application {
    pub fn new() -> Self {
        let app = gtk::Application::new(
            Some(config::APP_ID),
            gio::ApplicationFlags::NON_UNIQUE | gio::ApplicationFlags::HANDLES_OPEN,
        )
        .unwrap();
        let window = Window::new();

        let application = Self { app, window };

        application.setup_widgets();
        application.setup_gactions();
        application.setup_signals();
        application
    }

    fn setup_widgets(&self) {
        let builder = gtk::Builder::from_resource("/org/gnome/gitlab/YaLTeR/Identity/shortcuts.ui");
        let shortcuts: gtk::ShortcutsWindow = builder.get_object("shortcuts").unwrap();
        self.window.window.set_help_overlay(Some(&shortcuts));
    }

    fn setup_gactions(&self) {
        // Quit
        let action = gio::SimpleAction::new("quit", None);
        action.connect_activate({
            let app = self.app.downgrade();
            move |_, _| {
                let app = app.upgrade().unwrap();
                app.quit();
            }
        });
        self.app.add_action(&action);
        self.app.set_accels_for_action("app.quit", &["<primary>q"]);

        // About
        let action = gio::SimpleAction::new("about", None);
        action.connect_activate({
            let window = self.window.window.downgrade();
            move |_, _| {
                let window = window.upgrade().unwrap();
                let builder = gtk::Builder::from_resource(
                    "/org/gnome/gitlab/YaLTeR/Identity/about_dialog.ui",
                );
                let about_dialog: gtk::AboutDialog = builder.get_object("about_dialog").unwrap();
                about_dialog.set_transient_for(Some(&window));

                about_dialog.connect_response(|dialog, _| dialog.close());
                about_dialog.show();
            }
        });
        self.app.add_action(&action);
        self.app
            .set_accels_for_action("win.show-help-overlay", &["<primary>question"]);

        // Switching pages
        for i in 0..10 {
            let page = if i == 0 { 10 } else { i };
            let action_name = format!("switch-to-page-{}", page);
            let action = gio::SimpleAction::new(&action_name, None);
            action.connect_activate({
                let window = Rc::downgrade(&self.window);
                move |_, _| {
                    let window = window.upgrade().unwrap();
                    window.set_visible_child(page);
                }
            });
            self.app.add_action(&action);
            self.app
                .set_accels_for_action(&format!("app.{}", action_name), &[i.to_string().as_ref()]);
        }

        // Open
        let action = gio::SimpleAction::new("open", None);
        action.connect_activate({
            let window = Rc::downgrade(&self.window);
            move |_, _| {
                let window = window.upgrade().unwrap();
                window.show_open_dialog();
            }
        });
        self.app.add_action(&action);
        self.app.set_accels_for_action("app.open", &["<primary>o"]);
    }

    fn setup_signals(&self) {
        self.app.connect_startup(|_| hdy::init());
        self.app.connect_open({
            let window = Rc::downgrade(&self.window);
            move |app, files, _| {
                let window = window.upgrade().unwrap();

                for file in files {
                    window.add_file(file);
                }

                app.activate();
            }
        });
        self.app.connect_activate({
            let window = self.window.window.downgrade();
            move |app| {
                let window = window.upgrade().unwrap();
                window.set_application(Some(app));
                app.add_window(&window);
                window.show_all();
            }
        });
    }

    pub fn run(&self) {
        g_debug!(
            config::LOG_DOMAIN,
            "Identity{} ({})",
            config::NAME_SUFFIX,
            config::APP_ID
        );
        g_debug!(
            config::LOG_DOMAIN,
            "Version: {} ({})",
            config::VERSION,
            config::PROFILE
        );
        g_debug!(config::LOG_DOMAIN, "Datadir: {}", config::PKGDATADIR);

        let args: Vec<String> = env::args().collect();
        self.app.run(&args);
    }
}
