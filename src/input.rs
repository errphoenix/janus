const KEYBOARD_ENTRIES: usize = 512;
const MOUSE_ENTRIES: usize = 24;

const RELEASE_SIGNAL: u16 = 0xFFFF;
const MAX_HOLD_FRAMES: u16 = 0xFFFF - 1;

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
                    let code = code as usize;
                    if code < KEYBOARD_ENTRIES {
                        match key.state {
                            ElementState::Pressed => {
                                if self.keyboard[code] == 0 {
                                    self.keyboard[code] = 1;
                                }
                            }
                            ElementState::Released => {
                                if self.keyboard[code] > 0 && self.keyboard[code] != RELEASE_SIGNAL
                                {
                                    self.keyboard[code] = RELEASE_SIGNAL;
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let code = {
                    let button_index: MouseButtonIndex = (*button).into();
                    usize::from(button_index)
                };

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

    pub fn key_frames(&self, code: winit::keyboard::KeyCode) -> u16 {
        self.keyboard[code as usize]
    }

    pub fn key_down(&self, code: winit::keyboard::KeyCode) -> bool {
        self.key_frames(code) != 0
    }

    pub fn key_pressed(&self, code: winit::keyboard::KeyCode) -> bool {
        self.key_frames(code) == 1
    }

    pub fn key_released(&self, code: winit::keyboard::KeyCode) -> bool {
        self.key_frames(code) == RELEASE_SIGNAL
    }

    pub fn key_held(&self, code: winit::keyboard::KeyCode) -> bool {
        self.key_frames(code) > 1
    }

    pub fn mouse_frames(&self, code: winit::event::MouseButton) -> u16 {
        let code = {
            let button_index: MouseButtonIndex = code.into();
            usize::from(button_index)
        };

        self.mouse[code]
    }

    pub fn mouse_down(&self, code: winit::event::MouseButton) -> bool {
        self.mouse_frames(code) != 0
    }

    pub fn mouse_pressed(&self, code: winit::event::MouseButton) -> bool {
        self.mouse_frames(code) == 1
    }

    pub fn mouse_released(&self, code: winit::event::MouseButton) -> bool {
        self.mouse_frames(code) == RELEASE_SIGNAL
    }

    pub fn mouse_held(&self, code: winit::event::MouseButton) -> bool {
        self.mouse_frames(code) > 1
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MouseButtonIndex(usize);

impl MouseButtonIndex {
    const LEFT: usize = 0;
    const RIGHT: usize = 1;
    const MIDDLE: usize = 2;
    const BACK: usize = 3;
    const FORWARD: usize = 4;

    const OTHER_OFFSET: usize = 5;
}

impl From<MouseButtonIndex> for usize {
    fn from(value: MouseButtonIndex) -> Self {
        value.0
    }
}

impl From<winit::event::MouseButton> for MouseButtonIndex {
    fn from(value: winit::event::MouseButton) -> Self {
        match value {
            winit::event::MouseButton::Left => Self(Self::LEFT),
            winit::event::MouseButton::Right => Self(Self::RIGHT),
            winit::event::MouseButton::Middle => Self(Self::MIDDLE),
            winit::event::MouseButton::Back => Self(Self::FORWARD),
            winit::event::MouseButton::Forward => Self(Self::BACK),
            winit::event::MouseButton::Other(id) => {
                Self((id as usize + Self::OTHER_OFFSET).min(MOUSE_ENTRIES - 1))
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

    pub fn current(&self) -> (f64, f64) {
        self.current
    }

    pub fn current_f32(&self) -> (f32, f32) {
        (self.current.0 as f32, self.current.1 as f32)
    }

    pub fn delta(&self) -> (f64, f64) {
        self.delta
    }

    pub fn delta_f32(&self) -> (f32, f32) {
        (self.delta.0 as f32, self.delta.1 as f32)
    }

    pub fn x(&self) -> f64 {
        self.current.0
    }

    pub fn y(&self) -> f64 {
        self.current.1
    }

    pub fn dx(&self) -> f64 {
        self.delta.0
    }

    pub fn dy(&self) -> f64 {
        self.delta.1
    }

    pub fn x_f32(&self) -> f32 {
        self.current.0 as f32
    }

    pub fn y_f32(&self) -> f32 {
        self.current.1 as f32
    }

    pub fn dx_f32(&self) -> f32 {
        self.delta.0 as f32
    }

    pub fn dy_f32(&self) -> f32 {
        self.delta.1 as f32
    }
}
