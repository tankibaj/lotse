"""Synthesize the film's UI sound effects (original, no samples, no licences). Writes public/sfx/*.wav (44.1 kHz mono)."""
import math, random, struct, wave, os

SR = 44100
OUT = os.path.join(os.path.dirname(__file__), "..", "public", "sfx")
os.makedirs(OUT, exist_ok=True)
random.seed(7)

def write(name, samples, peak=0.8):
    m = max(1e-9, max(abs(x) for x in samples))
    g = peak / m
    with wave.open(os.path.join(OUT, name + ".wav"), "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, x * g)) * 32767)) for x in samples))

def sine_env(freq_fn, dur, decay, attack=0.002):
    n = int(SR * dur); ph = 0.0; out = []
    for i in range(n):
        t = i / SR
        ph += 2 * math.pi * freq_fn(t) / SR
        a = min(1, t / attack) * math.exp(-t / decay)
        out.append(math.sin(ph) * a)
    return out

def noise_burst(dur, decay, hp=0.9):
    n = int(SR * dur); prev = 0.0; out = []
    for i in range(n):
        t = i / SR
        x = random.uniform(-1, 1)
        y = hp * (x - prev); prev = x
        out.append(y * math.exp(-t / decay))
    return out

def mix(*parts):
    n = max(len(p[0]) + int(p[1] * SR) for p in parts)
    out = [0.0] * n
    for samples, delay, gain in parts:
        o = int(delay * SR)
        for i, x in enumerate(samples):
            out[o + i] += x * gain
    return out

# soft pop: quick pitch drop
write("pop", mix((sine_env(lambda t: 640 - 260 * min(1, t / 0.09), 0.22, 0.05), 0, 1.0), (noise_burst(0.01, 0.004), 0, 0.15)))
write("pop-hi", mix((sine_env(lambda t: 900 - 320 * min(1, t / 0.08), 0.2, 0.045), 0, 1.0), (noise_burst(0.008, 0.003), 0, 0.12)))
write("tick", mix((sine_env(lambda t: 2100, 0.05, 0.008), 0, 0.6), (noise_burst(0.012, 0.004), 0, 0.5)))
write("click", mix((noise_burst(0.02, 0.005), 0, 0.9), (sine_env(lambda t: 170 - 60 * min(1, t / 0.04), 0.09, 0.025), 0, 0.9)))
write("key", mix((noise_burst(0.018, 0.005, 0.8), 0, 0.8), (sine_env(lambda t: 220 - 80 * min(1, t / 0.03), 0.08, 0.02), 0, 0.8), (noise_burst(0.012, 0.004, 0.8), 0.07, 0.35)))
write("thud", sine_env(lambda t: 90 - 45 * min(1, t / 0.18), 0.45, 0.12, 0.004))
write("ping", mix((sine_env(lambda t: 1318, 0.7, 0.16, 0.004), 0, 0.8), (sine_env(lambda t: 1760, 0.6, 0.13, 0.004), 0.09, 0.6)))
write("chime", mix(*[(sine_env(lambda t, f=f: f, 0.9, 0.2, 0.004), d, 0.7) for f, d in [(1046.5, 0), (1318.5, 0.11), (1568.0, 0.22)]]))

def sweep(dur, lo, hi, up=True):
    n = int(SR * dur); out = []; lp = 0.0
    for i in range(n):
        t = i / SR; p = i / n
        env = math.sin(math.pi * p) ** 2
        f = (lo + (hi - lo) * (p if up else 1 - p))
        k = 1 - math.exp(-2 * math.pi * f / SR)
        lp += k * (random.uniform(-1, 1) - lp)
        out.append(lp * env)
    return out
write("whoosh", sweep(1.0, 300, 4200, True), 0.7)
write("swish", sweep(0.28, 600, 3500, True), 0.6)
write("whoosh-soft", sweep(0.7, 400, 2600, False), 0.6)
print("ok")
