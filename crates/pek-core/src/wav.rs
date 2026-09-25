//! Minimal WAV handling for user-replaceable sounds: volume scaling of 16-bit PCM files.

fn chunks(wav: &[u8]) -> impl Iterator<Item = (&[u8], std::ops::Range<usize>)> {
    let mut pos = 12;
    std::iter::from_fn(move || {
        if pos + 8 > wav.len() {
            return None;
        }
        let id = &wav[pos..pos + 4];
        let size = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8..(pos + 8 + size).min(wav.len());
        pos = body.end + (size & 1);
        Some((id, body))
    })
}

/// Returns a copy of a 16-bit PCM WAV with samples multiplied by `volume` (0.0..=1.0),
/// or `None` if the file is not a WAV of that kind.
pub fn scale_pcm16(wav: &[u8], volume: f32) -> Option<Vec<u8>> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let fmt = chunks(wav).find(|(id, _)| *id == b"fmt ")?.1;
    let format = u16::from_le_bytes(wav.get(fmt.start..fmt.start + 2)?.try_into().ok()?);
    let bits = u16::from_le_bytes(wav.get(fmt.start + 14..fmt.start + 16)?.try_into().ok()?);
    if format != 1 || bits != 16 {
        return None;
    }
    let data = chunks(wav).find(|(id, _)| *id == b"data")?.1;
    let mut out = wav.to_vec();
    let volume = volume.clamp(0.0, 1.0);
    for i in (data.start..data.end.saturating_sub(1)).step_by(2) {
        let s = i16::from_le_bytes([out[i], out[i + 1]]) as f32 * volume;
        out[i..i + 2].copy_from_slice(&(s as i16).to_le_bytes());
    }
    Some(out)
}
