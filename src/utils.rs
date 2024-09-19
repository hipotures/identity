use gettextrs::gettext;
use gtk::gdk;
use gtk::prelude::*;

pub fn shortcut_with_arg(
    keyval: gdk::Key,
    mods: gdk::ModifierType,
    action_name: &str,
    arguments: &glib::Variant,
) -> gtk::Shortcut {
    let shortcut = gtk::Shortcut::new(
        Some(gtk::KeyvalTrigger::new(keyval, mods)),
        Some(gtk::NamedAction::new(action_name)),
    );
    shortcut.set_arguments(Some(arguments));
    shortcut
}

pub fn fractional_scale(widget: &impl WidgetExt) -> f64 {
    if let Some(surface) = widget.native().and_then(|x| x.surface()) {
        surface.scale()
    } else {
        f64::from(widget.scale_factor())
    }
}

fn freplace(mut s: String, args: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    for arg in args {
        s = s.replacen("{}", arg.as_ref(), 1);
    }

    s
}

pub fn gettext_f(format: &str, args: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let s = gettext(format);
    freplace(s, args)
}
