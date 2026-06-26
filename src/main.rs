#[macro_use]
extern crate tracing;

use std::env;
use std::path::{Path, PathBuf};

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
mod glycin;
mod media_properties;
mod page;
mod page_grid;
mod path_recorder;
mod picture;
mod player;
mod scale_request;
mod texture_paintable;
mod thumbnail_paintable;
mod utils;
mod window;

fn main() -> ExitCode {
    // Prefer ngl renderer, unless the user explicitly asks for something else.
    // This is because the Vulkan renderer doesn't properly import textures
    // when using VA-API, which results in constant downloading and uploading
    // of textures to host memory; moreover 'nearest' filtering is also broken
    // with Vulkan.
    //
    // https://gitlab.freedesktop.org/mesa/mesa/-/issues/11629
    // https://gitlab.gnome.org/GNOME/gtk/-/issues/6913
    if env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: "This function is safe to call in a single-threaded program."
        // We call this here before we call into any other library, so we can be
        // quite sure that we're still single-threaded at this point.
        unsafe {
            env::set_var("GSK_RENDERER", "ngl");
        }
    }

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

    let current_exe = env::current_exe().expect("unable to get executable path");
    let resource_path = resources_file_path(
        env::var_os("MESON_DEVENV").is_some(),
        &current_exe,
        config::RESOURCES_FILE,
    );
    let res = gio::Resource::load(&resource_path).unwrap_or_else(|_| {
        panic!(
            "could not load the gresource file from {}",
            resource_path.display()
        )
    });
    gio::resources_register(&res);

    gst::init().unwrap();
    gstgtk4::plugin_register_static().expect("could not initialize gst-plugin-gtk4");
    gstdav1d::plugin_register_static().expect("could not initialize gst-plugin-dav1d");
    gstrswebp::plugin_register_static().expect("could not initialize gst-plugin-webp");

    Application::new().run()
}

fn build_dir_resources_file(exe_path: &Path) -> PathBuf {
    let mut resource_path = exe_path.to_owned();
    resource_path.pop();
    resource_path.pop();
    resource_path.push("data");
    resource_path.push("resources");
    resource_path.push("resources.gresource");
    resource_path
}

fn resources_file_path(
    meson_devenv: bool,
    exe_path: &Path,
    installed_resources_file: &str,
) -> PathBuf {
    let build_resources_file = build_dir_resources_file(exe_path);
    if meson_devenv || build_resources_file.is_file() {
        build_resources_file
    } else {
        PathBuf::from(installed_resources_file)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn direct_build_binary_uses_build_dir_resources_when_available() {
        let root = env::temp_dir().join(format!(
            "identity-resource-path-test-{}",
            std::process::id()
        ));
        let exe = root.join("_build").join("src").join("identity");
        let resources = root
            .join("_build")
            .join("data")
            .join("resources")
            .join("resources.gresource");
        fs::create_dir_all(resources.parent().unwrap()).unwrap();
        fs::write(&resources, []).unwrap();

        assert_eq!(
            resources_file_path(false, &exe, "/usr/local/share/identity/resources.gresource"),
            resources
        );

        fs::remove_dir_all(root).unwrap();
    }
}
