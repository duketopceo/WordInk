//! The audio a cloud request carries: raw WAV or lossless FLAC.
//!
//! A 16 kHz 16-bit WAV is 32 kB a second, so a two-minute dictation is a
//! 3.6 MB upload and the upload, not the model, is most of the wait. FLAC
//! carries the same samples bit for bit in about half the bytes, so the
//! transcript cannot change. The encoder here is a small pure-Rust one
//! (fixed predictors, Rice-coded residuals) that needs no dependency and
//! runs at several hundred times real time.

use std::time::Instant;

use super::{Provider, SAMPLE_RATE, wav_bytes};

/// How a clip is encoded for upload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadFormat {
    /// 16 kHz mono 16-bit PCM in a RIFF container.
    Wav,
    /// The same samples, losslessly compressed.
    Flac,
}

impl UploadFormat {
    /// The file extension, and the `format` OpenRouter's `input_audio` takes.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
        }
    }

    pub const fn mime(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Flac => "audio/flac",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            _ => None,
        }
    }
}

impl Provider {
    /// The format this provider's audio goes up in. Groq, OpenAI and the
    /// gateway (OpenAI-compatible) document FLAC for `/audio/transcriptions`.
    /// OpenRouter's chat-completions `input_audio` takes FLAC too (a model
    /// that refuses it gets WAV once); Deepgram stays on WAV, not measured here. `WORDINK_UPLOAD_FORMAT=wav|flac` overrides every
    /// provider, to measure or to rule the encoder out.
    pub fn upload_format(self) -> UploadFormat {
        self.upload_format_with(std::env::var("WORDINK_UPLOAD_FORMAT").ok().as_deref())
    }

    fn upload_format_with(self, forced: Option<&str>) -> UploadFormat {
        if let Some(forced) = forced.and_then(UploadFormat::from_name) {
            return forced;
        }
        match self {
            Self::Groq | Self::OpenAi | Self::Gateway => UploadFormat::Flac,
            // Measured: OpenRouter's default chat-audio model reads FLAC.
            Self::OpenRouter => UploadFormat::Flac,
            Self::Deepgram => UploadFormat::Wav,
        }
    }
}

/// A clip, encoded.
pub struct Encoded {
    pub format: UploadFormat,
    pub bytes: Vec<u8>,
    pub encode_ms: u128,
}

/// What one request cost to build and send, for the log and the compare tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestStats {
    pub format: UploadFormat,
    /// The whole request body, framing included.
    pub request_bytes: usize,
    pub encode_ms: u128,
    /// Connecting, uploading, and waiting for the reply's headers.
    pub send_ms: u128,
    /// Reading the reply body.
    pub read_ms: u128,
}

/// `samples` as 16 kHz mono audio in `format`.
pub fn encode(samples: &[f32], format: UploadFormat) -> Encoded {
    let started = Instant::now();
    let bytes = match format {
        UploadFormat::Wav => wav_bytes(samples),
        UploadFormat::Flac => flac_bytes(samples),
    };
    Encoded {
        format,
        bytes,
        encode_ms: started.elapsed().as_millis(),
    }
}

/// The 16-bit samples [`wav_bytes`] writes for `samples`.
fn pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
        .collect()
}

/// Mono 16 kHz 16-bit FLAC bytes: the samples of [`wav_bytes`], compressed.
pub fn flac_bytes(samples: &[f32]) -> Vec<u8> {
    encode_flac(&pcm16(samples), SAMPLE_RATE)
}

const BLOCK: usize = 4096;
/// Partition orders tried, 0 (one Rice parameter per block) to this.
const MAX_PARTITION_ORDER: u32 = 6;

struct Bits {
    bytes: Vec<u8>,
    accumulator: u64,
    count: u32,
}

