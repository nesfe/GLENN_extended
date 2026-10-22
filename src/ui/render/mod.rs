//! Depth-tested native scene rendered into an egui texture. Geometry and tracer
//! positions come from the numerical model; the shader only shades and advects.
mod scene;
use bytemuck::{Pod, Zeroable};
use eframe::{egui, egui_wgpu::RenderState, wgpu};
use glam::Mat4;
pub use scene::Scene;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
    data: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    camera: [[f32; 4]; 4],
    clock: [f32; 4],
}

struct Target {
    size: [u32; 2],
    color: wgpu::TextureView,
    multisample: wgpu::TextureView,
    depth: wgpu::TextureView,
    id: egui::TextureId,
}
pub struct Renderer {
    state: RenderState,
    opaque: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    target: Option<Target>,
    vertices: Option<wgpu::Buffer>,
    solid_count: u32,
    total_count: u32,
}
impl Renderer {
    pub fn new(state: RenderState) -> Self {
        let device = &state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Calculated flow scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Scene camera and convective time"),
            contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let binding_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &binding_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&binding_layout],
            push_constant_ranges: &[],
        });
        let pipeline = |transparent| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if transparent { "Flow with wing occlusion" } else { "Wing surface" }),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x4,3=>Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: if transparent { Some(wgpu::BlendState::ALPHA_BLENDING) } else { None }, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: !transparent,
                depth_compare: wgpu::CompareFunction::LessEqual, stencil: Default::default(), bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: 4, ..Default::default() },
            multiview: None, cache: None,
        })
        };
        Self {
            opaque: pipeline(false),
            transparent: pipeline(true),
            state,
            uniform,
            binding,
            target: None,
            vertices: None,
            solid_count: 0,
            total_count: 0,
        }
    }
    pub fn upload(&mut self, scene: &Scene) {
        self.vertices = Some(self.state.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Wing and integrated streamlines"),
                contents: bytemuck::cast_slice(&scene.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            },
        ));
        self.solid_count = scene.solid_count;
        self.total_count = scene.vertices.len() as u32;
    }
    fn resize(&mut self, size: [u32; 2]) {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let texture = |format, samples, usage| {
            self.state
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Tunnel viewport"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color = texture(
            wgpu::TextureFormat::Rgba8Unorm,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let multisample = texture(
            wgpu::TextureFormat::Rgba8Unorm,
            4,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            4,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let mut renderer = self.state.renderer.write();
        let id = if let Some(old) = &self.target {
            renderer.update_egui_texture_from_wgpu_texture(
                &self.state.device,
                &color,
                wgpu::FilterMode::Linear,
                old.id,
            );
            old.id
        } else {
            renderer.register_native_texture(&self.state.device, &color, wgpu::FilterMode::Linear)
        };
        self.target = Some(Target {
            size,
            color,
            multisample,
            depth,
            id,
        });
    }
    pub fn render(&mut self, size: [u32; 2], camera: Mat4, time: f32) -> egui::TextureId {
        self.resize(size.map(|v| v.clamp(1, 4096)));
        let eye = -camera
            .inverse()
            .transform_vector3(glam::Vec3::Z)
            .normalize();
        self.state.queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&Uniforms {
                camera: camera.to_cols_array_2d(),
                clock: [time, eye.x, eye.y, eye.z],
            }),
        );
        let target = self.target.as_ref().unwrap();
        let mut encoder = self
            .state
            .device
            .create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Aerodynamic tunnel"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.multisample,
                    resolve_target: Some(&target.color),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.035,
                            g: 0.062,
                            b: 0.095,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some(vertices) = &self.vertices {
                pass.set_bind_group(0, &self.binding, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_pipeline(&self.opaque);
                pass.draw(0..self.solid_count, 0..1);
                pass.set_pipeline(&self.transparent);
                pass.draw(self.solid_count..self.total_count, 0..1);
            }
        }
        self.state.queue.submit([encoder.finish()]);
        target.id
    }
}
