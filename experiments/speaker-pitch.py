"""Does a speaker-recognition method track who is actually speaking?

Models agreeing with themselves proves nothing: an earlier version of this
experiment reported clean two-speaker alternation and was wrong. This scores
every method against something the models never see - pitch. When the two
voices differ in pitch (here a male and a female host, an octave apart), a
2-means split of per-window fundamental frequency is an independent answer.

    uv run --python 3.12 --with numpy --with sherpa-onnx \\
        experiments/speaker-pitch.py <meeting dir with kept audio> \\
        <pyannote segmentation model.onnx> <speaker embedding model.onnx>

(The segmentation model is only used to reproduce the diarization numbers
below; the app itself no longer uses it.)

What it found, on one 149 s recording of two podcast hosts, over 197 windows:

    always guessing the more frequent voice      70.6%
    the engine's own diarization                 54.9%   <- looked right, was not
    ours: average linkage                        93.9%
    ours: complete linkage                       92.4%
    ours: spherical k-means                      91.9%
    ours: single linkage                         70.1%   (chains; expected)

and clustering a subsample, then assigning the rest to the nearest centre:

    every 1st / 2nd / 3rd / 4th / 6th window     92.9 / 93.4 / 92.9 / 93.4 / 92.4%

What it does not show: one recording; two voices an octave apart, which is
the easy case; and pitch is a proxy, not a label. Two speakers of the same
sex and similar pitch cannot be refereed by pitch at all.
"""
import sys, wave, pathlib
import numpy as np
import sherpa_onnx

meeting, seg, emb = map(pathlib.Path, sys.argv[1:4])

def read(p):
    with wave.open(str(p)) as w:
        return np.frombuffer(w.readframes(w.getnframes()), "<i2").astype(np.float32) / 32768.0

wavs = sorted((meeting / "audio").glob("*.wav"))
x = np.concatenate([read(w) for w in wavs])
sr = 16000
print(f"{len(wavs)} stretches, {len(x)/sr:.0f}s")

cfg = sherpa_onnx.OfflineSpeakerDiarizationConfig(
    segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
        pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(model=str(seg)), num_threads=2),
    embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(emb), num_threads=2),
    clustering=sherpa_onnx.FastClusteringConfig(num_clusters=2),
    min_duration_on=0.3, min_duration_off=0.5)
turns = [(s.start, s.end, s.speaker) for s in sherpa_onnx.OfflineSpeakerDiarization(cfg).process(x).sort_by_start_time()]
turns = [t for t in turns if t[1] - t[0] >= 1.0]
print(len(turns), "turns of a second or more")

ext = sherpa_onnx.SpeakerEmbeddingExtractor(sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(emb), num_threads=2))

