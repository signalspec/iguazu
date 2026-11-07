use ecow::EcoString;
use egui::{Rect, Vec2};
use iguazu::{IdxRange, schema::EntityStream, time::{Time, TimeRange}, view::{SpectrogramView, Timebase}};
use iguazu_egui_heatmap::{Colormap, Heatmap, SampleOrder, TextureOptions, Transformation};

use crate::{ViewerContext, timeline::{TimelineResponse, row_label, scale::Scale, stream_rect}};

pub(crate) struct SpectrogramRow<'a> {
    sample_rate: f64,
    view: SpectrogramView<'a>,
    label: EcoString,
    heatmap: Heatmap<u8>,
}

impl<'a> SpectrogramRow<'a> {
    pub fn new(vcx: &'a ViewerContext, label: EcoString, entity: &EntityStream, timebase: &Timebase<'a>,) -> Option<Self> {
        let sample_rate = timebase.uniform_sample_rate()?;
        let view = SpectrogramView::new(&vcx.view_manager, entity)?;

        let heatmap = vcx.heatmap_renderer.heatmap(512, 2048, TextureOptions {
            address_mode_u: iguazu_egui_heatmap::AddressMode::Repeat,
            address_mode_v: iguazu_egui_heatmap::AddressMode::Repeat,
            mag_filter: iguazu_egui_heatmap::FilterMode::Nearest,
            min_filter: iguazu_egui_heatmap::FilterMode::Nearest,
        });

        Some(Self { sample_rate, view, label, heatmap })
    }

    pub fn time_range(&self) -> Option<TimeRange> {
        Some(TimeRange {
            min: Time::ZERO,
            max: (self.view.state().end as i128) * Time::period_float(self.sample_rate),
        })
    }

    pub fn render(&self, ui: &mut egui::Ui, scale: &Scale) -> TimelineResponse{
        row_label(ui, &self.label);
        let rect = stream_rect(ui, scale);

        let padded_rect = rect.shrink2(Vec2::new(0.0, 8.0));

        if !ui.is_rect_visible(padded_rect) {
            return TimelineResponse::default();
        }

        let fft_size = self.view.fft_size();

        let idx_scale = scale.idx_scale(self.sample_rate);
        let level = idx_scale.min_visible_width_log2(ui.pixels_per_point());
        let state = self.view.state();
        let x_range = IdxRange {
            min: idx_scale.visible.min.max(fft_size / 2),
            max: (idx_scale.visible.max + 1).min(state.end),
        };

        let mut scratch = self.view.scratch();

        let mut max_i = x_range.min;
        let mut max_v = 0;

        let scaled_range = x_range.divide(1 << level);

        for (v, i) in scaled_range.into_iter().enumerate() {
            if v >= self.heatmap.height() as usize {
                break;
            }

            let Some(spectrogram_slice) = scratch.spectrogram_slice(i, level) else { break; };
            self.heatmap.write(spectrogram_slice, 0, v as u32, fft_size as u32, 1);

            max_i = i;
            max_v = v;
        }

        let painter = ui.painter_at(rect);

        let paint_rect = Rect::from_x_y_ranges(idx_scale.x_from_idx(scaled_range.min << level)..=idx_scale.x_from_idx(max_i << level), padded_rect.y_range());
        painter.add(self.heatmap.shape_at(paint_rect, iguazu_egui_heatmap::DisplayOptions {
            colormap: Colormap::Viridis,
            alpha: 1.0,
            transformation: Transformation::new((-0.5..=0.5).into(), (0.0..=(max_v as f32 / self.heatmap.height() as f32)).into(), SampleOrder::UpRight),
            value_range: (0.0..=1.0).into(),
        }));

        TimelineResponse {
            snap_to_time: None,
        }
    }
}
