use glib::prelude::*;
use glib::subclass::prelude::*;
use gtk::{gdk, glib};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleRequest {
    FitToAllocation,
    Set(f64),
}

impl From<f64> for ScaleRequest {
    fn from(value: f64) -> Self {
        if value == 0. {
            ScaleRequest::FitToAllocation
        } else {
            ScaleRequest::Set(value.clamp(0., 10.))
        }
    }
}

impl glib::HasParamSpec for ScaleRequest {
    type ParamSpec = glib::ParamSpecDouble;
    type SetValue = Self;
    type BuilderFn = fn(&str) -> glib::ParamSpecDoubleBuilder;

    fn param_spec_builder() -> Self::BuilderFn {
        Self::ParamSpec::builder
    }
}

impl From<ScaleRequest> for glib::Value {
    fn from(value: ScaleRequest) -> Self {
        value.to_value()
    }
}

impl glib::ToValue for ScaleRequest {
    fn to_value(&self) -> glib::Value {
        match *self {
            ScaleRequest::FitToAllocation => 0.,
            ScaleRequest::Set(scale) => scale,
        }
        .to_value()
    }

    fn value_type(&self) -> glib::Type {
        f64::static_type()
    }
}

unsafe impl<'a> glib::value::FromValue<'a> for ScaleRequest {
    type Checker = glib::value::GenericValueTypeChecker<f64>;

    unsafe fn from_value(value: &'a glib::Value) -> Self {
        f64::from_value(value).into()
    }
}

impl Default for ScaleRequest {
    fn default() -> Self {
        Self::FitToAllocation
    }
}

mod imp {
    use std::cell::{Cell, RefCell};
    use std::marker::PhantomData;

