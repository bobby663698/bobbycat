#!/usr/bin/env python3
"""Builds the BobbyCat shipping sound library: assets/audio/*.wav + manifest.json.

Every sound here is synthesised from scratch by this file - noise, sine, and
envelopes - so the library is original work released CC0-1.0 and carries no
third party's samples. The app reads assets/audio/manifest.json (see
docs/AUDIO_LIBRARY.md); this script is what writes both halves of that pair.

    python tools/make_audio.py
"""

import hashlib
import json
import math
import pathlib
import struct

import numpy as np

RATE = 44100
OUT = pathlib.Path(__file__).resolve().parent.parent / "assets" / "audio"
RNG = np.random.default_rng(20261119)


def t(seconds):
    return np.arange(int(RATE * seconds), dtype=np.float64) / RATE


def env(n, attack=0.01, decay=0.25, curve=2.0):
    """Attack then exponential-ish decay, both as fractions of the length."""
    a = max(1, int(n * attack))
    out = np.ones(n)
    out[:a] = np.linspace(0.0, 1.0, a) ** 0.6
    d = max(1, n - a)
    out[a:] = (np.linspace(1.0, 0.0, d) ** curve)
    return out * (1.0 - math.exp(-decay * 40))


def onepole_lp(x, cutoff):
    a = math.exp(-2.0 * math.pi * cutoff / RATE)
    y = np.empty_like(x)
    acc = 0.0
    for i, v in enumerate(x):
        acc = (1.0 - a) * v + a * acc
        y[i] = acc
    return y


def onepole_hp(x, cutoff):
    return x - onepole_lp(x, cutoff)


def noise(n):
    return RNG.standard_normal(n)


def sweep_noise(dur, f_start, f_end, width=0.55):
    """Band-passed noise whose centre slides from f_start to f_end."""
    n = int(RATE * dur)
    src = noise(n)
    freqs = np.linspace(f_start, f_end, n)
    # A cheap time-varying band: high-pass then low-pass, each retuned every
    # 256 samples, which is fine for a whoosh.
    out = np.empty(n)
    block = 256
    hp_state = 0.0
    lp_state = 0.0
    prev_in_hp = 0.0
    for start in range(0, n, block):
        f = freqs[start]
        a_lp = math.exp(-2.0 * math.pi * f * (1.0 + width) / RATE)
        a_hp = math.exp(-2.0 * math.pi * f * (1.0 - width) / RATE)
        for i in range(start, min(start + block, n)):
            v = src[i]
            hp = a_hp * (hp_state + v - prev_in_hp)
            prev_in_hp = v
            hp_state = hp
            lp_state = (1.0 - a_lp) * hp + a_lp * lp_state
            out[i] = lp_state
    peak = np.max(np.abs(out)) or 1.0
    return out / peak


def whoosh(dur, f_start, f_end, curve=2.4):
    n = int(RATE * dur)
    body = sweep_noise(dur, f_start, f_end)
    shape = np.linspace(0.0, 1.0, n)
    amp = np.sin(np.pi * shape) ** curve
    return body * amp


def impact(dur=0.55, f0=62.0, click=0.35):
    tt = t(dur)
    body = np.sin(2 * np.pi * f0 * tt * (1.0 - 0.25 * tt / dur))
    body *= np.exp(-tt * 9.0)
    transient = onepole_hp(noise(len(tt)), 1800.0) * np.exp(-tt * 60.0) * click
    out = body + transient
    return out / (np.max(np.abs(out)) or 1.0)


def tick(dur=0.3, f=2400.0):
    tt = t(dur)
    partial = (
        np.sin(2 * np.pi * f * tt)
        + 0.55 * np.sin(2 * np.pi * f * 2.71 * tt)
        + 0.3 * np.sin(2 * np.pi * f * 5.13 * tt)
    )
    out = partial * np.exp(-tt * 26.0)
    return out / (np.max(np.abs(out)) or 1.0)


def riser(dur=1.25, f0=180.0, f1=1500.0):
    tt = t(dur)
    phase = 2 * np.pi * np.cumsum(f0 * (f1 / f0) ** (tt / dur)) / RATE
    tone = np.sin(phase) * 0.6
    air = onepole_hp(noise(len(tt)), 900.0) * 0.35
    amp = (tt / dur) ** 2.2
    out = (tone + air) * amp
    return out / (np.max(np.abs(out)) or 1.0)


def sub_drop(dur=0.9, f0=140.0, f1=34.0):
    tt = t(dur)
    phase = 2 * np.pi * np.cumsum(f0 * (f1 / f0) ** (tt / dur)) / RATE
    out = np.sin(phase) * np.exp(-tt * 2.6)
    out += onepole_lp(noise(len(tt)), 400.0) * np.exp(-tt * 12.0) * 0.15
    return out / (np.max(np.abs(out)) or 1.0)


def click(dur=0.05):
    tt = t(dur)
    out = onepole_hp(noise(len(tt)), 3000.0) * np.exp(-tt * 220.0)
    out += np.sin(2 * np.pi * 1800.0 * tt) * np.exp(-tt * 300.0) * 0.5
    return out / (np.max(np.abs(out)) or 1.0)


