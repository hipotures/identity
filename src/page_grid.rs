use adw::subclass::prelude::*;
use gtk::glib;

use crate::page::Page;

mod imp {
    use std::cell::RefCell;

    use glib::{clone, error, Properties, SignalHandlerId};
    use gtk::prelude::*;
    use gtk::CompositeTemplate;

    use super::*;
    use crate::G_LOG_DOMAIN;

    #[derive(Debug, Default, CompositeTemplate, Properties)]
    #[template(resource = "/org/gnome/gitlab/YaLTeR/Identity/ui/page_grid.ui")]
    #[properties(wrapper_type = super::PageGrid)]
    pub struct PageGrid {
        #[template_child]
        box_layout: TemplateChild<gtk::BoxLayout>,

        #[property(get, set = Self::set_selected_page, explicit_notify)]
        selected_page: RefCell<Option<Page>>,

        pages: RefCell<Vec<(Page, SignalHandlerId)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PageGrid {
        const NAME: &'static str = "IdPageGrid";
        type Type = super::PageGrid;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("id-page-grid");

            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PageGrid {
        fn constructed(&self) {
            self.parent_constructed();

            self.obj().add_css_class("row");
        }

        fn properties() -> &'static [glib::ParamSpec] {
            Self::derived_properties()
        }

        fn property(&self, id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            self.derived_property(id, pspec)
        }

        fn set_property(&self, id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            self.derived_set_property(id, value, pspec);
        }
    }

    impl WidgetImpl for PageGrid {}
    impl BinImpl for PageGrid {}

    impl PageGrid {
        pub fn append(&self, page: Page) {
            page.set_parent(&*self.obj());
            page.set_show_overlay(true);

            let id = page.connect_local(
                "activate",
                false,
                clone!(@weak self as imp => @default-return None, move |args| {
                    let page = args[0].get().unwrap();
                    imp.set_selected_page(Some(page));
                    None
                }),
            );
            self.pages.borrow_mut().push((page.clone(), id));

            if self.selected_page.borrow().is_none() {
                self.set_selected_page(Some(page));
            }
        }

        pub fn n_pages(&self) -> i32 {
            self.pages.borrow().len().try_into().unwrap()
        }

        pub fn nth_page(&self, n: i32) -> Page {
            self.pages.borrow()[n as usize].0.clone()
        }

        pub fn close_page(&self, page: &Page) {
            let new_selection = page
                .prev_sibling()
                .or_else(|| page.next_sibling())
                .map(|widget| widget.downcast().unwrap());

            {
                let mut pages = self.pages.borrow_mut();
                if let Some(idx) = pages.iter().position(|(p, _)| p == page) {
                    let id = pages.remove(idx).1;
                    page.disconnect(id);
                } else {
                    error!("`pages` should have entry for a page being removed");
                }
            }

            page.unparent();
            page.set_show_overlay(false);

            {
                let selected_page = &mut *self.selected_page.borrow_mut();
                if selected_page.as_ref() != Some(page) {
                    return;
                }
            }
            self.set_selected_page(new_selection);
        }

        pub fn set_orientation(&self, value: gtk::Orientation) {
            self.box_layout.set_orientation(value);

            match value {
                gtk::Orientation::Horizontal => {
                    self.obj().remove_css_class("column");
                    self.obj().add_css_class("row");
                }
                gtk::Orientation::Vertical => {
                    self.obj().remove_css_class("row");
                    self.obj().add_css_class("column");
                }
                _ => unreachable!(),
            }
        }

        pub fn set_selected_page(&self, value: Option<Page>) {
            {
                let selected_page = &mut *self.selected_page.borrow_mut();
                if *selected_page == value {
                    return;
                }

                if let Some(old) = selected_page.take() {
                    old.remove_css_class("selected");
                }

                if let Some(new) = &value {
                    new.add_css_class("selected");
                }

                *selected_page = value;
            }

            self.obj().notify_selected_page();
        }
    }
}

glib::wrapper! {
    pub struct PageGrid(ObjectSubclass<imp::PageGrid>)
        @extends adw::Bin, gtk::Widget;
}

impl PageGrid {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn append(&self, page: Page) {
        self.imp().append(page);
    }

    pub fn n_pages(&self) -> i32 {
        self.imp().n_pages()
    }

    pub fn nth_page(&self, n: i32) -> Page {
        self.imp().nth_page(n)
    }

    pub fn close_page(&self, page: &Page) {
        self.imp().close_page(page);
    }

    pub fn set_orientation(&self, value: gtk::Orientation) {
        self.imp().set_orientation(value);
    }

    // FIXME: replace with nullable attribute when new gtk-rs releases.
    pub fn set_selected_page_(&self, value: Option<Page>) {
        self.imp().set_selected_page(value);
    }
}

impl Default for PageGrid {
    fn default() -> Self {
        Self::new()
    }
}