impl Bits {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            accumulator: 0,
            count: 0,
        }
    }

    /// Appends the low `width` bits of `value`, most significant first.
    fn put(&mut self, value: u64, width: u32) {
        debug_assert!(width <= 32);
        if width == 0 {
            return;
        }
        self.accumulator = (self.accumulator << width) | (value & ((1_u64 << width) - 1));
        self.count += width;
        while self.count >= 8 {
            self.count -= 8;
            self.bytes.push((self.accumulator >> self.count) as u8);
        }
        self.accumulator &= (1_u64 << self.count) - 1;
    }

    fn put_signed(&mut self, value: i32, width: u32) {
        self.put(value as u32 as u64, width);
    }

    /// `quotient` zeros and a one.
    fn put_unary(&mut self, mut quotient: u32) {
        while quotient >= 32 {
            self.put(0, 32);
            quotient -= 32;
        }
        self.put(1, quotient + 1);
    }

    fn align(&mut self) {
        if self.count > 0 {
            self.put(0, 8 - self.count);
        }
    }

    fn bit_len(&self) -> usize {
        self.bytes.len() * 8 + self.count as usize
    }
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0_u8;
    for byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn crc16_table() -> [u16; 256] {
    let mut table = [0_u16; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut crc = (index as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
        *slot = crc;
    }
    table
}

fn crc16(table: &[u16; 256], bytes: &[u8]) -> u16 {
    let mut crc = 0_u16;
    for byte in bytes {
        crc = (crc << 8) ^ table[usize::from((crc >> 8) as u8 ^ byte)];
    }
    crc
}

/// FLAC's UTF-8 style number, for the frame number.
fn put_utf8(bits: &mut Bits, value: u64) {
    if value < 0x80 {
        bits.put(value, 8);
        return;
    }
    // The smallest count of continuation bytes whose lead byte holds the
    // rest: n continuation bytes carry 5n + 6 bits.
    let mut continuation = 1_u32;
    while value >> (5 * continuation + 6) != 0 {
        continuation += 1;
    }
    let lead_mask = (0xFF00_u32 >> (continuation + 1)) as u8;
    bits.put(u64::from(lead_mask) | (value >> (6 * continuation)), 8);
    for index in (0..continuation).rev() {
        bits.put(0x80 | ((value >> (6 * index)) & 0x3F), 8);
    }
}

fn zigzag(residual: i32) -> u32 {
    ((residual << 1) ^ (residual >> 31)) as u32
}

/// The residual of fixed predictor `order` (0 to 4) over `block`.
fn fixed_residual(block: &[i16], order: usize) -> Vec<i32> {
    let sample = |index: usize| i32::from(block[index]);
    (order..block.len())
        .map(|i| match order {
            0 => sample(i),
            1 => sample(i) - sample(i - 1),
            2 => sample(i) - 2 * sample(i - 1) + sample(i - 2),
            3 => sample(i) - 3 * sample(i - 1) + 3 * sample(i - 2) - sample(i - 3),
            _ => {
                sample(i) - 4 * sample(i - 1) + 6 * sample(i - 2) - 4 * sample(i - 3)
                    + sample(i - 4)
            }
        })
        .collect()
}

/// The Rice parameter that codes a partition of `count` values whose
/// zigzagged sum is `sum` in the fewest bits (estimated from the sum, which
/// is exact enough to choose by; the coding itself is exact).
fn rice_parameter(sum: u64, count: usize) -> (u32, u64) {
    let mut best = (0_u32, u64::MAX);
    for k in 0..=24_u32 {
        let bits = count as u64 * u64::from(k + 1) + (sum >> k);
        if bits < best.1 {
            best = (k, bits);
        }
    }
    best
}

/// The partition order and Rice parameters that code `residual` (the
/// zigzagged values of a block, after `order` warm-up samples) in the
/// fewest estimated bits.
fn plan_partitions(residual: &[u32], block: usize, order: usize) -> (u32, Vec<u32>, u64) {
    let mut best: (u32, Vec<u32>, u64) = (0, Vec::new(), u64::MAX);
    for partition_order in 0..=MAX_PARTITION_ORDER {
        let partitions = 1_usize << partition_order;
        if !block.is_multiple_of(partitions) || block / partitions <= order {
            break;
        }
        let size = block / partitions;
        let mut parameters = Vec::with_capacity(partitions);
        let mut bits = 0_u64;
        let mut start = 0;
        for index in 0..partitions {
            let count = if index == 0 { size - order } else { size };
            let sum: u64 = residual[start..start + count]
                .iter()
                .map(|&value| u64::from(value))
                .sum();
            let (k, cost) = rice_parameter(sum, count);
            parameters.push(k);
            bits += cost + 5;
            start += count;
        }
        if bits < best.2 {
            best = (partition_order, parameters, bits);
        }
    }
    best
}

/// Appends the subframe for `block`: constant, fixed predictor, or verbatim,
/// whichever is smallest.
fn put_subframe(bits: &mut Bits, block: &[i16]) {
    if block.iter().all(|&sample| sample == block[0]) {
        bits.put(0, 8);
        bits.put_signed(i32::from(block[0]), 16);
        return;
    }
    let verbatim_bits = block.len() as u64 * 16;
    #[allow(clippy::type_complexity)]
    let mut best: Option<(usize, Vec<i32>, u32, Vec<u32>, u64)> = None;
    for order in 0..=4_usize.min(block.len().saturating_sub(1)) {
        let residual = fixed_residual(block, order);
        let zigzagged: Vec<u32> = residual.iter().map(|&value| zigzag(value)).collect();
        let (partition_order, parameters, bits_needed) =
            plan_partitions(&zigzagged, block.len(), order);
        if parameters.is_empty() {
            continue;
        }
        let total = bits_needed + order as u64 * 16 + 6;
        if best.as_ref().is_none_or(|current| total < current.4) {
            best = Some((order, residual, partition_order, parameters, total));
        }
    }
    match best {
        Some((order, residual, partition_order, parameters, total)) if total < verbatim_bits => {
            bits.put(0, 1);
            bits.put(0b001000 | order as u64, 6);
            bits.put(0, 1);
            for &sample in &block[..order] {
                bits.put_signed(i32::from(sample), 16);
            }
            // Rice coding with 5-bit parameters.
            bits.put(1, 2);
            bits.put(u64::from(partition_order), 4);
            let partitions = 1_usize << partition_order;
            let size = block.len() >> partition_order;
            let mut start = 0;
            for (index, &k) in parameters.iter().enumerate() {
                let count = if index == 0 { size - order } else { size };
                bits.put(u64::from(k), 5);
                for &value in &residual[start..start + count] {
                    let value = zigzag(value);
                    bits.put_unary(value >> k);
                    bits.put(u64::from(value), k);
                }
                start += count;
            }
            debug_assert_eq!(start, residual.len());
            debug_assert_eq!(parameters.len(), partitions);
        }
        _ => {
            bits.put(0, 1);
            bits.put(0b000001, 6);
            bits.put(0, 1);
            for &sample in block {
                bits.put_signed(i32::from(sample), 16);
            }
        }
    }
}

/// A FLAC stream of mono 16-bit `samples` at `rate` Hz.
pub fn encode_flac(samples: &[i16], rate: u32) -> Vec<u8> {
    let table = crc16_table();
    let mut frames: Vec<Vec<u8>> = Vec::new();
    for (index, block) in samples.chunks(BLOCK).enumerate() {
        let mut bits = Bits::new();
        // Sync, fixed block size.
        bits.put(0b11_1111_1111_1110, 14);
        bits.put(0, 1);
        bits.put(0, 1);
        // Block size: 4096 has a code of its own; a last short block says
        // its size after the frame number.
        let full = block.len() == BLOCK;
        bits.put(if full { 0b1100 } else { 0b0111 }, 4);
        // 16 kHz has a code; any other rate is read from STREAMINFO.
        bits.put(if rate == 16_000 { 0b0101 } else { 0b0000 }, 4);
        // One channel, 16 bits a sample.
        bits.put(0, 4);
        bits.put(0b100, 3);
        bits.put(0, 1);
        put_utf8(&mut bits, index as u64);
        if !full {
            bits.put((block.len() - 1) as u64, 16);
        }
        let header = bits.bytes.clone();
        bits.put(u64::from(crc8(&header)), 8);
        put_subframe(&mut bits, block);
        bits.align();
        let crc = crc16(&table, &bits.bytes);
        bits.put(u64::from(crc), 16);
        frames.push(bits.bytes);
    }
    let min_frame = frames.iter().map(Vec::len).min().unwrap_or(0);
    let max_frame = frames.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = Vec::with_capacity(42 + frames.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(b"fLaC");
    let mut info = Bits::new();
    // The only metadata block, so the last one.
    info.put(1, 1);
    info.put(0, 7);
    info.put(34, 24);
    let block_size = BLOCK.min(samples.len().max(16)) as u64;
    info.put(block_size, 16);
    info.put(block_size, 16);
    info.put(min_frame as u64, 24);
    info.put(max_frame as u64, 24);
    info.put(u64::from(rate), 20);
    info.put(0, 3);
    info.put(15, 5);
    info.put(samples.len() as u64 >> 32, 4);
    info.put(samples.len() as u64 & 0xFFFF_FFFF, 32);
    // No MD5: all zeros means unknown.
    for _ in 0..4 {
        info.put(0, 32);
    }
    debug_assert_eq!(info.bit_len() % 8, 0);
    out.extend_from_slice(&info.bytes);
    for frame in frames {
        out.extend_from_slice(&frame);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic stand-in for speech: a voiced pitch with harmonics, a
    /// syllable-rate envelope, pauses, and a little broadband noise.
    fn speech_like(samples: usize) -> Vec<i16> {
        let mut noise = 0x2545_F491_u32;
        (0..samples)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                let envelope = (0.5 + 0.5 * (t * 2.0 * std::f32::consts::PI * 3.0).sin()).powi(2);
                let pause = if (t * 1.3).fract() > 0.85 { 0.02 } else { 1.0 };
                let pitch = 120.0 + 15.0 * (t * 0.9).sin();
                let voiced: f32 = (1..=8)
                    .map(|h| (t * pitch * h as f32 * 2.0 * std::f32::consts::PI).sin() / h as f32)
                    .sum();
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let hiss = ((noise >> 16) as f32 / 65_536.0 - 0.5) * 0.02;
                ((voiced * 0.18 * envelope * pause + hiss) * 32_767.0) as i16
            })
            .collect()
    }

    fn decode(flac: &[u8]) -> (claxon::metadata::StreamInfo, Vec<i32>) {
        let mut reader = claxon::FlacReader::new(flac).expect("a valid FLAC stream");
        let info = reader.streaminfo();
        let samples = reader
            .samples()
            .collect::<Result<Vec<_>, _>>()
            .expect("every frame decodes, CRCs included");
        (info, samples)
    }

    fn assert_round_trip(samples: &[i16]) {
        let (info, decoded) = decode(&encode_flac(samples, 16_000));
        assert_eq!(
            (info.sample_rate, info.channels, info.bits_per_sample),
            (16_000, 1, 16)
        );
        assert_eq!(info.samples, Some(samples.len() as u64));
        let expected: Vec<i32> = samples.iter().map(|&s| i32::from(s)).collect();
        assert_eq!(decoded, expected, "FLAC is lossless");
    }

    #[test]
    fn flac_decodes_to_exactly_the_pcm_it_was_made_from() {
        // Whole blocks, a short last block, one block, and just over one.
        for length in [4096 * 3, 4096 * 3 + 777, 4096, 4097, 1000, 100, 17] {
            assert_round_trip(&speech_like(length));
        }
    }

    #[test]
    fn flac_round_trips_long_clips_past_the_one_byte_frame_numbers() {
        // 40 s is 157 frames: the frame number needs two bytes from 128 on.
        assert_round_trip(&speech_like(16_000 * 40));
    }

    #[test]
    fn flac_round_trips_silence_extremes_and_noise() {
        assert_round_trip(&vec![0; 10_000]);
        assert_round_trip(&vec![-32_768; 5_000]);
        let alternating: Vec<i16> = (0..9_000)
            .map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN })
            .collect();
        assert_round_trip(&alternating);
        let mut state = 7_u32;
        let noise: Vec<i16> = (0..9_000)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 16) as i16
            })
            .collect();
        assert_round_trip(&noise);
        // Full-scale noise cannot be compressed, so it is stored verbatim:
        // the stream is no more than a header and a few bytes a frame larger.
        assert!(encode_flac(&noise, 16_000).len() < noise.len() * 2 + 100);
    }

    #[test]
    fn flac_frame_numbers_are_utf8() {
        for value in [0_u64, 0x7F, 0x80, 0x7FF, 0x800, 0xFFFF, 0x1_0000, 0x10_FFFF] {
            let mut bits = Bits::new();
            put_utf8(&mut bits, value);
            let mut expected = [0_u8; 4];
            let expected = char::from_u32(value as u32)
                .unwrap()
                .encode_utf8(&mut expected)
                .as_bytes()
                .to_vec();
            assert_eq!(bits.bytes, expected, "{value:#x}");
        }
    }

    #[test]
    fn speech_is_at_least_a_third_smaller_as_flac_than_as_wav() {
        let pcm = speech_like(16_000 * 10);
        let samples: Vec<f32> = pcm.iter().map(|&s| f32::from(s) / 32_767.0).collect();
        let wav = encode(&samples, UploadFormat::Wav);
        let flac = encode(&samples, UploadFormat::Flac);
        assert!(
            flac.bytes.len() * 3 < wav.bytes.len() * 2,
            "{} bytes of FLAC against {} of WAV",
            flac.bytes.len(),
            wav.bytes.len()
        );
        // And the same samples come back from what was sent.
        let (_, decoded) = decode(&flac.bytes);
        let sent: Vec<i32> = pcm16(&samples).iter().map(|&s| i32::from(s)).collect();
        assert_eq!(decoded, sent);
    }

    #[test]
    fn groq_openai_and_the_gateway_send_flac_and_the_rest_wav_unless_forced() {
        for provider in [Provider::Groq, Provider::OpenAi, Provider::Gateway] {
            assert_eq!(provider.upload_format_with(None), UploadFormat::Flac);
            assert_eq!(provider.upload_format_with(Some("wav")), UploadFormat::Wav);
        }
        assert_eq!(
            Provider::OpenRouter.upload_format_with(None),
            UploadFormat::Flac
        );
        assert_eq!(
            Provider::Deepgram.upload_format_with(None),
            UploadFormat::Wav
        );
        assert_eq!(
            Provider::Deepgram.upload_format_with(Some("FLAC")),
            UploadFormat::Flac
        );
        assert_eq!(
            Provider::Groq.upload_format_with(Some("opus")),
            UploadFormat::Flac,
            "an unknown name changes nothing"
        );
    }
}
