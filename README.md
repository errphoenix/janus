# Janus
[![Crates.io]](https://img.shields.io/badge/crates.io-private--internal-blue)

**Minimal, low-overhead OpenGL application context and input manager with cross-thread synchronization in Rust**

### See **[Razed](https://github.com/errphoenix/Razed)**

At a high level, Janus provides:
* Generic application context management: to guarantee **one-time initialization** of the application context with mutable access to both render state and simulation state, before thread splitting.
* The `gl` module: containing OpenGL bindings *if the `expose_gl` feature is enabled*.
* Some basic, useful OpenGL abstractions and utils, such as:
  * The `texture` module: providing abstractions for OpenGL texture initialization, upload, sub-upload, binding, and more. Also featuring state-caching to avoid unnecessary state changes *if the `textures` feature is enabled*.
  * **Safe** functions to query OpenGL strings
  * Utility function `align_to_gl_ssbo`: to align values to the `GL_SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT` supported by the machine. This is used in **[Ethel](https://github.com/errphoenix/ethel)** for its OpenGL buffers abstractions.
  * Assertions (`assert_gl!()` & `debug_assert_gl!()`): to ensure the current thread has a valid OpenGL context initialized.
* **String hashing** through `fnv1a`: for efficient string storage and look-ups. This is used in **[Ethel](https://github.com/errphoenix/ethel)** as base for a more complete string hashing and caching system.
* Efficient **no-block** cross-thread input system: this is integrated with `winit`'s input events to deliver real-time input events from the render thread (where the winit window also resides) to the simulation/logic thread.
* A basic `BufferedRoutine` utility: to manage non-trivial complex parallelized `rayon` loops, through thread-local scratch buffers *if the `jobs` feature is enabled*.
* Some custom multi-threaded primitives:
  * **Mirror**: a highly specialised Mutex-like synchronisation primitive, that holds a local cached value and only synchronizes if necessary. 
  * **TriCell**: a "mini triple-buffer" for `Clone + Copy` types. Also works hand-in-hand with **[Ethel](https://github.com/errphoenix/ethel)**'s triple-buffered thread synchronisation.
* Simple input system that preserves the correct sequence of events and efficient polling of contiuous inputs.

## Purpose
The purpose of Janus is to aid in the development of **[Ethel](https://github.com/errphoenix/ethel)**, a higher level toolkit built on Janus, and **[Razed](https://github.com/errphoenix/Razed)**, a "game engine + game" implementation of Janus and Ethel.

For this reason, there is no exact roadmap for Janus.

## Development with Janus
Janus defines the core application lifetime, acting as a very thin and unobtrusive framework.

### Core Application Initialization
`Context` initialization is generic over three types that define its behaviour:
* `State: Update + Default + Sync + Send + 'static` the **simulation/logic state** that will reside on the simulation thread after initialization. This is where the core application logic must reside, including input polling.
   
   #### Example
   ```rust
   impl Update for ImState {
       fn step_duration(&self) -> Duration {
           // maybe get from some user option, or hard code it
           Duration::from_millis(8)
       }
       
       // delta-accumulated frame loop
       // depending on the target step duration and capabilities of the 
       // machine, this can be called multiple times in one frame
       // the `delta` param here indicates the time since the last
       // update() call
       fn update(&mut self, delta: DeltaTime) {
           self.physics.do_physics();
       }
       
       // once-per-frame loop, runs before update()
       // here is where you would have most of your logic, such
       // as input polling or non-physics stuff
       // the `delta` param here indicates the time since the last
       // new_frame() call
       fn new_frame(&mut self, frame_delta: DeltaTime) {
           if self.has_pressed_q() {
               panic!("user has pressed Q, help")
           }
       }
       
       // once-per-frame loop, runs after all calls to update() have
       // finished
       // can be useful for frame finalization or gpu sync
       fn finish_frame(&mut self) {}
   }
   ``` 
* `Render: Draw + Default` the **render state** that will reside on the renderer thread after initialization. This must explicitly handle gpu work, such as shader and command dispatches. This is the only place where the OpenGL context is available, after initialization.
    
    #### Example
    ```rust
    impl Draw for ImRender {
        // called when the window resolution changes for any reason
        // useful to synchronize framebuffers and whatnot..
        fn set_resolution(&mut self, resolution: (f32, f32)) {
            self.gl_viewport(resolution);
        }
        
        // all custom drawing logic must be here
        // the `delta` param indicates the time since the last
        // draw() call
        fn draw(&mut self, delta: DeltaTime) {
            // draw!
        }
    }
    ```
* `Init: Setup<State, render>` that manages the **custom initialization** of `State` and `Render` *after* window initialization and OpenGL context initialization. The OpenGL context is available here. The `Setup` trait also has a blanket implementation matching its `Setup::init` function signature.
    
   #### Example:
   ```rust
   fn initialize(state: &mut State, renderer: &mut Render) -> Result<(), &'static str> {
       let her_thing = renderer.my_thing();
       if !state.register_her_thing(her_thing) {
           return Err("NOO");
       }
       Ok(()
   }
   ```
   *where `State` and `Render` are your `Update` and `Draw` types*

`State` and `Render` must also provide `Default` implementations that define their default state *before* custom initialization done by `Init`.

### Wiring it together
```rust

#[derive(Default] // or custom implement it
struct ImState {
    ...
}
impl Update for ImState {
    ...
}

#[derive(Default] // or custom implement it
struct ImRender {
    ...
}
impl Draw for ImRender {
    ...
}

// define initial window parameters
const DISPLAY_PARAMS: DisplayParameters = DisplayParameters::fullscreen("title);

fn main() {
    // pre-initialization, such as assets, fonts...
    
    // initialize janus input system
    // input_dispatcher will be sent off to janus' Context
    // input_system must be stored in your `State`
    let (input_system, input_dispatcher) = janus::input::stream();
    
    let ctx = janus::context::Context::new(
        initialize, // can also be a closure, if you prefer
        input_dispatcher,
        DISPLAY_PARAMS,
    );
    
    janus::run(ctx);
}

fn initialize(state: &mut ImState, render: &mut ImRender) -> Result<(), &'static str> {
    ...
    Ok(())
}

```
