use std::sync::atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};

const KEYBOARD_ENTRIES: usize = 512;
const MOUSE_ENTRIES: usize = 24;

const RELEASE_SIGNAL: u16 = 0xFFFF;
const MAX_HOLD_FRAMES: u16 = 0xFFFF - 1;

const INPUT_QUEUE_SECTIONS: usize = 4;
const INPUT_QUEUE_FOLDS: usize = 8;
const INPUT_QUEUE_FOLD_CAP: usize = size_of::<AtomicU64>() / size_of::<InputSyncPacket>();
const INPUT_QUEUE_SECTION_CAP: usize = INPUT_QUEUE_FOLD_CAP * INPUT_QUEUE_FOLDS;

#[derive(Clone, Copy, Debug)]
pub enum InputSyncPacket {
    Keyboard {
        code: KeyboardKeyCode,
        down: bool,
    },
    Mouse {
        button: MouseButtonIndex,
        down: bool,
    },
}

pub struct InputSyncBuffer {
    // a fold can contain 2 packets each - atomic u128 seems to not exist
    // the index of a fold is calculated like:
    // fold_i = floor(inner_index / 2)
    // fold_offset = mod(inner_index, 2)
    queue: [[AtomicU64; INPUT_QUEUE_FOLDS]; INPUT_QUEUE_SECTIONS],

    head: InputSyncIndex,
    tail: InputSyncIndex,
    //todo
}

pub struct InputSyncIndex(AtomicU16);

// bit shift tests (used vscodium 'value of literal' tooltip)
// will remove
const SEPARATE: u16 = (4 as u16) | (2 as u16);
const ENCODED: u16 = (0x4u8 as u16) << 8 | 0x2u8 as u16;
const DECODE_INNER_INDEX: u8 = (0x402 >> 8 & 0x00FF) as u8;
const DECODE_SECTION: u8 = (0x402 & 0x00FF) as u8;

impl InputSyncIndex {
    pub fn new(inner_index: u8, section: u8) -> Self {
        let encoded = (inner_index as u16) << 8 | section as u16;
        Self(AtomicU16::new(encoded))
    }

    pub fn get_encoded(&self) -> u16 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn index_and_section(&self) -> (u8, u8) {
        let encoded = self.get_encoded();
        let inner_index = (encoded >> 8 & 0x00FF) as u8;
        let section = (encoded & 0x00FF) as u8;
        (inner_index, section)
    }

    //todo
}

// todo

#[derive(Debug, Default)]
pub struct InputState {
    keys: Keys,
    cursor: Cursor,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

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

#[derive(Debug)]
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

    pub fn handle_key_events(&mut self, event: &winit::event::WindowEvent) {
        use winit::event::{ElementState, WindowEvent};
        use winit::keyboard::PhysicalKey;

        match event {
            WindowEvent::KeyboardInput { event: key, .. } => {
                if let PhysicalKey::Code(code) = key.physical_key {
                    let code = {
                        let key_code: KeyboardKeyCode = code.into();
                        u16::from(key_code)
                    } as usize;

                    match key.state {
                        ElementState::Pressed => {
                            if self.keyboard[code] == 0 {
                                self.keyboard[code] = 1;
                            }
                        }
                        ElementState::Released => {
                            if self.keyboard[code] > 0 && self.keyboard[code] != RELEASE_SIGNAL {
                                self.keyboard[code] = RELEASE_SIGNAL;
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let code = {
                    let button_index: MouseButtonIndex = (*button).into();
                    u16::from(button_index)
                } as usize;

                match state {
                    ElementState::Pressed => {
                        if self.mouse[code] == 0 {
                            self.mouse[code] = 1;
                        }
                    }
                    ElementState::Released => {
                        if self.mouse[code] > 0 && self.mouse[code] != RELEASE_SIGNAL {
                            self.mouse[code] = RELEASE_SIGNAL;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    #[inline(always)]
    pub fn key_frames(&self, code: winit::keyboard::KeyCode) -> u16 {
        self.keyboard[code as usize]
    }

    #[inline(always)]
    pub fn key_down(&self, code: winit::keyboard::KeyCode) -> bool {
        self.key_frames(code) != 0
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
    pub fn mouse_frames(&self, code: winit::event::MouseButton) -> u16 {
        let code = {
            let button_index: MouseButtonIndex = code.into();
            u16::from(button_index)
        };

        self.mouse[code as usize]
    }

    #[inline(always)]
    pub fn mouse_down(&self, code: winit::event::MouseButton) -> bool {
        self.mouse_frames(code) != 0
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
    pub fn mouse_held(&self, code: winit::event::MouseButton) -> bool {
        self.mouse_frames(code) > 1
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct KeyboardKeyCode(u16);

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
struct MouseButtonIndex(u16);

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

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Cursor {
    current: (f64, f64),
    delta: (f64, f64),
}

impl Cursor {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn update(&mut self) {
        self.delta = Default::default();
    }

    pub fn handle_cursor_events(&mut self, event: &winit::event::WindowEvent) {
        match event {
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                self.current = (position.x, position.y);
            }
            _ => {}
        }
    }

    pub fn handle_cursor_device_events(&mut self, event: &winit::event::DeviceEvent) {
        match event {
            winit::event::DeviceEvent::MouseMotion { delta } => {
                self.delta = *delta;
            }
            _ => {}
        }
    }

    #[inline(always)]
    pub fn current(&self) -> (f64, f64) {
        self.current
    }

    #[inline(always)]
    pub fn current_f32(&self) -> (f32, f32) {
        (self.current.0 as f32, self.current.1 as f32)
    }

    #[inline(always)]
    pub fn delta(&self) -> (f64, f64) {
        self.delta
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
