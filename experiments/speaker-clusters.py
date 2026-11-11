#!/usr/bin/env python3
"""Does a downmixed call separate into speakers?

This answers one question and then should be thrown away: when the far side
of a meeting arrives as a single mixed stream, can it be told apart by voice?
Everything else about speaker names - storing a signature, matching it next
week, letting a model guess who "Priya" is - is cheap once that is known and
worthless if the answer is no.

It needs a meeting recorded with "Keep meeting audio" switched on in
Settings > Audio input, because the app deletes each stretch the moment it
has been transcribed.

    uv run --python 3.12 --with sherpa-onnx --with numpy \
        experiments/speaker-clusters.py \
        --meeting ~/Library/Application\\ Support/Open\\ Dictate/meetings/<id> \
        --segmentation .../sherpa-onnx-pyannote-segmentation-3-0/model.onnx \
        --embedding   .../3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx

Both models come from https://github.com/k2-fsa/sherpa-onnx/releases
(tags speaker-segmentation-models and speaker-recongition-models). Nothing
is downloaded by this script.

A first version of this took an embedding per fixed 1.5 s window and
clustered those by hand. It reported two speakers - and fifteen at a 2 s
window, and one at 3 s. Fixed windows straddle turns, so they measure the
grid rather than the conversation. This uses the segmentation model to find
where the turns actually are first, which is what that model is for.
"""

import argparse
import json
import pathlib
import re
import sys
import wave

import numpy as np


def read_wav(path):
    """16 kHz mono float32, which is what the app writes and the models want."""
    with wave.open(str(path), "rb") as w:
        if w.getsampwidth() != 2 or w.getnchannels() != 1:
            raise SystemExit(f"{path.name}: expected 16-bit mono, got {w.getsampwidth()*8}-bit {w.getnchannels()}ch")
        return w.getframerate(), np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float32) / 32768.0


def labels_from_levels(meeting):
    """index -> {voice, secs, text}, out of the app's own levels.log.

    The only place the stretch index and the side it was assigned to appear
    together, and the index is what the audio files are named by.
    """
    out, index = {}, None
    path = meeting / "levels.log"
    if not path.is_file():
        return out
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--meeting", required=True, type=pathlib.Path)
    ap.add_argument("--segmentation", required=True, type=pathlib.Path)
    ap.add_argument("--embedding", required=True, type=pathlib.Path)
    ap.add_argument("--side", default="them", choices=["them", "you", "both", "all"])
    ap.add_argument("--speakers", type=int, default=0, help="how many people, if you know; 0 lets it decide")
    ap.add_argument("--threshold", type=float, default=0.5, help="used only when --speakers is 0")
    args = ap.parse_args()

    import sherpa_onnx

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

    # One timeline, stretches in order, so the segmentation model sees a
    # conversation rather than a pile of clips. Where each stretch landed is
    # kept so its speakers can be read back against its transcript line.
    rate, pieces, spans = None, [], []
    for path in wavs:
        r, samples = read_wav(path)
        rate = rate or r
        if r != rate:
            raise SystemExit(f"{path.name}: {r} Hz among {rate} Hz")
        start = sum(len(p) for p in pieces) / rate
        pieces.append(samples)
        spans.append((path.stem, start, start + len(samples) / rate))
    joined = np.concatenate(pieces)
    print(f"{len(wavs)} stretches on the '{args.side}' side, {len(joined)/rate:.0f}s in all")

    config = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(model=str(args.segmentation)),
            num_threads=2,
        ),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(args.embedding), num_threads=2),
        clustering=sherpa_onnx.FastClusteringConfig(
            num_clusters=args.speakers if args.speakers > 0 else -1,
            threshold=args.threshold,
        ),
        min_duration_on=0.3,
        min_duration_off=0.5,
    )
    if not config.validate():
        raise SystemExit("sherpa-onnx rejected the configuration; check both model paths")

    sd = sherpa_onnx.OfflineSpeakerDiarization(config)
    if sd.sample_rate != rate:
        raise SystemExit(f"the models want {sd.sample_rate} Hz, the meeting is {rate} Hz")

    result = sd.process(joined).sort_by_start_time()
    segments = [(s.start, s.end, s.speaker) for s in result]
    if not segments:
        raise SystemExit("No speech segments came back at all.")

    speakers = sorted({s for _, _, s in segments})
    talk = {s: sum(e - b for b, e, who in segments if who == s) for s in speakers}
    total = sum(talk.values())
    print(f"\n{len(speakers)} speaker(s), {len(segments)} turns\n")
    for s in speakers:
        print(f"  speaker {s}: {talk[s]:6.1f}s  {talk[s]/total:4.0%} of the talking")

    print("\nturns, against what was transcribed\n")
    for stem, begin, end in spans:
        inside = [(max(b, begin), min(e, end), who) for b, e, who in segments if e > begin and b < end]
        who = "".join(f"{w}" for _, _, w in inside) or "-"
        said = labels.get(int(stem), {}).get("text", "")
        print(f"  {stem}  {begin:6.1f}-{end:6.1f}s  speakers {who}")
        print(f"         {said[:72]}")

    changes = sum(1 for i in range(1, len(segments)) if segments[i][2] != segments[i - 1][2])
    print(f"\n{changes} speaker changes across {len(joined)/rate:.0f}s "
          f"(about one every {len(joined)/rate/max(changes,1):.1f}s)")
    print(
        "\nRead the turns above against the recording. If the speaker numbers\n"
        "track who was actually talking, a stored signature per speaker is\n"
        "worth building. If they do not, nothing downstream of this works."
    )


if __name__ == "__main__":
    sys.exit(main())
