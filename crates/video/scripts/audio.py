"""Quiet original nature bed: stream, wind, and a few distant birds."""
from pathlib import Path
import wave
import numpy as np

SR = 48000
DURATION = 30
rng = np.random.default_rng(23)
n = SR * DURATION
t = np.arange(n) / SR

white = rng.normal(0, 1, n)
kernel = np.exp(-np.arange(1800) / 420)
stream = np.convolve(white, kernel, mode="same")
stream /= np.max(np.abs(stream))
wind = np.convolve(rng.normal(0, 1, n), np.exp(-np.arange(8000) / 2400), mode="same")
wind /= np.max(np.abs(wind))
gust = 0.65 + 0.35 * np.sin(2 * np.pi * 0.07 * t + 0.4)
mix = np.zeros((n, 2))
bed = (stream * 0.55 + wind * gust * 0.28) * 0.11
mix[:, 0] += bed * 0.92
mix[:, 1] += bed * 1.0
for start, freq, pan in (
    (4.2, 2480, -0.35),
    (11.6, 2760, 0.4),
    (18.4, 2320, -0.15),
    (23.8, 3010, 0.25),
):
    x = np.arange(int(SR * 0.22)) / SR
    chirp = np.sin(2 * np.pi * freq * x * (1 + 0.04 * x)) * np.exp(-x * 14) * 0.035
    offset = int(start * SR)
    mix[offset : offset + len(chirp), 0] += chirp * (0.5 - pan / 2)
    mix[offset : offset + len(chirp), 1] += chirp * (0.5 + pan / 2)
fade = np.clip(t / 0.8, 0, 1) * np.clip((DURATION - t) / 1.2, 0, 1)
mix *= fade[:, None]
peak = np.max(np.abs(mix))
mix *= 0.22 / max(peak, 1e-6)
output=Path(__file__).resolve().parent.parent/'public/audio/score.wav'
with wave.open(str(output),'wb') as f:
    f.setnchannels(2);f.setsampwidth(2);f.setframerate(SR);f.writeframes((mix*32767).astype('<i2').tobytes())
print(output)
