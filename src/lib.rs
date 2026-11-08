use std::{
    marker::PhantomData,
    num::NonZeroU64,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use bytemuck::bytes_of;
use egui::{Color32, PaintCallback, Rangef, Rect};
use egui_wgpu::{ScreenDescriptor, wgpu};

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

#[derive(Clone)]
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
    pub fn write_all(&self, data: &[S]) {
        self.write(data, 0, 0, self.inner.texture.width(), self.inner.texture.height());
    }

    pub fn write(&self, data: &[S], x: u32, y: u32, width: u32, height: u32) {
        self.inner.write(bytemuck::cast_slice(data), x, y, width, height, size_of::<S>() as u32)
    }

    pub fn shape_at(&self, rect: Rect, options: DisplayOptions) -> PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            HeatmapCallback {
                inner: self.inner.clone(),
                options,
                rect,
                slot: AtomicU32::new(u32::MAX), // assigned in `prepare`.
            },
        )
    }
}

struct HeatmapInner {
    resources: Arc<HeatmapRenderer>,
    bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
}

impl HeatmapInner {
    pub fn write(&self, data: &[u8], x: u32, y: u32, width: u32, height: u32, bytes_per_elem: u32) {
        self.resources.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0},
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * bytes_per_elem),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

struct HeatmapCallback {
    inner: Arc<HeatmapInner>,
    options: DisplayOptions,
    rect: Rect,
    slot: AtomicU32,
}

impl HeatmapCallback {
    fn uniforms(&self, screen_descriptor: &ScreenDescriptor) -> Uniforms {
        let (colormap, color1, color2) = match self.options.colormap {
            Colormap::Linear(color1, color2) => (1, color1.to_normalized_gamma_f32(), color2.to_normalized_gamma_f32()),
            Colormap::Viridis => (16, [0.0; 4], [0.0; 4]),
            Colormap::Plasma => (17, [0.0; 4], [0.0; 4]),
            Colormap::Magma => (18, [0.0; 4], [0.0; 4]),
            Colormap::Inferno => (19, [0.0; 4], [0.0; 4]),
        };

        let rect = {
            let pixels_per_point = screen_descriptor.pixels_per_point;
            let screen_size_px = screen_descriptor.size_in_pixels;
            let screen_w = screen_size_px[0] as f32;
            let screen_h = screen_size_px[1] as f32;
            let left = 2.0 * (pixels_per_point * self.rect.min.x) / screen_w - 1.0;
            let right = 2.0 * (pixels_per_point * self.rect.max.x) / screen_w - 1.0;
            let top = 1.0 - 2.0 * (pixels_per_point * self.rect.min.y) / screen_h;
            let bottom = 1.0 - 2.0 * (pixels_per_point * self.rect.max.y) / screen_h;
            [left, right, top, bottom]
        };

        Uniforms {
            rect,
            matrix: self.options.transformation.wgpu_matrix(),
            color1,
            color2,
            colormap,
            alpha: self.options.alpha,
            min: self.options.value_range.min,
            max: self.options.value_range.max,
        }
    }
}

impl egui_wgpu::CallbackTrait for HeatmapCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen_descriptor: &ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let slot = self.inner.resources.uniform_buffers.lock().unwrap().stage(self.uniforms(screen_descriptor));
        self.slot.store(slot, Ordering::Relaxed);
        Vec::new()
    }

    fn finish_prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        self.inner.resources.uniform_buffers.lock().unwrap().flush(&self.inner.resources);
        Vec::new()
    }

    fn paint<'a>(
        &'a self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _resources: &'a egui_wgpu::CallbackResources,
    ) {
        // Set viewport to entire window; quad is positioned via `Uniforms::rect`.
        let [screen_w, screen_h] = info.screen_size_px;
        render_pass.set_viewport(0.0, 0.0, screen_w as f32, screen_h as f32, 0.0, 1.0);

        render_pass.set_pipeline(&self.inner.resources.pipeline);
        render_pass.set_bind_group(0, &self.inner.bind_group, &[]);

        let uniform_buffers = self.inner.resources.uniform_buffers.lock().unwrap();
        let offset = self.slot.load(Ordering::Relaxed) * UNIFORM_BUFFER_STRIDE as u32;
        render_pass.set_bind_group(1, &uniform_buffers.bind_group, &[offset]);
        render_pass.draw(0..4, 0..1);
    }
}