def f0(seg):
    """Median pitch over the clearly voiced 40 ms frames, by autocorrelation."""
    n = int(0.04 * sr); lo, hi = int(sr / 400), int(sr / 70)
    vals = []
    for i in range(0, len(seg) - n, n // 2):
        f = seg[i:i + n] - seg[i:i + n].mean()
        e = float(np.dot(f, f))
        if e < 1e-3 * n * 0.01:
            continue
        ac = np.correlate(f, f, "full")[n - 1:]
        k = lo + int(np.argmax(ac[lo:hi]))
        if ac[k] / (ac[0] + 1e-9) > 0.5:
            vals.append(sr / k)
    return float(np.median(vals)) if len(vals) >= 5 else float("nan")

rows = []
for a, b, who in turns:
    seg = x[int(a * sr):int(b * sr)]
    s = ext.create_stream(); s.accept_waveform(sr, seg); s.input_finished()
    if not ext.is_ready(s):
        continue
    v = np.array(ext.compute(s), dtype=np.float32); v /= np.linalg.norm(v)
    rows.append((a, b, who, f0(seg), v))

p = np.array([r[3] for r in rows])
ok = ~np.isnan(p)
print(f"{ok.sum()} of {len(rows)} turns have a clear pitch")
print("pitch percentiles (Hz):", np.percentile(p[ok], [5, 25, 50, 75, 95]).round(0))
hist, edges = np.histogram(p[ok], bins=np.arange(70, 330, 20))
print("pitch histogram:")
for h, e in zip(hist, edges):
    print(f"  {int(e):3d}-{int(e)+20:3d} Hz  {'#' * h}")

# two-means on log pitch: the independent split
lp = np.log(p[ok]); c = np.array([lp.min(), lp.max()])
for _ in range(50):
    lab = (np.abs(lp[:, None] - c[None]).argmin(1)); c = np.array([lp[lab == k].mean() if (lab == k).any() else c[k] for k in (0, 1)])
print(f"pitch groups: {np.exp(c[0]):.0f} Hz and {np.exp(c[1]):.0f} Hz  (gap {abs(np.exp(c[1])-np.exp(c[0])):.0f} Hz)")
truth = np.full(len(rows), -1); truth[ok] = lab
dur = np.array([r[1] - r[0] for r in rows])

def score(name, labels):
    labels = np.array(labels); m = ok
    best = 0.0
    for flip in (0, 1):
        l = labels[m] ^ flip if labels.max() <= 1 else labels[m]
        best = max(best, float(dur[m][l == truth[m]].sum() / dur[m].sum()))
    print(f"  {name:34s} agrees with the pitch split on {best*100:5.1f}% of the talking")

print("\nagainst the pitch split:")
score("sherpa (told 2)", [r[2] for r in rows])

V = np.stack([r[4] for r in rows]); S = V @ V.T

def agglo(S, k, link):
    n = len(S); cl = [[i] for i in range(n)]
    while len(cl) > k:
        best, pr = -9, None
        for i in range(len(cl)):
            for j in range(i + 1, len(cl)):
                blk = S[np.ix_(cl[i], cl[j])]
                v = blk.mean() if link == "average" else blk.min() if link == "complete" else blk.max()
                if v > best: best, pr = v, (i, j)
        i, j = pr; cl[i] += cl[j]; cl.pop(j)
    lab = np.zeros(n, int)
    for g, m in enumerate(cl):
        lab[m] = g
    return lab

for link in ("average", "complete", "single"):
    score(f"ours, {link} linkage", agglo(S, 2, link))

# spherical k-means, a different family altogether
rng = np.random.default_rng(0)
best_lab, best_obj = None, -9
for _ in range(20):
    c = V[rng.choice(len(V), 2, replace=False)]
    for _ in range(50):
        lab = (V @ c.T).argmax(1)
        c = np.stack([V[lab == k].mean(0) if (lab == k).any() else c[k] for k in (0, 1)]); c /= np.linalg.norm(c, axis=1, keepdims=True)
    obj = float((V * c[lab]).sum())
    if obj > best_obj: best_obj, best_lab = obj, lab
score("ours, spherical k-means", best_lab)

# ---------------------------------------------------------------------------
# Many more points: sliding windows, each with its own pitch. 23 turns is too
# few to pick an algorithm on; this is the same question with ~180 samples.
print("\n=== windows (1.5s every 0.75s) ===")
W, H = 1.5, 0.75
wins = []
for a in np.arange(0, len(x) / sr - W, H):
    seg = x[int(a * sr):int((a + W) * sr)]
    if float(np.sqrt(np.mean(seg ** 2))) < 0.005:
        continue
    pf = f0(seg)
    if np.isnan(pf):
        continue
    s = ext.create_stream(); s.accept_waveform(sr, seg); s.input_finished()
    if not ext.is_ready(s):
        continue
    v = np.array(ext.compute(s), dtype=np.float32); v /= np.linalg.norm(v)
    mid = a + W / 2
    who = next((t[2] for t in turns if t[0] <= mid < t[1]), -1)
    wins.append((pf, v, who))
pw = np.array([w[0] for w in wins]); VW = np.stack([w[1] for w in wins]); SW = VW @ VW.T
whos = np.array([w[2] for w in wins])
lp = np.log(pw); c = np.array([lp.min(), lp.max()])
for _ in range(50):
    lab = (np.abs(lp[:, None] - c[None]).argmin(1)); c = np.array([lp[lab == k].mean() for k in (0, 1)])
truthw = lab
print(f"{len(wins)} windows; pitch groups {np.exp(c[0]):.0f} Hz / {np.exp(c[1]):.0f} Hz; "
      f"split {int((truthw==0).sum())}/{int((truthw==1).sum())} windows")
print(f"majority-class baseline: {max((truthw==0).mean(), (truthw==1).mean())*100:.1f}%")

def acc(name, l):
    l = np.array(l); a = max((l == truthw).mean(), ((1 - l) == truthw).mean()) if l.max() <= 1 else float("nan")
    print(f"  {name:34s} {a*100:5.1f}% of windows")

m = whos >= 0
if m.sum():
    l = whos[m]; t = truthw[m]
    a = max((l == t).mean(), ((1 - l) == t).mean())
    print(f"  {'sherpa turns (windows inside one)':34s} {a*100:5.1f}% of {m.sum()} windows")
for link in ("average", "complete", "single"):
    acc(f"ours, {link} linkage", agglo(SW, 2, link))
best_lab, best_obj = None, -9
for _ in range(20):
    c2 = VW[rng.choice(len(VW), 2, replace=False)]
    for _ in range(50):
        lab = (VW @ c2.T).argmax(1)
        c2 = np.stack([VW[lab == k].mean(0) if (lab == k).any() else c2[k] for k in (0, 1)]); c2 /= np.linalg.norm(c2, axis=1, keepdims=True)
    obj = float((VW * c2[lab]).sum())
    if obj > best_obj: best_obj, best_lab = obj, lab
acc("ours, spherical k-means", best_lab)

# ---------------------------------------------------------------------------
# A long meeting has thousands of windows and average linkage is cubic, so the
# real design clusters a subsample and assigns the rest to the nearest centre.
# Does that cost accuracy?
print("\n=== subsample, cluster, assign the rest ===")
for every in (1, 2, 3, 4, 6):
    idx = np.arange(0, len(VW), every)
    sub = agglo(SW[np.ix_(idx, idx)], 2, "average")
    cen = np.stack([VW[idx][sub == k].mean(0) for k in (0, 1)]); cen /= np.linalg.norm(cen, axis=1, keepdims=True)
    full = (VW @ cen.T).argmax(1)
    a = max((full == truthw).mean(), ((1 - full) == truthw).mean())
    print(f"  cluster every {every}th window ({len(idx):3d} of {len(VW)}), assign all -> {a*100:5.1f}%")

# And at other counts, to see what a wrong guess at the number of people does.
print("\n=== what the count does (average linkage, all windows) ===")
for k in (1, 2, 3, 4):
    lab = agglo(SW, k, "average")
    sizes = np.bincount(lab)
    print(f"  k={k}: window shares {', '.join(f'{s/len(lab)*100:.0f}%' for s in sorted(sizes, reverse=True))}")
