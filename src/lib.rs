use std::sync::Arc;
use std::time::Instant;

use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

pub mod renderer;

pub use renderer::{
    Camera, GpuContext, Instance, InstancedModel, Model, Pipeline, PipelineBuilder, RenderCtx,
    Renderer,
};

// ── Public context types ──────────────────────────────────────────────────────

pub struct UpdateCtx {
    /// Seconds since the previous frame.
    pub dt: f32,
    /// Seconds since the app started.
    pub elapsed: f32,
    /// Current window dimensions in physical pixels.
    pub window_size: (u32, u32),
}

// ── AppBuilder ────────────────────────────────────────────────────────────────

/// Entry point for the engine. Create with [`AppBuilder::new`] or
/// [`AppBuilder::with_init`], attach update/render callbacks, then call
/// [`AppBuilder::run`].
pub struct AppBuilder<S: 'static> {
    init: Box<dyn FnOnce(&Renderer) -> S>,
}

impl<S: 'static> AppBuilder<S> {
    /// Use a pre-built state value. Handy when your state doesn't depend on
    /// the GPU (or you initialise GPU resources lazily in `update`).
    pub fn new(state: S) -> Self {
        Self {
            init: Box::new(|_| state),
        }
    }

    /// Provide an initialiser that receives the `Renderer` once the window is
    /// ready. Use this to load models and textures before the first frame.
    pub fn with_init(f: impl FnOnce(&Renderer) -> S + 'static) -> Self {
        Self { init: Box::new(f) }
    }

    /// Start the event loop. Blocks until the window is closed.
    pub fn run(
        self,
        update: impl FnMut(&mut S, &UpdateCtx) + 'static,
        render: impl for<'pass> FnMut(&mut S, &mut RenderCtx<'pass>) + 'static,
    ) {
        env_logger::init();

        let event_loop = EventLoop::new().unwrap();
        event_loop.set_control_flow(ControlFlow::Poll);

        let mut app: App<S> = App {
            app_state: None,
            renderer: None,
            init_fn: Some(self.init),
            update_fn: Box::new(update),
            render_fn: Box::new(render),
            start: Instant::now(),
            last_update: Instant::now(),
        };

        event_loop.run_app(&mut app).unwrap();
    }
}

// ── Internal App (winit ApplicationHandler) ───────────────────────────────────

struct AppState<S> {
    state: S,
    update_fn: Box<dyn FnMut(&mut S, &UpdateCtx)>,
    render_fn: Box<dyn for<'pass> FnMut(&mut S, &mut RenderCtx<'pass>)>,
}

struct App<S: 'static> {
    app_state: Option<AppState<S>>,
    renderer: Option<Renderer>,
    init_fn: Option<Box<dyn FnOnce(&Renderer) -> S>>,
    update_fn: Box<dyn FnMut(&mut S, &UpdateCtx)>,
    render_fn: Box<dyn for<'pass> FnMut(&mut S, &mut RenderCtx<'pass>)>,
    start: Instant,
    last_update: Instant,
}

impl<S: 'static> ApplicationHandler for App<S> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes())
                .unwrap(),
        );

        let renderer = pollster::block_on(Renderer::new(window));

        let state = self
            .init_fn
            .take()
            .expect("resumed called twice")(&renderer);

        // Move update_fn and render_fn out of self and into AppState.
        // We swap in dummy no-ops while building AppState, then immediately
        // replace them — the dummies are never actually called.
        let update_fn = std::mem::replace(
            &mut self.update_fn,
            Box::new(|_: &mut S, _: &UpdateCtx| {}),
        );
        let render_fn = std::mem::replace(
            &mut self.render_fn,
            Box::new(|_: &mut S, _: &mut RenderCtx<'_>| {}),
        );

        self.app_state = Some(AppState {
            state,
            update_fn,
            render_fn,
        });

        self.renderer = Some(renderer);
        self.renderer.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last_update).as_secs_f32();
                let elapsed = now.duration_since(self.start).as_secs_f32();
                self.last_update = now;

                let renderer = self.renderer.as_mut().unwrap();
                let app = self.app_state.as_mut().unwrap();

                let update_ctx = UpdateCtx {
                    dt,
                    elapsed,
                    window_size: renderer.size(),
                };
                (app.update_fn)(&mut app.state, &update_ctx);

                let state = &mut app.state;
                let render_fn = &mut app.render_fn;
                renderer.render_frame(wgpu::Color::BLACK, |ctx| {
                    render_fn(state, ctx);
                });

                renderer.window.request_redraw();
            }

            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
            }

            _ => (),
        }
    }
}
