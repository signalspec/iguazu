use num_complex::Complex;

use crate::Idx;
use crate::schema::{Entity, EntityStream, attribute::core::Role};
use crate::stream::StreamState;
use crate::util::stft::{StftComplex, StftComplexScratch};

use crate::view::{NumberView, ViewManager};

pub struct SpectrogramView<'a> {
    view: NumberView<'a>,
    stft: StftComplex,
}

impl<'a> SpectrogramView<'a> {
    pub fn new(vm: &'a ViewManager, entity: &EntityStream) -> Option<Self> {
        let fft_width = 512;
        match entity {
            Entity::Tuple { names, inner, ..} if entity.role() == Some(Role::Complex) && names.len() == 2 => {
                match &**inner {
                    Entity::Data { field, data, .. } => {
                        Some(Self::complex(NumberView::new(vm, data, field)?, fft_width))
                    }
                    _ => None
                }
            }
            _ => None
        }
    }

    pub fn complex(view: NumberView<'a>, fft_width: usize) -> Self {
        Self { view, stft: StftComplex::new(fft_width) }
    }

    pub fn state(&self) -> StreamState {
        let inner_state = self.view.state();
        StreamState {
            end: inner_state.end / 2,
            streaming: inner_state.streaming,
        }
    }

    pub fn scratch(&self) -> Scratch<'a, '_> {
        Scratch {
            view: self,
            complex: vec![Complex::new(0.0, 0.0); self.stft.size()].into_boxed_slice(),
            scratch: self.stft.scratch(),
        }
    }

    pub fn fft_size(&self) -> u64 {
        self.stft.size() as u64
    }
}

pub struct Scratch<'v, 'a> {
    view: &'a SpectrogramView<'v>,
    complex: Box<[Complex<f32>]>,
    scratch: StftComplexScratch,
}

impl<'v, 'a> Scratch<'v, 'a> {
    pub fn spectrogram_slice(&mut self, center: Idx, level: u8) -> Option<&[u8]> {
        let start = 2 * (center << level).saturating_sub(self.view.fft_size() / 2);
        let copied = self.view.view.read_f32_slice(start, bytemuck::cast_slice_mut(&mut self.complex));
        if copied != self.complex.len() * 2 {
            return None;
        }
        Some(self.view.stft.perform(&mut self.scratch, &mut self.complex))
    }
}
