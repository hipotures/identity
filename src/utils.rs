use gtk::gdk;

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
