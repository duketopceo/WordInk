use wordink_core::audio::{encode_wav, Resampler};

fn sine(rate: u32, freq: f64, seconds: f64) -> Vec<f32> {
    let n = (rate as f64 * seconds) as usize;
    (0..n)
        .map(|i| (0.8 * (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin()) as f32)
        .collect()
}

/// Estimates frequency from interpolated rising zero crossings, ignoring the
/// first and last 10% of the signal (filter edges).
fn estimate_freq(samples: &[f32], rate: u32) -> f64 {
    let lo = samples.len() / 10;
    let hi = samples.len() - lo;
    let mut crossings = Vec::new();
    for i in lo..hi - 1 {
        let (a, b) = (samples[i] as f64, samples[i + 1] as f64);
        if a < 0.0 && b >= 0.0 {
            crossings.push(i as f64 + a / (a - b));
        }
    }
    let periods = (crossings.len() - 1) as f64;
    let span = crossings.last().unwrap() - crossings.first().unwrap();
    rate as f64 * periods / span
}

fn resample_chunked(input: &[f32], from: u32, to: u32, chunk: usize) -> Vec<f32> {
    let mut r = Resampler::new(from, to);
    let mut out = Vec::new();
    for c in input.chunks(chunk) {
        r.process(c, &mut out);
    }
    r.flush(&mut out);
    out
}

#[test]
fn resample_48k_to_16k_keeps_tone_and_length() {
    let input = sine(48_000, 1_000.0, 1.0);
    let out = resample_chunked(&input, 48_000, 16_000, 128);
    assert!((out.len() as i64 - 16_000).abs() <= 1, "len {}", out.len());
    let f = estimate_freq(&out, 16_000);
    assert!((f - 1_000.0).abs() / 1_000.0 < 0.01, "freq {f}");
    // Amplitude survives the passband.
    let peak = out[2_000..14_000].iter().fold(0f32, |m, x| m.max(x.abs()));
    assert!((peak - 0.8).abs() < 0.05, "peak {peak}");
}

#[test]
fn resample_44_1k_to_24k_keeps_tone_and_length() {
    let input = sine(44_100, 1_000.0, 1.0);
    let out = resample_chunked(&input, 44_100, 24_000, 441);
    assert!((out.len() as i64 - 24_000).abs() <= 1, "len {}", out.len());
    let f = estimate_freq(&out, 24_000);
    assert!((f - 1_000.0).abs() / 1_000.0 < 0.01, "freq {f}");
}

/// Minimal RIFF/WAVE decoder used to check the encoder's output.
fn decode_wav(bytes: &[u8]) -> (u16, u32, u16, Vec<i16>) {
    let u16le = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let u32le = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(u32le(4) as usize, bytes.len() - 8);
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(&bytes[12..16], b"fmt ");
    assert_eq!(u32le(16), 16);
    assert_eq!(u16le(20), 1, "PCM format");
    let channels = u16le(22);
    let rate = u32le(24);
    let byte_rate = u32le(28);
    let block_align = u16le(32);
    let bits = u16le(34);
    assert_eq!(block_align, channels * bits / 8);
    assert_eq!(byte_rate, rate * block_align as u32);
    assert_eq!(&bytes[36..40], b"data");
    let len = u32le(40) as usize;
    assert_eq!(len, bytes.len() - 44);
    let samples = bytes[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    (channels, rate, bits, samples)
}

#[test]
fn wav_header_round_trips() {
    let samples: Vec<i16> = vec![0, 1, -1, i16::MAX, i16::MIN, 12_345, -12_345];
    let wav = encode_wav(&samples, 16_000);
    let (channels, rate, bits, decoded) = decode_wav(&wav);
    assert_eq!(channels, 1);
    assert_eq!(rate, 16_000);
    assert_eq!(bits, 16);
    assert_eq!(decoded, samples);
}
