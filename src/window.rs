use std::cell::RefCell;

use gettextrs::gettext;
use glib::{debug, warn};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio};

use crate::G_LOG_DOMAIN;

// Copied from Loupe.
const HOTKEY_SCALE_FACTOR: f64 = 1.5;

/// MIME types for the file chooser filter.
const MIME_TYPES: &[&str] = &[
    "image/bmp",
    "image/jpeg",
    "image/jpg",
    "image/pjpeg",
    "image/png",
    "image/tiff",
    "image/x-bmp",
    "image/x-gray",
    "image/x-icb",
    "image/x-ico",
    "image/x-png",
    "image/x-portable-anymap",
    "image/x-portable-bitmap",
    "image/x-portable-graymap",
    "image/x-portable-pixmap",
    "image/x-xbitmap",
    "image/x-xpixmap",
    "image/x-pcx",
    "image/svg+xml",
    "image/svg+xml-compressed",
    "image/vnd.wap.wbmp",
    "image/webp",
    "image/x-icns",
    "video/3gp",
    "video/3gpp",
    "video/3gpp2",
    "video/dv",
    "video/divx",
    "video/fli",
    "video/flv",
    "video/mp2t",
    "video/mp4",
    "video/mp4v-es",
    "video/mpeg",
    "video/mpeg-system",
    "video/msvideo",
    "video/ogg",
    "video/quicktime",
    "video/vivo",
    "video/vnd.divx",
    "video/vnd.mpegurl",
    "video/vnd.rn-realvideo",
    "video/vnd.vivo",
    "video/webm",
    "video/x-anim",
    "video/x-avi",
    "video/x-flc",
    "video/x-fli",
    "video/x-flic",
    "video/x-flv",
    "video/x-m4v",
    "video/x-matroska",
    "video/x-mjpeg",
    "video/x-mpeg",
    "video/x-mpeg2",
    "video/x-ms-asf",
    "video/x-ms-asf-plugin",
    "video/x-ms-asx",
    "video/x-msvideo",
    "video/x-ms-wm",
    "video/x-ms-wmv",
    "video/x-ms-wvx",
    "video/x-nsv",
    "video/x-ogm+ogg",
    "video/x-theora",
    "video/x-theora+ogg",
];

mod imp {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::fs::File;
    use std::marker::PhantomData;
    use std::str::FromStr;
    use std::time::Duration;

    use adw::subclass::prelude::*;
    use ashpd::desktop::open_uri::OpenDirectoryRequest;
    use glib::{clone, closure, error, Properties, SignalHandlerId, SourceId};
    use gst::prelude::*;
    use gtk::gdk::{self, Key, ModifierType};
    use gtk::{glib, CompositeTemplate};

    use super::*;
    use crate::application::Application;
    use crate::config;
    use crate::media_properties::MediaProperties;
    use crate::page::Page;
    use crate::page_grid::PageGrid;
    use crate::picture::Picture;
    use crate::player::Player;
    use crate::scale_request::ScaleRequest;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    enum DisplayMode {
        #[default]
        Tabbed,
        Row,
        Column,
    }

    impl ToString for DisplayMode {
        fn to_string(&self) -> String {
            match self {
                Self::Tabbed => "tabbed",
                Self::Row => "row",
                Self::Column => "column",
            }
            .to_string()
        }
    }

