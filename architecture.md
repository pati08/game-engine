# Graphics Engine Architecture

## Goals

- Lightweight — minimal imposed structure on the user
- Built-in utilities for common tasks (mesh rendering, model loading, textures, camera)
- Always allow dropping down to raw wgpu when needed
- User owns their own state — no required inheritance or trait implementations on domain types

---

## Current Issues (to fix)

1. **`AppBuilder::build()` drops everything** (`lib.rs:114`) — `world`, `update_fn`, and
   `fixed_update_fn` are silently discarded. The returned `App` has no connection to them.
2. **`App::window_event` never calls `render()`** — `RedrawRequested` just re-requests a
   redraw but never actually renders anything.
3. **`World`/`Drawable` traits leak wgpu internals** — user code has to import and touch
   `wgpu::RenderPass`, which defeats the purpose of a library abstraction.

---

## Layered Design

```
Layer 3: App runner        (optional — bypass with raw winit if desired)
Layer 2: RenderCtx         (high-level draw calls + escape hatch to raw wgpu)
Layer 1: Built-in utils    (Mesh, Model, Texture, Camera, Pipeline)
Layer 0: GpuContext        (device, queue — public fields, fully accessible)
```

Users can enter at any layer. The app runner is optional — nothing stops someone from
creating a `Renderer` and managing winit themselves.

---

## Module Structure

```
src/
├── lib.rs
├── app.rs          <- AppBuilder, event loop, UpdateCtx
└── renderer/
    ├── mod.rs      <- Renderer (surface, swapchain, resize)
    ├── gpu.rs      <- GpuContext { pub device, pub queue }
    ├── context.rs  <- RenderCtx (high-level + escape hatch)
    ├── pipeline.rs <- Pipeline, PipelineBuilder
    ├── mesh.rs     <- Mesh, MeshVertex (vertex types split out from model.rs)
    ├── model.rs    <- Model, Material, loader
    ├── texture.rs  <- Texture (keep as-is)
    ├── camera.rs   <- Camera (pure math utility, no GPU state)
    └── instance.rs <- Instance, InstanceRaw (keep as-is)
```

---

## Key Types

### `GpuContext` — raw GPU access, always public

```rust
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}
```

### `Renderer` — surface and swapchain management

Holds a `GpuContext`. Handles resize, depth texture, surface configuration. The user
doesn't drive the swapchain directly, but can access the GPU context at any time.

```rust
pub struct Renderer {
    pub gpu: Arc<GpuContext>,
    // surface, config, depth texture — managed internally
}

impl Renderer {
    pub async fn load_model(&self, path: &str) -> Result<Model>;
    pub async fn load_texture(&self, path: &str) -> Result<Texture>;
}
```

### `RenderCtx` — per-frame render interface

What the user receives in their render callback. Supports both high-level draw calls and
raw wgpu access. The default 3D pipeline is active unless overridden.

```rust
pub struct RenderCtx<'a> { /* internal */ }

impl RenderCtx<'_> {
    // High-level built-ins
    pub fn clear(&mut self, color: wgpu::Color);
    pub fn set_camera(&mut self, camera: &Camera);
    pub fn set_pipeline(&mut self, pipeline: &Pipeline);
    pub fn draw_model(&mut self, model: &Model);
    pub fn draw_model_instanced(&mut self, model: &Model, instances: &[Instance]);
    pub fn draw_mesh(&mut self, mesh: &Mesh, material: &Material);

    // Escape hatch — raw wgpu when needed
    pub fn render_pass(&mut self) -> &mut wgpu::RenderPass;
    pub fn device(&self) -> &wgpu::Device;
    pub fn queue(&self) -> &wgpu::Queue;
}
```

### `Camera` — pure math, no GPU state

The user owns the camera in their own state struct. GPU buffers and bind groups live
inside `RenderCtx`. The user just calls `ctx.set_camera(&self.camera)` each frame.

