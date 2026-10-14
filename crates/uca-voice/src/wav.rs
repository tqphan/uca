// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A small RIFF/WAVE reader for 16-bit PCM, as Windows speech synthesis
//! returns it (13, decided: no WAV crate).

use crate::tts::PcmBuffer;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WavError {
    #[error("not a RIFF/WAVE stream")]
    NotWave,
    #[error("no fmt chunk before the data")]
    NoFormat,
    #[error("unsupported WAV format {format} with {bits} bits")]
    Unsupported { format: u16, bits: u16 },
    #[error("no data chunk")]
    NoData,
}

/// Decodes 16-bit PCM WAV to mono f32, averaging channels.
pub fn decode(bytes: &[u8]) -> Result<PcmBuffer, WavError> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(WavError::NotWave);
    }
    let mut pos = 12;
    let mut fmt: Option<(u16, u32, u16)> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size).min(bytes.len());
        let body = &bytes[body_start..body_end];
        match id {
            b"fmt " => {
                if body.len() < 16 {
                    return Err(WavError::NoFormat);
                }
                let format = u16::from_le_bytes([body[0], body[1]]);
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                let bits = u16::from_le_bytes([body[14], body[15]]);
                if !matches!(format, WAVE_FORMAT_PCM | WAVE_FORMAT_EXTENSIBLE)
                    || bits != 16
                    || channels == 0
                {
                    return Err(WavError::Unsupported { format, bits });
                }
                fmt = Some((format, rate, channels));
            }
            b"data" => {
                let (_, rate, channels) = fmt.ok_or(WavError::NoFormat)?;
                return Ok(PcmBuffer {
                    samples: samples(body, channels),
                    sample_rate: rate,
                });
            }
            _ => {}
        }
        // Chunks are padded to an even size.
        pos = body_start.saturating_add(size + (size & 1));
    }
    Err(WavError::NoData)
}

fn samples(data: &[u8], channels: u16) -> Vec<f32> {
    let channels = usize::from(channels);
    data.chunks_exact(2 * channels)
        .map(|frame| {
            let sum: f32 = frame
                .as_chunks::<2>()
                .0
                .iter()
                .map(|s| f32::from(i16::from_le_bytes(*s)) / 32_768.0)
                .sum();
            sum / channels as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, extra_chunk: bool, data: &[i16]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(b"WAVE");
        if extra_chunk {
            body.extend_from_slice(b"LIST");
            body.extend_from_slice(&3u32.to_le_bytes());
            body.extend_from_slice(b"abc\0");
        }
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&channels.to_le_bytes());
        body.extend_from_slice(&rate.to_le_bytes());
        body.extend_from_slice(&(rate * 2 * u32::from(channels)).to_le_bytes());
        body.extend_from_slice(&(2 * channels).to_le_bytes());
        body.extend_from_slice(&16u16.to_le_bytes());
        body.extend_from_slice(b"data");
        body.extend_from_slice(&((data.len() * 2) as u32).to_le_bytes());
        for s in data {
            body.extend_from_slice(&s.to_le_bytes());
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn mono_16_bit() {
        let pcm = decode(&wav(1, 22_050, false, &[0, 16_384, -32_768])).unwrap();
        assert_eq!(pcm.sample_rate, 22_050);
        assert_eq!(pcm.samples, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn stereo_is_averaged_and_odd_chunks_are_skipped() {
        let pcm = decode(&wav(2, 16_000, true, &[16_384, 0, -16_384, -16_384])).unwrap();
        assert_eq!(pcm.sample_rate, 16_000);
        assert_eq!(pcm.samples, vec![0.25, -0.5]);
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert_eq!(decode(b"nope"), Err(WavError::NotWave));
        let mut w = wav(1, 8_000, false, &[1]);
        w[34] = 8; // 8 bits per sample
        assert!(matches!(
            decode(&w),
            Err(WavError::Unsupported { bits: 8, .. })
        ));
        let w = wav(1, 8_000, false, &[]);
        assert_eq!(decode(&w[..36]), Err(WavError::NoData));
    }
}
