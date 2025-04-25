use std::{num::NonZeroU64, sync::Arc};

use bytemuck::{Zeroable, bytes_of};
use egui_wgpu::{self, wgpu::{self, util::DeviceExt}, ScreenDescriptor};
use egui::{PaintCallback, Rangef, Rect};

mod transformation;
pub use transformation::{Transformation,SampleOrder};

pub struct Heatmap {
    width: u32,
    height: u32,
    state: Arc<HeatmapState>,
}

impl Heatmap {
    pub fn write(&mut self, data: &[u8]) {
        let texture_size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };

        self.state.resources.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.state.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.width),
                rows_per_image: Some(self.height),
            },
            texture_size,
        );
    }

    pub fn shape_at(&self, rect: Rect, transformation: impl Into<Transformation>, range: Rangef) -> PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            HeatmapCallback {
                state: self.state.clone(),
                transformation: transformation.into(), 
                range
            },
        )
    }
}

struct HeatmapCallback {
    state: Arc<HeatmapState>,
    transformation: Transformation,
    range: Rangef,
}

impl egui_wgpu::CallbackTrait for HeatmapCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        queue.write_buffer(
            &self.state.uniform_buffer,
            0,
            bytes_of(&Uniforms {
                matrix: self.transformation.wgpu_matrix(),
                min: self.range.min,
                max: self.range.max,
                pad: Default::default(),
            }),
        );
    
        Vec::new()
    }

    fn paint<'a>(
        &'a self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _resources: &'a egui_wgpu::CallbackResources,
    ) {
        render_pass.set_pipeline(&self.state.resources.pipeline);
        render_pass.set_bind_group(0, &self.state.bind_group, &[]);
        render_pass.draw(0..4, 0..1);
    }
}



#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable, Default)]
struct Uniforms {
    matrix: [f32; 12], // 3x3 matrix with wgpu padding
    min: f32,
    max: f32,
    pad: [f32; 2],
}

pub struct HeatmapRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

struct HeatmapState {
    resources: Arc<HeatmapRenderer>,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    texture: wgpu::Texture,
}

impl HeatmapRenderer {
    pub fn new(wgpu_render_state: &egui_wgpu::RenderState) -> HeatmapRenderer {
        let device = wgpu_render_state.device.clone();
        let queue = wgpu_render_state.queue.clone();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("heatmap"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./heatmap.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("heatmap"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("heatmap"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("heatmap"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu_render_state.target_format.into())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        HeatmapRenderer {
            device,
            queue,
            bind_group_layout,
            pipeline,
        }
    }

    pub fn heatmap(self: &Arc<Self>, width: u32, height: u32) -> Heatmap {
        let uniform_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("heatmap"),
            contents: bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
        });

        let texture = self.create_texture(width, height);
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("heatmap"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let state = Arc::new(HeatmapState { bind_group, uniform_buffer, texture, resources: self.clone() });

        Heatmap { width, height, state }

    }

    fn create_texture(&self, width: u32, height: u32) -> wgpu::Texture {
        let texture_size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(
            &wgpu::TextureDescriptor {
                size: texture_size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                label: Some("heatmap_data"),
                view_formats: &[],
            }
        );
        texture
    }
}