```rust
pub struct Camera {
    pub eye: cgmath::Point3<f32>,
    pub target: cgmath::Point3<f32>,
    pub up: cgmath::Vector3<f32>,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
}
// No GPU resources on this type.
```

### `Pipeline` / `PipelineBuilder`

The default 3D pipeline is used automatically by `draw_model` / `draw_mesh`. Users can
build custom pipelines and activate them with `ctx.set_pipeline()`.

```rust
let pipeline = PipelineBuilder::new(&renderer.gpu)
    .vertex_shader(include_wgsl!("custom.wgsl"))
    .fragment_shader(include_wgsl!("custom.wgsl"))
    .topology(wgpu::PrimitiveTopology::LineList)
    .depth_test(true)
    .build();

// Activate in render:
ctx.set_pipeline(&pipeline);
ctx.draw_model(&model);
// ctx.use_default_pipeline(); // to reset
```

### `AppBuilder` — the app runner

No traits required on user state. The user passes their state into the builder, then
provides two closures that each receive `&mut S`.

```rust
pub struct AppBuilder<S: 'static> {
    state: S,
}

impl<S: 'static> AppBuilder<S> {
    pub fn new(state: S) -> Self;

    pub fn run(
        self,
        update: impl FnMut(&mut S, &UpdateCtx) + 'static,
        render: impl FnMut(&mut S, &mut RenderCtx) + 'static,
    ) -> !;
}
```

### `UpdateCtx` — what the update closure receives

```rust
pub struct UpdateCtx<'a> {
    pub dt: f32,
    pub elapsed: f32,
    pub renderer: &'a Renderer,  // for loading assets mid-game if needed
    // input state (keys, mouse) — add later
}
```

---

## Example Usage

### Simple case

```rust
struct Game {
    model: Model,
    camera: Camera,
    angle: f32,
}

fn main() {
    // Game is initialized once we have access to the renderer.
    // One option: use Option<Game> and initialize on first update.
    let game: Option<Game> = None;

    AppBuilder::new(game)
        .run(
            |state, ctx| {
                let game = state.get_or_insert_with(|| {
                    let model = pollster::block_on(ctx.renderer.load_model("cube.obj")).unwrap();
                    Game { model, camera: Camera::default(), angle: 0.0 }
                });
                game.angle += ctx.dt;
            },
            |state, ctx| {
                let Some(game) = state else { return };
                ctx.clear(wgpu::Color::BLACK);
                ctx.set_camera(&game.camera);
                ctx.draw_model(&game.model);
            },
        );
}
```

### Advanced — bypass the runner entirely

```rust
// Create a Renderer and manage winit yourself.
// Nothing in the library prevents this.
let renderer = Renderer::new(window).await;
let model = renderer.load_model("cube.obj").await?;

// Your own winit event loop, your own render pass, etc.
// Use renderer.gpu.device / renderer.gpu.queue directly.
```

---

## What to Keep from Existing Code

| Item | Status |
|---|---|
| `Model`, `Mesh`, `Material` structs | Keep |
| `DrawModel` trait on `wgpu::RenderPass` | Keep as internal impl, don't re-export |
| `Texture` and texture loading | Keep |
| `Instance`, `InstanceRaw` | Keep |
| `ModelVertex`, `Vertex` trait | Keep, move to `mesh.rs` |
| `Camera` math | Keep, remove GPU state from it |

## What to Restructure

| Current | Change |
|---|---|
| `State` (monolith in `renderer/mod.rs`) | Split into `GpuContext` + `Renderer` + `Pipeline` |
| `Camera` owned by `State` | Move to user-owned utility type, strip GPU state |
| `World` / `Drawable` traits | Remove — replace with closure-based `AppBuilder` |
| `AppBuilder::build()` | Actually wire up state and callbacks, fix the drop bug |
| `App::window_event` `RedrawRequested` | Call `render()` here |
| `State::render(&[&Model], &[&InstancedModel])` | Replace with `RenderCtx` pattern |
