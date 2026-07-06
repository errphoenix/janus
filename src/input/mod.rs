pub mod stream;

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub use stream::{DeltaPacket, IterInputStream};
pub use winit::event::MouseButton;
use winit::event::MouseScrollDelta;
pub use winit::keyboard::KeyCode;

use crate::{input::stream::InputStream, sync};

const KEYBOARD_ENTRIES: usize = 512;
const MOUSE_ENTRIES: usize = 24;

const RELEASE_SIGNAL: u16 = 0xFFFF;
const MAX_HOLD_FRAMES: u16 = 0xFFFF - 1;

pub const SLOT_COUNT: usize = 12;
pub const SECTION_COUNT: usize = 6;

pub fn stream<const SLOTS: usize, const SECTIONS: usize>() -> (
    InputState<SLOTS, SECTIONS>,
    InputDispatcher<SLOTS, SECTIONS>,
) {
    let stream = Arc::new(InputStream::new());

    let state = InputState {
        stream: Arc::clone(&stream),
        ..Default::default()
    };

    // clones local value and shared arc data used to sync
    let cursor_options = state.cursor_options.clone();
    let cursor = state.snapshot.cursor.clone();
    let mouse_wheel = state.snapshot.mouse_wheel.clone();

    let dispatcher = InputDispatcher {
        stream,
        cursor_options,
        cursor,
        mouse_wheel,
    };

    (state, dispatcher)
}

type CursorValues = (f64, f64);
type MouseWheelValue = f32;

/// This is the proper "owner" of the input synchronisation structures,
/// despite shared ownership.
///
/// This is the only place where [`TriCell::advance`] or similar main
/// synchronisation duties should occur.
#[derive(Debug, Default)]
pub struct InputDispatcher<const SLOTS: usize, const SECTIONS: usize> {
    stream: Arc<InputStream<SLOTS, SECTIONS>>,

    cursor_options: Arc<CursorOptions>,
    cursor: Arc<Cursor>,
    mouse_wheel: Arc<sync::TriCell<MouseWheelValue>>,
}

impl<const SLOTS: usize, const SECTIONS: usize> InputDispatcher<SLOTS, SECTIONS> {
    pub fn sync(&mut self) {
        self.stream.frame_front();

        let cursor_abs = self.cursor.current.get();

        let _ = self.cursor.current.advance();
        let _ = self.cursor.delta.advance();
        let _ = self.mouse_wheel.advance();

        self.cursor.current.set(cursor_abs);
        self.cursor.delta.set((0.0, 0.0));
        self.mouse_wheel.set(0.0);

        // cursor options handled separately
    }

    pub fn cursor_options(&self) -> &CursorOptions {
        &self.cursor_options
    }

    pub fn cursor_options_shared(&self) -> &Arc<CursorOptions> {
        &self.cursor_options
    }

    pub fn handle_mouse_events(&mut self, event: &winit::event::WindowEvent) {
        match event {
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                self.cursor.current.set((position.x, position.y));
            }
            winit::event::WindowEvent::MouseWheel {
                delta: MouseScrollDelta::LineDelta(_, delta),
                ..
            } => {
                self.mouse_wheel.set_with(|d| d + delta);
            }
            _ => {}
        }
    }

    pub fn handle_raw_cursor_events(&mut self, event: &winit::event::DeviceEvent) {
        match event {
            winit::event::DeviceEvent::MouseMotion { delta: (dx, dy) } => {
                self.cursor
                    .delta
                    .set_with(|(odx, ody)| (odx + dx, ody + dy));
            }
            _ => {}
        }
    }

    pub fn handle_key_event(&mut self, event: &winit::event::WindowEvent) {
        use winit::event::{ElementState, WindowEvent};
        use winit::keyboard::PhysicalKey;

        match event {
            WindowEvent::KeyboardInput { event: key, .. } => {
                if let PhysicalKey::Code(code) = key.physical_key {
                    let code: KeyboardKeyCode = code.into();
                    let down = matches!(key.state, ElementState::Pressed);
                    self.stream.push_front(DeltaPacket::Keyboard { code, down });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button: MouseButtonIndex = (*button).into();
                let down = matches!(*state, ElementState::Pressed);
                self.stream.push_front(DeltaPacket::Mouse { button, down });
            }
            _ => {}
        }
    }
}

