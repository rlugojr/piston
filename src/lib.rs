//! A generic event loop for games and interactive applications

#![deny(missing_docs)]
#![deny(missing_copy_implementations)]
#![feature(old_io)]
#![feature(std_misc)]

extern crate clock_ticks;
#[macro_use]
extern crate quack;

use std::old_io::timer::sleep;
use std::time::duration::Duration;
use quack::{ Associative, ActOn, Action, GetFrom, Get, Pair };
use std::cmp;
use std::marker::{ PhantomData };

/// Required to use the event loop.
pub trait Window {
    type Event;

    /// Returns true if window should close.
    fn should_close(&self) -> bool;

    /// Gets the size of the window in user coordinates.
    fn size(&self) -> [u32; 2];

    /// Swaps render buffers.
    fn swap_buffers(&mut self);

    /// Polls event from window.
    fn poll_event(&mut self) -> Option<Self::Event>;
}

impl<T> Window for T
    where
        (PollEvent, T): Pair<Data = PollEvent, Object = T>
            + Associative 
            + ActOn<Result = Option<<(PollEvent, T) as quack::Associative>::Type>>,
        (ShouldClose, T): Pair<Data = ShouldClose, Object = T>
            + GetFrom,
        (SwapBuffers, T): Pair<Data = SwapBuffers, Object = T>
            + ActOn,
        (Size, T): Pair<Data = Size, Object = T>
            + GetFrom
{
    type Event = <(PollEvent, T) as Associative>::Type;

    #[inline(always)]
    fn should_close(&self) -> bool {
        let ShouldClose(val) = self.get();
        val
    }

    #[inline(always)]
    fn size(&self) -> [u32; 2] {
        let Size(size) = self.get();
        size
    }

    #[inline(always)]
    fn swap_buffers(&mut self) {
        self.action(SwapBuffers);
    }

    #[inline(always)]
    fn poll_event(&mut self) -> Option<<Self as Window>::Event> {
        self.action(PollEvent)
    }
}

/// Whether window should close or not.
#[derive(Copy)]
pub struct ShouldClose(pub bool);

impl Sized for ShouldClose {}

/// The size of the window.
#[derive(Copy)]
pub struct Size(pub [u32; 2]);

impl Sized for Size {}

/// Tells window to swap buffers.
///
/// ~~~ignore
/// use current::Action;
///
/// ...
/// window.action(SwapBuffers);
/// ~~~
#[derive(Copy)]
pub struct SwapBuffers;

impl Sized for SwapBuffers {}

/// Polls event from window.
///
/// ~~~ignore
/// use current::Action;
///
/// ...
/// let e = window.action(PollEvent);
/// ~~~
#[derive(Copy)]
pub struct PollEvent;

impl Sized for PollEvent {}

/// Render arguments
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct RenderArgs {
    /// Extrapolated time in seconds, used to do smooth animation.
    pub ext_dt: f64,
    /// The width of rendered area.
    pub width: u32,
    /// The height of rendered area.
    pub height: u32,
}

/// Update arguments, such as delta time in seconds
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct UpdateArgs {
    /// Delta time in seconds.
    pub dt: f64,
}

/// Idle arguments, such as expected idle time in seconds.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct IdleArgs {
    /// Expected idle time in seconds.
    pub dt: f64
}

/// Methods required to map from consumed event to emitted event.
pub trait EventMap<I> {
    /// Creates a render event.
    fn render(args: RenderArgs) -> Self;
    /// Creates an update event.
    fn update(args: UpdateArgs) -> Self;
    /// Creates an input event.
    fn input(args: I) -> Self;
    /// Creates an idle event.
    fn idle(IdleArgs) -> Self;
}

/// Tells whether last emitted event was idle or not.
#[derive(Copy, Debug, PartialEq, Eq)]
enum Idle {
    No,
    Yes
}

#[derive(Copy, Debug)]
enum State {
    Render,
    SwapBuffers,
    UpdateLoop(Idle),
    HandleEvents,
    Update,
}

/// The number of updates per second
///
/// This is the fixed update rate on average over time.
/// If the event loop lags, it will try to catch up.
#[derive(Copy)]
pub struct Ups(pub u64);

quack_set! {
    events: Events[W, I, E]
    fn (ups: Ups) [] {
        let frames = ups.0;
        events.dt_update_in_ns = BILLION / frames;
        events.dt = 1.0 / frames as f64;
    }
}

/// The maximum number of frames per second
///
/// The frame rate can be lower because the
/// next frame is always scheduled from the previous frame.
/// This causes the frames to "slip" over time.
#[derive(Copy)]
pub struct MaxFps(pub u64);

quack_set! {
    this: Events[W, I, E]
    fn (max_fps: MaxFps) [] {
        this.dt_frame_in_ns = BILLION / max_fps.0;
    }
}

