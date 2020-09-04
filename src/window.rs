use gtk::prelude::*;
use libhandy as hdy;

use crate::config::{APP_ID, LOG_DOMAIN, PROFILE};
use crate::window_state;

pub struct Window {
    pub window: hdy::ApplicationWindow,
}

impl Window {
    pub fn new() -> Self {
        let settings = gio::Settings::new(APP_ID);

        let builder = gtk::Builder::from_resource("/org/gnome/gitlab/YaLTeR/Identity/window.ui");
        let window: hdy::ApplicationWindow = builder.get_object("window").unwrap();

        // Devel Profile
        if PROFILE == "Devel" {
            window.get_style_context().add_class("devel");
        }

        // load latest window state
        window_state::load(&window, &settings);

        // save window state on delete event
        window.connect_delete_event(move |window, _| {
            if let Err(err) = window_state::save(&window, &settings) {
                g_warning!(LOG_DOMAIN, "Failed to save window state, {}", err);
            }
            Inhibit(false)
        });

        Window { window }
    }
}
