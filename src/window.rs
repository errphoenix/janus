use std::{ffi::CString, num::NonZeroU32, time::Instant};

use glutin::{
    config::{Config, ConfigTemplateBuilder, GetGlConfig, GlConfig},
    context::{ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentContext, Version},
    display::GetGlDisplay,
    prelude::{GlDisplay, NotCurrentGlContext, PossiblyCurrentGlContext},
    surface::{GlSurface, Surface, WindowSurface},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use tracing::{Level, event};
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    raw_window_handle::HasWindowHandle,
    window::Window,
};

use crate::{
    context::{Context, Draw, Setup, StateHandle, Update},
    gl::{self, get_gl_string},
};

#[derive(Debug, Clone)]
pub struct DisplayParameters {
    pub(crate) title: &'static str,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) mode: DisplayWindowMode,
}

#[derive(Debug, Clone)]
pub enum DisplayWindowMode {
    Window,

    /// Borderless fullscreen
    FullScreen,
}

impl DisplayParameters {
    pub const fn windowed(title: &'static str, width: u32, height: u32) -> Self {
        Self {
            title,
            width,
            height,
            mode: DisplayWindowMode::Window,
        }
    }

    pub const fn fullscreen(title: &'static str) -> Self {
        Self {
            title,
            width: 1,
            height: 1,
            mode: DisplayWindowMode::FullScreen,
        }
    }

    pub const fn new(
        title: &'static str,
        width: u32,
        height: u32,
        mode: DisplayWindowMode,
    ) -> Self {
        Self {
            title,
            width,
            height,
            mode,
        }
    }
}

