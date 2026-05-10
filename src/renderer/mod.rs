use std::sync::Arc;

use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, Buffer, BufferUsages, PipelineCompilationOptions,
    PipelineLayoutDescriptor, RenderPipelineDescriptor, ShaderStages, SurfaceConfiguration,
    util::{BufferInitDescriptor, DeviceExt},
};
use winit::window::Window;

use camera::CameraUniform;
use model::{ModelVertex, Vertex};

pub use camera::Camera;
pub use instance::Instance;
pub use model::{Material, Mesh, Model};

mod camera;
pub mod instance;
mod model;
mod resources;
mod texture;

use model::DrawModel;

// ── GPU context ──────────────────────────────────────────────────────────────

pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

// ── Default 3-D pipeline ─────────────────────────────────────────────────────

struct DefaultPipeline {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: Buffer,
    camera_bind_group: BindGroup,
    camera_bind_group_layout: BindGroupLayout,
    texture_bind_group_layout: BindGroupLayout,
}

impl DefaultPipeline {
    fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("camera bind group layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let camera_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("camera buffer"),
            contents: bytemuck::cast_slice(&[CameraUniform::new()]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let camera_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &camera_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("render pipeline layout"),
            bind_group_layouts: &[&texture_bind_group_layout, &camera_bind_group_layout],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[ModelVertex::desc(), instance::InstanceRaw::desc()],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        DefaultPipeline {
            pipeline,
            camera_buffer,
            camera_bind_group,
            camera_bind_group_layout,
            texture_bind_group_layout,
        }
    }
}

// ── Renderer ─────────────────────────────────────────────────────────────────

pub struct Renderer {
    pub gpu: Arc<GpuContext>,
    pub window: Arc<Window>,
    size: winit::dpi::PhysicalSize<u32>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    config: SurfaceConfiguration,
    depth_texture: texture::Texture,
    dummy_instance_buffer: Buffer,
    default_pipeline: DefaultPipeline,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();

        let size = window.inner_size();
        let surface = instance.create_surface(window.clone()).unwrap();
        let cap = surface.get_capabilities(&adapter);
        let surface_format = cap.formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: cap.present_modes[0],
            alpha_mode: cap.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let gpu = Arc::new(GpuContext { device, queue });
        let depth_texture =
            texture::Texture::create_depth_texture(&gpu.device, &config, "depth texture");
        let dummy_instance_buffer = gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("dummy instance buffer"),
            contents: bytemuck::cast_slice(&[Instance::default().to_raw()]),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let default_pipeline =
            DefaultPipeline::new(&gpu.device, surface_format.add_srgb_suffix());

        let renderer = Renderer {
            gpu,
            window,
            size,
            surface,
            surface_format,
            config,
            depth_texture,
            dummy_instance_buffer,
            default_pipeline,
        };

        renderer.configure_surface();
        renderer
    }