    use glib::{clone, Properties};
    use gtk::graphene;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::Picture)]
    pub struct Picture {
        paintable: RefCell<Option<gdk::Paintable>>,
        invalidate_size_id: RefCell<Option<glib::SignalHandlerId>>,
        invalidate_contents_id: RefCell<Option<glib::SignalHandlerId>>,

        #[property(get, set = Self::set_scale_request, minimum = 0., maximum = 10.)]
        scale_request: Cell<ScaleRequest>,
        #[property(get)]
        scale: Cell<f64>,

        // These two properties contain a normalized scroll position. They range from 0 to 1, which
        // corresponds to the adjustment value going from minimum to maximum. That is, 0 will
        // correspond to adjustment.value() == 0, and 1 will correspond to adjustment.value() ==
        // adjustment.upper() - adjustment.page_size().
        //
        // Changing these properties will update the adjustment values, if valid adjustments are
        // present, and changing the adjustment values will in turn update these properties. This
        // way it's possible to set the scroll position even before `Picture` has been first
        // allocated and before it has valid adjustments.
        //
        // Another important effect is that these properties are preserved across paintable size
        // changes and across allocation changes. So for example resizing the window or changing the
        // scale factor when the picture is scrolled up-left will preserve this up-left scroll
        // position, and resizing the window or changing the scale factor when the picture is
        // scrolled down-right will also preserve this down-right scroll position.
        #[property(get, set = Self::set_h_scroll_pos, minimum = 0., maximum = 1., explicit_notify)]
        h_scroll_pos: Cell<f64>,
        #[property(get, set = Self::set_v_scroll_pos, minimum = 0., maximum = 1., explicit_notify)]
        v_scroll_pos: Cell<f64>,

        #[property(
            type = Option<gtk::Adjustment>,
            override_interface = gtk::Scrollable,
            get = |_| self.hadjustment.borrow().as_ref().map(|x| x.0.clone()),
            set = Self::set_hadjustment,
        )]
        hadjustment: RefCell<Option<(gtk::Adjustment, glib::SignalHandlerId)>>,
        #[property(
            type = Option<gtk::Adjustment>,
            override_interface = gtk::Scrollable,
            get = |_| self.vadjustment.borrow().as_ref().map(|x| x.0.clone()),
            set = Self::set_vadjustment,
        )]
        vadjustment: RefCell<Option<(gtk::Adjustment, glib::SignalHandlerId)>>,

        #[property(
            override_interface = gtk::Scrollable,
            get = |_| gtk::ScrollablePolicy::Minimum,
            set = |_, _: gtk::ScrollablePolicy| (),
        )]
        hscroll_policy: PhantomData<gtk::ScrollablePolicy>,
        #[property(
            override_interface = gtk::Scrollable,
            get = |_| gtk::ScrollablePolicy::Minimum,
            set = |_, _: gtk::ScrollablePolicy| (),
        )]
        vscroll_policy: PhantomData<gtk::ScrollablePolicy>,

        zoom_initial_scale: Cell<Option<f64>>,
        zoom_pivot_image_pos: Cell<Option<(f64, f64)>>,

        pointer_position: Cell<Option<(f64, f64)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Picture {
        const NAME: &'static str = "IdPicture";
        type Type = super::Picture;
        type ParentType = gtk::Widget;
        type Interfaces = (gtk::Scrollable,);

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("id-picture");
        }
    }

    impl ObjectImpl for Picture {
        fn constructed(&self) {
            let obj = self.obj();

            self.parent_constructed();

            obj.set_overflow(gtk::Overflow::Hidden);

            obj.connect_scale_factor_notify(|obj| obj.queue_resize());

            // Track the cursor position for zooming.
            let motion_controller = gtk::EventControllerMotion::new();
            motion_controller.connect_enter(clone!(@weak obj => move |_, x, y| {
                obj.imp().pointer_position.set(Some((x, y)));
            }));
            motion_controller.connect_motion(clone!(@weak obj => move |_, x, y| {
                obj.imp().pointer_position.set(Some((x, y)));
            }));
            motion_controller.connect_leave(clone!(@weak obj => move |_| {
                obj.imp().pointer_position.set(None);
            }));
            obj.add_controller(motion_controller);

            // Set up scroll to zoom.
            let scroll_controller =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
            scroll_controller.connect_scroll(
                clone!(@weak obj => @default-return gtk::Inhibit(false), move |event, _, delta_y| {
                    let scale = obj.scale();
                    if scale == 0. {
                        return gtk::Inhibit(false);
                    }

                    if event.current_event_state().contains(gdk::ModifierType::CONTROL_MASK) {
                        // Max with 0.1 here so it doesn't become 0 (fit to allocation).
                        let new_scale = (-delta_y * 0.1 + scale).max(0.1);

                        let pointer_pos = obj.imp().pointer_position.get();
                        obj.imp().zoom_begin(pointer_pos);
                        obj.imp().zoom_update(pointer_pos, new_scale);

                        return gtk::Inhibit(true);
                    }

                    gtk::Inhibit(false)
                }),
            );
            obj.add_controller(scroll_controller);

            let gesture_zoom = gtk::GestureZoom::new();
            gesture_zoom.connect_begin(clone!(@weak obj => move |gesture, _| {
                let scale = obj.scale();
                if scale == 0. {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                }

                obj.imp().zoom_initial_scale.set(Some(scale));
                obj.imp().zoom_begin(gesture.bounding_box_center());
            }));
            gesture_zoom.connect_scale_changed(clone!(@weak obj => move |gesture, scale| {
                let initial_scale = obj
                    .imp()
                    .zoom_initial_scale
                    .get()
                    .expect("zoom gesture progressing without initial scale");

                // Max with 0.1 here so it doesn't become 0 (fit to allocation).
                let new_scale = (initial_scale * scale).max(0.1);

                obj.imp()
                    .zoom_update(gesture.bounding_box_center(), new_scale);
            }));
            gesture_zoom.connect_end(clone!(@weak obj => move |_, _| {
                obj.imp().zoom_initial_scale.set(None);
            }));
            obj.add_controller(gesture_zoom);
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

    impl WidgetImpl for Picture {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::ConstantSize
        }

        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let scale = match self.scale_request.get() {
                ScaleRequest::FitToAllocation => return (0, 0, -1, -1),
                ScaleRequest::Set(x) => x,
            };

            let paintable = self.paintable.borrow();
            let paintable = match &*paintable {
                Some(x) => x,
                None => return (0, 0, -1, -1),
            };

            let paintable_width = paintable.intrinsic_width();
            let paintable_height = paintable.intrinsic_height();
            let paintable_ratio = paintable.intrinsic_aspect_ratio();

            // If the paintable doesn't have an intrinsic size, we can only meaningfully fit to
            // allocation.
            if paintable_ratio == 0. || paintable_width == 0 || paintable_height == 0 {
                return (0, 0, -1, -1);
            }

            let size = (match orientation {
                gtk::Orientation::Horizontal => (paintable_width as f64 * scale).ceil() as i32,
                gtk::Orientation::Vertical => (paintable_height as f64 * scale).ceil() as i32,
                _ => unreachable!(),
            }) / self.obj().scale_factor();

            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let widget = self.obj();
            self.update_scale(width, height, widget.scale_factor());
            self.configure_adjustments(width, height, widget.scale_factor());
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();

            let paintable = self.paintable.borrow();
            let paintable = match &*paintable {
                Some(x) => x,
                None => return,
            };

            let widget_width = widget.width();
            let widget_height = widget.height();
            let paintable_width = paintable.intrinsic_width();
            let paintable_height = paintable.intrinsic_height();
            let paintable_ratio = paintable.intrinsic_aspect_ratio();

            let scale_request = self.scale_request.get();

            // If the paintable doesn't have an intrinsic size, we can only meaningfully fit to
            // allocation.
            if paintable_ratio == 0. || paintable_width == 0 || paintable_height == 0 {
                paintable.snapshot(snapshot, widget_width as f64, widget_height as f64);

                return;
            }

            // Compute the content width and height.
            let (w, h) = match scale_request {
                ScaleRequest::FitToAllocation => {
                    let widget_ratio = widget_width as f64 / widget_height as f64;

                    if paintable_ratio > widget_ratio {
                        (widget_width as f64, widget_width as f64 / paintable_ratio)
                    } else {
                        (widget_height as f64 * paintable_ratio, widget_height as f64)
                    }
                }
                ScaleRequest::Set(scale) => (
                    paintable_width as f64 * scale / widget.scale_factor() as f64,
                    paintable_height as f64 * scale / widget.scale_factor() as f64,
                ),
            };

            // Either center and pixel-align it, or take the scroll position into account.
            let x = if w < widget_width as f64 {
                ((widget_width as f64 - w) / 2.).floor()
            } else {
                self.hadjustment
                    .borrow()
                    .as_ref()
                    .map(|(adj, _)| -adj.value())
                    .unwrap_or(0.)
                    .round()
            };
            let y = if h < widget_height as f64 {
                ((widget_height as f64 - h) / 2.).floor()
            } else {
                self.vadjustment
                    .borrow()
                    .as_ref()
                    .map(|(adj, _)| -adj.value())
                    .unwrap_or(0.)
                    .round()
            };

            snapshot.save();
            snapshot.translate(&graphene::Point::new(x as f32, y as f32));
            paintable.snapshot(snapshot, w, h);
            snapshot.restore();
        }
    }

    impl ScrollableImpl for Picture {}

    impl Picture {
        pub fn paintable(&self) -> Option<gdk::Paintable> {
            self.paintable.borrow().clone()
        }

        pub fn set_paintable(&self, paintable: Option<impl IsA<gdk::Paintable>>) {
            let obj = self.obj();

            let paintable = paintable.map(|p| p.upcast());

            let invalidate_size_id = paintable.as_ref().map(|p| {
                p.connect_invalidate_size(clone!(@weak obj => move |_| obj.queue_resize()))
            });
            let invalidate_contents_id = paintable.as_ref().map(|p| {
                p.connect_invalidate_contents(clone!(@weak obj => move |_| obj.queue_draw()))
            });

            let old_invalidate_size_id = self.invalidate_size_id.replace(invalidate_size_id);
            let old_invalidate_contents_id =
                self.invalidate_contents_id.replace(invalidate_contents_id);

            if let Some(old_paintable) = self.paintable.replace(paintable) {
                old_paintable.disconnect(
                    old_invalidate_size_id.expect("`paintable` set without invalidate-size id"),
                );
                old_paintable.disconnect(
                    old_invalidate_contents_id
                        .expect("`paintable` set without invalidate-contents id"),
                );
            }

            obj.queue_resize();
        }

        pub fn update_scale_request(&self, scale_request: ScaleRequest) {
            let obj = self.obj();

            if self.scale_request.get() != scale_request {
                self.scale_request.set(scale_request);
                obj.queue_resize();
                obj.notify_scale_request();
            }
        }

        pub fn set_scale_request(&self, scale_request: ScaleRequest) {
            if self.scale_request.get() == scale_request {
                return;
            }

            match scale_request {
                ScaleRequest::FitToAllocation => self.update_scale_request(scale_request),
                ScaleRequest::Set(new_scale) => {
                    // Use zoom methods to zoom with a center pivot point.
                    self.zoom_begin(None);
                    self.zoom_update(None, new_scale);
                }
            }
        }

        fn update_scale(&self, width: i32, height: i32, scale_factor: i32) {
            let scale = self.compute_scale(width, height, scale_factor);
            if self.scale.get() != scale {
                self.scale.set(scale);
                self.obj().notify_scale();
            }
        }

        fn compute_scale(&self, width: i32, height: i32, scale_factor: i32) -> f64 {
            if let ScaleRequest::Set(scale) = self.scale_request.get() {
                return scale;
            }

            let paintable = self.paintable.borrow();
            let paintable = match &*paintable {
                Some(x) => x,
                None => return 0.,
            };

            let paintable_width = paintable.intrinsic_width();
            let paintable_height = paintable.intrinsic_height();
            let paintable_ratio = paintable.intrinsic_aspect_ratio();

            // If the paintable doesn't have an intrinsic size, we can't compute a meaningful scale.
            if paintable_ratio == 0. || paintable_width == 0 || paintable_height == 0 {
                return 0.;
            }

            let widget_ratio = width as f64 / height as f64;
            (if paintable_ratio > widget_ratio {
                width as f64 / paintable_width as f64
            } else {
                height as f64 / paintable_height as f64
            }) * scale_factor as f64
        }

        pub fn set_h_scroll_pos(&self, mut value: f64) {
            value = value.clamp(0., 1.);

            if self.h_scroll_pos.get() != value {
                self.h_scroll_pos.set(value);
                self.obj().notify_h_scroll_pos();
                self.obj().queue_allocate();
            }
        }

        pub fn set_v_scroll_pos(&self, mut value: f64) {
            value = value.clamp(0., 1.);

            if self.v_scroll_pos.get() != value {
                self.v_scroll_pos.set(value);
                self.obj().notify_v_scroll_pos();
                self.obj().queue_allocate();
            }
        }

        pub fn set_hadjustment(&self, adj: Option<gtk::Adjustment>) {
            if let Some((old_adj, handler_id)) = self.hadjustment.take() {
                old_adj.disconnect(handler_id);
            }

            if let Some(adj) = adj {
                let obj = self.obj();

                let handler_id = adj.connect_value_changed(clone!(@weak obj => move |adj| {
                    obj.set_h_scroll_pos(normalized_adjustment_value(adj));
                }));

                self.hadjustment.replace(Some((adj, handler_id)));
            }
        }

        pub fn set_vadjustment(&self, adj: Option<gtk::Adjustment>) {
            if let Some((old_adj, handler_id)) = self.vadjustment.take() {
                old_adj.disconnect(handler_id);
            }

            if let Some(adj) = adj {
                let obj = self.obj();

                let handler_id = adj.connect_value_changed(clone!(@weak obj => move |adj| {
                    obj.set_v_scroll_pos(normalized_adjustment_value(adj));
                }));

                self.vadjustment.replace(Some((adj, handler_id)));
            }
        }

        fn configure_adjustments_with_values(
            &self,
            width: i32,
            height: i32,
            content_width: i32,
            content_height: i32,
        ) {
            let adj = self.hadjustment.borrow();
            if let Some((adj, handler_id)) = &*adj {
                adj.block_signal(handler_id);
                adj.configure(
                    self.h_scroll_pos.get() * (content_width - width) as f64,
                    0.,
                    content_width as f64,
                    width as f64 * 0.1,
                    width as f64 * 0.9,
                    width as f64,
                );
                adj.unblock_signal(handler_id);
            }

            let adj = self.vadjustment.borrow();
            if let Some((adj, handler_id)) = &*adj {
                adj.block_signal(handler_id);
                adj.configure(
                    self.v_scroll_pos.get() * (content_height - height) as f64,
                    0.,
                    content_height as f64,
                    height as f64 * 0.1,
                    height as f64 * 0.9,
                    height as f64,
                );
                adj.unblock_signal(handler_id);
            }
        }

        fn configure_adjustments(&self, width: i32, height: i32, scale_factor: i32) {
            let scale = match self.scale_request.get() {
                ScaleRequest::FitToAllocation => {
                    self.configure_adjustments_with_values(width, height, width, height);
                    return;
                }
                ScaleRequest::Set(x) => x,
            };

            let paintable = self.paintable.borrow();
            let paintable = match &*paintable {
                Some(x) => x,
                None => {
                    self.configure_adjustments_with_values(width, height, width, height);
                    return;
                }
            };

            let paintable_width = paintable.intrinsic_width();
            let paintable_height = paintable.intrinsic_height();
            let paintable_ratio = paintable.intrinsic_aspect_ratio();

            // If the paintable doesn't have an intrinsic size, we can only meaningfully fit to
            // allocation.
            if paintable_ratio == 0. || paintable_width == 0 || paintable_height == 0 {
                self.configure_adjustments_with_values(width, height, width, height);
                return;
            }

            // Compute target width and height.
            let w = (paintable_width as f64 * scale / scale_factor as f64).ceil() as i32;
            let h = (paintable_height as f64 * scale / scale_factor as f64).ceil() as i32;
            self.configure_adjustments_with_values(width, height, w, h);
        }

        fn image_pos_for_pointer_pos(
            &self,
            (pointer_x, pointer_y): (f64, f64),
            scale: f64,
        ) -> Option<(f64, f64)> {
            if scale == 0. {
                return None;
            }

            let paintable = self.paintable.borrow();
            let paintable = paintable.as_ref()?;

            let obj = self.obj();
            let widget_width = obj.width();
            let widget_height = obj.height();
            let paintable_width = paintable.intrinsic_width();
            let paintable_height = paintable.intrinsic_height();
            let paintable_ratio = paintable.intrinsic_aspect_ratio();

            // If the paintable doesn't have an intrinsic size, we can only meaningfully fit to
            // allocation.
            if paintable_ratio == 0. || paintable_width == 0 || paintable_height == 0 {
                return None;
            }

            // Compute the content width and height.
            let w = paintable_width as f64 * scale / obj.scale_factor() as f64;
            let h = paintable_height as f64 * scale / obj.scale_factor() as f64;

            // Either center and pixel-align it, or take the scroll position into account.
            let content_x = if w < widget_width as f64 {
                ((widget_width as f64 - w) / 2.).floor()
            } else {
                -(self.h_scroll_pos.get() * (w.ceil() as i32 - widget_width) as f64)
            };
            let content_y = if h < widget_height as f64 {
                ((widget_height as f64 - h) / 2.).floor()
            } else {
                -(self.v_scroll_pos.get() * (h.ceil() as i32 - widget_height) as f64)
            };

            let x = (pointer_x - content_x) * (paintable_width as f64 / w);
            let y = (pointer_y - content_y) * (paintable_height as f64 / h);

            Some((x, y))
        }

        fn zoom_begin(&self, pivot_pointer_pos: Option<(f64, f64)>) {
            let obj = self.obj();
            let pivot_pointer_pos = pivot_pointer_pos
                .unwrap_or_else(|| (obj.width() as f64 / 2., obj.height() as f64 / 2.));
            self.zoom_pivot_image_pos
                .set(self.image_pos_for_pointer_pos(pivot_pointer_pos, self.scale.get()));
        }

        fn zoom_update(&self, pivot_pointer_pos: Option<(f64, f64)>, new_scale: f64) {
            let obj = self.obj();

            self.update_scale_request(ScaleRequest::from(new_scale));

            let (image_x, image_y) = match self.zoom_pivot_image_pos.get() {
                Some(x) => x,
                None => return,
            };

            let pivot_pointer_pos = pivot_pointer_pos
                .unwrap_or_else(|| (obj.width() as f64 / 2., obj.height() as f64 / 2.));

            let (new_image_x, new_image_y) =
                match self.image_pos_for_pointer_pos(pivot_pointer_pos, new_scale) {
                    Some(x) => x,
                    None => {
                        self.zoom_pivot_image_pos.set(None);
                        return;
                    }
                };

            let paintable = obj.imp().paintable.borrow();
            let paintable = paintable.as_ref().expect("zooming without a paintable");

            // To match pivot image position, compute the difference in image pixels, convert it to
            // h_scroll_pos and v_scroll_pos units post-scaling, and add to h_scroll_pos and
            // v_scroll_pos. Then during the next size_allocate() the adjustment values will be set
            // in accordance with the new h_scroll_pos and v_scroll_pos values.
            let image_dx_norm = (new_image_x - image_x) / paintable.intrinsic_width() as f64;
            let image_dy_norm = (new_image_y - image_y) / paintable.intrinsic_height() as f64;

            let content_w =
                paintable.intrinsic_width() as f64 * new_scale / obj.scale_factor() as f64;
            let content_h =
                paintable.intrinsic_height() as f64 * new_scale / obj.scale_factor() as f64;

            if (obj.width() as f64) < content_w {
                let h_scroll_pos_frac = 1. - obj.width() as f64 / content_w;
                self.set_h_scroll_pos(self.h_scroll_pos.get() - image_dx_norm / h_scroll_pos_frac);
            }

            if (obj.height() as f64) < content_h {
                let v_scroll_pos_frac = 1. - obj.height() as f64 / content_h;
                self.set_v_scroll_pos(self.v_scroll_pos.get() - image_dy_norm / v_scroll_pos_frac);
            }
        }
    }

    fn normalized_adjustment_value(adj: &gtk::Adjustment) -> f64 {
        let upper = adj.upper() - adj.page_size();
        if upper == 0. {
            0.
        } else {
            adj.value() / upper
        }
    }
}

glib::wrapper! {
    pub struct Picture(ObjectSubclass<imp::Picture>)
        @extends gtk::Widget, @implements gtk::Scrollable;
}

impl Picture {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn paintable(&self) -> Option<gdk::Paintable> {
        self.imp().paintable()
    }

    pub fn set_paintable(&self, paintable: Option<impl IsA<gdk::Paintable>>) {
        self.imp().set_paintable(paintable);
    }
}

impl Default for Picture {
    fn default() -> Self {
        Self::new()
    }
}
