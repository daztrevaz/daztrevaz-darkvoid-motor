"""Offline A0 analysis only. No synthesis, downloads or private text in Git.

Encoder architecture and feature parameters follow the MIT Chatterbox encoder:
https://github.com/resemble-ai/chatterbox/tree/ab55923c27757c90cfcc407baa57dc25a7d25217/src/chatterbox/models/voice_encoder
This diagnostic uses existing weights, NumPy, SciPy and PyTorch directly; it
neither installs nor imports Chatterbox, PyKakasi, librosa or libsndfile.
Its short-segment comparisons are not a calibrated speaker verification test.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import time
import wave

ROOT = Path(r"D:\DARKVOID\modelos-teste")
AUDIO = Path(r"D:\DARKVOID\provavoz")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--save", action="store_true")
    args = parser.parse_args()
    # Set before third-party imports. No new network access or downloads.
    for key, folder in {
        "HF_HOME": "hf", "TORCH_HOME": "torch", "TEMP": "tmp", "TMP": "tmp",
        "PYTHONPYCACHEPREFIX": "pycache", "NUMBA_CACHE_DIR": "numba-cache",
        "CUDA_CACHE_PATH": "cuda-cache", "XDG_CACHE_HOME": "cache",
    }.items():
        os.environ[key] = str(ROOT / folder)
    for key in ("HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE",
                "HF_HUB_DISABLE_TELEMETRY", "DO_NOT_TRACK"):
        os.environ[key] = "1"
    import numpy as np
    from scipy.signal import resample_poly
    import torch
    from torch import nn

    started = time.perf_counter()
    torch.set_num_threads(6)
    source = AUDIO / "qwen3-voz-03.wav"
    with wave.open(str(source), "rb") as wav:
        params = wav.getparams()
        assert params.nchannels == 1 and params.sampwidth == 2
        assert params.framerate == 24000 and params.comptype == "NONE"
        pcm = wav.readframes(params.nframes)
    samples = np.frombuffer(pcm, dtype="<i2")
    zeros = np.flatnonzero(samples == 0)
    runs = np.split(zeros, np.flatnonzero(np.diff(zeros) > 1) + 1)
    gaps = [r for r in runs if r.size >= params.framerate * .2]
    if len(gaps) != 2:
        raise SystemExit("A0 interrompida: não foram encontradas duas pausas de 200 ms.")
    # Cut inside each exact-zero run; preserve every source sample, including silence.
    cuts = [0] + [(int(r[0]) + int(r[-1]) + 1) // 2 for r in gaps] + [len(samples)]

    class Encoder(nn.Module):
        def __init__(self):
            super().__init__()
            self.lstm = nn.LSTM(40, 256, num_layers=3, batch_first=True)
            self.proj = nn.Linear(256, 256)
            self.similarity_weight = nn.Parameter(torch.zeros(1))
            self.similarity_bias = nn.Parameter(torch.zeros(1))

        def forward(self, features):
            _, (hidden, _) = self.lstm(features)
            return torch.nn.functional.normalize(self.proj(hidden[-1]).relu(), dim=1)

    checkpoint = ROOT / "models" / "chatterbox" / "ve.pt"
    encoder = Encoder().eval()
    encoder.load_state_dict(torch.load(checkpoint, map_location="cpu", weights_only=True))

    # Slaney mel scale and area-normalized triangular filters, 40 bands, 0–8 kHz.
    def hz_to_mel(hz):
        hz = np.asarray(hz, dtype=float)
        return np.where(hz < 1000, hz / (200 / 3),
                        15 + np.log(np.maximum(hz, 1) / 1000) / (np.log(6.4) / 27))

    mel_points = np.linspace(0, hz_to_mel(8000), 42)
    frequencies = np.where(mel_points < 15, mel_points * (200 / 3),
                           1000 * np.exp((mel_points - 15) * (np.log(6.4) / 27)))
    fft_frequencies = np.fft.rfftfreq(400, 1 / 16000)
    left = (fft_frequencies[None] - frequencies[:-2, None]) / np.diff(frequencies)[:-1, None]
    right = (frequencies[2:, None] - fft_frequencies[None]) / np.diff(frequencies)[1:, None]
    basis = np.maximum(0, np.minimum(left, right))
    basis *= (2 / (frequencies[2:] - frequencies[:-2]))[:, None]
    basis = torch.tensor(basis, dtype=torch.float32)

    def embedding(raw):
        signal = resample_poly(raw.astype(np.float32) / 32768, 2, 3)
        # Feature extraction only: references on disk are never resampled or normalized.
        spectrum = torch.stft(torch.from_numpy(signal), n_fft=400, hop_length=160,
                              win_length=400, window=torch.hann_window(400),
                              center=True, pad_mode="reflect", return_complex=True)
        mel = (basis @ spectrum.abs().square()).T
        # 160-frame partials, 80-frame hop, last partial requires 80% coverage.
        count, remainder = divmod(max(len(mel) - 160 + 80, 0), 80)
        if count == 0 or (remainder + 80) / 160 >= .8:
            count += 1
        target = 160 + 80 * (count - 1)
        mel = torch.nn.functional.pad(mel, (0, 0, 0, max(0, target - len(mel))))
        windows = torch.stack([mel[i * 80:i * 80 + 160] for i in range(count)])
        with torch.inference_mode():
            vector = encoder(windows).mean(0)
            return torch.nn.functional.normalize(vector, dim=0).numpy()

    rows, embeddings, halves = [], [], []
    for label, begin, end in zip("ABC", cuts, cuts[1:]):
        chunk = samples[begin:end]
        embeddings.append(embedding(chunk))
        midpoint = len(chunk) // 2
        halves.append([embedding(chunk[:midpoint]), embedding(chunk[midpoint:])])
        rows.append({"voice": label, "begin_frame": begin, "end_frame": end,
                     "frames": end - begin, "duration_s": (end - begin) / params.framerate,
                     "pcm_sha256": hashlib.sha256(chunk.tobytes()).hexdigest()})
    vectors = np.stack(embeddings)
    half_vectors = np.stack([v for pair in halves for v in pair])
    report = {
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "source_bytes": source.stat().st_size, "sample_rate": params.framerate,
        "channels": 1, "sample_width_bytes": 2, "source_duration_s": len(samples) / params.framerate,
        "zero_runs_frames": [[int(r[0]), int(r[-1]) + 1] for r in gaps],
        "references": rows, "checkpoint_sha256": hashlib.sha256(checkpoint.read_bytes()).hexdigest(),
        "full_cosine": (vectors @ vectors.T).tolist(),
        "half_labels": [label + suffix for label in "ABC" for suffix in ("1", "2")],
        "half_cosine": (half_vectors @ half_vectors.T).tolist(),
        "method": "Existing MIT ve.pt; CPU float32; Slaney power mel; 160-frame windows, 50% overlap; mean L2 embeddings; cosine. Analysis-only scipy resample_poly, no trim. Not calibrated.",
        "new_download_bytes": 0, "analysis_s": time.perf_counter() - started,
        "versions": {"torch": torch.__version__, "numpy": np.__version__},
        "identity_confirmed": False,
        "identity_note": "Distances require interpretation; no validated same/different threshold for these short synthetic clips.",
    }
    if args.save:
        out = AUDIO / "prova2" / "referencias"
        out.mkdir(parents=True, exist_ok=True)
        for row in rows:
            path = out / ("voz-" + row["voice"] + ".wav")
            expected = samples[row["begin_frame"]:row["end_frame"]].tobytes()
            if path.exists():
                with wave.open(str(path), "rb") as existing:
                    assert existing.getparams()[:3] == params[:3]
                    assert existing.readframes(existing.getnframes()) == expected
            else:
                with wave.open(str(path), "wb") as result:
                    result.setparams(params)
                    result.writeframes(expected)
            with wave.open(str(path), "rb") as result:
                assert result.readframes(result.getnframes()) == expected
        evidence = AUDIO / "prova2" / "diagnostico"
        evidence.mkdir(exist_ok=True)
        (evidence / "a0.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
