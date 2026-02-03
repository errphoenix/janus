use anyhow::Result;
use std::time::Instant;
use std::{ops::Deref, time::Duration};

#[cfg(feature = "render")]
use std::thread::JoinHandle;

#[cfg(feature = "input")]
use crate::input::{self, InputDispatcher as DispatchInput};

/// A stateful context defines only initialization logic (which should also
/// initialize the state) and loop logic.
#[cfg(feature = "render")]
pub type StatefulContext<Init, State> = Context<Init, State, EmptyRoutine>;

/// A stateful context defines only initialization logic (which should also
/// initialize the state) and loop logic.
#[cfg(not(feature = "render"))]
pub type StatefulContext<Init, State> = Context<Init, State>;

/// A rendering context is not aware of logical state, it only cares about
/// render logic and any state that may be related to it.
#[cfg(feature = "render")]
pub type RenderingContext<Render> = Context<EmptyRoutine, EmptyRoutine, Render>;

/// A dumb context has no state and no logic of any kind. It's only function is
/// that of creating an empty window.
///
/// This should only be used for testing or debugging.
#[cfg(feature = "render")]
pub type DumbContext = Context<EmptyRoutine, EmptyRoutine, EmptyRoutine>;

/// A dumb context has no state and no logic of any kind.
///
/// This should only be used for testing or debugging.
#[cfg(not(feature = "render"))]
pub type DumbContext = Context<EmptyRoutine, EmptyRoutine>;

#[cfg(feature = "input")]
type InputDispatcher = DispatchInput<{ input::SLOT_COUNT }, { input::SECTION_COUNT }>;

/// Stores glutin's context handles and implements winit's event handling.
///
/// This also makes use of 3 traits that define the application's routine:
/// * [`Setup`] which runs after the creation of the context and collaborates
///   to state initialisation.
/// * [`Update`] which defines the state and handles the logic to run every
///  'tick' in a continuous loop managed by the context.
/// * [`Draw`] which defines render logic to run every frame before the window
///   swaps buffers.
#[cfg(feature = "render")]
pub struct Context<Init, State, Render>
where
    Init: Setup<State, Render> + Sized,
    State: Update + Default + Sized + Sync + Send,
    Render: Draw + Default + Sized,
{
    pub(crate) init: Option<Init>,
    pub state_handle: StateHandle<State>,
    pub renderer: Render,

    #[cfg(feature = "input")]
    pub(crate) input_dispatcher: InputDispatcher,

    logic_thread: Option<JoinHandle<()>>,

    pub(crate) render_delta: DeltaCycle,

    #[cfg(feature = "render")]
    pub(crate) parameters: crate::window::DisplayParameters,
    #[cfg(feature = "render")]
    pub(crate) display: Option<crate::window::DisplayHandle>,
    #[cfg(feature = "render")]
    pub(crate) gl_ctx: Option<glutin::context::PossiblyCurrentContext>,
    #[cfg(feature = "render")]
    pub(crate) gl_display: crate::window::GlDisplayState,
}

#[cfg(feature = "render")]
impl<Init, State, Render> Drop for Context<Init, State, Render>
where
    Init: Setup<State, Render> + Sized,
    State: Update + Default + Sized + Sync + Send,
    Render: Draw + Default + Sized,
{
    fn drop(&mut self) {
        if let Some(thread) = self.logic_thread.take() {
            thread
                .join()
                .expect("logic thread has failed to join the main thread during context drop");
        }
    }
}

#[cfg(feature = "render")]
pub enum StateHandle<State>
where
    State: Update + Default + Sized + Sync + Send,
{
    Acquired(JoinHandle<()>),
    Preparing,
    Uninitialised(State),
}

/// Pure logic context manager.
///
/// This also makes use of 2 traits that define the application's routine:
/// * [`Setup`] which runs after the creation of the context and collaborates
///   to state initialisation.
/// * [`Update`] which defines the state and handles the logic to run every
///  'tick' in a continuous loop managed by the context.
#[cfg(not(feature = "render"))]
pub struct Context<Init, State>
where
    Init: Setup<State> + Sized,
    State: Update + Default + Sized,
{
    init: Option<Init>,
    pub state: State,

    delta: DeltaCycle,
}

