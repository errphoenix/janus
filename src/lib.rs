#[cfg(feature = "state")]
pub mod context;

#[cfg(feature = "input")]
pub mod input;

#[cfg(feature = "textures")]
pub mod texture;

#[cfg(feature = "render")]
pub mod window;

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