// todo: change to single AtomicU8
#[derive(Debug, Default)]
pub struct CursorOptions {
    pub grabbed: AtomicBool,
    pub dirty: AtomicBool,
}

impl CursorOptions {
    pub fn check_grabbed(&self) -> bool {
        self.grabbed.load(Ordering::Relaxed)
    }

    pub fn check_dirty(&self) -> bool {
        self.grabbed.load(Ordering::Relaxed)
    }

    pub fn set_grabbed(&self, grabbed: bool) {
        let changed = self
            .grabbed
            .compare_exchange(!grabbed, grabbed, Ordering::Acquire, Ordering::Relaxed)
            .is_ok();

        if changed {
            self.dirty.store(true, Ordering::Relaxed);
        }
    }
}

/// Read-only view of the current state of the input as received from the
/// window thread.
#[derive(Debug, Default)]
pub struct InputState<const SLOTS: usize, const SECTIONS: usize> {
    snapshot: InputSnapshot,
    cursor_options: Arc<CursorOptions>,
    stream: Arc<InputStream<SLOTS, SECTIONS>>,
}

impl<const SLOTS: usize, const SECTIONS: usize> InputState<SLOTS, SECTIONS> {
    pub fn cursor_options(&self) -> &Arc<CursorOptions> {
        &self.cursor_options
    }

    pub fn sync(&mut self) {
        self.snapshot.keys.update();
        self.stream.frame_back();

        // cursor options handled separately
    }

    pub fn poll_key_events(&mut self) {
        self.stream
            .drain_back()
            .for_each(|ev| self.snapshot.keys.press_change(ev));
    }

    pub fn mouse_wheel(&self) -> MouseWheelValue {
        self.snapshot.mouse_wheel()
    }

    pub fn pop_key_event(&mut self) -> Option<KeyEvent> {
        self.snapshot.keys.pop_key_event()
    }

    pub fn keys(&self) -> &Keys {
        &self.snapshot.keys
    }

    pub fn cursor(&self) -> &Cursor {
        &self.snapshot.cursor
    }

    pub fn snapshot(&self) -> &InputSnapshot {
        &self.snapshot
    }
}

#[derive(Debug, Default)]
pub struct InputSnapshot {
    keys: Keys,
    cursor: Arc<Cursor>,
    mouse_wheel: Arc<sync::TriCell<MouseWheelValue>>,
}

impl InputSnapshot {
    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    pub fn mouse_wheel(&self) -> MouseWheelValue {
        self.mouse_wheel.get()
    }

    pub fn cursor_shared(&self) -> &Arc<Cursor> {
        &self.cursor
    }

    pub fn mouse_wheel_shared(&self) -> &Arc<sync::TriCell<MouseWheelValue>> {
        &self.mouse_wheel
    }

