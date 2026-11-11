#!/usr/bin/env python3
"""Does a downmixed call separate into speakers?

This answers one question and then should be thrown away: when the far side
of a meeting arrives as a single mixed stream, do speaker embeddings cluster
by who was talking, or by nothing in particular? Everything else about
speaker names - storing a signature, matching it next week, letting a model
guess who "Priya" is - is cheap once that is known and worthless if the
answer is no.

It needs a meeting recorded with "Keep meeting audio" switched on in
Settings > Audio input, because the app deletes each stretch the moment it
has been transcribed and there is no way to run this on a meeting whose
audio has gone.

Run:

    uv run --with sherpa-onnx --with numpy \
        experiments/speaker-clusters.py \
        --meeting ~/Library/Application\\ Support/Open\\ Dictate/meetings/<id> \
        --model /path/to/embedding.onnx

Models live at
https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models
- 3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx is a reasonable first
try for English. Nothing is downloaded by this script.
"""

import argparse
import json
import pathlib
import re
import sys
import wave

import numpy as np


def read_wav(path):
    """16 kHz mono float32, which is what the app writes and what the model wants."""
    with wave.open(str(path), "rb") as w:
        if w.getsampwidth() != 2 or w.getnchannels() != 1:
            raise SystemExit(f"{path.name}: expected 16-bit mono, got {w.getsampwidth()*8}-bit {w.getnchannels()}ch")
        rate = w.getframerate()
        raw = w.readframes(w.getnframes())
    return rate, np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0


def labels_from_levels(meeting):
    """index -> (voice, seconds, text), read out of the app's own levels.log.

    The log is the only place the stretch index and the side it was assigned
    to appear together, and the index is what the audio files are named by.
    """
    out = {}
    path = meeting / "levels.log"
    if not path.is_file():
        return out
    index = None
    for line in path.read_text(errors="replace").splitlines():
        head = re.match(r"^(\d{3})\s+([\d.]+)s\s+(\S+)\s", line)
        if head:
            index = int(head.group(1))
            out[index] = {"voice": head.group(3), "secs": float(head.group(2)), "text": ""}
            continue
        said = re.match(r"^\s+(you|them|both):\s*(.*)$", line)
        if said and index is not None:
            out[index]["text"] = said.group(2)
    return out


def embed_all(model, wavs):
    import sherpa_onnx

    config = sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=model, num_threads=2, debug=False)
    if not config.validate():
        raise SystemExit(f"sherpa-onnx rejected the model at {model}")
    extractor = sherpa_onnx.SpeakerEmbeddingExtractor(config)

    vectors, kept = [], []
    for path in wavs:
        rate, samples = read_wav(path)
        stream = extractor.create_stream()
        stream.accept_waveform(sample_rate=rate, waveform=samples)
        stream.input_finished()
        if not extractor.is_ready(stream):
            print(f"  {path.name}: too short for the model, skipped")
            continue
        v = np.asarray(extractor.compute(stream), dtype=np.float32)
        n = np.linalg.norm(v)
        if n == 0:
            print(f"  {path.name}: empty embedding, skipped")
            continue
        vectors.append(v / n)
        kept.append(path)
    return np.vstack(vectors) if vectors else np.zeros((0, 0)), kept


def cluster(sim, threshold):
    """Average-linkage agglomerative clustering on a cosine similarity matrix.

    Written out rather than pulled from scikit-learn: it is twenty lines, and
    one fewer dependency on a machine that has to install them to run this.
    """
    groups = [[i] for i in range(sim.shape[0])]
    while len(groups) > 1:
        best, pair = -1.0, None
        for a in range(len(groups)):
            for b in range(a + 1, len(groups)):
                s = float(np.mean(sim[np.ix_(groups[a], groups[b])]))
                if s > best:
                    best, pair = s, (a, b)
        if best < threshold:
            break
        a, b = pair
        groups[a] = groups[a] + groups[b]
        groups.pop(b)
    labels = np.zeros(sim.shape[0], dtype=int)
    for n, g in enumerate(sorted(groups, key=len, reverse=True)):
        for i in g:
            labels[i] = n
    return labels


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--meeting", required=True, type=pathlib.Path)
    ap.add_argument("--model", required=True, type=pathlib.Path)
    ap.add_argument("--side", default="them", choices=["them", "you", "both", "all"],
                    help="which side to cluster; the far side is the one that needs it")
    ap.add_argument("--threshold", type=float, default=0.5, help="stop merging below this cosine similarity")
    args = ap.parse_args()

    audio = args.meeting / "audio"
    if not audio.is_dir() or not any(audio.glob("*.wav")):
        raise SystemExit(
            f"No stretch audio in {audio}.\n"
            "Record a meeting with Settings > Audio input > Keep meeting audio switched on."
        )

    labels = labels_from_levels(args.meeting)
    wavs = sorted(audio.glob("*.wav"))
    if args.side != "all":
        wavs = [w for w in wavs if labels.get(int(w.stem), {}).get("voice") == args.side]
    if not wavs:
        raise SystemExit(f"No stretches on the '{args.side}' side in this meeting.")

    print(f"{len(wavs)} stretches on the '{args.side}' side")
    vectors, kept = embed_all(str(args.model), wavs)
    if len(kept) < 2:
        raise SystemExit("Fewer than two usable stretches; nothing to compare.")

    sim = vectors @ vectors.T
    groups = cluster(sim, args.threshold)

    print(f"\n{groups.max() + 1} cluster(s) at threshold {args.threshold}\n")
    for path, g in zip(kept, groups):
        meta = labels.get(int(path.stem), {})
        print(f"  speaker {g}  {path.stem}  {meta.get('secs', 0):5.1f}s  {meta.get('text', '')[:58]}")

    same = np.array([sim[i, j] for i in range(len(kept)) for j in range(i + 1, len(kept)) if groups[i] == groups[j]])
    diff = np.array([sim[i, j] for i in range(len(kept)) for j in range(i + 1, len(kept)) if groups[i] != groups[j]])
    print("\nhow well it separated")
    print(f"  within a speaker : {same.mean():.3f}" if same.size else "  within a speaker : n/a")
    print(f"  between speakers : {diff.mean():.3f}" if diff.size else "  between speakers : n/a (one cluster)")
    if same.size and diff.size:
        print(f"  gap              : {same.mean() - diff.mean():.3f}   (bigger is better; under ~0.1 is not separation)")

    weak = [(kept[i].stem, float(np.max(np.delete(sim[i], i)))) for i in range(len(kept))]
    weak = [w for w in weak if w[1] < args.threshold]
    if weak:
        print(f"\n{len(weak)} stretch(es) matched nothing above the threshold - overlapping speech, or a speaker heard once:")
        for stem, s in weak:
            print(f"  {stem}  best match {s:.3f}")

    print(
        "\nNow compare the speaker numbers above against who actually spoke.\n"
        "If they line up, storing a signature per speaker is worth building.\n"
        "If they do not, nothing downstream of this will work either."
    )


if __name__ == "__main__":
    sys.exit(main())
