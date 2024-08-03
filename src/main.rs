#[macro_use]
extern crate tracing;

use std::env;

use gettextrs::*;
use glib::ExitCode;
use gtk::prelude::*;
use gtk::{gio, glib};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

mod application;
use application::Application;
#[rustfmt::skip]
mod config;
mod media_properties;
mod page;
mod page_grid;
mod picture;
mod player;
mod scale_request;
mod thumbnail_paintable;
mod utils;
mod window;

fn main() -> ExitCode {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let only_message = tracing_subscriber::fmt::format::debug_fn(|writer, field, value| {
        if field.name() == "message" {
            write!(writer, "{value:?}")
        } else {
            Ok(())
        }
    });

    let (chrome_layer, _guard) = if env::var_os("IDENTITY_PROFILE_CHROME").is_some() {
        let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file("trace.json")
            .include_args(true)
            .include_locations(false)
            .trace_style(tracing_chrome::TraceStyle::Async)
            .build();
        (Some(layer), Some(guard))
    } else {
        (None, None)
    };

    let tracy_layer = if std::env::var_os("IDENTITY_PROFILE_TRACY").is_some() {
        Some(tracing_tracy::TracyLayer::default())
    } else {
        None
    };

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .fmt_fields(only_message);

    tracing_subscriber::registry()
        .with(filter)
        .with(chrome_layer)
        .with(tracy_layer)
        .with(fmt_layer)
        .init();

    glib::log_set_default_handler(glib::rust_log_handler);

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

    gst::init().unwrap();
    gstgtk4::plugin_register_static().expect("could not initialize gst-plugin-gtk4");
    gstdav1d::plugin_register_static().expect("could not initialize gst-plugin-dav1d");
    gstrswebp::plugin_register_static().expect("could not initialize gst-plugin-webp");

    Application::new().run()
}