    pub fn keys_mut(&mut self) -> &mut Keys {
        &mut self.keys
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyEvent {
    Mouse {
        code: u16,
        release: bool,
        press_time: u32,
    },
    Keyboard {
        code: u16,
        release: bool,
        press_time: u32,
    },
}
impl KeyEvent {
    pub const fn is_mouse(self) -> bool {
        matches!(self, KeyEvent::Mouse { .. })
    }

    pub const fn is_keyboard(self) -> bool {
        matches!(self, KeyEvent::Keyboard { .. })
    }

    pub const fn code(self) -> u16 {
        match self {
            KeyEvent::Mouse { code, .. } => code,
            KeyEvent::Keyboard { code, .. } => code,
        }
    }

    pub const fn is_released(self) -> bool {
        match self {
            KeyEvent::Mouse { release, .. } => release,
            KeyEvent::Keyboard { release, .. } => release,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Keys {
    /// index is represented by key id, value is held frames
    keyboard: [u16; KEYBOARD_ENTRIES],
    /// index is represented by button id, value is held frames
    mouse: [u16; MOUSE_ENTRIES],
    local_key_queue: VecDeque<KeyEvent>,
}
impl Default for Keys {
    fn default() -> Self {
        Self::new()
    }
}
impl Keys {
    pub fn new() -> Self {
        Self {
            keyboard: [0u16; KEYBOARD_ENTRIES],
            mouse: [0u16; MOUSE_ENTRIES],
            local_key_queue: VecDeque::new(),
        }
    }

    pub fn update(&mut self) {
        for state in self.keyboard.iter_mut() {
            match *state {
                RELEASE_SIGNAL => *state = 0,
                1..=MAX_HOLD_FRAMES => *state += 1,
                _ => {}
            }
        }
        for state in self.mouse.iter_mut() {
            match *state {
                RELEASE_SIGNAL => *state = 0,
                1..=MAX_HOLD_FRAMES => *state += 1,
                _ => {}
            }
        }
    }

    pub fn press_change(&mut self, delta: DeltaPacket) {
        match delta {
            DeltaPacket::Keyboard { code, down } => {
                let code = code.0;
                let index = u16::from(code) as usize;
                if down {
                    if self.keyboard[index] == 0 {
                        self.keyboard[index] = 1;
                    }
                    self.local_key_queue.push_back(KeyEvent::Keyboard {
                        code,
                        release: false,
                        press_time: self.keyboard[index] as u32,
                    });
                } else {
                    if self.keyboard[index] > 0 && self.keyboard[index] != RELEASE_SIGNAL {
                        self.keyboard[index] = RELEASE_SIGNAL;
                        self.local_key_queue.push_back(KeyEvent::Keyboard {
                            code: code,
                            release: true,
                            press_time: self.keyboard[index] as u32,
                        });
                    }
                }
            }
            DeltaPacket::Mouse { button, down } => {
                let code = button.0;
                let index = u16::from(button) as usize;
                if down {
                    if self.mouse[index] == 0 {
                        self.mouse[index] = 1;
                    }
                    self.local_key_queue.push_back(KeyEvent::Mouse {
                        code: code,
                        release: false,
                        press_time: self.mouse[index] as u32,
                    });
                } else {
                    if self.mouse[index] > 0 && self.mouse[index] != RELEASE_SIGNAL {
                        self.mouse[index] = RELEASE_SIGNAL;
                        self.local_key_queue.push_back(KeyEvent::Mouse {
                            code: code,
                            release: true,
                            press_time: self.mouse[index] as u32,
                        });
                    }
                }
            }
        }
    }

    pub fn pop_key_event(&mut self) -> Option<KeyEvent> {
        self.local_key_queue.pop_back()
    }

    #[inline(always)]
    pub fn key_frames(&self, code: winit::keyboard::KeyCode) -> u16 {
        self.keyboard[code as usize]
    }

    #[inline(always)]
    pub fn key_down(&self, code: winit::keyboard::KeyCode) -> bool {
        let frames = self.key_frames(code);
        frames != 0 && frames != RELEASE_SIGNAL
    }

    #[inline(always)]
    pub fn key_pressed(&self, code: winit::keyboard::KeyCode) -> bool {
        self.key_frames(code) == 1
    }

    #[inline(always)]
    pub fn key_released(&self, code: winit::keyboard::KeyCode) -> bool {
        self.key_frames(code) == RELEASE_SIGNAL
    }

    #[inline(always)]
    pub fn key_held(&self, code: winit::keyboard::KeyCode) -> bool {
        self.key_frames(code) > 1
    }

    #[inline(always)]
    fn mouse_code(&self, code: impl Into<MouseButtonIndex>) -> u16 {
        let button_index: MouseButtonIndex = code.into();
        u16::from(button_index)
    }

    #[inline(always)]
    fn mouse_frames(&self, code: winit::event::MouseButton) -> u16 {
        let code = self.mouse_code(code);
        self.mouse[code as usize]
    }

    #[inline(always)]
    pub fn mouse_frames_held(&self, code: winit::event::MouseButton) -> u16 {
        let frames = self.mouse_frames(code);
        if frames == RELEASE_SIGNAL {
            return 0;
        }
        frames
    }

    #[inline(always)]
    pub fn mouse_down(&self, code: winit::event::MouseButton) -> bool {
        let frames = self.mouse_frames(code);
        frames != 0 && frames != RELEASE_SIGNAL
    }

    #[inline(always)]
    pub fn mouse_pressed(&self, code: winit::event::MouseButton) -> bool {
        self.mouse_frames(code) == 1
    }

    #[inline(always)]
    pub fn mouse_released(&self, code: winit::event::MouseButton) -> bool {
        self.mouse_frames(code) == RELEASE_SIGNAL
    }

    #[inline(always)]
    pub fn mouse_held(&self, code: winit::event::MouseButton, frame_delay: u16) -> bool {
        self.mouse_frames_held(code) > frame_delay
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyboardKeyCode(u16);

impl From<KeyboardKeyCode> for u16 {
    #[inline(always)]
    fn from(value: KeyboardKeyCode) -> Self {
        value.0
    }
}

impl From<winit::keyboard::KeyCode> for KeyboardKeyCode {
    fn from(value: winit::keyboard::KeyCode) -> Self {
        Self((value as u16).min(KEYBOARD_ENTRIES as u16 - 1))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MouseButtonIndex(u16);

impl MouseButtonIndex {
    const LEFT: u16 = 0;
    const RIGHT: u16 = 1;
    const MIDDLE: u16 = 2;
    const BACK: u16 = 3;
    const FORWARD: u16 = 4;

    const OTHER_OFFSET: u16 = 5;
}

impl From<MouseButtonIndex> for u16 {
    #[inline(always)]
    fn from(value: MouseButtonIndex) -> Self {
        value.0
    }
}

impl From<winit::event::MouseButton> for MouseButtonIndex {
    #[inline(always)]
    fn from(value: winit::event::MouseButton) -> Self {
        match value {
            winit::event::MouseButton::Left => Self(Self::LEFT),
            winit::event::MouseButton::Right => Self(Self::RIGHT),
            winit::event::MouseButton::Middle => Self(Self::MIDDLE),
            winit::event::MouseButton::Back => Self(Self::FORWARD),
            winit::event::MouseButton::Forward => Self(Self::BACK),
            winit::event::MouseButton::Other(id) => {
                Self((id as u16 + Self::OTHER_OFFSET).min(MOUSE_ENTRIES as u16 - 1))
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Cursor {
    current: sync::TriCell<CursorValues>,
    delta: sync::TriCell<CursorValues>,
}

impl Cursor {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn current(&self) -> (f64, f64) {
        *self.current
    }

    #[inline(always)]
    pub fn current_f32(&self) -> (f32, f32) {
        (self.current.0 as f32, self.current.1 as f32)
    }

    #[inline(always)]
    pub fn delta(&self) -> (f64, f64) {
        *self.delta
    }

    #[inline(always)]
    pub fn delta_f32(&self) -> (f32, f32) {
        (self.delta.0 as f32, self.delta.1 as f32)
    }

    #[inline(always)]
    pub fn x(&self) -> f64 {
        self.current.0
    }

    #[inline(always)]
    pub fn y(&self) -> f64 {
        self.current.1
    }

    #[inline(always)]
    pub fn dx(&self) -> f64 {
        self.delta.0
    }

    #[inline(always)]
    pub fn dy(&self) -> f64 {
        self.delta.1
    }

    #[inline(always)]
    pub fn x_f32(&self) -> f32 {
        self.current.0 as f32
    }

    #[inline(always)]
    pub fn y_f32(&self) -> f32 {
        self.current.1 as f32
    }

    #[inline(always)]
    pub fn dx_f32(&self) -> f32 {
        self.delta.0 as f32
    }

    #[inline(always)]
    pub fn dy_f32(&self) -> f32 {
        self.delta.1 as f32
    }
}