def pop(dur=0.13, f0=520.0):
    tt = t(dur)
    phase = 2 * np.pi * np.cumsum(f0 * (f0 * 1.9 / f0) ** (tt / dur)) / RATE
    out = np.sin(phase) * np.exp(-tt * 34.0)
    return out / (np.max(np.abs(out)) or 1.0)


def room(dur=3.0):
    n = int(RATE * dur)
    brown = np.cumsum(noise(n))
    brown -= np.linspace(brown[0], brown[-1], n)
    brown = onepole_lp(brown, 700.0)
    brown /= np.max(np.abs(brown)) or 1.0
    fade = np.minimum(1.0, np.minimum(np.linspace(0, 1, n) * 12, (1 - np.linspace(0, 1, n)) * 12))
    return brown * fade * 0.6


def pulse(dur=4.0, bpm=100.0):
    n = int(RATE * dur)
    out = np.zeros(n)
    beat = 60.0 / bpm
    chords = [(220.0, 277.18, 329.63), (196.0, 246.94, 293.66)]
    for i in range(int(dur / (beat / 2))):
        start = int(i * beat / 2 * RATE)
        if start >= n:
            break
        chord = chords[(i // 8) % len(chords)]
        f = chord[i % len(chord)]
        length = int(RATE * 0.34)
        end = min(n, start + length)
        tt = np.arange(end - start) / RATE
        note = (np.sin(2 * np.pi * f * tt) + 0.4 * np.sin(2 * np.pi * f * 2 * tt))
        note *= np.exp(-tt * 7.0) * 0.5
        out[start:end] += note
        if i % 2 == 0:
            kick_t = np.arange(min(n - start, int(RATE * 0.16))) / RATE
            k = np.sin(2 * np.pi * 55.0 * kick_t * (1.0 - 0.6 * kick_t)) * np.exp(-kick_t * 18.0)
            out[start:start + len(kick_t)] += k * 0.7
    out /= np.max(np.abs(out)) or 1.0
    return out


SOUNDS = [
    # id, name, category, builder, seconds, source note
    ("whoosh-air-short", "Air whoosh, short", "Whoosh", lambda: whoosh(0.42, 320.0, 2600.0), "BobbyCat synthesiser: swept band-passed noise"),
    ("whoosh-deep-slow", "Deep whoosh", "Whoosh", lambda: whoosh(0.85, 140.0, 1400.0, 1.8), "BobbyCat synthesiser: swept band-passed noise"),
    ("impact-low-thud", "Low thud", "Impact", lambda: impact(0.55, 62.0, 0.35), "BobbyCat synthesiser: pitch-falling sine plus noise transient"),
    ("impact-metal-tick", "Metal tick", "Impact", lambda: tick(0.35), "BobbyCat synthesiser: inharmonic partials with fast decay"),
    ("riser-short-tension", "Short riser", "Riser", lambda: riser(1.25), "BobbyCat synthesiser: exponential frequency ramp"),
    ("sub-drop-down", "Sub drop", "Riser", lambda: sub_drop(0.9), "BobbyCat synthesiser: falling sub sine"),
    ("click-ui-soft", "Soft click", "Impact", lambda: click(0.05), "BobbyCat synthesiser: filtered noise burst"),
    ("pop-bubble", "Pop", "Impact", lambda: pop(0.13), "BobbyCat synthesiser: rising sine blip"),
    ("ambience-room-tone", "Room tone", "Ambience", lambda: room(3.0), "BobbyCat synthesiser: filtered brown noise"),
    ("music-pulse-loop", "Pulse loop", "Music", lambda: pulse(4.0), "BobbyCat synthesiser: chord pulse with kick"),
]


def write_wav(path, samples, rate=RATE):
    data = np.clip(samples, -1.0, 1.0)
    pcm = (data * 32767.0).astype("<i2").tobytes()
    header = struct.pack(
        "<4sI4s4sIHHIIHH4sI",
        b"RIFF", 36 + len(pcm), b"WAVE",
        b"fmt ", 16, 1, 1, rate, rate * 2, 2, 16,
        b"data", len(pcm),
    )
    path.write_bytes(header + pcm)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    sounds = []
    for sid, name, category, build, source in SOUNDS:
        samples = build()
        file = f"{sid}.wav"
        path = OUT / file
        write_wav(path, samples)
        raw = path.read_bytes()
        sounds.append({
            "id": sid,
            "name": name,
            "category": category,
            "file": file,
            "duration": round(len(samples) / RATE, 3),
            "bytes": len(raw),
            "license": "CC0-1.0",
            "source": source,
            "author": "BobbyCat",
            "sha256": hashlib.sha256(raw).hexdigest(),
        })
        print(f"{file:28} {len(raw)/1024:8.1f} KB  {len(samples)/RATE:5.2f}s")

    manifest = {
        "version": 1,
        "sounds": sounds,
    }
    (OUT / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    total = sum(s["bytes"] for s in sounds)
    print(f"\n{len(sounds)} sounds, {total/1024/1024:.2f} MB, manifest at {OUT/'manifest.json'}")


if __name__ == "__main__":
    main()
