//! Audio pipeline: resampling, PCM16 conversion and WAV encoding.
//!
//! Hosts capture mono Float32 audio at whatever rate the device runs. The core
//! turns it into PCM16 at 16 kHz or 24 kHz (KTD4).

use core::f64::consts::PI;

/// Kernel half-width, in zero crossings of the low-pass sinc.
const ZERO_CROSSINGS: f64 = 12.0;
/// Low-pass cutoff as a fraction of the lower Nyquist frequency, leaving room
/// for the filter's transition band below it.
const CUTOFF: f64 = 0.9;

/// Streaming windowed-sinc resampler for mono Float32 audio.
///
/// Feed chunks of any size with [`process`](Self::process), then call
/// [`flush`](Self::flush) once at the end to drain the filter tail. The total
/// output length is `ceil(input_len * to / from)`.
#[derive(Debug, Clone)]
pub struct Resampler {
    /// Input and output rates, reduced by their greatest common divisor.
    from: u64,
    to: u64,
    /// Low-pass cutoff relative to the input Nyquist frequency.
    cutoff: f64,
    /// Kernel half-width in input samples.
    half: f64,
    /// Buffered input; `buf[0]` is absolute input index `base`.
    buf: Vec<f32>,
    base: u64,
    /// Total input samples received.
    received: u64,
    /// Index of the next output sample.
    next: u64,
}

impl Resampler {
    /// Creates a resampler from `from` Hz to `to` Hz. Both must be non-zero.
    pub fn new(from: u32, to: u32) -> Self {
        let g = gcd(from as u64, to as u64);
        let cutoff = CUTOFF * (to as f64 / from as f64).min(1.0);
        Self {
            from: from as u64 / g,
            to: to as u64 / g,
            cutoff,
            half: ZERO_CROSSINGS / cutoff,
            buf: Vec::new(),
            base: 0,
            received: 0,
            next: 0,
        }
    }

    /// Resamples `input`, appending every output sample that is now complete
    /// to `out`. Output lags input by the filter's half-width.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.buf.extend_from_slice(input);
        self.received += input.len() as u64;
        loop {
            let p = self.position(self.next);
            // Every input sample the kernel touches must have arrived.
            if (p + self.half).floor() as u64 >= self.received {
                break;
            }
            out.push(self.sample_at(p));
            self.next += 1;
        }
        self.trim();
    }

    /// Drains the remaining output, treating input past the end as silence,
    /// and resets the resampler for reuse.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let total = (self.received * self.to).div_ceil(self.from);
        while self.next < total {
            let p = self.position(self.next);
            out.push(self.sample_at(p));
            self.next += 1;
        }
        *self = Self {
            buf: Vec::new(),
            base: 0,
            received: 0,
            next: 0,
            ..*self
        };
    }

    /// Input-sample position of output sample `n`.
    fn position(&self, n: u64) -> f64 {
        (n * self.from) as f64 / self.to as f64
    }

    /// Filtered value at fractional input position `p`.
    fn sample_at(&self, p: f64) -> f32 {
        let first = (p - self.half).floor() as i64 + 1;
        let last = (p + self.half).floor() as i64;
        let (mut acc, mut norm) = (0.0f64, 0.0f64);
        for i in first..=last {
            let d = i as f64 - p;
            let w = self.kernel(d);
            norm += w;
            if i >= 0 && (i as u64) < self.received {
                acc += w * self.buf[(i as u64 - self.base) as usize] as f64;
            }
        }
        if norm == 0.0 {
            0.0
        } else {
            (acc / norm) as f32
        }
    }

    /// Blackman-windowed sinc low-pass at distance `d` input samples.
    fn kernel(&self, d: f64) -> f64 {
        let x = self.cutoff * d;
        let sinc = if x == 0.0 {
            1.0
        } else {
            (PI * x).sin() / (PI * x)
        };
        let t = d / self.half;
        let window = 0.42 + 0.5 * (PI * t).cos() + 0.08 * (2.0 * PI * t).cos();
        sinc * window
    }

    /// Drops buffered input the next output sample no longer needs.
    fn trim(&mut self) {
        let p = self.position(self.next);
        let keep_from = ((p - self.half).floor().max(0.0) as u64).min(self.received);
        if keep_from > self.base {
            self.buf.drain(..(keep_from - self.base) as usize);
            self.base = keep_from;
        }
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Converts Float32 samples in -1.0..=1.0 to PCM16, clamping out-of-range input.
pub fn to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
        .collect()
}

/// Encodes mono PCM16 samples as a RIFF/WAVE file.
pub fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}
