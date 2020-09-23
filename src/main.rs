#[macro_use]
extern crate glib;

use gettextrs::*;
use gstreamer as gst;

mod application;
#[rustfmt::skip]
mod config;
#[rustfmt::skip]
mod static_resources;
mod window;

use application::Application;
use config::{GETTEXT_PACKAGE, LOCALEDIR};

fn main() {
    // Required for GStreamer on X11.
    #[cfg(target_os = "linux")]
    unsafe {
        #[link(name = "X11")]
        extern "C" {
            fn XInitThreads() -> std::os::raw::c_int;
        }

        XInitThreads();
    }

    // Prepare i18n
    setlocale(LocaleCategory::LcAll, "");
    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR);
    textdomain(GETTEXT_PACKAGE);

    glib::set_application_name(&format!("Identity{}", config::NAME_SUFFIX));
    glib::set_prgname(Some("identity"));

    gst::init().expect("Unable to start GStreamer");
    gtk::init().expect("Unable to start GTK3");

    static_resources::init().expect("Failed to initialize the resource file.");

    let app = Application::new();
    app.run();
}
