use std::{marker::PhantomData, num::NonZeroU64, sync::Arc};

use bytemuck::{Zeroable, bytes_of};
use egui_wgpu::{self, wgpu::{self, util::DeviceExt}, ScreenDescriptor};
use egui::{Color32, PaintCallback, Rangef, Rect};

mod transformation;
pub use transformation::{Transformation,SampleOrder};

pub use egui_wgpu::wgpu::{AddressMode, FilterMode};

pub trait Sample: bytemuck::Pod {
    const TEXTURE_FORMAT: wgpu::TextureFormat;
}

impl Sample for u8 {
    const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
}

impl Sample for u16 {
    const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Unorm;
}

impl Sample for f32 {
    const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
}

pub struct Heatmap<S> {
    inner: Arc<HeatmapInner>,
    sample: PhantomData<S>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Colormap {
    Linear(Color32, Color32),
    Viridis,
    Plasma,
    Magma,
    Inferno,
}

#[derive(Clone, Debug)]
pub struct DisplayOptions {
    pub colormap: Colormap,
    pub alpha: f32,
    pub transformation: Transformation,
    pub value_range: Rangef
}

#[derive(Clone, Debug, Default)]
pub struct TextureOptions {
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
}

impl<S: Sample> Heatmap<S> {
    pub fn write(&self, data: &[S]) {
        self.inner.write(bytemuck::cast_slice(data));
    }

    pub fn shape_at(&self, rect: Rect, options: DisplayOptions) -> PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            HeatmapCallback {
                inner: self.inner.clone(),
                options,
            },
        )
    }
}

struct HeatmapInner {
    resources: Arc<HeatmapRenderer>,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    texture: wgpu::Texture,
}

impl HeatmapInner {
    pub fn write(&self, data: &[u8]) {
        let texture_size = wgpu::Extent3d {
            width: self.texture.width(),
            height: self.texture.height(),
            depth_or_array_layers: 1,
        };

        self.resources.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.texture.width()),
                rows_per_image: Some(self.texture.height()),
            },
            texture_size,
        );
    }
}

struct HeatmapCallback {
    inner: Arc<HeatmapInner>,
    options: DisplayOptions,
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

        let (colormap, color1, color2) = match self.options.colormap {
            Colormap::Linear(color1, color2) => (1, color1.to_normalized_gamma_f32(), color2.to_normalized_gamma_f32()),
            Colormap::Viridis => (16, [0.0; 4], [0.0; 4]),
            Colormap::Plasma => (17, [0.0; 4], [0.0; 4]),
            Colormap::Magma => (18, [0.0; 4], [0.0; 4]),
            Colormap::Inferno => (19, [0.0; 4], [0.0; 4]),
        };

        queue.write_buffer(
            &self.inner.uniform_buffer,
            0,
            bytes_of(&Uniforms {
                matrix: self.options.transformation.wgpu_matrix(),
                color1,
                color2,
                colormap,
                alpha: self.options.alpha,
                min: self.options.value_range.min,
                max: self.options.value_range.max,
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
        render_pass.set_pipeline(&self.inner.resources.pipeline);
        render_pass.set_bind_group(0, &self.inner.bind_group, &[]);
        render_pass.draw(0..4, 0..1);
    }
}


#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable, Default)]
struct Uniforms {
    matrix: [f32; 12], // 3x3 matrix with wgpu padding
    color1: [f32; 4],
    color2: [f32; 4],
    alpha: f32,
    colormap: u32,
    min: f32,
    max: f32,
}

pub struct HeatmapRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
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
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
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
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu_render_state.target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::default(),
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        HeatmapRenderer {
            device,
            queue,
            bind_group_layout,
            pipeline,
        }
    }

    pub fn heatmap<S: Sample>(self: &Arc<Self>, width: u32, height: u32, opts: TextureOptions) -> Heatmap<S> {
        let inner = self.create_inner(width, height, opts, S::TEXTURE_FORMAT);
        Heatmap { inner, sample: PhantomData }
    }

    fn create_inner(self: &Arc<Self>, width: u32, height: u32, opts: TextureOptions, texture_format: wgpu::TextureFormat) -> Arc<HeatmapInner> {
        let uniform_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("heatmap"),
            contents: bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
        });

        let texture = self.create_texture(width, height, texture_format);
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: opts.address_mode_u,
            address_mode_v: opts.address_mode_v,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: opts.mag_filter,
            min_filter: opts.min_filter,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
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

        Arc::new(HeatmapInner { bind_group, uniform_buffer, texture, resources: self.clone() })
    }

    fn create_texture(&self, width: u32, height: u32, texture_format: wgpu::TextureFormat) -> wgpu::Texture {
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
                format: texture_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                label: Some("heatmap_data"),
                view_formats: &[],
            }
        );
        texture
    }
}