/// An event loop iterator
///
/// *Warning: Because the iterator polls events from the window back-end,
/// it must be used on the same thread as the window back-end (usually main thread),
/// unless the window back-end supports multi-thread event polling.*
///
/// Example:
///
/// ~~~ignore
/// fn main() {
///     let opengl = shader_version::opengl::OpenGL_3_2;
///     let window = Sdl2Window::new(
///         opengl,
///         WindowSettings {
///             title: "Example".to_string(),
///             size: [500, 500],
///             fullscreen: false,
///             exit_on_esc: true,
///             samples: 0,
///         }
///     )
///     let ref mut gl = Gl::new();
///     let window = RefCell::new(window);
///     for e in Events::new(&window)
///         .set(Ups(120))
///         .set(MaxFps(60)) {
///         use event::RenderEvent;
///         e.render(|args| {
///             // Set the viewport in window to render graphics.
///             gl.viewport(0, 0, args.width as i32, args.height as i32);
///             // Create graphics context with absolute coordinates.
///             let c = Context::abs(args.width as f64, args.height as f64);
///             // Do rendering here.
///         });
///     }
/// }
/// ~~~
pub struct Events<W, I, E> {
    window: W,
    state: State,
    last_update: u64,
    last_frame: u64,
    dt_update_in_ns: u64,
    dt_frame_in_ns: u64,
    dt: f64,
    _marker_i: PhantomData<I>,
    _marker_e: PhantomData<E>,
}

static BILLION: u64 = 1_000_000_000;

/// The default updates per second.
pub const DEFAULT_UPS: Ups = Ups(120);
/// The default maximum frames per second.
pub const DEFAULT_MAX_FPS: MaxFps = MaxFps(60);

impl<W, I, E> Events<W, I, E> {
    /// Creates a new event iterator with default UPS and FPS settings.
    pub fn new(window: W) -> Events<W, I, E> {
        let start = clock_ticks::precise_time_ns();
        let Ups(updates_per_second) = DEFAULT_UPS;
        let MaxFps(max_frames_per_second) = DEFAULT_MAX_FPS;
        Events {
            window: window,
            state: State::Render,
            last_update: start,
            last_frame: start,
            dt_update_in_ns: BILLION / updates_per_second,
            dt_frame_in_ns: BILLION / max_frames_per_second,
            dt: 1.0 / updates_per_second as f64,
            _marker_i: PhantomData,
            _marker_e: PhantomData,
        }
    }
}

impl<W, I, E>
Iterator
for Events<W, I, E>
    where
        W: Window<Event = I>,
        E: EventMap<I>,
{
    type Item = E;

    /// Returns the next game event.
    fn next(&mut self) -> Option<E> {
        loop {
            self.state = match self.state {
                State::Render => {
                    if self.window.should_close() { return None; }

                    let start_render = clock_ticks::precise_time_ns();
                    self.last_frame = start_render;

                    let [w, h] = self.window.size();
                    if w != 0 && h != 0 {
                        // Swap buffers next time.
                        self.state = State::SwapBuffers;
                        return Some(EventMap::render(RenderArgs {
                            // Extrapolate time forward to allow smooth motion.
                            ext_dt: (start_render - self.last_update) as f64
                                    / BILLION as f64,
                            width: w,
                            height: h,
                        }));
                    }

                    State::UpdateLoop(Idle::No)
                }
                State::SwapBuffers => {
                    self.window.swap_buffers();
                    State::UpdateLoop(Idle::No)
                }
                State::UpdateLoop(ref mut idle) => {
                    let current_time = clock_ticks::precise_time_ns();
                    let next_frame = self.last_frame + self.dt_frame_in_ns;
                    let next_update = self.last_update + self.dt_update_in_ns;
                    let next_event = cmp::min(next_frame, next_update);
                    if next_event > current_time {
                        if let Some(x) = self.window.poll_event() {
                            *idle = Idle::No;
                            return Some(EventMap::input(x));
                        } else if *idle == Idle::No {
                            *idle = Idle::Yes;
                            let seconds = ((next_event - current_time) as f64) / (BILLION as f64);
                            return Some(EventMap::idle(IdleArgs { dt: seconds }))
                        }
                        sleep( Duration::nanoseconds((next_event - current_time) as i64) );
                        State::UpdateLoop(Idle::No)
                    } else if next_event == next_frame {
                        State::Render
                    } else {
                        State::HandleEvents
                    }
                }
                State::HandleEvents => {
                    // Handle all events before updating.
                    match self.window.poll_event() {
                        None => State::Update,
                        Some(x) => { return Some(EventMap::input(x)); },
                    }
                }
                State::Update => {
                    self.state = State::UpdateLoop(Idle::No);
                    self.last_update += self.dt_update_in_ns;
                    return Some(EventMap::update(UpdateArgs{ dt: self.dt }));
                }
            };
        }
    }
}
