"""Three offline VoiceDesign auditions. Text and audio stay outside Git.

Uses the local model and synthesis method from tools/n5.0/audition.py.
Each candidate is one call, with no paragraph splitting or voice reference.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import time
import wave


ROOT = Path(r"D:\DARKVOID\modelos-teste")
OUTPUT = Path(r"D:\DARKVOID\provavoz\prova2\referencias-en")
DESCRIPTIONS = (
    ("EN-1", "A native General American English adult male documentary narrator. "
     "A very deep, full-bodied bass voice with dark chest resonance and a sober, "
     "mysterious tone. Calm, measured pacing for a mystery documentary. Clear "
     "diction, natural American vowels and consonants, no foreign accent. "
     "Natural spoken narration, no whispering, no singing."),
    ("EN-2", "A native General American English adult male documentary narrator. "
     "A deep, full-bodied bass voice with warm chest resonance and a close, "
     "approachable tone. Calm, measured pacing for a mystery documentary. "
     "Clear diction, natural American vowels and consonants, no foreign accent. "
     "Natural spoken narration, no whispering, no singing."),
    ("EN-3", "A native neutral British English adult male documentary narrator. "
     "A deep, full-bodied bass voice with restrained chest resonance and a "
     "sober, composed tone. Calm, measured pacing for a mystery documentary. "
     "Clear diction, natural standard British vowels and consonants, no foreign "
     "accent and no exaggerated theatrical accent. Natural spoken narration, "
     "no whispering, no singing."),
)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--text-file", type=Path, required=True)
    args = parser.parse_args()
    if not args.text_file.resolve().is_relative_to(OUTPUT.resolve()):
        raise SystemExit("Texto deve permanecer na pasta externa de audições EN.")
    text = args.text_file.read_text(encoding="utf-8-sig").strip()
    if not text or "\n" in text:
        raise SystemExit("Forneça um único parágrafo, comum às três candidatas.")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    if any((OUTPUT / f"{name}.wav").exists() for name, _ in DESCRIPTIONS):
        raise SystemExit("Audição existente; recusa sobrescrever ou regenerar vozes.")
    for key, directory in {
        "HF_HOME": "hf", "TORCH_HOME": "torch", "HF_HUB_CACHE": "hf/hub",
        "HF_XET_CACHE": "hf/xet", "HUGGINGFACE_HUB_CACHE": "hf/hub",
        "TRANSFORMERS_CACHE": "hf/transformers", "XDG_CACHE_HOME": "cache",
        "PIP_CACHE_DIR": "pip-cache", "UV_CACHE_DIR": "uv-cache",
        "NUMBA_CACHE_DIR": "numba-cache", "CUDA_CACHE_PATH": "cuda-cache",
        "TORCH_EXTENSIONS_DIR": "torch-extensions",
        "TORCHINDUCTOR_CACHE_DIR": "inductor-cache", "TRITON_CACHE_DIR": "triton-cache",
        "MPLCONFIGDIR": "matplotlib", "TEMP": "tmp", "TMP": "tmp",
        "PYTHONPYCACHEPREFIX": "pycache",
    }.items():
        os.environ[key] = str(ROOT / directory)
    for key in ("HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "HF_HUB_DISABLE_TELEMETRY",
                "HF_HUB_DISABLE_XET", "DO_NOT_TRACK"):
        os.environ[key] = "1"
    os.environ["GRADIO_ANALYTICS_ENABLED"] = "False"
    os.environ["WANDB_DISABLED"] = "true"
    import numpy as np
    import torch
    from qwen_tts import Qwen3TTSModel

    if not torch.cuda.is_available():
        raise SystemExit("GPU CUDA indisponível; sem fallback.")
    torch.set_num_threads(6)
    started = time.perf_counter()
    model = Qwen3TTSModel.from_pretrained(
        str(ROOT / "models" / "qwen3"), device_map="cuda:0",
        dtype=torch.bfloat16, attn_implementation="sdpa", local_files_only=True)
    torch.cuda.synchronize()
    report = {
        "text": text, "text_sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
        "language": "English", "seed": 42, "dtype": "bfloat16", "attention": "sdpa",
        "model": "Qwen3-TTS-12Hz-1.7B-VoiceDesign", "model_load_s": time.perf_counter() - started,
        "generate_defaults": model.generate_defaults, "new_download_bytes": 0,
        "normalization": False, "effects": False, "reference_audio_used": False,
        "torch": torch.__version__, "gpu": torch.cuda.get_device_name(), "candidates": [],
        "listening": "Pending DTRZ; requested accent and timbre are not verified claims.",
    }
    # The plan survives a synthesis failure; no private text is printed to a log.
    manifest = OUTPUT / "auditions-en.json"
    report["requested_descriptions"] = dict(DESCRIPTIONS)
    manifest.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    for name, description in DESCRIPTIONS:
        random.seed(42)
        np.random.seed(42)
        torch.manual_seed(42)
        torch.cuda.manual_seed_all(42)
        torch.cuda.reset_peak_memory_stats()
        started = time.perf_counter()
        outputs, rate = model.generate_voice_design(
            text=text, language="English", instruct=description)
        torch.cuda.synchronize()
        elapsed = time.perf_counter() - started
        raw = np.asarray(outputs[0], dtype=np.float32)
        if raw.ndim != 1 or not raw.size or not np.isfinite(raw).all() or rate != 24000:
            raise RuntimeError("Formato de áudio inesperado; audições interrompidas.")
        # Fixed PCM16 quantization, matching N5.0. No gain adjustment or effects.
        pcm = (np.clip(raw, -1, 1) * 32767).astype("<i2").tobytes()
        path = OUTPUT / f"{name}.wav"
        with wave.open(str(path), "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(rate)
            wav.writeframes(pcm)
        row = {
            "id": name, "description": description, "duration_s": raw.size / rate,
            "frames": int(raw.size), "sample_rate": rate, "channels": 1,
            "sample_width_bytes": 2, "generation_s": elapsed,
            "peak_allocated_bytes": torch.cuda.max_memory_allocated(),
            "peak_reserved_bytes": torch.cuda.max_memory_reserved(),
            "out_of_range_samples": int(np.count_nonzero(np.abs(raw) > 1)),
            "peak_amplitude_float": float(np.abs(raw).max()),
            "file_bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "pcm_sha256": hashlib.sha256(pcm).hexdigest(),
        }
        report["candidates"].append(row)
        manifest.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
        print(json.dumps(row, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
