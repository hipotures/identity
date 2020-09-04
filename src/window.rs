use gtk::prelude::*;
use libhandy as hdy;

use crate::config::{APP_ID, LOG_DOMAIN, PROFILE};
use crate::window_state;

pub struct Window {
    pub widget: hdy::ApplicationWindow,
    settings: gio::Settings,
}

impl Window {
    pub fn new() -> Self {
        let settings = gio::Settings::new(APP_ID);

        let builder = gtk::Builder::from_resource("/org/gnome/gitlab/YaLTeR/Identity/window.ui");
        let window: hdy::ApplicationWindow = builder.get_object("window").unwrap();

        let window_widget = Window {
            widget: window,
            settings,
        };

        window_widget.init();
        window_widget
    }

    fn init(&self) {
        // Devel Profile
        if PROFILE == "Devel" {
            self.widget.get_style_context().add_class("devel");
        }

        // load latest window state
        window_state::load(&self.widget, &self.settings);

        // save window state on delete event
        self.widget.connect_delete_event({
            let settings = self.settings.clone();
            move |window, _| {
                if let Err(err) = window_state::save(&window, &settings) {
                    g_warning!(LOG_DOMAIN, "Failed to save window state, {}", err);
                }
                Inhibit(false)
            }
        });
    }
}
