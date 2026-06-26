use glib::prelude::*;
use glib::subclass::prelude::*;
use gtk::{gdk, glib};

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::cmp;
    use std::marker::PhantomData;
    use std::sync::OnceLock;

    use glib::subclass::Signal;
    use glib::{clone, Propagation, Properties};
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{graphene, gsk};

    use super::*;
    use crate::scale_request::ScaleRequest;
    use crate::thumbnail_paintable::ThumbnailPaintable;
    use crate::utils::fractional_scale;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::Picture)]
    pub struct Picture {
        rotation: Cell<u32>,

        paintable: RefCell<Option<gdk::Paintable>>,
        invalidate_size_id: RefCell<Option<glib::SignalHandlerId>>,
        invalidate_contents_id: RefCell<Option<glib::SignalHandlerId>>,

        surface_signals: OnceCell<glib::SignalGroup>,

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
        // changes and across allocation changes. So for example resizing the window or changing
        // the scale factor when the picture is scrolled up-left will preserve this up-left
        // scroll position, and resizing the window or changing the scale factor when the
        // picture is scrolled down-right will also preserve this down-right scroll
        // position.
        #[property(get, set = Self::set_h_scroll_pos, minimum = 0., maximum = 1., explicit_notify)]
        h_scroll_pos: Cell<f64>,
        #[property(get, set = Self::set_v_scroll_pos, minimum = 0., maximum = 1., explicit_notify)]
        v_scroll_pos: Cell<f64>,

        #[property(
            type = Option<gtk::Adjustment>,
            override_interface = gtk::Scrollable,
            get = Self::hadjustment,
            set = Self::set_hadjustment,
        )]
        hadjustment: RefCell<Option<(gtk::Adjustment, glib::SignalHandlerId)>>,
        #[property(
            type = Option<gtk::Adjustment>,
            override_interface = gtk::Scrollable,
            get = Self::vadjustment,
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
        zoom_initial_bbox_center: Cell<Option<(f64, f64)>>,
        zoom_pivot_image_pos: Cell<Option<(f64, f64)>>,

        gesture_drag: gtk::GestureDrag,
        pan_pivot_pointer_pos: Cell<Option<(f64, f64)>>,
        pan_pivot_image_pos: Cell<Option<(f64, f64)>>,

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

            let surface_signals = glib::SignalGroup::new::<gdk::Surface>();
            surface_signals.connect_notify_local(
                Some("scale"),
                clone!(
                    #[weak]
                    obj,
                    move |_, _| {
                        obj.queue_resize();
                    },
                ),
            );
            obj.connect_realize(clone!(
                #[weak]
                surface_signals,
                move |obj| {
                    surface_signals.set_target(obj.native().and_then(|x| x.surface()).as_ref());
                },
            ));
            obj.connect_unrealize(clone!(
                #[weak]
                surface_signals,
                move |_| {
                    surface_signals.set_target(gdk::Surface::NONE);
                },
            ));
            self.surface_signals.set(surface_signals).unwrap();

            // Track the cursor position for zooming.
            let motion_controller = gtk::EventControllerMotion::new();
            motion_controller.connect_enter(clone!(
                #[weak]
                obj,
                move |_, x, y| {
                    obj.imp().pointer_position.set(Some((x, y)));
                }
            ));
            motion_controller.connect_motion(clone!(
                #[weak]
                obj,
                move |_, x, y| {
                    obj.imp().pointer_position.set(Some((x, y)));
                }
            ));
            motion_controller.connect_leave(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.imp().pointer_position.set(None);
                }
            ));
            obj.add_controller(motion_controller);

            // Set up scroll to zoom.
            //
            // Both axes because we rely on this scroll controller to stop kinetic scrolling on
            // other pages when attemtping a touchpad pan.
            let scroll_controller =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            scroll_controller.connect_scroll(clone!(
                #[weak]
                obj,
                #[upgrade_or]
                Propagation::Proceed,
                move |event, _, delta_y| {
                    let scale = obj.scale();
                    if scale == 0. || obj.imp().is_panning() || obj.imp().is_zooming() {
                        // Stop propagation because we don't want the scroll to come through if
                        // we're in the middle of a pan or a pinch zoom.
                        return Propagation::Stop;
                    }

                    // Don't trigger scroll gesture on touchpads, it's too fast and impresize on
                    // them. Touchpads have pinch zoom for this.
                    if event.current_event_device().map(|x| x.source())
                        == Some(gdk::InputSource::Touchpad)
                    {
                        // Take this opportunity to stop other scrolled windows' kinetic scrolling
                        // though.
                        obj.emit_by_name::<()>("stop-kinetic-scrolling", &[&obj]);
                        return Propagation::Proceed;
                    }

                    // Leave Control and Shift scrolling for the scrolled window.
                    if event
                        .current_event_state()
                        .contains(gdk::ModifierType::CONTROL_MASK)
                        || event
                            .current_event_state()
                            .contains(gdk::ModifierType::SHIFT_MASK)
                    {
                        return Propagation::Proceed;
                    }

                    // Use exponential scaling since zoom is always multiplicative with the existing
                    // value. This is the right thing since `exp(n/2)^2 == exp(n)`.
                    // (two small steps are the same as one larger step)
                    //
                    // The factor of 1.3× per scroll was copied from Loupe.
                    let factor = f64::exp(-delta_y * f64::ln(1.3));

                    // Max with 0.1 here so it doesn't become 0 (fit to allocation).
                    let new_scale = (factor * scale).max(0.1);

                    let pointer_pos = obj.imp().pointer_position.get();
                    obj.imp().zoom_begin(pointer_pos);
                    obj.imp().zoom_update(pointer_pos, new_scale);
                    obj.imp().zoom_end();

                    Propagation::Stop
                }
            ));
            obj.add_controller(scroll_controller);

            // Set up the pinch zoom gesture.
            let gesture_zoom = gtk::GestureZoom::new();
            gesture_zoom.connect_begin(clone!(
                #[weak]
                obj,
                move |gesture, _| {
                    let scale = obj.scale();
                    if scale == 0. || obj.imp().is_panning() {
                        gesture.set_state(gtk::EventSequenceState::Denied);
                        return;
                    }

                    gesture.set_state(gtk::EventSequenceState::Claimed);

                    // Tell Page to stop the kinetic scrolling.
                    obj.emit_by_name::<()>("stop-kinetic-scrolling", &[&None::<super::Picture>]);

                    obj.imp().zoom_initial_scale.set(Some(scale));
                    obj.imp()
                        .zoom_initial_bbox_center
                        .set(gesture.bounding_box_center());
                    obj.imp().zoom_begin(gesture.bounding_box_center());
                }
            ));
            gesture_zoom.connect_scale_changed(clone!(
                #[weak]
                obj,
                move |gesture, scale| {
                    let initial_scale = obj
                        .imp()
                        .zoom_initial_scale
                        .get()
                        .expect("zoom gesture progressing without initial scale");

                    // Max with 0.1 here so it doesn't become 0 (fit to allocation).
                    let new_scale = (initial_scale * scale).max(0.1);

                    // Pan while zooming only on touchscreen and not on touchpad.
                    let bbox_center = if gesture.device().map(|x| x.source())
                        == Some(gdk::InputSource::Touchscreen)
                    {
                        gesture.bounding_box_center()
                    } else {
                        obj.imp().zoom_initial_bbox_center.get()
                    };

                    obj.imp().zoom_update(bbox_center, new_scale);
                }
            ));
            gesture_zoom.connect_end(clone!(
                #[weak]
                obj,
                move |_, _| {
                    obj.imp().zoom_initial_scale.set(None);
                    obj.imp().zoom_initial_bbox_center.set(None);
                    obj.imp().zoom_end();
                }
            ));
            obj.add_controller(gesture_zoom);

            // Set up click and drag to pan.
            self.gesture_drag.connect_drag_begin(clone!(
                #[weak(rename_to = imp)]
                self,
                move |gesture, _, _| {
                    if imp.is_zooming() || !(imp.is_hscrollable() || imp.is_vscrollable()) {
                        gesture.set_state(gtk::EventSequenceState::Denied);
                        return;
                    }

                    if gesture.device().map(|x| x.source()) == Some(gdk::InputSource::Touchscreen) {
                        // Touchscreens use ScrolledWindow's panning.
                        gesture.set_state(gtk::EventSequenceState::Denied);

                        // Stop kinetic scrolling even if we're about to get denied due to
                        // touchscreen, because if touchscreen tries to pan a different scrolled
                        // window than the one with kinetic scrolling, it needs the kinetic
                        // scrolling stopped everywhere.
                        //
                        // We pass this picture to ignore its scrolled window for resetting, because
                        // if we don't, then the touchscreen pan gesture will break.
                        imp.obj()
                            .emit_by_name::<()>("stop-kinetic-scrolling", &[&*imp.obj()]);

                        return;
                    }

                    imp.obj()
                        .emit_by_name::<()>("stop-kinetic-scrolling", &[&None::<super::Picture>]);
                }
            ));
            self.gesture_drag.connect_drag_update(clone!(
                #[weak(rename_to = imp)]
                self,
                move |gesture, offset_x, offset_y| {
                    if imp.is_panning() {
                        if imp.pan_update(offset_x, offset_y).is_none() {
                            imp.pan_end();
                        }
                    } else {
                        let reached_threshold = imp.obj().drag_check_threshold(
                            0,
                            0,
                            offset_x.ceil() as i32,
                            offset_y.ceil() as i32,
                        );
                        if !reached_threshold {
                            return;
                        }

                        if imp.is_zooming() {
                            // Started zooming in the meantime...
                            gesture.set_state(gtk::EventSequenceState::Denied);
                            return;
                        }

                        let Some((start_x, start_y)) = gesture.start_point() else {
                            // Not sure when this can fail.
                            gesture.set_state(gtk::EventSequenceState::Denied);
                            return;
                        };

                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        imp.pan_begin(start_x, start_y);
                        imp.obj()
                            .set_cursor(gdk::Cursor::from_name("grabbing", None).as_ref());
                    }
                }
            ));
            self.gesture_drag.connect_drag_end(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _| {
                    imp.pan_end();

                    imp.obj().set_cursor(None);
                }
            ));
            obj.add_controller(self.gesture_drag.clone());

            let drag_source = gtk::DragSource::new();
            drag_source.set_exclusive(true);
            drag_source.connect_prepare(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                None,
                move |source, _, _| {
                    if imp.is_zooming() || (imp.is_hscrollable() || imp.is_vscrollable()) {
                        source.set_state(gtk::EventSequenceState::Denied);
                        return None;
                    }

                    imp.obj()
                        .emit_by_name::<Option<gdk::ContentProvider>>("get-content-provider", &[])
                }
            ));
            drag_source.connect_drag_begin(clone!(
                #[weak(rename_to = imp)]
                self,
                move |source, drag| {
                    if let Some(paintable) = imp.thumbnail() {
                        let hot_x = paintable.intrinsic_width() / 2 + 16;
                        let hot_y = paintable.intrinsic_height() / 2 + 16;
                        source.set_icon(Some(&paintable), hot_x, hot_y);

                        let icon = gtk::DragIcon::for_drag(drag);
                        icon.add_css_class("drag-thumbnail");
                        // So border-radius works.
                        icon.set_overflow(gtk::Overflow::Hidden);
                    }
                }
            ));
            obj.add_controller(drag_source);
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("stop-kinetic-scrolling")
                        .param_types([super::Picture::static_type()])
                        .build(),
                    Signal::builder("get-content-provider")
                        .return_type::<gdk::ContentProvider>()
                        .build(),
                ]
            })
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

            let rotation = self.rotation.get();
            let size = match orientation {
                gtk::Orientation::Horizontal => {
                    if rotation == 90 || rotation == 270 {
                        paintable_height as f64 * scale
                    } else {
                        paintable_width as f64 * scale
                    }
                }
                gtk::Orientation::Vertical => {
                    if rotation == 90 || rotation == 270 {
                        paintable_width as f64 * scale
                    } else {
                        paintable_height as f64 * scale
                    }
                }
                _ => unreachable!(),
            };
            let size = (size / self.fractional_scale()).ceil() as i32;

            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let scale_factor = self.fractional_scale();
            self.update_scale(width, height, scale_factor);
            self.configure_adjustments(width, height, scale_factor);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();

            let paintable = self.paintable.borrow();
            let paintable = match &*paintable {
                Some(x) => x,
                None => return,
            };

            let rotation = self.rotation.get();
            let widget_width = widget.width();
            let widget_height = widget.height();
            let paintable_width = paintable.intrinsic_width();
            let paintable_height = paintable.intrinsic_height();
            let paintable_ratio = paintable.intrinsic_aspect_ratio();

            let scale_request = self.scale_request.get();

            // If the paintable doesn't have an intrinsic size, we can only meaningfully fit to
            // allocation.
            if paintable_ratio == 0. || paintable_width == 0 || paintable_height == 0 {
                self.snapshot_paintable(
                    snapshot,
                    paintable,
                    widget_width as f64,
                    widget_height as f64,
                );

                return;
            }

            // For 90°/270°, swap effective dimensions for aspect-ratio fitting.
            let (eff_w, eff_h) = if rotation == 90 || rotation == 270 {
                (widget_height, widget_width)
            } else {
                (widget_width, widget_height)
            };

            // Compute the content width and height.
            let (w, h) = match scale_request {
                ScaleRequest::FitToAllocation => {
                    let eff_ratio = eff_w as f64 / eff_h as f64;

                    if paintable_ratio > eff_ratio {
                        (eff_w as f64, eff_w as f64 / paintable_ratio)
                    } else {
                        (eff_h as f64 * paintable_ratio, eff_h as f64)
                    }
                }
                ScaleRequest::Set(scale) => {
                    let scale_factor = self.fractional_scale();
                    (
                        paintable_width as f64 * scale / scale_factor,
                        paintable_height as f64 * scale / scale_factor,
                    )
                }
            };

            // Round the sizes to avoid artifacts at the sides.
            let w = w.round();
            let h = h.round();

            snapshot.save();

            if rotation == 0 {
                // Non-rotated: center or apply scroll offset.
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
                snapshot.translate(&graphene::Point::new(x as f32, y as f32));
                self.snapshot_paintable(snapshot, paintable, w, h);
            } else {
                // Rotated: rotate around widget center, then center image in effective space.
                //
                // The effective coordinate space (eff_w × eff_h) has its origin at the widget
                // center after applying the rotation transform. To place the image at position
                // (x, y) within that space we translate by (x − eff_w/2, y − eff_h/2).
                let x = if w < eff_w as f64 {
                    ((eff_w as f64 - w) / 2.).floor()
                } else {
                    self.hadjustment
                        .borrow()
                        .as_ref()
                        .map(|(adj, _)| -adj.value())
                        .unwrap_or(0.)
                        .round()
                };
                let y = if h < eff_h as f64 {
                    ((eff_h as f64 - h) / 2.).floor()
                } else {
                    self.vadjustment
                        .borrow()
                        .as_ref()
                        .map(|(adj, _)| -adj.value())
                        .unwrap_or(0.)
                        .round()
                };
                snapshot.translate(&graphene::Point::new(
                    widget_width as f32 / 2.,
                    widget_height as f32 / 2.,
                ));
                snapshot.rotate(rotation as f32);
                snapshot.translate(&graphene::Point::new(
                    (x - eff_w as f64 / 2.) as f32,
                    (y - eff_h as f64 / 2.) as f32,
                ));
                self.snapshot_paintable(snapshot, paintable, w, h);
            }

            snapshot.restore();
        }

        fn unmap(&self) {
            self.gesture_drag.set_state(gtk::EventSequenceState::Denied);

            self.parent_unmap();
        }
    }

    impl ScrollableImpl for Picture {}

    impl Picture {
        pub fn set_rotation(&self, degrees: u32) {
            if self.rotation.get() == degrees {
                return;
            }
            self.rotation.set(degrees);
            self.obj().queue_resize();
        }

        pub fn paintable(&self) -> Option<gdk::Paintable> {
            self.paintable.borrow().clone()
        }

        pub fn set_paintable(&self, paintable: Option<impl IsA<gdk::Paintable>>) {
            let obj = self.obj();

            let paintable = paintable.map(|p| p.upcast());

            let invalidate_size_id = paintable.as_ref().map(|p| {
                p.connect_invalidate_size(clone!(
                    #[weak]
                    obj,
                    move |_| obj.queue_resize()
                ))
            });
            let invalidate_contents_id = paintable.as_ref().map(|p| {
                p.connect_invalidate_contents(clone!(
                    #[weak]
                    obj,
                    move |_| obj.queue_draw()
                ))
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
                    self.zoom_end();
                }
            }
        }

        fn update_scale(&self, width: i32, height: i32, scale_factor: f64) {
            let scale = self.compute_scale(width, height, scale_factor);
            if self.scale.get() != scale {
                self.scale.set(scale);
                self.obj().notify_scale();
            }
        }

        fn compute_scale(&self, width: i32, height: i32, scale_factor: f64) -> f64 {
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

            let rotation = self.rotation.get();
            let (eff_w, eff_h) = if rotation == 90 || rotation == 270 {
                (height, width)
            } else {
                (width, height)
            };
            let widget_ratio = eff_w as f64 / eff_h as f64;
            (if paintable_ratio > widget_ratio {
                eff_w as f64 / paintable_width as f64
            } else {
                eff_h as f64 / paintable_height as f64
            }) * scale_factor
        }

        fn fractional_scale(&self) -> f64 {
            fractional_scale(&*self.obj())
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

        fn hadjustment(&self) -> Option<gtk::Adjustment> {
            self.hadjustment.borrow().as_ref().map(|x| x.0.clone())
        }

        pub fn set_hadjustment(&self, adj: Option<gtk::Adjustment>) {
            if let Some((old_adj, handler_id)) = self.hadjustment.take() {
                old_adj.disconnect(handler_id);
            }

            if let Some(adj) = adj {
                let obj = self.obj();

                let handler_id = adj.connect_value_changed(clone!(
                    #[weak]
                    obj,
                    move |adj| {
                        obj.set_h_scroll_pos(normalized_adjustment_value(adj));
                    }
                ));

                self.hadjustment.replace(Some((adj, handler_id)));
            }
        }

        fn vadjustment(&self) -> Option<gtk::Adjustment> {
            self.vadjustment.borrow().as_ref().map(|x| x.0.clone())
        }

        pub fn set_vadjustment(&self, adj: Option<gtk::Adjustment>) {
            if let Some((old_adj, handler_id)) = self.vadjustment.take() {
                old_adj.disconnect(handler_id);
            }

            if let Some(adj) = adj {
                let obj = self.obj();

                let handler_id = adj.connect_value_changed(clone!(
                    #[weak]
                    obj,
                    move |adj| {
                        obj.set_v_scroll_pos(normalized_adjustment_value(adj));
                    }
                ));

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
                    cmp::max(content_width, width) as f64,
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
                    cmp::max(content_height, height) as f64,
                    height as f64 * 0.1,
                    height as f64 * 0.9,
                    height as f64,
                );
                adj.unblock_signal(handler_id);
            }
        }

        fn configure_adjustments(&self, width: i32, height: i32, scale_factor: f64) {
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
            let w = (paintable_width as f64 * scale / scale_factor).ceil() as i32;
            let h = (paintable_height as f64 * scale / scale_factor).ceil() as i32;

            // For 90°/270°, use the effective viewport dimensions so the scroll range is
            // correct in the rotated coordinate space (eff_w = widget_height, etc.).
            let (eff_w, eff_h) = self.effective_dimensions();
            self.configure_adjustments_with_values(eff_w, eff_h, w, h);
        }

        fn image_pos_for_pointer_pos(
            &self,
            pointer_pos: (f64, f64),
            scale: f64,
        ) -> Option<(f64, f64)> {
            if scale == 0. {
                return None;
            }

            let paintable = self.paintable.borrow();
            let paintable = paintable.as_ref()?;

            let paintable_width = paintable.intrinsic_width();
            let paintable_height = paintable.intrinsic_height();
            let paintable_ratio = paintable.intrinsic_aspect_ratio();

            // If the paintable doesn't have an intrinsic size, we can only meaningfully fit to
            // allocation.
            if paintable_ratio == 0. || paintable_width == 0 || paintable_height == 0 {
                return None;
            }

            // Transform pointer from screen space to the effective (rotated) drawing space.
            let (eff_w, eff_h) = self.effective_dimensions();
            let (pointer_x, pointer_y) = self.screen_to_effective(pointer_pos);

            // Compute the content width and height.
            let scale_factor = self.fractional_scale();
            let w = paintable_width as f64 * scale / scale_factor;
            let h = paintable_height as f64 * scale / scale_factor;

            // Either center and pixel-align it, or take the scroll position into account.
            let content_x = if w < eff_w as f64 {
                ((eff_w as f64 - w) / 2.).floor()
            } else {
                -(self.h_scroll_pos.get() * (w.ceil() as i32 - eff_w) as f64)
            };
            let content_y = if h < eff_h as f64 {
                ((eff_h as f64 - h) / 2.).floor()
            } else {
                -(self.v_scroll_pos.get() * (h.ceil() as i32 - eff_h) as f64)
            };

            let x = (pointer_x - content_x) * (paintable_width as f64 / w);
            let y = (pointer_y - content_y) * (paintable_height as f64 / h);

            Some((x, y))
        }

        pub fn image_pos_for_widget_pos(&self, x: f64, y: f64) -> Option<(f64, f64)> {
            self.image_pos_for_pointer_pos((x, y), self.scale.get())
        }

        pub fn paintable_dimensions(&self) -> Option<(u32, u32)> {
            let paintable = self.paintable.borrow();
            let paintable = paintable.as_ref()?;
            let width = paintable.intrinsic_width();
            let height = paintable.intrinsic_height();
            if width <= 0 || height <= 0 {
                return None;
            }

            Some((width as u32, height as u32))
        }

        fn is_zooming(&self) -> bool {
            self.zoom_pivot_image_pos.get().is_some()
        }

        fn zoom_pivot_pointer_pos(&self, pivot_pointer_pos: Option<(f64, f64)>) -> (f64, f64) {
            let obj = self.obj();
            match pivot_pointer_pos {
                Some(x) => x,
                None => {
                    let pivot_x = if self.is_hscrollable() {
                        self.h_scroll_pos.get()
                    } else {
                        0.5
                    };
                    let pivot_y = if self.is_vscrollable() {
                        self.v_scroll_pos.get()
                    } else {
                        0.5
                    };
                    (obj.width() as f64 * pivot_x, obj.height() as f64 * pivot_y)
                }
            }
        }

        fn zoom_begin(&self, pivot_pointer_pos: Option<(f64, f64)>) {
            let pivot_pointer_pos = self.zoom_pivot_pointer_pos(pivot_pointer_pos);
            self.zoom_pivot_image_pos
                .set(self.image_pos_for_pointer_pos(pivot_pointer_pos, self.scale.get()));
        }

        fn zoom_end(&self) {
            self.zoom_pivot_image_pos.set(None);
        }

        fn zoom_update(&self, pivot_pointer_pos: Option<(f64, f64)>, new_scale: f64) {
            // Convert to ScaleRequest and back to ensure correct clamping.
            let scale_request = ScaleRequest::from(new_scale);
            self.update_scale_request(scale_request);
            let new_scale: f64 = scale_request.into();

            let (image_x, image_y) = match self.zoom_pivot_image_pos.get() {
                Some(x) => x,
                None => return,
            };

            let pivot_pointer_pos = self.zoom_pivot_pointer_pos(pivot_pointer_pos);

            let (new_image_x, new_image_y) =
                match self.image_pos_for_pointer_pos(pivot_pointer_pos, new_scale) {
                    Some(x) => x,
                    None => {
                        self.zoom_end();
                        return;
                    }
                };

            let paintable = self.paintable.borrow();
            let paintable = paintable.as_ref().expect("zooming without a paintable");

            // To match pivot image position, compute the difference in image pixels, convert it to
            // h_scroll_pos and v_scroll_pos units post-scaling, and add to h_scroll_pos and
            // v_scroll_pos. Then during the next size_allocate() the adjustment values will be set
            // in accordance with the new h_scroll_pos and v_scroll_pos values.
            let image_dx_norm = (new_image_x - image_x) / paintable.intrinsic_width() as f64;
            let image_dy_norm = (new_image_y - image_y) / paintable.intrinsic_height() as f64;

            let scale_factor = self.fractional_scale();
            let content_w = paintable.intrinsic_width() as f64 * new_scale / scale_factor;
            let content_h = paintable.intrinsic_height() as f64 * new_scale / scale_factor;

            let (eff_w, eff_h) = self.effective_dimensions();

            if (eff_w as f64) < content_w {
                let h_scroll_pos_frac = 1. - eff_w as f64 / content_w;
                self.set_h_scroll_pos(self.h_scroll_pos.get() - image_dx_norm / h_scroll_pos_frac);
            }

            if (eff_h as f64) < content_h {
                let v_scroll_pos_frac = 1. - eff_h as f64 / content_h;
                self.set_v_scroll_pos(self.v_scroll_pos.get() - image_dy_norm / v_scroll_pos_frac);
            }
        }

        fn is_panning(&self) -> bool {
            self.pan_pivot_pointer_pos.get().is_some()
        }

        fn pan_begin(&self, start_x: f64, start_y: f64) {
            self.pan_pivot_pointer_pos.set(Some((start_x, start_y)));
            self.pan_pivot_image_pos
                .set(self.image_pos_for_pointer_pos((start_x, start_y), self.scale.get()));
        }

        fn pan_end(&self) {
            self.pan_pivot_pointer_pos.set(None);
            self.pan_pivot_image_pos.set(None);
        }

        fn pan_update(&self, offset_x: f64, offset_y: f64) -> Option<()> {
            let scale = self.scale.get();

            let (start_x, start_y) = self.pan_pivot_pointer_pos.get()?;
            let new_pivot_pointer_pos = (start_x + offset_x, start_y + offset_y);

            let (image_x, image_y) = self.pan_pivot_image_pos.get()?;
            let (new_image_x, new_image_y) =
                self.image_pos_for_pointer_pos(new_pivot_pointer_pos, scale)?;

            let paintable = self.paintable.borrow();
            let paintable = paintable.as_ref().expect("panning without a paintable");

            // To match pivot image position, compute the difference in image pixels, convert it to
            // h_scroll_pos and v_scroll_pos units post-scaling, and add to h_scroll_pos and
            // v_scroll_pos. Then during the next size_allocate() the adjustment values will be set
            // in accordance with the new h_scroll_pos and v_scroll_pos values.
            let image_dx_norm = (new_image_x - image_x) / paintable.intrinsic_width() as f64;
            let image_dy_norm = (new_image_y - image_y) / paintable.intrinsic_height() as f64;

            let scale_factor = self.fractional_scale();
            let content_w = paintable.intrinsic_width() as f64 * scale / scale_factor;
            let content_h = paintable.intrinsic_height() as f64 * scale / scale_factor;

            let (eff_w, eff_h) = self.effective_dimensions();

            if (eff_w as f64) < content_w {
                let h_scroll_pos_frac = 1. - eff_w as f64 / content_w;
                self.set_h_scroll_pos(self.h_scroll_pos.get() - image_dx_norm / h_scroll_pos_frac);
            }

            if (eff_h as f64) < content_h {
                let v_scroll_pos_frac = 1. - eff_h as f64 / content_h;
                self.set_v_scroll_pos(self.v_scroll_pos.get() - image_dy_norm / v_scroll_pos_frac);
            }

            Some(())
        }

        /// Returns effective (width, height) in the rotated coordinate space.
        /// For 90°/270° the widget width and height are swapped.
        fn effective_dimensions(&self) -> (i32, i32) {
            let obj = self.obj();
            let w = obj.width();
            let h = obj.height();
            if self.rotation.get() == 90 || self.rotation.get() == 270 {
                (h, w)
            } else {
                (w, h)
            }
        }

        /// Transforms a pointer position from widget (screen) space into the effective
        /// drawing space used after the rotation transform is applied.
        fn screen_to_effective(&self, (px, py): (f64, f64)) -> (f64, f64) {
            let rotation = self.rotation.get();
            if rotation == 0 {
                return (px, py);
            }
            let obj = self.obj();
            let cx = obj.width() as f64 / 2.;
            let cy = obj.height() as f64 / 2.;
            let ox = px - cx;
            let oy = py - cy;
            // Inverse rotation: rotate by -R to get from screen back to effective space.
            let angle = -(rotation as f64).to_radians();
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            let ox_rot = ox * cos_a - oy * sin_a;
            let oy_rot = ox * sin_a + oy * cos_a;
            let (eff_w, eff_h) = self.effective_dimensions();
            (ox_rot + eff_w as f64 / 2., oy_rot + eff_h as f64 / 2.)
        }

        fn is_hscrollable(&self) -> bool {
            let Some((adj, _)) = &*self.hadjustment.borrow() else {
                return false;
            };
            adj.upper() - adj.page_size() > 0.
        }

        fn is_vscrollable(&self) -> bool {
            let Some((adj, _)) = &*self.vadjustment.borrow() else {
                return false;
            };
            adj.upper() - adj.page_size() > 0.
        }

        fn thumbnail(&self) -> Option<gdk::Paintable> {
            let paintable = self.paintable()?;
            Some(ThumbnailPaintable::new(&paintable).upcast())
        }

        fn snapshot_paintable(
            &self,
            snapshot: &gtk::Snapshot,
            paintable: &gdk::Paintable,
            width: f64,
            height: f64,
        ) {
            let scale = self.fractional_scale();
            snapshot.scale(1. / scale as f32, 1. / scale as f32);

            let filter = if self.scale.get() >= 1. {
                gsk::ScalingFilter::Nearest
            } else {
                gsk::ScalingFilter::Linear
            };
            paintable.set_property("scaling-filter", filter);

            paintable.snapshot(snapshot, width * scale, height * scale);
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
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Picture {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_rotation(&self, degrees: u32) {
        self.imp().set_rotation(degrees);
    }

    pub fn paintable(&self) -> Option<gdk::Paintable> {
        self.imp().paintable()
    }

    pub fn set_paintable(&self, paintable: Option<impl IsA<gdk::Paintable>>) {
        self.imp().set_paintable(paintable);
    }

    pub fn image_pos_for_widget_pos(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        self.imp().image_pos_for_widget_pos(x, y)
    }

    pub fn paintable_dimensions(&self) -> Option<(u32, u32)> {
        self.imp().paintable_dimensions()
    }
}

impl Default for Picture {
    fn default() -> Self {
        Self::new()
    }
}
