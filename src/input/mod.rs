pub mod stream;

use std::sync::Arc;

pub use stream::{DeltaPacket, IterInputStream};
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

use crate::input::stream::InputStream;
use crate::sync;

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
    let cursor_abs = state.snapshot.cursor.current.clone();
    let cursor_delta = state.snapshot.cursor.delta.clone();
    let dispatcher = InputDispatcher {
        stream,
        cursor_abs,
        cursor_delta,
    };

    (state, dispatcher)
}

#[derive(Clone, Debug, Default)]
pub struct InputDispatcher<const SLOTS: usize, const SECTIONS: usize> {
    stream: Arc<InputStream<SLOTS, SECTIONS>>,

    cursor_delta: sync::Mirror<(f64, f64)>,
    cursor_abs: sync::Mirror<(f64, f64)>,
}

impl<const SLOTS: usize, const SECTIONS: usize> InputDispatcher<SLOTS, SECTIONS> {
    pub fn sync(&mut self) {
        self.stream.frame_front();
        self.cursor_delta.publish((0.0, 0.0));
    }

    pub fn handle_cursor_events(&mut self, event: &winit::event::WindowEvent) {
        match event {
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                self.cursor_abs.publish((position.x, position.y));
            }
            _ => {}
        }
    }

    pub fn handle_raw_cursor_events(&mut self, event: &winit::event::DeviceEvent) {
        match event {
            winit::event::DeviceEvent::MouseMotion { delta } => {
                self.cursor_delta.publish(*delta);
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

#[derive(Clone, Debug, Default)]
pub struct InputState<const SLOTS: usize, const SECTIONS: usize> {
    snapshot: InputSnapshot,
    stream: Arc<InputStream<SLOTS, SECTIONS>>,
}

impl<const SLOTS: usize, const SECTIONS: usize> InputState<SLOTS, SECTIONS> {
    pub fn sync(&mut self) {
        self.snapshot.cursor.sync();
        self.snapshot.keys.update();
        self.stream.frame_back();
    }

    pub fn poll_key_events(&mut self) {
        self.stream
            .drain_back()
            .for_each(|ev| self.snapshot.keys.press_change(ev));
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

#[derive(Clone, Debug, Default)]
pub struct InputSnapshot {
    keys: Keys,
    cursor: Cursor,
}

impl InputSnapshot {
    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    pub fn keys_mut(&mut self) -> &mut Keys {
        &mut self.keys
    }

    pub fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    pub fn disjoint_mut(&mut self) -> (&mut Keys, &mut Cursor) {
        (&mut self.keys, &mut self.cursor)
    }
}

#[derive(Clone, Debug)]
pub struct Keys {
    keyboard: [u16; KEYBOARD_ENTRIES],
    mouse: [u16; MOUSE_ENTRIES],
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
                let code = u16::from(code) as usize;
                if down {
                    if self.keyboard[code] == 0 {
                        self.keyboard[code] = 1;
                    }
                } else {
                    if self.keyboard[code] > 0 && self.keyboard[code] != RELEASE_SIGNAL {
                        self.keyboard[code] = RELEASE_SIGNAL;
                    }
                }
            }
            DeltaPacket::Mouse { button, down } => {
                let code = u16::from(button) as usize;
                if down {
                    if self.mouse[code] == 0 {
                        self.mouse[code] = 1;
                    }
                } else {
                    if self.mouse[code] > 0 && self.mouse[code] != RELEASE_SIGNAL {
                        self.mouse[code] = RELEASE_SIGNAL;
                    }
                }
            }
        }
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

#[derive(Clone, Debug, Default)]
pub struct Cursor {
    current: sync::Mirror<(f64, f64)>,
    delta: sync::Mirror<(f64, f64)>,
}

impl Cursor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sync(&mut self) {
        let _ = self.current.sync();
        let _ = self.delta.sync();
    }

    #[inline(always)]
    pub fn update(&mut self) {
        self.delta = Default::default();
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
