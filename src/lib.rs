#[cfg(feature = "state")]
pub mod context;

#[cfg(feature = "input")]
pub mod input;

#[cfg(feature = "textures")]
pub mod texture;

#[cfg(feature = "render")]
pub mod window;

#[cfg(feature = "jobs")]
pub mod jobs;

pub mod sync;

#[cfg(all(feature = "render", feature = "state"))]
pub fn run<Init, State, Render>(mut context: Context<Init, State, Render>)
where
    Init: Setup<State, Render>,
    State: Update + Default + Sync + Send + 'static,
    Render: Draw + Default,
{
    let ev_loop = EventLoop::new().unwrap();
    ev_loop.set_control_flow(ControlFlow::Poll);

    let _ = ev_loop.run_app(&mut context);
}

use std::hash::{BuildHasherDefault, Hasher};

#[cfg(feature = "state")]
use context::{Context, Setup, Update};

#[cfg(all(feature = "state", feature = "render"))]
use context::Draw;
#[cfg(all(feature = "state", feature = "render"))]
use winit::event_loop::{ControlFlow, EventLoop};

#[cfg(all(not(feature = "render"), feature = "state"))]
pub fn run<Init, State>(mut _context: Context<Init, State>)
where
    Init: Setup<State>,
    State: Update + Default,
{
    unimplemented!("headless runtime is not implemented")
}

#[cfg(feature = "expose_gl")]
pub mod gl;
#[cfg(feature = "expose_gl")]
pub use gl::{GL_SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT, align_to_gl_ssbo};

#[cfg(not(feature = "expose_gl"))]
pub(crate) mod gl;

#[cfg(feature = "render")]
pub trait GlProperty {
    fn property_enum(self) -> u32;
}

#[cfg(feature = "render")]
pub trait GpuResource {
    fn resource_id(&self) -> u32;
}

#[inline(always)]
pub fn is_wayland() -> bool {
    match option_env!("WAYLAND") {
        Some(env) => i32::from_str_radix(env, 2).unwrap_or(0) == 1,
        None => false,
    }
}

/// Implementation of the `fnv1a` hashing algorithms for strings.
pub const fn hash_string(string: &str) -> StringHash {
    hash_string_b(string.as_bytes())
}

/// Implementation of the `fnv1a` hashing algorithms for strings as raw bytes.
pub const fn hash_string_b(bytes: &[u8]) -> StringHash {
    const BIAS: u64 = 0xcbf29ce484222325;
    const MUL: u64 = 0x10000000000001b3;

    let mut hash = BIAS;
    let mut i = 0;

    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(MUL);
        i += 1;
    }

    StringHash(hash)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StringHash(u64);

impl std::fmt::Display for StringHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl StringHash {
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl Into<u64> for StringHash {
    fn into(self) -> u64 {
        self.as_u64()
    }
}

impl Hasher for StringHash {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!()
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }
}

pub type StringHasher = BuildHasherDefault<StringHash>;
