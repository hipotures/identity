/// Controlling multiple synchronized playback sources.
use glib::subclass::prelude::*;
use gtk::glib;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::iter;
    use std::marker::PhantomData;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, OnceLock};
    use std::thread::JoinHandle;

    use glib::subclass::Signal;
    use glib::{clone, ControlFlow, Properties};
    use gst::bus::BusWatchGuard;
    use gst::prelude::*;

    use super::*;

    enum Request {
        Seek(gst::ClockTime),
        StepForwards,
        StepBack,
        SetPlaying(bool),
        SetRate(f64),
        Exit,
    }

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
        /// Combined (maximal) duration of the playback sources.
        duration: Cell<Option<gst::ClockTime>>,
        /// Sender to the processing thread.
        sender: OnceCell<Sender<Request>>,
        /// Join handle for the processing thread.
        join_handle: RefCell<Option<JoinHandle<()>>>,
        /// Number of seeks queued for the processing thread to handle.
        seeks_queued: Arc<AtomicUsize>,
        /// Playback rate. 1.0 = normal speed.
        #[property(get, set = Self::set_playback_rate, explicit_notify, minimum = 0.001, maximum = 100., default = 1.0)]
        playback_rate: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Player {
        const NAME: &'static str = "IdPlayer";
        type Type = super::Player;
        type ParentType = glib::Object;
    }

    impl ObjectImpl for Player {
        fn constructed(&self) {
            self.playback_rate.set(1.0);

            // Subscribe to bus messages.
            let bus = self.pipeline.bus().unwrap();
            let watch = bus
                .add_watch_local(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[upgrade_or]
                    ControlFlow::Break,
                    move |_, msg| {
                        imp.on_bus_message(msg);
                        ControlFlow::Continue
                    }
                ))
                .unwrap();
            self.bus_watch_guard.set(watch).unwrap();

            // Pre-roll the (empty) pipeline.
            self.pipeline.set_state(gst::State::Paused).unwrap();

            // Start the processing thread.
            let (sender, receiver) = channel();
            let join_handle = std::thread::Builder::new()
                .name("Processing Thread".to_owned())
                .spawn({
                    let pipeline = self.pipeline.clone();
                    let seeks_queued = self.seeks_queued.clone();
                    move || processing_thread(pipeline, seeks_queued, receiver)
                })
                .unwrap();
            self.sender.set(sender).unwrap();
            self.join_handle.replace(Some(join_handle));
        }

        fn dispose(&self) {
            debug!("Player::dispose");
            self.send(Request::Exit);

            let span = info_span!("join");
            if let Err(err) = span.in_scope(|| self.join_handle.borrow_mut().take().unwrap().join())
            {
                warn!("error joining the processing thread: {err:?}");
            }
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![Signal::builder("source-error")
                    .param_types([gst::Element::static_type()])
                    .build()]
            })
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

        pub fn set_is_playing(&self, play: bool) {
            if self.is_playing.get() == play {
                return;
            }

            self.send(Request::SetPlaying(play));
        }

        fn seek_to_time(&self, time: gst::ClockTime) {
            self.send(Request::Seek(time));
        }

        pub fn seek(&self, to: f64) {
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
                MessageView::Tag(tag) => {
                    let tags = tag.tags();
                    debug!("bus: got tags: {tags:?}");
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

        pub fn step_forward(&self) {
            if self.is_playing.get() {
                // Only step while paused.
                return;
            }

            self.send(Request::StepForwards);
        }

        pub fn step_back(&self) {
            if self.is_playing.get() {
                // Only step while paused.
                return;
            }

            self.send(Request::StepBack);
        }

        fn send(&self, request: Request) {
            if matches!(request, Request::Seek(_)) {
                self.seeks_queued.fetch_add(1, Ordering::SeqCst);
            }

            if self.sender.get().unwrap().send(request).is_err() {
                error!("processing thread shut down unexpectedly");
            }
        }

        pub fn has_seeks_queued(&self) -> bool {
            self.seeks_queued.load(Ordering::SeqCst) > 0
        }

        pub fn set_playback_rate(&self, rate: f64) {
            if self.playback_rate.get() == rate {
                return;
            }
            self.playback_rate.set(rate);
            self.obj().notify_playback_rate();
            if let Some(sender) = self.sender.get() {
                let _ = sender.send(Request::SetRate(rate));
            }
        }
    }

    // Seeking and state changes happen on a thread because, with enough heavy videos in the
    // pipeline, they will block for several frames. And we never want the UI thread to block.
    fn processing_thread(
        pipeline: gst::Pipeline,
        seeks_queued: Arc<AtomicUsize>,
        receiver: Receiver<Request>,
    ) {
        let mut current_rate: f64 = 1.0;

        'outer: loop {
            let mut seek_to = gst::ClockTime::NONE;
            let mut seeks_batched = 0;
            let mut step = 0i64;
            let mut set_playing = None;
            let mut set_rate: Option<f64> = None;

            // Receive all requests thus far. Do one blocking recv and follow up with non-blocking
            // ones to avoid busy-looping.
            let Ok(request) = receiver.recv() else {
                // The channel hung up.
                break;
            };

            for request in iter::once(request).chain(receiver.try_iter()) {
                match request {
                    Request::Seek(to) => {
                        seek_to = Some(to);
                        seeks_batched += 1;
                    }
                    Request::StepForwards => step += 1,
                    Request::StepBack => step -= 1,
                    Request::SetPlaying(play) => set_playing = Some(play),
                    Request::SetRate(rate) => set_rate = Some(rate),
                    Request::Exit => break 'outer,
                }
            }

            if seek_to.is_none() && step == 0 && set_playing.is_none() && set_rate.is_none() {
                // Either the channel hung up, or the requests cancelled each other.
                continue;
            }

            // Seek if requested.
            if let Some(seek_to) = seek_to {
                let _span = info_span!("seek").entered();
                debug!("seeking to {seek_to:?}");

                // Use seek() with the current rate to preserve playback speed.
                // Seeking always resets direction to forwards.
                let rate = current_rate.abs();
                if let Err(err) = pipeline.seek(
                    rate,
                    gst::SeekFlags::FLUSH,
                    gst::SeekType::Set,
                    seek_to,
                    gst::SeekType::End,
                    gst::ClockTime::ZERO,
                ) {
                    // This can happen if there's a broken playbin in the pipeline that nevertheless
                    // hasn't sent an error to the bus yet.
                    warn!("error seeking: {err:?}");
                }
                // We change current_rate unconditionally because one broken pipeline will cause an
                // error to return, but for other pipelines the seek will still succeed.
                current_rate = rate;
                seeks_queued.fetch_sub(seeks_batched, Ordering::SeqCst);
            }

            // If a backwards step is requested and we're not playing backwards, reverse direction.
            if step < 0 && current_rate >= 0. {
                let _span = info_span!("set backwards").entered();
                if let Some(position) = pipeline.query_position::<gst::ClockTime>() {
                    debug!("changing playback direction to backwards");

                    if let Err(err) = pipeline.seek(
                        -current_rate.abs(),
                        gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                        gst::SeekType::Set,
                        gst::ClockTime::ZERO,
                        gst::SeekType::Set,
                        position,
                    ) {
                        warn!("error seeking: {err:?}");
                    }
                    current_rate = -current_rate.abs();

                    // Reversing playback direction already steps 1 frame in most cases.
                    // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/20
                    step += 1;
                }
            }

            // If a forwards step, or playback, is requested and we're not playing forwards, reverse
            // direction.
            if (step > 0 || set_playing == Some(true)) && current_rate < 0. {
                let _span = info_span!("set forwards").entered();
                if let Some(position) = pipeline.query_position::<gst::ClockTime>() {
                    debug!("changing playback direction to forwards");

                    if let Err(err) = pipeline.seek(
                        current_rate.abs(),
                        gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                        gst::SeekType::Set,
                        position,
                        gst::SeekType::End,
                        gst::ClockTime::ZERO,
                    ) {
                        warn!("error seeking: {err:?}");
                    }
                    current_rate = current_rate.abs();

                    // Reversing playback direction already steps 1 frame in most cases.
                    // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/20
                    step -= 1;
                }
            }

            // Step if requested.
            if step != 0 {
                let _span = info_span!("step").entered();
                debug!("stepping by {step} frames");

                pipeline.send_event(gst::event::Step::new(
                    gst::format::Buffers::from_u64(step.unsigned_abs()),
                    1.,
                    true,
                    false,
                ));
            }

            // Change playback rate if requested.
            if let Some(rate) = set_rate {
                if let Some(position) = pipeline.query_position::<gst::ClockTime>() {
                    let signed = if current_rate < 0. { -rate } else { rate };
                    let result = if signed > 0. {
                        pipeline.seek(
                            signed,
                            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                            gst::SeekType::Set,
                            position,
                            gst::SeekType::End,
                            gst::ClockTime::ZERO,
                        )
                    } else {
                        pipeline.seek(
                            signed,
                            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                            gst::SeekType::Set,
                            gst::ClockTime::ZERO,
                            gst::SeekType::Set,
                            position,
                        )
                    };
                    if result.is_ok() {
                        current_rate = signed;
                    }
                }
            }

            // Play/pause if requested.
            if let Some(play) = set_playing {
                let target_state = if play {
                    gst::State::Playing
                } else {
                    gst::State::Paused
                };

                let _span = info_span!("set state").entered();
                debug!("setting state to {target_state:?}");

                if let Err(err) = pipeline.set_state(target_state) {
                    warn!("error setting pipeline state to {target_state:?}: {err:?}");
                }
            }
        }

        // Set the state to Null before exiting.
        debug!("setting state to Null before exiting");
        let _span = info_span!("set state").entered();
        if let Err(err) = pipeline.set_state(gst::State::Null) {
            // I got this to return Err once by opening a file GStreamer couldn't play and a
            // regular video file.
            warn!("error setting pipeline state to Null: {err:?}");
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

    pub fn has_seeks_queued(&self) -> bool {
        self.imp().has_seeks_queued()
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}
