use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
};

pub use winit::event::MouseButton as WinitMouseButton;
pub use winit::keyboard::KeyCode as WinitKeyboardKey;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepeatState {
    times: i32,
}

impl RepeatState {
    fn press(&mut self) {
        self.times += 1
    }

    fn release(&mut self) {
        self.times = -1;
    }

    fn reset(&mut self) {
        self.times = 0;
    }

    pub fn is_pressed(&self) -> bool {
        self.times == 1
    }

    pub fn is_released(&self) -> bool {
        self.times == -1
    }

    pub fn is_down(&self) -> bool {
        self.times > 0
    }

    pub fn is_held(&self) -> bool {
        self.times > 1
    }
}

#[derive(Debug)]
pub struct EventInputBuffer<E: Clone + Copy> {
    buffer: VecDeque<InputState<E>>,
    frame: usize,
}

impl<E: Clone + Copy> Default for EventInputBuffer<E> {
    fn default() -> Self {
        Self {
            buffer: Default::default(),
            frame: Default::default(),
        }
    }
}

impl<E: Clone + Copy> EventInputBuffer<E> {
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::new(),
            frame: 0,
        }
    }

    pub fn release(&mut self) {
        self.buffer.iter_mut().for_each(|ev| {
            if ev.is_down() {
                ev.release();
            }
        });
    }

    pub fn frame(&mut self) {
        self.frame += 1;
    }

    pub fn next(&mut self) -> Option<InputState<E>> {
        let ev = self.buffer.pop_back();
        if let Some(mut ev) = ev.clone() {
            if ev.frame > self.frame {
                // end of current frame events reached
                return None;
            }

            if ev.times == 0 {
                return self.next();
            }

            if ev.times > 0 {
                ev.press();
                self.buffer.push_front(ev.next_frame());
            }
        }
        ev
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl<E: Clone + Copy> Iterator for EventInputBuffer<E> {
    type Item = InputState<E>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next()
    }
}

#[derive(Debug, Default)]
pub struct InputBuffer {
    keyboard: EventInputBuffer<WinitKeyboardKey>,
    mouse: EventInputBuffer<WinitMouseButton>,

    cursor: (f64, f64),
    cursor_delta: (f64, f64),
}

impl InputBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn frame(&mut self) {
        self.keyboard.frame();
        self.mouse.frame();
    }

    pub fn clear_all(&mut self) {
        self.keyboard.clear();
        self.mouse.clear();
    }

    pub fn release_all(&mut self) {
        self.release_keyboard();
        self.release_mouse();
    }

    pub fn release_keyboard(&mut self) {
        self.keyboard.release();
    }

    pub fn release_mouse(&mut self) {
        self.mouse.release();
    }

    pub fn keyboard(&mut self) -> &mut EventInputBuffer<WinitKeyboardKey> {
        &mut self.keyboard
    }

    pub fn mouse(&mut self) -> &mut EventInputBuffer<WinitMouseButton> {
        &mut self.mouse
    }

    pub fn cursor(&self) -> (f64, f64) {
        self.cursor
    }

    pub fn cursor_delta(&self) -> (f64, f64) {
        self.cursor_delta
    }

    pub fn x(&self) -> f64 {
        self.cursor.0
    }

    pub fn y(&self) -> f64 {
        self.cursor.1
    }

    pub fn x_delta(&self) -> f64 {
        self.cursor_delta.0
    }

    pub fn y_delta(&self) -> f64 {
        self.cursor_delta.1
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InputState<T>
where
    T: Clone + Copy,
{
    frame: usize,
    key: T,
    state: RepeatState,
}

impl<T> InputState<T>
where
    T: Clone + Copy,
{
    pub fn key(self) -> T {
        self.key
    }

    fn next_frame(self) -> Self {
        Self {
            frame: self.frame + 1,
            key: self.key,
            state: self.state,
        }
    }
}

impl<T> Deref for InputState<T>
where
    T: Clone + Copy,
{
    type Target = RepeatState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<T> DerefMut for InputState<T>
where
    T: Clone + Copy,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}
