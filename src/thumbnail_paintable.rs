use glib::prelude::*;
use glib::subclass::prelude::*;
use gtk::{gdk, glib};

mod imp {
    use std::cell::{Cell, OnceCell};

    use gdk::prelude::*;
    use gdk::subclass::prelude::*;
    use glib::{clone, Properties};

    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::ThumbnailPaintable)]
    pub struct ThumbnailPaintable {
        #[property(get, set, construct_only)]
        paintable: OnceCell<gdk::Paintable>,

        size: Cell<(i32, i32)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ThumbnailPaintable {
        const NAME: &'static str = "IdThumbnailPaintable";
        type Type = super::ThumbnailPaintable;
        type ParentType = glib::Object;
        type Interfaces = (gdk::Paintable,);
    }

    impl ObjectImpl for ThumbnailPaintable {
        fn properties() -> &'static [glib::ParamSpec] {
            Self::derived_properties()
        }

        fn property(&self, id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            self.derived_property(id, pspec)
        }

        fn set_property(&self, id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            self.derived_set_property(id, value, pspec);
        }

        fn constructed(&self) {
            let obj = &*self.obj();
            self.parent_constructed();

            self.paintable().connect_invalidate_contents(clone!(
                #[weak]
                obj,
                move |_| obj.invalidate_contents()
            ));
            self.paintable().connect_invalidate_size(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.imp().recompute_size();
                    obj.invalidate_size();
                }
            ));

            self.recompute_size();
        }
    }

    impl PaintableImpl for ThumbnailPaintable {
        fn current_image(&self) -> gdk::Paintable {
            self.paintable().current_image()
        }

        fn flags(&self) -> gdk::PaintableFlags {
            self.paintable().flags()
        }

        fn intrinsic_width(&self) -> i32 {
            self.size.get().0
        }

        fn intrinsic_height(&self) -> i32 {
            self.size.get().1
        }

        fn intrinsic_aspect_ratio(&self) -> f64 {
            self.paintable().intrinsic_aspect_ratio()
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            self.paintable().snapshot(snapshot, width, height);
        }
    }

    impl ThumbnailPaintable {
        fn paintable(&self) -> &gdk::Paintable {
            self.paintable.get().unwrap()
        }

        fn recompute_size(&self) {
            const THUMBNAIL_SIZE: f64 = 128.;

            let paintable = self.paintable();
            let width = paintable.intrinsic_width();
            let height = paintable.intrinsic_height();
            let long_side = i32::max(width, height);
            if long_side == 0 {
                self.size.set((0, 0));
                return;
            }

            let scale = f64::min(1., THUMBNAIL_SIZE / long_side as f64);
            let width = (width as f64 * scale).round() as i32;
            let height = (height as f64 * scale).round() as i32;
            self.size.set((width, height));
        }
    }
}

glib::wrapper! {
    pub struct ThumbnailPaintable(ObjectSubclass<imp::ThumbnailPaintable>)
        @implements gdk::Paintable;
}

impl ThumbnailPaintable {
    pub fn new(paintable: &impl IsA<gdk::Paintable>) -> Self {
        glib::Object::builder()
            .property("paintable", paintable)
            .build()
    }
}