    fn configure_surface(&self) {
        self.surface.configure(&self.gpu.device, &self.config);
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.configure_surface();
            self.depth_texture = texture::Texture::create_depth_texture(
                &self.gpu.device,
                &self.config,
                "depth texture",
            );
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Begins a frame, runs `f` with a `RenderCtx`, then presents.
    /// The default clear colour is `wgpu::Color::BLACK`; pass your own via
    /// `RenderCtx` if you want something different per-frame.
    pub fn render_frame<F>(&mut self, clear_color: wgpu::Color, f: F)
    where
        F: for<'pass> FnOnce(&mut RenderCtx<'pass>),
    {
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("failed to acquire next swapchain texture");
        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.surface_format.add_srgb_suffix()),
                ..Default::default()
            });

        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            render_pass.set_pipeline(&self.default_pipeline.pipeline);
            render_pass.set_bind_group(1, &self.default_pipeline.camera_bind_group, &[]);

            let mut ctx = RenderCtx {
                render_pass,
                gpu: &self.gpu,
                default_pipeline: &self.default_pipeline,
                dummy_instance_buffer: &self.dummy_instance_buffer,
            };

            f(&mut ctx);
        }

        self.gpu.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        surface_texture.present();
    }

    pub async fn load_model(&self, file_name: &str) -> anyhow::Result<Model> {
        resources::load_model(
            file_name,
            &self.gpu.device,
            &self.gpu.queue,
            &self.default_pipeline.texture_bind_group_layout,
        )
        .await
    }

    /// Upload a list of instances for a model. Returns an `InstancedModel`
    /// ready to pass to `RenderCtx::draw_model_instanced`.
    pub fn upload_instances(&self, model: Model, instances: &[Instance]) -> InstancedModel {
        let raw: Vec<_> = instances.iter().map(|i| i.to_raw()).collect();
        let instance_buffer = self.gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("instance buffer"),
            contents: bytemuck::cast_slice(&raw),
            usage: wgpu::BufferUsages::VERTEX,
        });
        InstancedModel {
            model,
            instance_buffer,
            instance_num: instances.len() as u32,
        }
    }
}

// ── RenderCtx ────────────────────────────────────────────────────────────────

pub struct RenderCtx<'pass> {
    /// Direct access to the underlying render pass for custom draw calls.
    pub render_pass: wgpu::RenderPass<'pass>,
    gpu: &'pass GpuContext,
    default_pipeline: &'pass DefaultPipeline,
    dummy_instance_buffer: &'pass Buffer,
}

impl<'pass> RenderCtx<'pass> {
    /// Upload camera matrices and activate them for subsequent draw calls.
    pub fn set_camera(&mut self, camera: &Camera) {
        let mut uniform = CameraUniform::new();
        uniform.update(camera);
        self.gpu.queue.write_buffer(
            &self.default_pipeline.camera_buffer,
            0,
            bytemuck::cast_slice(&[uniform]),
        );
    }

    /// Draw all meshes of a model using the default pipeline.
    pub fn draw_model(&mut self, model: &Model) {
        // SAFETY: Model's GPU resources (wgpu::Buffer, wgpu::BindGroup) are
        // internally reference-counted and remain valid for the render pass.
        let model: &'pass Model = unsafe { &*(model as *const Model) };
        self.render_pass
            .set_vertex_buffer(1, self.dummy_instance_buffer.slice(..));
        self.render_pass.draw_model(model);
    }

    /// Draw a model with per-instance transforms.
    pub fn draw_model_instanced(&mut self, instanced: &InstancedModel) {
        // SAFETY: same as draw_model.
        let model: &'pass Model = unsafe { &*((&instanced.model) as *const Model) };
        let buf: &'pass Buffer = unsafe { &*((&instanced.instance_buffer) as *const Buffer) };
        self.render_pass.set_vertex_buffer(1, buf.slice(..));
        self.render_pass
            .draw_model_instanced(model, 0..instanced.instance_num);
    }

    /// Switch to a custom pipeline. Camera and texture bind groups are
    /// re-bound automatically so `draw_model` still works.
    pub fn set_pipeline(&mut self, pipeline: &Pipeline) {
        // SAFETY: wgpu::RenderPipeline is internally ref-counted.
        let p: &'pass wgpu::RenderPipeline =
            unsafe { &*((&pipeline.pipeline) as *const wgpu::RenderPipeline) };
        self.render_pass.set_pipeline(p);
        self.render_pass
            .set_bind_group(1, &self.default_pipeline.camera_bind_group, &[]);
    }

    /// Restore the built-in 3-D pipeline.
    pub fn use_default_pipeline(&mut self) {
        self.render_pass
            .set_pipeline(&self.default_pipeline.pipeline);
        self.render_pass
            .set_bind_group(1, &self.default_pipeline.camera_bind_group, &[]);
    }