#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect: [f32; 4], // provided rect, in normalized device coordinates
    matrix: [f32; 12], // 3x3 matrix with wgpu padding
    color1: [f32; 4],
    color2: [f32; 4],
    alpha: f32,
    colormap: u32,
    min: f32,
    max: f32,
}

const UNIFORM_ALIGNMENT: u64 = 256;
const UNIFORM_BUFFER_STRIDE: u64 = (size_of::<Uniforms>() as u64).div_ceil(UNIFORM_ALIGNMENT) * UNIFORM_ALIGNMENT;

/// Buffer containing uniforms for all heatmaps
/// along with state kept between `prepare` and `finish_prepare`.
struct UniformBuffers {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pending: Vec<Uniforms>,
}

impl UniformBuffers {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> UniformBuffers {
        let (buffer, bind_group) = Self::create_buffer(device, layout, UNIFORM_BUFFER_STRIDE);
        UniformBuffers {
            buffer,
            bind_group,
            pending: Vec::new(),
        }
    }

    fn create_buffer(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("heatmap_uniforms"),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("heatmap_uniforms"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    // Bind exactly one slot; the dynamic offset selects which
                    // one. Binding the entire buffer would leave no room for
                    // any dynamic offset.
                    size: NonZeroU64::new(size_of::<Uniforms>() as u64),
                }),
            }],
        });
        (buffer, bind_group)
    }

    fn stage(&mut self, uniforms: Uniforms) -> u32 {
        let slot = self.pending.len() as u32;
        self.pending.push(uniforms);
        slot
    }

    fn flush(&mut self, renderer: &HeatmapRenderer) {
        let size = self.pending.len() as u64 * UNIFORM_BUFFER_STRIDE;
        if self.buffer.size() < size {
            let new_size = size.max(self.buffer.size() * 2);
            (self.buffer, self.bind_group) = Self::create_buffer(&renderer.device, &renderer.uniform_bind_group_layout, new_size);
        }

        if let Some(size) = NonZeroU64::new(size) {
            let mut view = renderer.queue.write_buffer_with(&self.buffer, 0, size).unwrap();

            for (i, uniforms) in self.pending.iter().enumerate() {
                let offset = i * UNIFORM_BUFFER_STRIDE as usize;
                view.slice(offset..offset + size_of::<Uniforms>()).copy_from_slice(bytes_of(uniforms));
            }

            self.pending.clear()
        }
    }
}

pub struct HeatmapRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform_buffers: Mutex<UniformBuffers>,
}

impl HeatmapRenderer {
    pub fn new(wgpu_render_state: &egui_wgpu::RenderState) -> HeatmapRenderer {
        let device = wgpu_render_state.device.clone();
        let queue = wgpu_render_state.queue.clone();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("heatmap"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./heatmap.wgsl").into()),
        });

        let texture_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("heatmap_texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniform_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("heatmap_uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("heatmap"),
            bind_group_layouts: &[Some(&texture_bind_group_layout), Some(&uniform_bind_group_layout)],
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

        let uniform_buffers = Mutex::new(UniformBuffers::new(&device, &uniform_bind_group_layout));

        HeatmapRenderer {
            device,
            queue,
            texture_bind_group_layout,
            uniform_bind_group_layout,
            pipeline,
            uniform_buffers,
        }
    }

    pub fn heatmap<S: Sample>(self: &Arc<Self>, width: u32, height: u32, opts: TextureOptions) -> Heatmap<S> {
        let inner = self.create_inner(width, height, opts, S::TEXTURE_FORMAT);
        Heatmap { inner, sample: PhantomData }
    }

    fn create_inner(self: &Arc<Self>, width: u32, height: u32, opts: TextureOptions, texture_format: wgpu::TextureFormat) -> Arc<HeatmapInner> {
        let texture_size = wgpu::Extent3d {
            width: width,
            height: height,
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
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Arc::new(HeatmapInner { bind_group, texture, resources: self.clone() })
    }
}
