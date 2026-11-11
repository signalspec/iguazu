use ecow::EcoString;
use egui::{Id, PaintCallback, Rangef, Rect, Vec2};
use iguazu::{IdxRange, schema::EntityStream, time::{Time, TimeRange}, view::{SpectrogramView, Timebase}};
use iguazu_egui_heatmap::{Colormap, Heatmap, SampleOrder, TextureOptions, Transformation};

use crate::{ViewerContext, timeline::{TimelineResponse, row_label, scale::Scale, stream_rect}};

pub(crate) struct SpectrogramRow<'a> {
    sample_rate: f64,
    view: SpectrogramView<'a>,
    label: EcoString,
    id: Id,
}

impl<'a> SpectrogramRow<'a> {
    pub fn new(vcx: &'a ViewerContext, id: Id, label: EcoString, entity: &EntityStream, timebase: &Timebase<'a>,) -> Option<Self> {
        let sample_rate = timebase.uniform_sample_rate()?;
        let view = SpectrogramView::new(&vcx.view_manager, entity)?;

        Some(Self { sample_rate, view, label, id })
    }

    pub fn time_range(&self) -> Option<TimeRange> {
        Some(TimeRange {
            min: Time::ZERO,
            max: (self.view.state().end as i128) * Time::period_float(self.sample_rate),
        })
    }

    pub fn render(&self, vcx: &'a ViewerContext, ui: &mut egui::Ui, scale: &Scale) -> TimelineResponse{
        row_label(ui, &self.label);
        let rect = stream_rect(ui, scale);

        if !ui.is_rect_visible(rect) {
            return TimelineResponse::default();
        }

        let fft_size = self.view.fft_size();

        let idx_scale = scale.idx_scale(self.sample_rate);
        let level = idx_scale.min_visible_width_log2(ui.pixels_per_point());
        let state = self.view.state();
        let i_range = IdxRange {
            min: idx_scale.visible.min.max(fft_size / 2),
            max: (idx_scale.visible.max + 1).min(state.end),
        };

        let scaled_range = i_range.divide(1 << level);

        let tex_id = self.id.with(fft_size).with(level);
        let mut texture = vcx.get_texture(tex_id).unwrap_or_else(|| {
            log::info!("Allocating new texture for {fft_size} {level}");
            Box::new(Texture::new(vcx))
        });
        texture.update(&self.view, level, scaled_range);

        let painter = ui.painter_at(rect);
        let y_range = rect.y_range().shrink(8.0);
        let x_range = idx_scale.x_from_idx(texture.range.min << level)..=idx_scale.x_from_idx(texture.range.max << level);
        let paint_rect = Rect::from_x_y_ranges(x_range, y_range);
        painter.add(texture.shape(paint_rect, Colormap::Viridis, (0.0..=1.0).into()));

        vcx.put_texture(tex_id, texture);

        TimelineResponse {
            snap_to_time: None,
        }
    }
}

struct Texture {
    texture: Heatmap<u8>,
    v_start: u32,
    range: IdxRange,
}

impl Texture {
    fn new(vcx: &ViewerContext) -> Self {
        let texture = vcx.heatmap_renderer.heatmap::<u8>(512, 2048, TextureOptions {
            address_mode_u: iguazu_egui_heatmap::AddressMode::Repeat,
            address_mode_v: iguazu_egui_heatmap::AddressMode::Repeat,
            mag_filter: iguazu_egui_heatmap::FilterMode::Nearest,
            min_filter: iguazu_egui_heatmap::FilterMode::Nearest,
        });

        Self { texture, v_start: 0, range: IdxRange { min: 0, max: 0 } }
    }

    fn update(&mut self, view: &SpectrogramView, level: u8, scaled_range: IdxRange) {
        let mut scratch = view.scratch();

        if self.range != scaled_range {
            self.range = IdxRange { min: scaled_range.min, max: scaled_range.min };

            for (v, i) in scaled_range.into_iter().enumerate() {
                if v >= self.texture.height() as usize {
                    break;
                }

                let Some(spectrogram_slice) = scratch.spectrogram_slice(i, level) else { break; };
                self.texture.write(spectrogram_slice, 0, v as u32, spectrogram_slice.len() as u32, 1);

                self.range.max = i + 1;
            }
        }
    }

    fn shape(&self, paint_rect: Rect, colormap: Colormap, value_range: Rangef) -> PaintCallback {
        let v_min = self.v_start as f32 / self.texture.height() as f32;
        let v_max = self.range.len() as f32 / self.texture.height() as f32;
        self.texture.shape_at(paint_rect, iguazu_egui_heatmap::DisplayOptions {
            colormap,
            alpha: 1.0,
            transformation: Transformation::new((-0.5..=0.5).into(), (v_min ..= v_max).into(), SampleOrder::UpRight),
            value_range,
        })
    }
}
