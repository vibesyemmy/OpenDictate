"""Scores the labels the Rust pipeline actually saved against pitch.

Same independent truth as before - a 2-means split of per-window pitch - but
applied to speakers.json as written by the app's own code, not to a Python
re-implementation of it.
"""
import json, sys, wave, pathlib
import numpy as np

meeting = pathlib.Path(sys.argv[1])
sp = json.loads((meeting / "speakers.json").read_text())
sr = 16000

def read(p):
    with wave.open(str(p)) as w:
        return np.frombuffer(w.readframes(w.getnframes()), "<i2").astype(np.float32) / 32768.0

files = sorted((meeting / "audio").glob("*.wav"))
starts = [int(f.stem) for f in files]
audio = {int(f.stem): read(f) for f in files}

def f0(seg):
    n = int(0.04 * sr); lo, hi = int(sr / 400), int(sr / 70); vals = []
    for i in range(0, len(seg) - n, n // 2):
        f = seg[i:i + n] - seg[i:i + n].mean()
        if float(np.dot(f, f)) < 1e-5 * n:
            continue
        ac = np.correlate(f, f, "full")[n - 1:]
        k = lo + int(np.argmax(ac[lo:hi]))
        if ac[k] / (ac[0] + 1e-9) > 0.5:
            vals.append(sr / k)
    return float(np.median(vals)) if len(vals) >= 5 else float("nan")

pitch, lab = [], []
for t, l in zip(sp["at_ms"], sp["labels"]):
    st = max(s for s in starts if s <= t)
    off = int((t - st) * sr / 1000)
    seg = audio[st][off:off + int(1.5 * sr)]
    pitch.append(f0(seg)); lab.append(l)
pitch, lab = np.array(pitch), np.array(lab)
ok = ~np.isnan(pitch)
lp = np.log(pitch[ok]); c = np.array([lp.min(), lp.max()])
for _ in range(50):
    a = np.abs(lp[:, None] - c[None]).argmin(1)
    c = np.array([lp[a == k].mean() for k in (0, 1)])
truth = a
mine = lab[ok]
base = max((truth == 0).mean(), (truth == 1).mean())
acc = max((mine == truth).mean(), ((1 - mine) == truth).mean())
print(f"{ok.sum()} of {len(pitch)} windows have a clear pitch; groups {np.exp(c[0]):.0f} Hz / {np.exp(c[1]):.0f} Hz")
print(f"majority-class baseline : {base*100:5.1f}%")
print(f"RUST PIPELINE vs pitch  : {acc*100:5.1f}%   (windows: {int(ok.sum())})")
