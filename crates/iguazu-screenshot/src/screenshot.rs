use std::{mem, time::Instant};

use eframe::egui_wgpu::{self, ScreenDescriptor, RendererOptions};
use egui::{Pos2, Rect, Vec2};
use futures_lite::future::block_on;
use wgpu::BufferView;
use image::{ImageBuffer, Rgba};

pub type WgpuImage = ImageBuffer::<Rgba<u8>, BufferView>;

pub struct Renderer {
    ctx: egui::Context,
    width: u32,
    height: u32,
    pixels_per_point: f32,
    output: egui::FullOutput,
    render_state: egui_wgpu::RenderState,
    parker: parking::Parker,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let config = egui_wgpu::WgpuConfiguration::default();
        let render_state = block_on(egui_wgpu::RenderState::create(
            &config,
            &instance,
            None,
            RendererOptions::default(),
        ))
        .unwrap();

        let ctx = egui::Context::default();
        let output = egui::FullOutput::default();
        let pixels_per_point = 1.0;

        let parker = parking::Parker::new();
        let unparker = parker.unparker().clone();
        ctx.set_request_repaint_callback(move |_| {
            unparker.unpark();
        });

        Self { ctx, output, width, height, pixels_per_point, render_state, parker }
    }

    pub fn egui_ctx(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn render_state(&self) -> &egui_wgpu::RenderState {
        &self.render_state
    }

    pub fn wait_next_frame_until(&self, deadline: Instant) -> bool {
        self.parker.park_deadline(deadline)
    }

    pub fn run_ui(&mut self, f: impl FnMut(&mut egui::Ui)) {
        // Clear textures unused after last frame (they may still be needed if we were to render the last frame)
        let mut renderer = self.render_state.renderer.write();
        for id in self.output.textures_delta.free.drain() {
            renderer.free_texture(&id);
        }

        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(self.width as f32, self.height as f32)));
        let full_output = self.ctx.run_ui(input, f);
        self.output.append(full_output);

        for (id, image_deltas) in self.output.textures_delta.set.drain() {
            for image_delta in image_deltas {
                renderer.update_texture(
                    &self.render_state.device,
                    &self.render_state.queue,
                    id,
                    &image_delta,
                );
            }
        }
    }

    pub fn render(&mut self) -> WgpuImage {
        let clipped_primitives = self.ctx.tessellate(mem::take(&mut self.output.shapes), self.pixels_per_point);

        // egui normally draws this as the CentralPanel background
        let clear_color = self.ctx.global_style().visuals.panel_fill.to_normalized_gamma_f32();

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.width, self.height],
            pixels_per_point: self.pixels_per_point,
        };

        let mut encoder = self.render_state.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("encoder"),
        });

        let mut renderer = self.render_state.renderer.write();
        let user_cmd_bufs = renderer.update_buffers(
            &self.render_state.device,
            &self.render_state.queue,
            &mut encoder,
            &clipped_primitives,
            &screen_descriptor,
        );

        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };

        let texture = self.render_state.device.create_texture(&wgpu::TextureDescriptor {
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.render_state.target_format,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
            label: None,
            view_formats: &[]
        });
        let texture_view = texture.create_view(&Default::default());

        let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui_render"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear_color[0] as f64,
                        g: clear_color[1] as f64,
                        b: clear_color[2] as f64,
                        a: clear_color[3] as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        renderer.render(
            &mut render_pass.forget_lifetime(),
            &clipped_primitives,
            &screen_descriptor,
        );

        let u32_size = std::mem::size_of::<u32>() as u32;

        let output_buffer_size = (u32_size * self.width * self.height) as wgpu::BufferAddress;
        let output_buffer = self.render_state.device.create_buffer(&wgpu::BufferDescriptor {
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            label: None,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::All,
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(u32_size * self.width),
                    rows_per_image: Some(self.height),
                },
            },
            size,
        );

        let encoded = encoder.finish();

        self.render_state.queue.submit(core::iter::chain(user_cmd_bufs, [encoded]));

        let buffer_slice = output_buffer.slice(..);
        buffer_slice.map_async(wgpu::MapMode::Read, |_| {});
        self.render_state.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buffer_slice.get_mapped_range().unwrap();
        ImageBuffer::<Rgba<u8>, BufferView>::from_raw(self.width, self.height, data).unwrap()
    }
}