    impl FromStr for DisplayMode {
        type Err = ();

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "tabbed" => Ok(Self::Tabbed),
                "row" => Ok(Self::Row),
                "column" => Ok(Self::Column),
                _ => Err(()),
            }
        }
    }

    #[derive(Debug, Default, CompositeTemplate, Properties)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/window.ui")]
    #[properties(wrapper_type = super::Window)]
    pub struct Window {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        tab_view: TemplateChild<adw::TabView>,
        #[template_child]
        page_grid: TemplateChild<PageGrid>,
        #[template_child]
        controls_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        play_pause_button: TemplateChild<gtk::Button>,
        #[template_child]
        time_label: TemplateChild<gtk::Label>,
        #[template_child]
        time_scale: TemplateChild<gtk::Scale>,
        #[template_child]
        time_adjustment: TemplateChild<gtk::Adjustment>,
        #[template_child]
        scale_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        scale_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        media_properties: TemplateChild<MediaProperties>,
        #[template_child]
        tabbed_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        row_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        column_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        display_mode_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        display_mode_selector: TemplateChild<gtk::Widget>,
        #[template_child]
        primary_menu_button_content: TemplateChild<gtk::MenuButton>,

        #[property(get, set)]
        is_playing: Cell<bool>,

        player: Player,

        page_bindings: RefCell<HashMap<Page, Vec<glib::Binding>>>,
        page_is_loading_notify_id: RefCell<HashMap<Page, SignalHandlerId>>,
        page_stop_kinetic_scrolling_id: RefCell<HashMap<Page, SignalHandlerId>>,
        switch_to_content_source_id: RefCell<Option<SourceId>>,

        #[property(type = String, get = Self::display_mode_str, set = Self::set_display_mode_str, explicit_notify)]
        display_mode: Cell<DisplayMode>,
        // Set to true during a display mode transition, disables on_tab_page_attached/detached
        // handlers, because we're not detaching and attaching pages to the window, but merely
        // moving them from one container to another.
        in_display_mode_transition: Cell<bool>,

        #[property(get, set = Self::set_scale_request, explicit_notify, minimum = 0., maximum = 10.)]
        scale_request: Cell<ScaleRequest>,

        // I like single lines and rustfmt ignores this attribute so I declare this one as allowed.
        #[property(get = Self::best_fit, set = Self::set_best_fit, default_value = true, explicit_notify)]
        best_fit: PhantomData<bool>,

        #[property(get, set = Self::set_h_scroll_pos, explicit_notify, minimum = 0., maximum = 1.)]
        h_scroll_pos: Cell<f64>,
        #[property(get, set = Self::set_v_scroll_pos, explicit_notify, minimum = 0., maximum = 1.)]
        v_scroll_pos: Cell<f64>,

        #[property(get, set = Self::set_selected_page, explicit_notify)]
        selected_page: RefCell<Option<Page>>,

        /// If a tab menu is open for a page, this is that page, otherwise `None`.
        menu_page: RefCell<glib::WeakRef<adw::TabPage>>,

        last_scale_factor: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "IdWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
            klass.bind_template_instance_callbacks();

            klass.install_property_action("win.play-pause", "is-playing");
            klass.install_action("win.open", None, |obj, _, _| obj.on_open_clicked());
            klass.install_action_async("win.paste", None, |obj, _, _| async move {
                obj.paste().await;
            });
            klass.install_action("win.copy", None, |obj, _, _| obj.imp().copy_file());
            klass.install_action_async("win.show-in-files", None, |obj, _, _| async move {
                obj.imp().show_in_files().await;
            });
            klass.install_action("win.close-tab", None, |obj, _, _| obj.imp().close_tab());
            klass.install_action("win.move-tab-to-new-window", None, |obj, _, _| {
                obj.imp().move_tab_to_new_window()
            });
            klass.install_action("win.step-forward", None, |obj, _, _| {
                obj.imp().player.step_forward()
            });
            klass.install_action("win.step-back", None, |obj, _, _| {
                obj.imp().player.step_back()
            });

            klass.install_action(
                "win.focus-tab",
                Some(i32::static_variant_type().as_str()),
                |obj, _, param| {
                    let index = param
                        .expect("missing parameter")
                        .get()
                        .expect("wrong parameter type");
                    obj.imp().focus_tab(index);
                },
            );

            for i in 0..10 {
                klass.add_binding_action(
                    Key::from_name(&format!("{i}")).unwrap(),
                    ModifierType::empty(),
                    "win.focus-tab",
                    Some(&((i + 9) % 10).to_variant()),
                );
                klass.add_binding_action(
                    Key::from_name(&format!("KP_{i}")).unwrap(),
                    ModifierType::empty(),
                    "win.focus-tab",
                    Some(&((i + 9) % 10).to_variant()),
                );
            }

            klass.install_property_action("win.set-best-fit", "best-fit");

            klass.install_action(
                "win.set-scale-request",
                Some(f64::static_variant_type().as_str()),
                |obj, _, param| {
                    let value: f64 = param
                        .expect("missing parameter")
                        .get()
                        .expect("wrong parameter type");
                    obj.imp().set_scale_request(ScaleRequest::from(value));
                },
            );

            klass.install_action("win.zoom-in", None, |obj, _, _| obj.imp().zoom_in());
            klass.install_action("win.zoom-out", None, |obj, _, _| obj.imp().zoom_out());

            // Add these two here so they don't show up in the shortcuts window.
            klass.add_binding_action(Key::equal, ModifierType::empty(), "win.zoom-in", None);
            klass.add_binding_action(Key::equal, ModifierType::CONTROL_MASK, "win.zoom-in", None);

            klass.install_action("win.media-properties", None, |window, _, _| {
                window.imp().media_properties.present();
            });

            klass.install_property_action("win.set-display-mode", "display-mode");

            // Add these here instead of set_accels_for_action so that they don't override typing in
            // the scale entry.
            klass.add_binding_action(Key::p, ModifierType::empty(), "win.play-pause", None);
            klass.add_binding_action(Key::v, ModifierType::CONTROL_MASK, "win.paste", None);
            klass.add_binding_action(Key::c, ModifierType::CONTROL_MASK, "win.copy", None);
            klass.add_binding_action(Key::period, ModifierType::empty(), "win.step-forward", None);
            klass.add_binding_action(Key::comma, ModifierType::empty(), "win.step-back", None);
            klass.add_binding_action(Key::f, ModifierType::empty(), "win.set-best-fit", None);
            klass.add_binding_action(Key::plus, ModifierType::empty(), "win.zoom-in", None);
            klass.add_binding_action(Key::minus, ModifierType::empty(), "win.zoom-out", None);
            klass.add_binding_action(
                Key::t,
                ModifierType::empty(),
                "win.set-display-mode",
                Some(&"tabbed".to_variant()),
            );
            klass.add_binding_action(
                Key::r,
                ModifierType::empty(),
                "win.set-display-mode",
                Some(&"row".to_variant()),
            );
            klass.add_binding_action(
                Key::c,
                ModifierType::empty(),
                "win.set-display-mode",
                Some(&"column".to_variant()),
            );

            klass.install_action("win.about", None, |window, _, _| {
                // Concat translated strings to reuse the metainfo translations.
                let list_points = [
                    gettext("Added row and column tiled display modes for side-by-side comparison."),
                    gettext("Changed mouse scroll and hotkey zoom to have consistent speed regardless of the current zoom level."),
                    gettext("Changed mouse scroll to zoom by default instead of panning, and to pan when Ctrl is held."),
                    gettext("Added panning by holding down the left mouse button and dragging when zoomed-in."),
                    gettext("Changed the playback position to update smoothly."),
                    gettext("You can now drag-and-drop a file out of Identity when it is not zoomed-in."),
                    gettext("Added Ctrl+C to copy the current file to clipboard."),
                    gettext("Added a context menu to tabs with a few common actions."),
                    gettext("Optimized video playback performance by enabling OpenGL video processing on compatible setups."),
                    gettext("Updated to the GNOME 44 platform."),
                    gettext("Updated translations."),
                ];
                let release_notes = String::from("<p>")
                    + &gettext("This release adds row and column display modes, reworks mouse gestures and adds drag-and-drop from Identity.")
                    + "</p><ul><li>"
                    + &list_points.join("</li><li>")
                    + "</li></ul>";

                let about_window = adw::AboutWindow::builder()
                    .transient_for(window)
                    .application_name(gettext("Identity"))
                    .application_icon(config::APP_ID)
                    .version(config::VERSION)
                    .license_type(gtk::License::Gpl30)
                    // Translators: name of the developer of the application.
                    .developer_name(gettext("Ivan Molodetskikh"))
                    .issue_url("https://gitlab.gnome.org/YaLTeR/identity/-/issues/new")
                    // Translators: shown in the About dialog, put your name here.
                    .translator_credits(gettext("translator-credits"))
                    .release_notes(release_notes)
                    .build();

                about_window.add_link(
                    // Translators: link title in the About dialog.
                    &gettext("Contribute Translations"),
                    "https://l10n.gnome.org/module/identity/",
                );
                about_window.present();
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            let obj = self.obj();
            self.parent_constructed();

            if config::PROFILE == "Devel" {
                obj.add_css_class("devel");
            }

            self.last_scale_factor.set(obj.scale_factor());

            self.controls_revealer.connect_reveal_child_notify(
                clone!(@weak obj => move |revealer| {
                    if revealer.reveals_child() {
                        obj.add_css_class("controls-visible");
                    } else {
                        obj.remove_css_class("controls-visible");
                    }
                }),
            );

            // FIXME: Remove when https://github.com/gtk-rs/gtk4-rs/issues/934 is fixed.
            self.tab_view.connect_page_detached(
                clone!(@weak self as imp => move |_, tab_page, _| imp.on_tab_page_detached(tab_page)),
            );
            self.tab_view
                .bind_property("selected-page", &*obj, "selected-page")
                .transform_to(|_, tab_page: Option<adw::TabPage>| {
                    Some(tab_page.map(|tab_page| {
                        tab_page
                            .child()
                            .downcast::<Page>()
                            .expect("tab page child has wrong type")
                    }))
                })
                .build();

            self.page_grid
                .bind_property("selected-page", &*obj, "selected-page")
                .build();

            // Bind the scale entry text.
            obj.property_expression("selected-page")
                .chain_property::<Page>("scale")
                .chain_closure::<String>(closure!(|_: Option<glib::Object>, scale: f64| {
                    if scale == 0. {
                        "".into()
                    } else {
                        format_scale(scale).to_value()
                    }
                }))
                .bind(&*self.scale_entry, "text", None::<&Self::Type>);

            // Bind properties of the media properties dialog.
            obj.property_expression("selected-page")
                .chain_closure::<bool>(closure!(
                    |_: Option<glib::Object>, selected_page: Option<Page>| {
                        selected_page.is_none()
                    }
                ))
                .bind(
                    &*self.media_properties,
                    "show-empty-state",
                    None::<&Self::Type>,
                );
            obj.property_expression("selected-page")
                .chain_property::<Page>("display-name")
                .bind(&*self.media_properties, "file-name", None::<&Self::Type>);
            obj.property_expression("selected-page")
                .chain_property::<Page>("file")
                .chain_closure::<Option<String>>(closure!(
                    |_: Option<glib::Object>, file: Option<gio::File>| {
                        file.and_then(|file| file.parent()).map(|parent| {
                            parent
                                .path()
                                .map(|path| path.to_string_lossy().into_owned())
                                .unwrap_or_else(|| parent.uri().into())
                        })
                    }
                ))
                .bind(
                    &*self.media_properties,
                    "file-location",
                    None::<&Self::Type>,
                );
            obj.property_expression("selected-page")
                .chain_property::<Page>("resolution")
                .bind(&*self.media_properties, "resolution", None::<&Self::Type>);
            obj.property_expression("selected-page")
                .chain_property::<Page>("framerate")
                .chain_closure::<String>(closure!(|_: Option<glib::Object>, framerate: f32| {
                    if framerate != 0. {
                        format!("{framerate:.2}")
                    } else {
                        gettext("N/A")
                    }
                }))
                .bind(&*self.media_properties, "frame-rate", None::<&Self::Type>);
            obj.property_expression("selected-page")
                .chain_property::<Page>("video-codec")
                .chain_closure::<String>(closure!(
                    |_: Option<glib::Object>, video_codec: Option<String>| {
                        video_codec.unwrap_or_else(|| gettext("N/A"))
                    }
                ))
                .bind(&*self.media_properties, "codec", None::<&Self::Type>);
            obj.property_expression("selected-page")
                .chain_property::<Page>("container-format")
                .chain_closure::<String>(closure!(
                    |_: Option<glib::Object>, container_format: Option<String>| {
                        container_format.unwrap_or_else(|| gettext("N/A"))
                    }
                ))
                .bind(&*self.media_properties, "container", None::<&Self::Type>);

            // Set up the drop target.
            let drop_target =
                gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
            drop_target.connect_accept(clone!(@weak obj => @default-return false, move |_, drop| {
                // Checks from the default handler.
                if !drop.actions().contains(gdk::DragAction::COPY) {
                    return false;
                }

                if !drop.formats().contains_type(gdk::FileList::static_type()) {
                    return false;
                }

                // Reject the drop if it comes from our own window. Otherwise it's too easy to
                // accidentally duplicate the files.
                if let Some(drag) = drop.drag() {
                    if let Some(native) = obj.native() {
                        if drag.surface() == native.surface() {
                            return false;
                        }
                    }
                }

                true
            }));
            drop_target.connect_drop(
                clone!(@weak obj => @default-return false, move |_, data, _, _| {
                    if let Ok(file_list) = data.get::<gdk::FileList>() {
                        for file in file_list.files().into_iter() {
                            obj.open_file(&file);
                        }

                        return true;
                    }

                    false
                }),
            );
            self.stack.add_controller(drop_target);

            // Set up the scale menu model.
            let menu = gio::Menu::new();

            let section = gio::Menu::new();
            // Translators: Entry in the scale/zoom menu that indicates that the image or video is
            // always resized to fit the window.
            section.append(Some(&gettext("Best Fit")), Some("win.set-best-fit"));
            menu.append_section(None, &section);

            let section = gio::Menu::new();
            section.append(Some("25%"), Some("win.set-scale-request(0.25)"));
            section.append(Some("50%"), Some("win.set-scale-request(0.5)"));
            section.append(Some("100%"), Some("win.set-scale-request(1.0)"));
            section.append(Some("200%"), Some("win.set-scale-request(2.0)"));
            section.append(Some("400%"), Some("win.set-scale-request(4.0)"));
            section.append(Some("800%"), Some("win.set-scale-request(8.0)"));
            menu.append_section(None, &section);

            self.scale_button.set_menu_model(Some(&menu));

            // Set up custom widgets in the primary menu.
            let popover = self.primary_menu_button_content.popover().unwrap();
            let popover_menu: gtk::PopoverMenu = popover.downcast().unwrap();
            popover_menu.add_child(&*self.display_mode_selector, "display-mode-selector");

            // Bind the player properties.
            self.player
                .bind_property("is-playing", &*obj, "is-playing")
                .bidirectional()
                .sync_create()
                .build();

            self.player
                .bind_property("progress", &*self.time_adjustment, "value")
                .bidirectional()
                .sync_create()
                .build();

            self.player
                .bind_property("has-duration", &*self.controls_revealer, "reveal-child")
                .sync_create()
                .build();

            self.player
                .bind_property("position", &*self.time_label, "label")
                .transform_to(|_, position| Some(format_position(position)))
                .sync_create()
                .build();

            self.player
                .bind_property("is-playing", &*self.play_pause_button, "icon-name")
                .transform_to(|_, is_playing| {
                    Some(if is_playing {
                        "media-playback-pause-symbolic"
                    } else {
                        "media-playback-start-symbolic"
                    })
                })
                .sync_create()
                .build();

            self.player.connect_local(
                "source-error",
                false,
                clone!(@weak self as imp => @default-return None, move |args: &[glib::Value]| {
                    let playbin: gst::Element = args[1].get().unwrap();
                    if let Some(page) = imp.find_page_for_playbin(&playbin) {
                        page.set_error();
                    } else {
                        error!("couldn't find page for playbin");
                    }

                    None
                }),
            );

            // Update playback position every frame.
            obj.add_tick_callback(|obj, _| {
                obj.imp().player.query_and_update_position();
                glib::Continue(true)
            });

            // Big hack: disable some GtkScale shortcuts that we want to use ourselves.
            for controller in self.time_scale.observe_controllers().snapshot() {
                if let Ok(controller) = controller.downcast::<gtk::ShortcutController>() {
                    if controller.name().as_deref() == Some("gtk-widget-class-shortcuts") {
                        for shortcut in controller.snapshot() {
                            let shortcut = shortcut
                                .downcast::<gtk::Shortcut>()
                                .expect("wrong item type in gtk::ShortcutController");
                            if let Some(trigger) = shortcut.trigger() {
                                if let Ok(trigger) = trigger.downcast::<gtk::KeyvalTrigger>() {
                                    match trigger.keyval() {
                                        gdk::Key::plus | gdk::Key::minus => {
                                            shortcut.set_trigger(None::<gtk::ShortcutTrigger>);
                                        }
                                        _ => (),
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        fn properties() -> &'static [glib::ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            self.derived_set_property(id, value, pspec);
        }

        fn property(&self, id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            self.derived_property(id, pspec)
        }
    }

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}

    #[gtk::template_callbacks]
    impl Window {
        fn find_page_for_playbin(&self, playbin: &gst::Element) -> Option<Page> {
            match self.display_mode.get() {
                DisplayMode::Tabbed => {
                    for i in 0..self.tab_view.n_pages() {
                        let page = self.tab_view.nth_page(i).child();
                        let page = page
                            .downcast::<Page>()
                            .expect("unexpected widget type in tab view");
                        if page.playbin().as_ref() == Some(playbin) {
                            return Some(page);
                        }
                    }
                }
                DisplayMode::Row | DisplayMode::Column => {
                    for i in 0..self.page_grid.n_pages() {
                        let page = self.page_grid.nth_page(i);
                        if page.playbin().as_ref() == Some(playbin) {
                            return Some(page);
                        }
                    }
                }
            }

            None
        }

        pub fn open_file(&self, file: &gio::File) {
            debug!("open_file(\"{}\")", file.uri());

            let page = Page::new(file);

            match self.display_mode.get() {
                DisplayMode::Tabbed => {
                    let tab_page = self.tab_view.append(&page);

                    page.bind_property("display-name", &tab_page, "title")
                        .sync_create()
                        .build();
                    page.bind_property("is-loading", &tab_page, "loading")
                        .sync_create()
                        .build();
                    page.bind_property("display-path", &tab_page, "tooltip")
                        .sync_create()
                        .build();

                    page.property_expression("is-error")
                        .chain_closure::<Option<gio::Icon>>(closure!(
                            |_: Option<glib::Object>, is_error: bool| {
                                if is_error {
                                    Some(gio::ThemedIcon::new("error-symbolic"))
                                } else {
                                    None
                                }
                            }
                        ))
                        .bind(&tab_page, "icon", None::<&Page>);
                }
                DisplayMode::Row | DisplayMode::Column => {
                    self.page_grid.append(page.clone());
                    self.on_page_attached(page);
                }
            }
        }

        fn on_page_attached(&self, page: Page) {
            debug!("page-attached");

            self.switch_to_content_after_timeout();

            let bindings = vec![
                self.obj()
                    .bind_property("scale-request", &page, "scale-request")
                    .bidirectional()
                    .sync_create()
                    .build(),
                self.obj()
                    .bind_property("h-scroll-pos", &page, "h-scroll-pos")
                    .bidirectional()
                    .sync_create()
                    .build(),
                self.obj()
                    .bind_property("v-scroll-pos", &page, "v-scroll-pos")
                    .bidirectional()
                    .sync_create()
                    .build(),
            ];
            if self
                .page_bindings
                .borrow_mut()
                .insert(page.clone(), bindings)
                .is_some()
            {
                error!("just attached page should not have property bindings");
            }

            let id = page.connect_local(
                "stop-kinetic-scrolling",
                false,
                clone!(@weak self as imp => @default-return None, move |args| {
                    let except_picture: Option<Picture> = args[1].get().unwrap();
                    imp.reset_kinetic_scrolling(except_picture.as_ref());
                    None
                }),
            );
            if self
                .page_stop_kinetic_scrolling_id
                .borrow_mut()
                .insert(page.clone(), id)
                .is_some()
            {
                error!("`page_stop_kinetic_scrolling_id` already had an entry for this page");
            };

            if page.is_error() {
                self.stack.set_visible_child_name("content");
                self.obj().present_if_not_visible();
            } else if let Some(playbin) = page.playbin() {
                self.player.attach_source(&playbin);
                self.stack.set_visible_child_name("content");
                self.obj().present_if_not_visible();
            } else {
                let id = page.connect_is_loading_notify(clone!(@weak self as imp => move |page| {
                    if let Some(playbin) = page.playbin() {
                        imp.player.attach_source(&playbin);
                    }

                    imp.stack.set_visible_child_name("content");
                    imp.obj().present_if_not_visible();
                }));

                if self
                    .page_is_loading_notify_id
                    .borrow_mut()
                    .insert(page, id)
                    .is_some()
                {
                    error!("`page_playbin_notify_id` already had an entry for this page");
                };
            }
        }

        fn on_page_detached(&self, page: Page) {
            debug!("page-detached");

            if let Some(bindings) = self.page_bindings.borrow_mut().remove(&page) {
                for binding in bindings {
                    binding.unbind();
                }
            } else {
                error!("detached page should have property bindings");
            }

            if let Some(id) = self
                .page_stop_kinetic_scrolling_id
                .borrow_mut()
                .remove(&page)
            {
                page.disconnect(id);
            } else {
                error!("detached page should have `page_stop_kinetic_scrolling_id` entry");
            }

            page.reset_kinetic_scrolling(None);

            if let Some(playbin) = page.playbin() {
                self.player.detach_source(&playbin);
            } else if let Some(id) = self.page_is_loading_notify_id.borrow_mut().remove(&page) {
                page.disconnect(id);
            }
        }

        #[template_callback]
        fn on_tab_page_attached(&self, tab_page: &adw::TabPage) {
            if self.in_display_mode_transition.get() {
                return;
            }

            let page: Page = tab_page
                .child()
                .downcast()
                .expect("tab page child has wrong type");

            self.on_page_attached(page);
        }

        #[template_callback]
        fn on_tab_page_detached(&self, tab_page: &adw::TabPage) {
            if self.in_display_mode_transition.get() {
                return;
            }

            if self.tab_view.n_pages() == 0 {
                self.stack.set_visible_child_name("empty");
            }

            let page: Page = tab_page
                .child()
                .downcast()
                .expect("tab page child has wrong type");

            self.on_page_detached(page);
        }

        #[template_callback]
        fn on_create_window(&self) -> adw::TabView {
            debug!("create-window");

            let application: Application = self
                .obj()
                .application()
                .expect("application was not set")
                .downcast()
                .expect("application has wrong type");
            let new_window = application.create_new_window();
            new_window.imp().tab_view.clone()
        }

        #[template_callback]
        fn on_setup_menu(&self, tab_page: Option<&adw::TabPage>) {
            debug!("setup-menu {tab_page:?}");

            let page = tab_page
                .map(|tab_page| tab_page.downgrade())
                .unwrap_or_else(glib::WeakRef::new);
            self.menu_page.replace(page);

            if let Some(tab_page) = tab_page {
                let page: Page = tab_page
                    .child()
                    .downcast()
                    .expect("tab page child has wrong type");

                let has_path = page.file().path().is_some();
                self.obj().action_set_enabled("win.show-in-files", has_path);
            } else {
                self.obj().action_set_enabled("win.show-in-files", true);
            }
        }

        fn menu_or_selected_tab_page(&self) -> Option<adw::TabPage> {
            self.menu_page
                .borrow()
                .upgrade()
                .or_else(|| self.tab_view.selected_page())
        }

        fn menu_or_selected_page(&self) -> Option<Page> {
            self.menu_page
                .borrow()
                .upgrade()
                .map(|tab_page| {
                    tab_page
                        .child()
                        .downcast()
                        .expect("tab page child has wrong type")
                })
                .or_else(|| self.selected_page())
        }

        pub fn copy_file(&self) {
            let Some(page) = self.menu_or_selected_page() else { return };

            let file_list = gdk::FileList::from_array(&[page.file()]);
            let content_provider = gdk::ContentProvider::for_value(&file_list.to_value());
            if let Err(err) = self.obj().clipboard().set_content(Some(&content_provider)) {
                error!("error copying: {err:?}");
            }
        }

        pub async fn show_in_files(&self) {
            let Some(page) = self.menu_or_selected_page() else { return };

            // The OpenDirectory portal wants a file descriptor. There's a way to get it without
            // leaving gio:
            //
            // 1. Get an input stream with page.file().read_future().await,
            // 2. Try to cast it to gio::FileDescriptorBased with dynamic_cast(),
            // 3. That object implements AsRawFd and can be passed straight to the portal.
            //
            // However, for files from remote mounts, the stream will not actually be a
            // gio::FileDescriptorBased. Despite simulating a local file system with GVFS, gio will
            // still open those files with non-FD-based streams, which is understandable, but alas
            // incompatible with the OpenDirectory portal. In fact, gio will go as far as detecting
            // when you try to open a gio::File from a GVFS-emulated local path, and still giving
            // you a non-FD-based stream.
            //
            // Thus, to support OpenDirectory on files from remote mounts, we use the plain old
            // std::fs::File::open(). Since it blocks, we do it on a different thread with
            // gio::spawn_blocking(). This actually works even better than gio::File::read_future()
            // because the latter seems to still block for a fraction of a second for files on
            // remote mounts, resulting in a visible UI freeze. That's likely a gio bug, but still
            // there's that.
            let Some(path) = page.file().path() else {
                debug!("file has no local path");
                return;
            };

            // Open the file in a separate thread because it blocks for remote-mounted files.
            let file = match gio::spawn_blocking(move || File::open(path)).await.unwrap() {
                Ok(file) => file,
                Err(err) => {
                    warn!("error opening file: {err:?}");
                    return;
                }
            };

            let Some(native) = self.obj().native() else {
                warn!("self.obj().native() returned None");
                return;
            };

            let identifier = ashpd::WindowIdentifier::from_native(&native).await;
            if let Err(err) = OpenDirectoryRequest::default()
                .identifier(identifier)
                .build(&file)
                .await
            {
                warn!("OpenDirectory returned an error: {:?}", err);
            }
        }

        pub fn close_tab(&self) {
            match self.display_mode.get() {
                DisplayMode::Tabbed => {
                    if let Some(page) = self.menu_or_selected_tab_page() {
                        self.tab_view.close_page(&page);
                        return;
                    }
                }
                DisplayMode::Row | DisplayMode::Column => {
                    if let Some(page) = self.menu_or_selected_page() {
                        self.page_grid.close_page(&page);
                        self.on_page_detached(page);

                        if self.page_grid.n_pages() == 0 {
                            self.stack.set_visible_child_name("empty");
                        }

                        return;
                    }
                }
            }

            self.obj().close();
        }

        pub fn move_tab_to_new_window(&self) {
            if let Some(page) = self.menu_or_selected_tab_page() {
                let application: Application = self
                    .obj()
                    .application()
                    .expect("application was not set")
                    .downcast()
                    .expect("application has wrong type");
                let new_window = application.create_new_window();
                self.tab_view
                    .transfer_page(&page, &new_window.imp().tab_view, 0);
                new_window.present();
            }
        }

        pub fn focus_tab(&self, index: i32) {
            match self.display_mode.get() {
                DisplayMode::Tabbed => {
                    if index < self.tab_view.n_pages() {
                        let page = self.tab_view.nth_page(index);
                        self.tab_view.set_selected_page(&page);
                    }
                }
                DisplayMode::Row | DisplayMode::Column => {
                    if index < self.page_grid.n_pages() {
                        let page = self.page_grid.nth_page(index);
                        self.page_grid.set_selected_page_(Some(page));
                    }
                }
            }
        }

        fn switch_to_content_after_timeout(&self) {
            let mut source_id = self.switch_to_content_source_id.borrow_mut();
            if source_id.is_some() {
                return;
            }

            let obj = self.obj();
            *source_id = Some(glib::timeout_add_local_once(
                Duration::from_millis(300),
                clone!(@weak obj => move || {
                    debug!("switch to content timeout callback");

                    let self_ = obj.imp();
                    let _ = self_.switch_to_content_source_id.take();

                    // The user could've closed the loading tab before the timeout fired or was
                    // cancelled. So check again here and only switch if there are open tabs.
                    if self_.tab_view.n_pages() > 0 {
                        self_.stack.set_visible_child_name("content");
                    }

                    obj.present_if_not_visible();
                }),
            ));
        }

        #[template_callback]
        fn on_visible_child_notify(&self) {
            if let Some(source_id) = self.switch_to_content_source_id.take() {
                source_id.remove();
            }
        }

        fn set_scale_request(&self, scale_request: ScaleRequest) {
            if self.scale_request.get() == scale_request {
                return;
            }

            self.scale_request.set(scale_request);
            self.obj().notify_scale_request();
            self.obj().notify_best_fit();
        }

        fn best_fit(&self) -> bool {
            self.scale_request.get() == ScaleRequest::FitToAllocation
        }

        fn set_best_fit(&self, val: bool) {
            self.set_scale_request(if val {
                ScaleRequest::FitToAllocation
            } else {
                ScaleRequest::Set(1.)
            })
        }

        fn set_h_scroll_pos(&self, mut value: f64) {
            value = value.clamp(0., 1.);

            if self.h_scroll_pos.get() == value {
                return;
            }

            self.h_scroll_pos.set(value);
            self.obj().notify_h_scroll_pos();
        }

        fn set_v_scroll_pos(&self, mut value: f64) {
            value = value.clamp(0., 1.);

            if self.v_scroll_pos.get() == value {
                return;
            }

            self.v_scroll_pos.set(value);
            self.obj().notify_v_scroll_pos();
        }

        fn selected_page(&self) -> Option<Page> {
            self.selected_page.borrow().clone()
        }

        fn set_selected_page(&self, page: Option<Page>) {
            if *self.selected_page.borrow() == page {
                return;
            }

            if let Some(old_page) = self.selected_page.replace(page) {
                old_page.reset_kinetic_scrolling(None);
            }

            self.obj().notify_selected_page();
        }

        fn display_mode_str(&self) -> String {
            self.display_mode.get().to_string()
        }

        fn set_display_mode(&self, value: DisplayMode) {
            let old_value = self.display_mode.get();
            if old_value == value {
                return;
            }

            self.display_mode.set(value);
            self.obj().notify_display_mode();

            // Actually switch the display mode.
            self.in_display_mode_transition.set(true);

            // Activate the correct ToggleButton.
            let button = match value {
                DisplayMode::Tabbed => &self.tabbed_button,
                DisplayMode::Row => &self.row_button,
                DisplayMode::Column => &self.column_button,
            };
            button.set_active(true);

            // Transfer the pages between widgets if necessary.
            match value {
                DisplayMode::Tabbed => {
                    match old_value {
                        DisplayMode::Tabbed => (),
                        DisplayMode::Row | DisplayMode::Column => {
                            let selected = self.selected_page();
                            self.page_grid.set_selected_page_(None);

                            // Close all pages first as it messes with selected page.
                            let n_pages = self.page_grid.n_pages();
                            let mut pages = vec![];
                            for _ in 0..n_pages {
                                let page = self.page_grid.nth_page(0);
                                self.page_grid.close_page(&page);
                                pages.push(page);
                            }

                            for page in pages {
                                // TODO: extract method
                                let tab_page = self.tab_view.append(&page);

                                page.bind_property("display-name", &tab_page, "title")
                                    .sync_create()
                                    .build();
                                page.bind_property("is-loading", &tab_page, "loading")
                                    .sync_create()
                                    .build();
                                page.bind_property("display-path", &tab_page, "tooltip")
                                    .sync_create()
                                    .build();

                                page.property_expression("is-error")
                                    .chain_closure::<Option<gio::Icon>>(closure!(
                                        |_: Option<glib::Object>, is_error: bool| {
                                            if is_error {
                                                Some(gio::ThemedIcon::new("error-symbolic"))
                                            } else {
                                                None
                                            }
                                        }
                                    ))
                                    .bind(&tab_page, "icon", None::<&Page>);

                                if selected == Some(page) {
                                    self.tab_view.set_selected_page(&tab_page);
                                }
                            }
                        }
                    }
                }
                DisplayMode::Row | DisplayMode::Column => {
                    match old_value {
                        DisplayMode::Tabbed => {
                            let selected = self.selected_page();

                            // Close all pages first as it messes with selected page.
                            let n_pages = self.tab_view.n_pages();
                            let mut pages = vec![];
                            for _ in 0..n_pages {
                                let tab_page = self.tab_view.nth_page(0);
                                self.tab_view.close_page(&tab_page);

                                let page = tab_page
                                    .child()
                                    .downcast::<Page>()
                                    .expect("unexpected widget type in tab view");
                                pages.push(page);
                            }

                            for page in pages {
                                self.page_grid.append(page);
                            }

                            self.page_grid.set_selected_page_(selected);
                        }
                        DisplayMode::Row | DisplayMode::Column => (),
                    }

                    // Set the correct orientation.
                    let orientation = match value {
                        DisplayMode::Row => gtk::Orientation::Horizontal,
                        DisplayMode::Column => gtk::Orientation::Vertical,
                        _ => unreachable!(),
                    };
                    self.page_grid.set_orientation(orientation);
                }
            }

            // Switch to the correct stack page.
            let visible_child_name = match value {
                DisplayMode::Tabbed => "tabbed",
                DisplayMode::Row | DisplayMode::Column => "grid",
            };
            self.display_mode_stack
                .set_visible_child_name(visible_child_name);

            self.in_display_mode_transition.set(false);
        }

        fn set_display_mode_str(&self, value: &str) {
            if let Ok(value) = value.parse() {
                self.set_display_mode(value);
            }
        }

        #[template_callback]
        fn on_scale_entry_activate(&self) {
            if let Some(page) = self.selected_page() {
                page.grab_focus_();
            }

            let text = self.scale_entry.text();
            let scale = parse_scale(&text);
            debug!("on_scale_entry_activate({text}): parsed: {scale:?}");

            let scale = match scale {
                Some(x) => x,
                None => return,
            };

            self.set_scale_request(ScaleRequest::from(scale));
        }

        fn zoom_in(&self) {
            if let Some(page) = self.selected_page() {
                let scale = page.scale();
                if scale != 0. {
                    let new_scale = scale * HOTKEY_SCALE_FACTOR;
                    self.set_scale_request(ScaleRequest::from(new_scale));
                }
            }
        }

        fn zoom_out(&self) {
            if let Some(page) = self.selected_page() {
                let scale = page.scale();
                if scale != 0. {
                    // Max with 0.1 here so it doesn't become 0 (fit to allocation).
                    let new_scale = (scale / HOTKEY_SCALE_FACTOR).max(0.1);
                    self.set_scale_request(ScaleRequest::from(new_scale));
                }
            }
        }

        #[template_callback]
        fn on_scale_factor_notify(&self) {
            let value = self.obj().scale_factor();

            if self.last_scale_factor.get() == value {
                return;
            }

            if let ScaleRequest::Set(scale) = self.scale_request.get() {
                let new_scale = scale / self.last_scale_factor.get() as f64 * value as f64;
                self.set_scale_request(ScaleRequest::Set(new_scale));
            }

            self.last_scale_factor.set(value);
        }

        fn reset_kinetic_scrolling(&self, except_picture: Option<&Picture>) {
            match self.display_mode.get() {
                DisplayMode::Tabbed => {
                    for i in 0..self.tab_view.n_pages() {
                        let page = self.tab_view.nth_page(i).child();
                        let page = page
                            .downcast::<Page>()
                            .expect("unexpected widget type in tab view");
                        page.reset_kinetic_scrolling(except_picture);
                    }
                }
                DisplayMode::Row | DisplayMode::Column => {
                    for i in 0..self.page_grid.n_pages() {
                        let page = self.page_grid.nth_page(i);
                        page.reset_kinetic_scrolling(except_picture);
                    }
                }
            }
        }
    }

    fn format_position(position: gst::ClockTime) -> String {
        let nanoseconds = position.nseconds();
        let mut seconds = nanoseconds / 1_000_000_000;
        let mut minutes = seconds / 60;
        let hours = minutes / 60;
        seconds %= 60;
        minutes %= 60;

        let label = if hours == 0 {
            format!("{minutes}:{seconds:02}")
        } else {
            format!("{hours}:{minutes:02}:{seconds:02}")
        };

        label
    }

    fn parse_scale(mut text: &str) -> Option<f64> {
        // `g_strtod ()` ignores leading whitespace, so just trim it from both sides.
        text = text.trim();

        if text.ends_with('%') {
            text = &text[..text.len() - 1];
        }

        // Use `g_strtod ()` to get both locale-aware and C-locale parsing.
        let scale = unsafe {
            let input = glib::translate::ToGlibPtr::to_glib_none(&text);
            let mut end_ptr = std::ptr::null_mut();
            let value = glib::ffi::g_strtod(input.0, &mut end_ptr);

            if *end_ptr != 0 {
                // The conversion failed or succeeded but didn't take the entire text.
                return None;
            }

            value
        };

        if scale.is_sign_negative() {
            return None;
        }

        Some(scale / 100.)
    }

    fn format_scale(mut scale: f64) -> glib::GString {
        // Round to get one decimal digit of precision.
        scale = (scale * 1000.).round();

        // Don't show the decimal digit if it's zero.
        let format = if scale % 10. == 0. {
            b"%.0f%%\0"
        } else {
            b"%.1f%%\0"
        };

        scale /= 10.;

        // Use `g_strdup_printf ()` to get locale-aware formatting.
        unsafe {
            glib::translate::from_glib_full(glib::ffi::g_strdup_printf(
                format.as_ptr().cast(),
                scale,
            ))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_parse_scale() {
            let check = |text: &str, expected| {
                assert_eq!(parse_scale(text), expected, "parsing `{text}`");
            };

            for scale in [0., 0.125, 0.25, 0.5, 1., 1.25, 1.5, 2., 4., 8.] {
                check(&format!("{:.1}", scale * 100.), Some(scale));
                check(&format!("{:.1}%", scale * 100.), Some(scale));
                check(&format!("{:.1}%%", scale * 100.), None);
                check(&format!("{:.1}a", scale * 100.), None);
                check(&format!("a{:.1}", scale * 100.), None);
                check(&format!("{:.1} ", scale * 100.), Some(scale));
                check(&format!("{:.1}  ", scale * 100.), Some(scale));
                check(&format!(" {:.1}", scale * 100.), Some(scale));
                check(&format!("  {:.1}", scale * 100.), Some(scale));
                check(&format!("-{:.1}", scale * 100.), None);
                check(&format!(" -{:.1}", scale * 100.), None);

                check(&format_scale(scale), Some(scale));
            }
        }
    }
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap;
}

#[gtk::template_callbacks]
impl Window {
    pub fn new(app: &impl IsA<gtk::Application>) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    pub fn open_file(&self, file: &gio::File) {
        self.imp().open_file(file);
    }

    fn present_if_not_visible(&self) {
        if !self.is_visible() {
            debug!("present_if_not_visible: presenting");

            self.present();
        }
    }

    async fn paste(&self) {
        let value = match self
            .clipboard()
            .read_value_future(gdk::FileList::static_type(), glib::PRIORITY_DEFAULT)
            .await
        {
            Ok(x) => x,
            Err(err) => {
                warn!("could not read clipboard contents: {err:?}");
                return;
            }
        };

        let file_list: gdk::FileList = match value.get() {
            Ok(x) => x,
            Err(err) => {
                warn!("could not convert value to `FileList`: {err:?}");
                return;
            }
        };

        for file in file_list.files() {
            self.open_file(&file);
        }
    }

    #[template_callback]
    fn on_open_clicked(&self) {
        let filter = gtk::FileFilter::new();
        // Translators: file chooser file filter name.
        filter.set_name(Some(&gettext("Videos and images")));
        for mime_type in MIME_TYPES {
            filter.add_mime_type(mime_type);
        }

        let file_chooser = gtk::FileChooserNative::builder()
            .transient_for(self)
            .modal(true)
            .action(gtk::FileChooserAction::Open)
            .select_multiple(true)
            // Translators: file chooser dialog title.
            .title(gettext("Open videos or images to compare"))
            .build();

        file_chooser.add_filter(&filter);

        file_chooser.connect_response({
            let obj = self.downgrade();
            let file_chooser = RefCell::new(Some(file_chooser.clone()));
            move |_, response| {
                if let Some(obj) = obj.upgrade() {
                    if let Some(file_chooser) = file_chooser.take() {
                        if response == gtk::ResponseType::Accept {
                            for file in file_chooser.files().snapshot().into_iter() {
                                let file: gio::File = file
                                    .downcast()
                                    .expect("unexpected type returned from file chooser");
                                obj.open_file(&file);
                            }
                        }
                    } else {
                        warn!("got file chooser response more than once");
                    }
                } else {
                    warn!("got file chooser response after window was freed");
                }
            }
        });

        file_chooser.show();
    }
}