    /// Raw wgpu render pass — escape hatch for custom pipelines / draw calls.
    pub fn render_pass(&mut self) -> &mut wgpu::RenderPass<'pass> {
        &mut self.render_pass
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.gpu.queue
    }
}

// ── InstancedModel ───────────────────────────────────────────────────────────

pub struct InstancedModel {
    pub model: Model,
    instance_buffer: Buffer,
    pub instance_num: u32,
}

// ── Pipeline / PipelineBuilder ───────────────────────────────────────────────

/// A compiled render pipeline. Create one with [`PipelineBuilder`] via
/// [`Renderer::pipeline_builder`].
pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
}

/// Builder for custom render pipelines that are compatible with the engine's
/// default bind group layouts (texture @ group 0, camera @ group 1) so that
/// `RenderCtx::draw_model` continues to work after `ctx.set_pipeline(...)`.
pub struct PipelineBuilder<'r> {
    renderer: &'r Renderer,
    vertex: Option<(wgpu::ShaderModule, &'static str)>,
    fragment: Option<(wgpu::ShaderModule, &'static str)>,
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    depth_test: bool,
    blend: Option<wgpu::BlendState>,
}

impl<'r> PipelineBuilder<'r> {
    fn new(renderer: &'r Renderer) -> Self {
        Self {
            renderer,
            vertex: None,
            fragment: None,
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: Some(wgpu::Face::Back),
            depth_test: true,
            blend: Some(wgpu::BlendState::REPLACE),
        }
    }

    /// Set the vertex shader. The entry point name must be a `'static` str
    /// (a string literal is fine).
    pub fn vertex_shader(mut self, module: wgpu::ShaderModule, entry: &'static str) -> Self {
        self.vertex = Some((module, entry));
        self
    }

    pub fn fragment_shader(mut self, module: wgpu::ShaderModule, entry: &'static str) -> Self {
        self.fragment = Some((module, entry));
        self
    }

    pub fn topology(mut self, topology: wgpu::PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }

    pub fn cull_mode(mut self, face: Option<wgpu::Face>) -> Self {
        self.cull_mode = face;
        self
    }

    pub fn depth_test(mut self, enabled: bool) -> Self {
        self.depth_test = enabled;
        self
    }

    pub fn blend(mut self, blend: Option<wgpu::BlendState>) -> Self {
        self.blend = blend;
        self
    }

    /// Compile and return the pipeline. Panics if no vertex shader was set.
    pub fn build(self) -> Pipeline {
        let device = &self.renderer.gpu.device;
        let dp = &self.renderer.default_pipeline;
        let surface_format = self.renderer.surface_format.add_srgb_suffix();

        let (vert_module, vert_entry) = self.vertex.expect("vertex shader is required");

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&dp.texture_bind_group_layout, &dp.camera_bind_group_layout],
            immediate_size: 0,
        });

        // Build the target list before the pipeline descriptor so the slice
        // reference lives long enough.
        let color_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: self.blend,
            write_mask: wgpu::ColorWrites::ALL,
        };
        let targets = [Some(color_target)];

        let frag_state = self.fragment.as_ref().map(|(module, entry)| wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            targets: &targets,
            compilation_options: PipelineCompilationOptions::default(),
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vert_module,
                entry_point: Some(vert_entry),
                buffers: &[ModelVertex::desc(), instance::InstanceRaw::desc()],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: frag_state,
            primitive: wgpu::PrimitiveState {
                topology: self.topology,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: self.cull_mode,
                ..Default::default()
            },
            depth_stencil: self.depth_test.then(|| wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Pipeline { pipeline }
    }
}

impl Renderer {
    /// Create a [`PipelineBuilder`] pre-configured with the engine's default
    /// bind group layouts so custom pipelines stay compatible with
    /// `RenderCtx::draw_model`.
    pub fn pipeline_builder(&self) -> PipelineBuilder<'_> {
        PipelineBuilder::new(self)
    }
}
