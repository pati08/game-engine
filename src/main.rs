use game_engine::{AppBuilder, Camera, UpdateCtx, renderer::RenderCtx};

struct GameState {
    camera: Camera,
    model: game_engine::Model, // load with AppBuilder::with_init
}

fn main() {
    AppBuilder::with_init(|renderer| {
        // let pipeline = renderer.pipeline_builder().fragment_shader(module, entry);
        GameState {
            camera: Camera {
                eye: (2.0, 4.0, 8.0).into(),
                target: (0.0, 0.0, 0.0).into(),
                up: cgmath::Vector3::unit_y(),
                aspect: 1.0,
                fovy: 45.0,
                znear: 0.1,
                zfar: 100.0,
            },
            model: pollster::block_on(renderer.load_model("12221_Cat_v1_l3.obj")).unwrap(),
        }
    })
    .run(
        |state: &mut GameState, ctx: &UpdateCtx| {
            let (w, h) = ctx.window_size;
            state.camera.aspect = w as f32 / h as f32;
        },
        |state: &mut GameState, ctx: &mut RenderCtx<'_>| {
            ctx.set_camera(&state.camera);
            ctx.draw_model(&state.model);
        },
    );
}
