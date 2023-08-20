/// Controlling multiple synchronized playback sources.
use glib::subclass::prelude::*;
use gtk::glib;

mod imp {
    use std::cell::{Cell, OnceCell};
    use std::marker::PhantomData;

    use glib::subclass::Signal;
    use glib::{clone, ControlFlow, Properties};
    use gst::bus::BusWatchGuard;
    use gst::prelude::*;
    use once_cell::sync::Lazy;

    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::Player)]
    pub struct Player {
        /// Whether the player is playing (as opposed to paused).
        ///
        /// Set to play or pause.
        #[property(get, set = Self::set_is_playing, explicit_notify)]
        is_playing: Cell<bool>,
        /// Current playback position normalized from 0 to 1.
        ///
        /// Updates when `query_and_update_position` is called.
        ///
        /// Set to seek.
        #[property(get, set = Self::seek, minimum = 0., maximum = 1., explicit_notify)]
        progress: Cell<f64>,
        /// Current playback position.
        ///
        /// Updates when `query_and_update_position` is called.
        #[property(get)]
        position: Cell<gst::ClockTime>,
        /// Whether the player has a duration (i.e. some source is a video).
        #[property(get = Self::has_duration)]
        has_duration: PhantomData<bool>,

        /// The pipeline containing the GStreamer playback sources.
        pipeline: gst::Pipeline,
        /// The bus watch on the pipeline.
        bus_watch_guard: OnceCell<BusWatchGuard>,
        /// Whether the pipeline is currently playing backwards.
        is_backwards: Cell<bool>,
        /// Combined (maximal) duration of the playback sources.
        duration: Cell<Option<gst::ClockTime>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Player {
        const NAME: &'static str = "IdPlayer";
        type Type = super::Player;
        type ParentType = glib::Object;
    }

    impl ObjectImpl for Player {
        fn constructed(&self) {
            // Subscribe to bus messages.
            let bus = self.pipeline.bus().unwrap();
            let watch = bus
                .add_watch_local(
                    clone!(@weak self as imp => @default-return ControlFlow::Break, move |_, msg| {
                        imp.on_bus_message(msg);
                        ControlFlow::Continue
                    }),
                )
                .unwrap();
            self.bus_watch_guard.set(watch).unwrap();

            // Pre-roll the (empty) pipeline.
            self.pipeline.set_state(gst::State::Paused).unwrap();
        }

        fn dispose(&self) {
            // I got this to return Err once by opening a file GStreamer couldn't play and a regular
            // video file.
            if let Err(err) = self.pipeline.set_state(gst::State::Null) {
                warn!("error setting pipeline state to Null: {err:?}");
            }
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![Signal::builder("source-error")
                    .param_types([gst::Element::static_type()])
                    .build()]
            });
            SIGNALS.as_ref()
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

    impl Player {
        fn query_position(&self) -> Option<gst::ClockTime> {
            self.pipeline.query_position::<gst::ClockTime>()
        }

        #[instrument("Player::set_is_playing", skip_all)]
        pub fn set_is_playing(&self, play: bool) {
            if self.is_playing.get() == play {
                return;
            }

            debug!("set_is_playing({play})");

            if play && self.is_backwards.get() {
                let Some(position) = self.query_position() else {
                    return;
                };

                if let Err(err) = self.pipeline.seek(
                    1.,
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::SeekType::Set,
                    position,
                    gst::SeekType::End,
                    Some(gst::ClockTime::ZERO),
                ) {
                    warn!("set_is_playing: error seeking: {err:?}");
                }

                self.is_backwards.set(false);
            }

            let target_state = if play {
                gst::State::Playing
            } else {
                gst::State::Paused
            };
            self.pipeline.set_state(target_state).unwrap();
        }

        #[instrument("Player::seek_to_time", skip_all)]
        fn seek_to_time(&self, time: gst::ClockTime) {
            debug!("seek_to_time({time})");

            if let Err(err) = self.pipeline.seek_simple(gst::SeekFlags::FLUSH, time) {
                // This can happen if there's a broken playbin in the pipeline that nevertheless
                // hasn't sent an error to the bus yet.
                warn!("error seeking: {err:?}");
            }

            self.is_backwards.set(false);
        }

        pub fn seek(&self, to: f64) {
            debug!("seek({to:.02})");

            let Some(duration) = self.duration.get() else {
                return;
            };

            let time = gst::ClockTime::from_nseconds((to * duration.nseconds() as f64) as u64);
            self.seek_to_time(time);
        }

        fn has_duration(&self) -> bool {
            self.duration.get().is_some()
        }

        fn on_bus_message(&self, msg: &gst::Message) {
            use gst::MessageView;
            match msg.view() {
                MessageView::StateChanged(state_changed)
                    if state_changed.src() == Some(self.pipeline.upcast_ref()) =>
                {
                    debug!(
                        "bus: StateChanged old: {:?}, current: {:?}, pending: {:?}",
                        state_changed.old(),
                        state_changed.current(),
                        state_changed.pending(),
                    );

                    use gst::State::*;
                    match (state_changed.current(), state_changed.pending()) {
                        (Playing, VoidPending) => {
                            self.is_playing.set(true);
                            self.obj().notify_is_playing();
                        }
                        (Paused, VoidPending) => {
                            self.is_playing.set(false);
                            self.obj().notify_is_playing();
                        }
                        (_, _) => (),
                    }
                }
                MessageView::DurationChanged(_) => {
                    debug!("bus: DurationChanged");
                    self.query_and_update_duration();
                }
                MessageView::Eos(_) => {
                    debug!("bus: Eos");
                    self.seek_to_time(gst::ClockTime::ZERO);
                }
                MessageView::Error(err) => {
                    warn!(
                        "bus: Error from {:?}: {} ({:?})",
                        err.src(),
                        err.error(),
                        err.debug(),
                    );

                    // Upon getting an error, find the source the error originates from and remove
                    // it from the pipeline. Note that find_immediate_child() can fail if an element
                    // throws multiple errors at once since the source will be removed from the
                    // pipeline the first time around.
                    if let Some(source) = err
                        .src()
                        .and_then(|obj| self.find_immediate_child(obj.clone()))
                    {
                        let source = source
                            .downcast::<gst::Element>()
                            .expect("immediate child didn't downcast to `gst::Element`");
                        self.detach_source(&source);

                        self.obj().emit_by_name::<()>("source-error", &[&source]);
                    }
                }
                _ => (),
            }
        }

        fn find_immediate_child(&self, mut object: gst::Object) -> Option<gst::Object> {
            loop {
                let parent = object.parent()?;
                if parent == self.pipeline {
                    return Some(object);
                }

                object = parent;
            }
        }

        fn query_and_update_duration(&self) {
            let duration = self.pipeline.query_duration::<gst::ClockTime>();
            debug!("update_duration: duration = {duration:?}");

            if self.duration.get() == duration {
                return;
            }

            self.duration.set(duration);
            self.obj().notify_has_duration();
            self.recompute_progress();
        }

        pub fn query_and_update_position(&self) {
            // We don't update the position if we get a `None`. This way, whenever the last video
            // tab is closed, the time label stays where it is as the controls revealer is closing.
            // Also, this way, during seeks, the position does not flash to zero momentarily.
            let Some(position) = self.query_position() else {
                return;
            };

            if self.position.get() == position {
                return;
            }

            self.position.set(position);
            self.obj().notify_position();
            self.recompute_progress();
        }

        fn recompute_progress(&self) {
            let progress = match self.duration.get() {
                Some(duration) => {
                    self.position.get().nseconds() as f64 / duration.nseconds() as f64
                }
                _ => 0.,
            };

            // Since we're dealing with cached duration and position, clamp to ensure our value is
            // always in range, even during partially outdated duration/position.
            let progress = progress.clamp(0., 1.);

            if self.progress.get() == progress {
                return;
            }

            self.progress.set(progress);
            self.obj().notify_progress();
        }

        #[instrument("Player::attach_source", skip_all)]
        pub fn attach_source(&self, source: &gst::Element) {
            debug!("Player::attach_source");

            if let Err(err) = self.pipeline.add(source) {
                error!("error adding source to pipeline: {err:?}");
                return;
            }

            // Query the new duration and position now that the source has been added.
            self.query_and_update_duration();
            self.query_and_update_position();

            if let Err(err) = source.sync_state_with_parent() {
                warn!("error syncing source state with parent: {err:?}");
            }

            // Seek to current position to put the new source at the same position.
            if let Some(time) = self.query_position() {
                self.seek_to_time(time);
            }
        }

        #[instrument("Player::detach_source", skip_all)]
        pub fn detach_source(&self, source: &gst::Element) {
            debug!("Player::detach_source");

            if let Err(err) = self.pipeline.remove(source) {
                // This can happen for example if the source was already removed after it errored,
                // and then the page was closed, which calls detach_source again.
                warn!("error removing source from pipeline: {err:?}");
                return;
            }

            // Query the new duration and position now that the source has been removed.
            self.query_and_update_duration();
            self.query_and_update_position();

            // Pause the source that is now on its own.
            if let Err(err) = source.set_state(gst::State::Paused) {
                warn!("error setting source state to Paused: {err:?}");
            }
        }

        /// Steps one frame into the current playback direction.
        pub fn step_frame(&self) {
            debug!("step_frame()");

            self.pipeline.send_event(gst::event::Step::new(
                gst::format::Buffers::from_u64(1),
                1.,
                true,
                false,
            ));
        }

        pub fn step_forward(&self) {
            if self.is_playing.get() {
                // Only step while paused.
                return;
            }

            debug!("step_forward: is_backwards: {}", self.is_backwards.get());

            if !self.is_backwards.get() {
                self.step_frame();
                return;
            }

            // Get the most up-to-date position for the seek.
            if let Some(position) = self.query_position() {
                // Reversing playback direction already steps 1 frame in most cases.
                // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/20
                if let Err(err) = self.pipeline.seek(
                    1.,
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::SeekType::Set,
                    position,
                    gst::SeekType::End,
                    gst::ClockTime::ZERO,
                ) {
                    warn!("step_forward: error seeking: {err:?}");
                }

                self.is_backwards.set(false);
            }
        }

        pub fn step_back(&self) {
            if self.is_playing.get() {
                // Only step while paused.
                return;
            }

            debug!("step_back: is_backwards: {}", self.is_backwards.get());

            if self.is_backwards.get() {
                self.step_frame();
                return;
            }

            // Get the most up-to-date position for the seek.
            if let Some(position) = self.query_position() {
                // Reversing playback direction already steps 1 frame in most cases.
                // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/20
                if let Err(err) = self.pipeline.seek(
                    -1.,
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::SeekType::Set,
                    gst::ClockTime::ZERO,
                    gst::SeekType::Set,
                    position,
                ) {
                    warn!("step_back: error seeking: {err:?}");
                }

                // TODO: failed seeks will update this, but they shouldn't.
                self.is_backwards.set(true);
            }
        }
    }
}

glib::wrapper! {
    pub struct Player(ObjectSubclass<imp::Player>);
}

impl Player {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Attaches the source to the `Player`, which takes control of it.
    pub fn attach_source(&self, source: &gst::Element) {
        self.imp().attach_source(source);
    }

    /// Detaches the source from the `Player`, giving the control back.
    pub fn detach_source(&self, source: &gst::Element) {
        self.imp().detach_source(source);
    }

    /// Steps one frame forward.
    pub fn step_forward(&self) {
        self.imp().step_forward();
    }

    /// Steps one frame back.
    pub fn step_back(&self) {
        self.imp().step_back();
    }

    /// Updates the position and progress properties to the latest values.
    ///
    /// This function should be called periodically (i.e. every frame) to ensure the latest position
    /// is available for display.
    pub fn query_and_update_position(&self) {
        self.imp().query_and_update_position();
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}
