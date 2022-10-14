use std::env;

use gettextrs::*;
use glib::{info, warn, GlibLogger, GlibLoggerDomain, GlibLoggerFormat};
use gtk::prelude::*;
use gtk::{gio, glib};

mod application;
use application::Application;
#[rustfmt::skip]
mod config;
mod page;
mod picture;
mod window;

const G_LOG_DOMAIN: &str = "Identity";

fn main() {
    static GLIB_LOGGER: GlibLogger =
        GlibLogger::new(GlibLoggerFormat::LineAndFile, GlibLoggerDomain::CrateTarget);

    let _ = log::set_logger(&GLIB_LOGGER);
    log::set_max_level(log::LevelFilter::Debug);

    info!("Identity version {}", config::VERSION);

    setlocale(LocaleCategory::LcAll, "");
    if let Err(err) = bindtextdomain(config::GETTEXT_PACKAGE, config::LOCALEDIR) {
        warn!("Error in bindtextdomain(): {}", err);
    }
    if let Err(err) = bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8") {
        warn!("Error in bind_textdomain_codeset(): {}", err);
    }
    if let Err(err) = textdomain(config::GETTEXT_PACKAGE) {
        warn!("Error in textdomain(): {}", err);
    }

    glib::set_application_name(&format!("{}{}", gettext("Identity"), config::NAME_SUFFIX));

    let res = match env::var("MESON_DEVENV") {
        Err(_) => {
            gio::Resource::load(config::RESOURCES_FILE).expect("could not load the gresource file")
        }
        Ok(_) => {
            let mut resource_path = env::current_exe().expect("unable to get executable path");
            resource_path.pop();
            resource_path.pop();
            resource_path.push("data");
            resource_path.push("resources");
            resource_path.push("resources.gresource");
            gio::Resource::load(&resource_path)
                .expect("unable to load resources.gresource from build dir")
        }
    };
    gio::resources_register(&res);

    gst::init().expect("could not initialize GStreamer");
    gstgtk4::plugin_register_static().expect("could not initialize gst-plugin-gtk4");

    std::process::exit(Application::new().run());
}