impl<Init, State, Render> ApplicationHandler for Context<Init, State, Render>
where
    Init: Setup<State, Render>,
    State: Update + Default + Sync + Send + 'static,
    Render: Draw + Default,
{
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let (window, config) = match &self.gl_display {
            GlDisplayState::Pending => {
                let attributes = self.build_attributes();
                let db = DisplayBuilder::new().with_window_attributes(Some(attributes));
                let template = ConfigTemplateBuilder::default();

                let (window, config) = match db.build(event_loop, template, pick_smooth_gl_config) {
                    Ok((Some(window), config)) => (window, config),
                    Ok((None, config)) => {
                        event!(
                            name: "display.build.fail",
                            Level::ERROR,
                            "Failed to build window with config: {config:?}"
                        );
                        return;
                    }
                    Err(err) => {
                        event!(
                            name: "display.init.fail",
                            Level::ERROR,
                            "Failed to initialise window and configuration: {err}",
                        );
                        event_loop.exit();
                        return;
                    }
                };

                self.gl_display = GlDisplayState::Created;
                self.gl_ctx = Some(create_gl_context(&window, &config).treat_as_possibly_current());

                (window, config)
            }
            GlDisplayState::Created => {
                let config = self.gl_ctx.as_ref().unwrap().config();
                match glutin_winit::finalize_window(event_loop, self.build_attributes(), &config) {
                    Ok(window) => (window, config),
                    Err(err) => {
                        eprintln!("Failed to finalise display: {err}");
                        event_loop.exit();
                        return;
                    }
                }
            }
        };

        let surface_attribs = window
            .build_surface_attributes(Default::default())
            .expect("failed to build surface attributes for window");

        let gl_surface = unsafe {
            config
                .display()
                .create_window_surface(&config, &surface_attribs)
                .expect("failed to create surface for window")
        };

        let gl_ctx = self
            .gl_ctx
            .as_ref()
            .expect("cannot initialise window: no context");
        gl_ctx.make_current(&gl_surface).unwrap();

        load_gl_symbols(&config.display());

        // Attempt to enable v-sync.
        // Based on my previous projects, this seems to not work correctly
        // on platforms that rely on wgl (windows).
        if let Err(err) = gl_surface.set_swap_interval(
            gl_ctx,
            glutin::surface::SwapInterval::Wait(NonZeroU32::new(1).unwrap()),
        ) {
            eprintln!("failed to set vsync to enabled: {err}");
        }

        // Set display and panic if it existed before.
        assert!(
            self.display
                .replace(DisplayHandle { gl_surface, window })
                .is_none()
        );

        if let Some(init) = self.init.take() {
            if let StateHandle::Uninitialised(state) = &mut self.state_handle {
                let timestamp = Instant::now();
                if let Err(e) = init.init(state, &mut self.renderer) {
                    event!(
                        name: "context.init.error",
                        Level::ERROR,
                        "Failed to initialise application state: {e}"
                    )
                } else {
                    let duration = Instant::now().duration_since(timestamp);
                    let millis = duration.as_millis();
                    event!(
                        name: "context.init.ok",
                        Level::INFO,
                        "Successfully initialised application state. Took {millis}ms"
                    );

                    event!(
                        name: "context.state-thread.create",
                        Level::INFO,
                        "Creating state/logic thread..."
                    );
                    self.initialise_thread();
                }
            } else {
                event!(
                    name: "context.init.stolen-state",
                    Level::ERROR,
                    "Failed to initialise application state: it is not in an unitialised state"
                );
            }
        }
    }

    #[cfg(feature = "input")]
    fn new_events(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        if cause == StartCause::Poll {
            self.input_dispatcher.sync();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => {
                if let Some(DisplayHandle { gl_surface, window }) = self.display.as_ref() {
                    let ctx = self.gl_ctx.as_ref().unwrap();

                    let delta = &mut self.render_delta;
                    self.renderer.draw(delta.delta());
                    delta.sync();

                    gl_surface.swap_buffers(ctx).unwrap();
                    window.request_redraw();
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),

            #[cfg(feature = "input")]
            window_ev => self.input_dispatcher.handle_key_event(&window_ev),

            #[cfg(not(feature = "input"))]
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // might be required for nvidia; needs testing.
        //let _display = self.gl_ctx.take().unwrap().display();

        self.display = None;
    }
}

pub enum GlDisplayState {
    /// The window has been initialised but has not been properly created
    /// and the OpenGL context is not yet created.
    Pending,

    /// The window and OpenGL context are fully initialised; usually no
    /// further operation is required.
    Created,
}

/// Wraps a winit window and a glutin surface.
pub struct DisplayHandle {
    gl_surface: Surface<WindowSurface>,

    // Dropped after glutin surface.
    window: Window,
}

impl DisplayHandle {
    pub fn surface(&self) -> &Surface<WindowSurface> {
        &self.gl_surface
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn window_mut(&mut self) -> &mut Window {
        &mut self.window
    }
}

fn pick_smooth_gl_config(options: Box<dyn Iterator<Item = Config> + '_>) -> Config {
    options
        .reduce(|accum, cfg| {
            if cfg.num_samples() > accum.num_samples() {
                cfg
            } else {
                accum
            }
        })
        .unwrap()
}

fn create_gl_context(window: &Window, conf: &Config) -> NotCurrentContext {
    let rwh = window.window_handle().ok().map(|wh| wh.as_raw());

    // If the OpenGL core profile creation fails, we might want to fallback to
    // GLES. We might also want to support legacy devices by supporting
    // OpenGL 2.1.
    // Not a priority right now and I can't be hassled, so for now we will
    // only be focusing on OpenGL 4.6 which most devices can run with no issues
    // anyways.
    let ctx_attr = ContextAttributesBuilder::new()
        .with_profile(GlProfile::Core)
        .with_context_api(ContextApi::OpenGl(Some(Version::new(4, 6))))
        .build(rwh);
    let gl_display = conf.display();

    unsafe {
        gl_display
            .create_context(conf, &ctx_attr)
            .expect("failed creation of context for OpenGL 4.6 Core Profile: unsupported platform?")
    }
}

fn load_gl_symbols<D: GlDisplay>(display: &D) {
    gl::load_with(|sym| {
        let sym = CString::new(sym).unwrap();
        display.get_proc_address(sym.as_c_str()) as *const _
    });

    let renderer = get_gl_string(gl::RENDERER);
    let version = get_gl_string(gl::VERSION);
    let shaders_ver = get_gl_string(gl::SHADING_LANGUAGE_VERSION);

    event!(
        name: "gl.info.renderer",
        Level::INFO,
        "Running on {renderer}"
    );
    event!(
        name: "gl.info.version",
        Level::INFO,
        "OpenGL Version: {version}"
    );
    event!(
        name: "gl.info.shader_version",
        Level::INFO,
        "Shaders version: {shaders_ver}"
    );

    #[cfg(feature = "expose_gl")]
    {
        let gl_alignment = unsafe {
            gl::GetIntegerv(
                gl::SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT,
                &raw mut crate::gl::GL_SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT,
            );
            crate::gl::GL_SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT
        };
        event!(
            name: "gl.info.ssbo_alignment_offset",
            Level::INFO,
            "[expose_gl] OpenGL Shader Storage alignment offset: {gl_alignment}"
        );
    }
}