#[cfg(not(feature = "render"))]
impl<Init, State> Context<Init, State>
where
    Init: Setup<State>,
    State: Update + Default,
{
    pub fn new(init: Init) -> Self {
        Self {
            init: Some(init),
            state: Default::default(),
            delta: Default::default(),
        }
    }
}

#[cfg(feature = "render")]
impl<Init, State, Render> Context<Init, State, Render>
where
    Init: Setup<State, Render>,
    State: Update + Default + Sync + Send + 'static,
    Render: Draw + Default,
{
    #[cfg(feature = "input")]
    pub fn new(
        init: Init,
        input_dispatcher: InputDispatcher,
        parameters: crate::window::DisplayParameters,
    ) -> Self {
        Self {
            init: Some(init),
            state_handle: StateHandle::Uninitialised(State::default()),
            renderer: Default::default(),

            input_dispatcher,

            logic_thread: None,
            render_delta: Default::default(),

            parameters,
            display: None,
            gl_ctx: None,
            gl_display: crate::window::GlDisplayState::Pending,
        }
    }

    #[cfg(not(feature = "input"))]
    pub fn new(init: Init, parameters: crate::window::DisplayParameters) -> Self {
        Self {
            init: Some(init),
            state_handle: StateHandle::Uninitialised(State::default()),
            renderer: Default::default(),

            logic_thread: None,
            render_delta: Default::default(),

            parameters,
            display: None,
            gl_ctx: None,
            gl_display: crate::window::GlDisplayState::Pending,
        }
    }

    pub(crate) fn initialise_thread(&mut self) {
        let state = std::mem::replace(&mut self.state_handle, StateHandle::Preparing);
        if let StateHandle::Uninitialised(mut state) = state {
            use tracing::{Level, event};

            let handle = std::thread::spawn(move || {
                let mut delta = {
                    let step = state.step_duration();
                    let now = Instant::now();
                    DeltaAccumulator::new(step, now)
                };

                let mut iter = 0;
                loop {
                    state.new_frame();

                    delta.accum();
                    while delta.overstep() {
                        if iter == 0 {
                            delta.set_step(state.step_duration());
                        }
                        state.update(delta.delta_step());
                        iter += 1;
                    }
                    if delta.step() > delta.accumulated() {
                        let ahead = delta.time_ahead();
                        std::thread::sleep(ahead * 3 / 4);

                        // todo: test/bench
                        //std::thread::yield_now();
                    }
                    iter = 0;
                }
            });
            self.state_handle = StateHandle::Acquired(handle);
            event!(
                name: "context.state-thread.acquire",
                Level::INFO,
                "State/logic thread successfully acquired application state."
            )
        } else {
            panic!(
                "state/logic thread could not be initialised: application state is already acquired"
            )
        }
    }

    pub(crate) fn build_attributes(&self) -> winit::window::WindowAttributes {
        use winit::{dpi::PhysicalSize, window::WindowAttributes};

        use crate::window::DisplayWindowMode;

        let attribs = WindowAttributes::default().with_title(self.parameters.title);
        match self.parameters.mode {
            DisplayWindowMode::Window => attribs.with_inner_size(PhysicalSize::new(
                self.parameters.width,
                self.parameters.height,
            )),
            DisplayWindowMode::FullScreen => {
                use winit::window::Fullscreen;

                attribs.with_fullscreen(Some(Fullscreen::Borderless(None)))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeltaCycle {
    last: Instant,
    delta: Duration,
}

#[derive(Clone, Debug, Default)]
pub struct DeltaAccumulator {
    step: Duration,
    accumulated: Duration,
    cycle: DeltaCycle,
}

impl DeltaAccumulator {
    pub fn new(step: Duration, start_time: Instant) -> Self {
        Self {
            step,
            cycle: DeltaCycle::new(start_time),
            ..Default::default()
        }
    }

    pub fn step(&self) -> Duration {
        self.step
    }

    pub fn set_step(&mut self, step: Duration) {
        self.step = step;
    }

    pub fn delta_cycle(&self) -> &DeltaCycle {
        &self.cycle
    }

    pub fn accumulated(&self) -> Duration {
        self.accumulated
    }

    /// Gets how "far ahead" the CPU is compared to the step speed.
    pub fn time_ahead(&self) -> Duration {
        self.step.saturating_sub(self.accumulated)
    }

    pub fn delta_step(&self) -> DeltaTime {
        self.step.into()
    }

    pub fn accum(&mut self) {
        self.cycle.sync();
        self.accumulated += self.cycle.delta_time();
    }

    pub fn overstep(&mut self) -> bool {
        let overstep = self.accumulated >= self.step;
        if overstep {
            self.accumulated -= self.step;
        }

        overstep
    }
}

impl Default for DeltaCycle {
    fn default() -> Self {
        Self {
            last: Instant::now(),
            delta: Default::default(),
        }
    }
}

impl DeltaCycle {
    pub fn new(start_time: Instant) -> Self {
        Self {
            last: start_time,
            ..Default::default()
        }
    }

    pub fn sync(&mut self) {
        let now = Instant::now();
        self.delta = now.duration_since(self.last);
        self.last = now;
    }

    pub fn delta_time(&self) -> Duration {
        self.delta
    }

    pub fn delta(&self) -> DeltaTime {
        self.delta.into()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DeltaTime(f64);

impl DeltaTime {
    pub fn as_f32(&self) -> f32 {
        self.0 as f32
    }

    pub fn as_f64(&self) -> f64 {
        self.0
    }
}

impl From<Duration> for DeltaTime {
    fn from(value: Duration) -> Self {
        DeltaTime(value.as_secs_f64())
    }
}

impl Deref for DeltaTime {
    type Target = f64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(not(feature = "render"))]
pub trait Setup<State>
where
    State: Update + Default,
{
    fn init(self, state: &mut State) -> Result<()>
    where
        Self: Sized;
}

#[cfg(feature = "render")]
pub trait Setup<State, Render>
where
    State: Update + Default,
    Render: Draw + Default,
{
    fn init(self, state: &mut State, renderer: &mut Render) -> Result<()>
    where
        Self: Sized;
}

pub trait Update {
    fn step_duration(&self) -> Duration;

    fn set_step_duration(&mut self, step: Duration);

    fn update(&mut self, delta: DeltaTime);

    /// Arbitrary logic to run when a new logic frame is started.
    ///
    /// This is guaranteed to run every frame, before the application's
    /// [`update cycle`]. It is still called independently of whether the
    /// [`update cycle`] runs or not due to [`delta accumulation`] conditions.
    ///
    /// Multiple iterations of the [`update cycle`] may occur in between
    /// `new_frame` calls due to [`delta accumulation`] conditions.
    ///
    /// If using the [`input`] features, this is where you will want your
    /// [`input frame sync`](crate::input::InputState::sync) to happen.
    ///
    /// [`update cycle`]: Update::update
    /// [`delta accumulation`]: DeltaAccumulator
    fn new_frame(&mut self) {}
}

#[cfg(feature = "render")]
pub trait Draw {
    fn draw(&mut self, delta: DeltaTime);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyRoutine;

#[cfg(feature = "render")]
impl<State, Render> Setup<State, Render> for EmptyRoutine
where
    State: Update + Default,
    Render: Draw + Default,
{
    fn init(self, _: &mut State, _: &mut Render) -> Result<()>
    where
        Self: Sized,
    {
        Ok(())
    }
}

impl Update for EmptyRoutine {
    fn update(&mut self, _: DeltaTime) {}

    fn step_duration(&self) -> Duration {
        Duration::default()
    }

    fn set_step_duration(&mut self, _: Duration) {}
}

#[cfg(feature = "render")]
impl Draw for EmptyRoutine {
    fn draw(&mut self, _: DeltaTime) {}
}

#[cfg(feature = "render")]
impl<State, Render, F> Setup<State, Render> for F
where
    State: Update + Default,
    Render: Draw + Default,
    F: FnOnce(&mut State, &mut Render) -> Result<()>,
{
    fn init(self, state: &mut State, renderer: &mut Render) -> Result<()>
    where
        Self: Sized,
    {
        self(state, renderer)
    }
}

#[cfg(not(feature = "render"))]
impl<State, F> Setup<State> for F
where
    State: Update + Default,
    F: FnOnce(&mut State) -> Result<()>,
{
    fn init(self, state: &mut State) -> Result<()>
    where
        Self: Sized,
    {
        self(state)
    }
}

#[cfg(not(feature = "render"))]
impl<State> Setup<State> for EmptyRoutine
where
    State: Update + Default,
{
    fn init(self, _: &mut State) -> Result<()>
    where
        Self: Sized,
    {
        Ok(())
    }
}
