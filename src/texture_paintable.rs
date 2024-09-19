use glib::subclass::prelude::*;
use gtk::{gdk, glib};

mod imp {
    use std::cell::{Cell, OnceCell};
    use std::sync::OnceLock;

    use gdk::subclass::prelude::*;
    use gtk::prelude::*;
    use gtk::{graphene, gsk};

    use super::*;

    #[derive(Debug)]
    pub struct TexturePaintable {
        texture: OnceCell<gdk::Texture>,
        scaling_filter: Cell<gsk::ScalingFilter>,
    }

    impl Default for TexturePaintable {
        fn default() -> Self {
            Self {
                texture: Default::default(),
                scaling_filter: Cell::new(gsk::ScalingFilter::Linear),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TexturePaintable {
        const NAME: &'static str = "IdTexturePaintable";
        type Type = super::TexturePaintable;
        type ParentType = glib::Object;
        type Interfaces = (gdk::Paintable,);
    }

    impl ObjectImpl for TexturePaintable {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: OnceLock<Vec<glib::ParamSpec>> = OnceLock::new();
            PROPERTIES.get_or_init(|| {
                vec![
                    glib::ParamSpecObject::builder::<gdk::Texture>("texture")
                        .readwrite()
                        .construct_only()
                        .build(),
                    glib::ParamSpecEnum::builder_with_default::<gsk::ScalingFilter>(
                        "scaling-filter",
                        gsk::ScalingFilter::Linear,
                    )
                    .readwrite()
                    .build(),
                ]
            })
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "texture" => self.texture().to_value(),
                "scaling-filter" => self.scaling_filter.get().to_value(),
                _ => unreachable!(),
            }
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "texture" => self.texture.set(value.get().unwrap()).unwrap(),
                "scaling-filter" => self.scaling_filter.set(value.get().unwrap()),
                _ => unreachable!(),
            }
        }
    }

    impl PaintableImpl for TexturePaintable {
        fn current_image(&self) -> gdk::Paintable {
            self.obj().clone().upcast()
        }

        fn flags(&self) -> gdk::PaintableFlags {
            gdk::PaintableFlags::SIZE | gdk::PaintableFlags::CONTENTS
        }

        fn intrinsic_width(&self) -> i32 {
            self.texture().width()
        }

        fn intrinsic_height(&self) -> i32 {
            self.texture().height()
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            let texture = self.texture();
            let filter = self.scaling_filter.get();
            let bounds = graphene::Rect::new(0., 0., width as f32, height as f32);
            snapshot.append_scaled_texture(texture, filter, &bounds);
        }
    }

    impl TexturePaintable {
        fn texture(&self) -> &gdk::Texture {
            self.texture.get().unwrap()
        }
    }
}

glib::wrapper! {
    pub struct TexturePaintable(ObjectSubclass<imp::TexturePaintable>)
        @implements gdk::Paintable;
}

impl TexturePaintable {
    pub fn new(texture: &gdk::Texture) -> Self {
        glib::Object::builder().property("texture", texture).build()
    }
}
