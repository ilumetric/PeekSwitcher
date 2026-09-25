"""Synthesizes placeholder sounds into target/sounds (the shipped ones in resources/sounds
are recorded separately; copy these over only if you want the synthesized set).

Deep, soft ambient chords in the spirit of Apple system sounds: slow attack, warm tone
(almost no bright overtones), gently detuned voices and a long airy reverb.

  en.wav   - layout switched to English: D major (D3 D4 F#4 A4)
  ru.wav   - layout switched to Russian: B minor (B2 B3 D4 F#4), the relative minor of D major
  typo.wav - a typo was fixed: one short muted note
  on.wav / off.wav - auto mode on / off: a rising / falling fifth

Any file can be replaced by your own 16-bit PCM WAV with the same name.
Run:  python tools/gen_sounds.py
"""
import os
import wave

import numpy as np

SR = 48000
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "target", "sounds")


def hz(midi):
    return 440.0 * 2 ** ((midi - 69) / 12)


def voice(freq, n, attack, tau, detune_cents):
    """A warm voice: sine body, faint 2nd/3rd harmonics, slow swell, long decay."""
    t = np.arange(n) / SR
    f = freq * 2 ** (detune_cents / 1200)
    sig = np.sin(2 * np.pi * f * t) + 0.10 * np.sin(2 * np.pi * 2 * f * t) + 0.025 * np.sin(2 * np.pi * 3 * f * t)
    env = (1 - np.exp(-t / attack)) * np.exp(-t / tau)
    return sig * env


def lowpass(x, cutoff):
    a = np.exp(-2 * np.pi * cutoff / SR)
    y = np.empty_like(x)
    acc = 0.0
    for i, v in enumerate(x):
        acc = (1 - a) * v + a * acc
        y[i] = acc
    return y


def reverb(buf, decay=0.85, wet=0.4, predelay=0.022, seed=11):
    rng = np.random.default_rng(seed)
    n = int(SR * decay * 3)
    t = np.arange(n) / SR
    out = np.zeros_like(buf)
    for ch in range(2):
        ir = rng.standard_normal(n) * np.exp(-t / decay)
        ir = lowpass(ir, 2400)  # dark, airy tail
        ir[: int(SR * predelay)] = 0
        ir /= np.sqrt(np.sum(ir**2))
        size = len(buf) + n
        spec = np.fft.rfft(buf[:, ch], size) * np.fft.rfft(ir, size)
        out[:, ch] = np.fft.irfft(spec, size)[: len(buf)]
    return buf * (1 - wet) + out * wet


def render(notes, length, attack=0.03, tau=0.9, peak=0.34, wet=0.4, cutoff=2600):
    """notes: (midi, start seconds, amplitude)."""
    n = int(SR * length)
    buf = np.zeros((n, 2))
    for midi, start, amp in notes:
        i = int(start * SR)
        m = n - i
        f = hz(midi)
        # Two slightly detuned voices per ear give width and a slow shimmer.
        left = voice(f, m, attack, tau, -4) + voice(f, m, attack, tau, +3)
        right = voice(f, m, attack, tau, +4) + voice(f, m, attack, tau, -3)
        buf[i:, 0] += left * amp
        buf[i:, 1] += right * amp
    for ch in range(2):
        buf[:, ch] = lowpass(buf[:, ch], cutoff)
    buf = reverb(buf, wet=wet)
    fade = int(SR * 0.25)
    buf[-fade:] *= np.linspace(1, 0, fade)[:, None] ** 2
    return buf * (peak / np.max(np.abs(buf)))


def save(name, buf):
    data = (np.clip(buf, -1, 1) * 32767).astype("<i2")
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(data.tobytes())
    print(f"{name}: {len(buf) / SR:.2f}s")


def main():
    os.makedirs(OUT, exist_ok=True)
    D3, D4, Fs4, A4 = 50, 62, 66, 69
    B2, B3 = 47, 59
    save("en.wav", render([(D3, 0.0, 0.45), (D4, 0.0, 0.8), (Fs4, 0.025, 0.6), (A4, 0.05, 0.5)], 2.0))
    save("ru.wav", render([(B2, 0.0, 0.5), (B3, 0.0, 0.8), (D4, 0.03, 0.62), (Fs4, 0.06, 0.5)], 2.1))
    save("typo.wav", render([(A4, 0.0, 1.0), (A4 - 12, 0.0, 0.35)], 0.9, attack=0.006, tau=0.16, peak=0.24, wet=0.3, cutoff=2200))
    save("on.wav", render([(D4, 0.0, 0.9), (A4, 0.11, 0.8)], 1.6, tau=0.6, peak=0.28))
    save("off.wav", render([(A4, 0.0, 0.8), (D4, 0.11, 0.9)], 1.6, tau=0.6, peak=0.26))


if __name__ == "__main__":
    main()
