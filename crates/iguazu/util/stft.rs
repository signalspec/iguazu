use std::f32::consts::{LOG10_2, TAU};

use itertools::multizip;
use num_complex::Complex;
use phastft::fft_f32_dit_with_planner;
use phastft::planner::{Direction, PlannerDit32};

pub struct StftComplex {
    window: Box<[f32]>,
    planner: PlannerDit32,
    power_scale: PowerScale,
}

pub struct StftComplexScratch {
    re: Box<[f32]>,
    im: Box<[f32]>,
    output: Box<[u8]>,
}

impl StftComplex {
    pub fn new(width: usize) -> Self {
        const QUANT_LSB: f64 = 1.0 / 32768.0;
        StftComplex {
            window: hann_window(width),
            planner: PlannerDit32::new(width),
            power_scale: PowerScale::new(width, QUANT_LSB),
        }
    }

    pub fn size(&self) -> usize {
        self.window.len()
    }

    pub fn scratch(&self) -> StftComplexScratch {
        StftComplexScratch {
            re: vec![0.0; self.size()].into_boxed_slice(),
            im: vec![0.0; self.size()].into_boxed_slice(),
            output: vec![0; self.size()].into_boxed_slice(),
        }
    }

    pub fn perform<'a>(&self, scratch: &'a mut StftComplexScratch, input: &[Complex<f32>]) -> &'a [u8] {
        for (i, w, re, im) in multizip((input.iter(), self.window.iter(), scratch.re.iter_mut(), scratch.im.iter_mut())) {
            *re = i.re * w;
            *im = i.im * w;
        }
        fft_f32_dit_with_planner(&mut scratch.re, &mut scratch.im, Direction::Forward, &self.planner);

        for (re, im, output) in multizip((scratch.re.iter(), scratch.im.iter(), scratch.output.iter_mut())) {
            *output = self.power_scale.compress(re * re + im * im);
        }
        &scratch.output
    }
}


fn hann_window(width: usize) -> Box<[f32]> {
    (0..width).map(|i| 0.5 * (1.0 - (TAU * i as f32 / width as f32).cos())).collect()
}

/// Linear map between FFT bin power and compressed u8.
pub struct PowerScale {
    pub floor: f32,
    pub scale: f32,
}

impl PowerScale {
    pub fn new(fft_size: usize, quant_lsb: f64) -> Self {
        let n = fft_size as f64;

        // Assuming Hann window
        let window_sum = n / 2.0;
        let window_sum_sq = 3.0 / 8.0 * n;

        // If all energy is in one bin its max is the sum of input * window with unnormalized FFT.
        let ceiling = 2.0 * window_sum.log2() as f32;

        // Variance of uncorrelated uniform quanitization noise is q^2 / 12 per component
        // (*2 for complex) then scaled by the window power.
        let floor = (quant_lsb * quant_lsb / 6.0 * window_sum_sq).log2() as f32;

        PowerScale { floor, scale: 255.0 / (ceiling - floor) }
    }

    /// Compress a squared bin magnitude
    pub fn compress(&self, power: f32) -> u8 {
        ((power.log2() - self.floor) * self.scale).round().clamp(0.0, 255.0) as u8
    }

    /// dBFS of a compressed value
    pub fn db(&self, value: u8) -> f32 {
        (self.floor + value as f32 / self.scale) * (10.0 * LOG10_2)
    }
}
