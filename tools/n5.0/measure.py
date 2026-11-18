"""One fresh GPU process per trial; private inputs and outputs remain outside Git."""

import argparse
import importlib.metadata
import json
import os
from pathlib import Path
import random
import re
import subprocess
import sys
import threading
import time
import wave


ROOT = Path(r"D:\DARKVOID\modelos-teste")
AUDIO = Path(r"D:\DARKVOID\provavoz")
DESCRIPTION = "masculina, grave, imponente, pausada, narrador de documentário"


def worker(candidate, trial):
    # Offline mode prevents implicit downloads by model loaders or tokenizers.
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    for key, folder in (("CUDA_CACHE_PATH", "cuda-cache"),
                        ("TORCH_EXTENSIONS_DIR", "torch-extensions"),
                        ("TORCHINDUCTOR_CACHE_DIR", "inductor-cache"),
                        ("TRITON_CACHE_DIR", "triton-cache")):
        os.environ[key] = str(ROOT / folder)
    for key in ("HF_HOME", "TORCH_HOME", "TEMP", "PYTHONPYCACHEPREFIX"):
        if not Path(os.environ[key]).resolve().is_relative_to(ROOT.resolve()):
            raise SystemExit(f"Cache outside approved directory: {key}")
    import numpy as np
    import psutil
    import torch

    if not torch.cuda.is_available():
        raise RuntimeError("GPU CUDA indisponível; teste interrompido sem fallback.")
    torch.set_num_threads(6)
    seed = 41 + trial
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    torch.cuda.manual_seed_all(seed)
    process = psutil.Process()
    peaks = {"rss_bytes": 0, "private_bytes": 0}
    stop = threading.Event()

    def sample():
        while not stop.is_set():
            info = process.memory_info()
            peaks["rss_bytes"] = max(peaks["rss_bytes"], info.rss,
                                     getattr(info, "peak_wset", 0))
            peaks["private_bytes"] = max(peaks["private_bytes"],
                                         getattr(info, "private", 0))
            stop.wait(0.05)

    monitor = threading.Thread(target=sample, daemon=True)
    monitor.start()
    text = (AUDIO / "roteiro-teste.txt").read_text(encoding="utf-8-sig").strip()
    paragraphs = [p.strip() for p in re.split(r"\n\s*\n", text) if p.strip()]
    metrics = {"candidate": candidate, "trial": trial, "seed": seed,
               "paragraphs": len(paragraphs), "words": len(text.split()),
               "gpu": torch.cuda.get_device_name(), "torch": torch.__version__,
               "versions": {name: importlib.metadata.version(name)
                            for name in ("transformers", "numpy", "psutil")}}
    torch.cuda.reset_peak_memory_stats()
    try:
        started = time.perf_counter()
        if candidate == "qwen3":
            from qwen_tts import Qwen3TTSModel
            model = Qwen3TTSModel.from_pretrained(
                str(ROOT / "models" / "qwen3"), device_map="cuda:0",
                dtype=torch.bfloat16, attn_implementation="sdpa",
                local_files_only=True)
            sample_rate = 24000
        else:
            from chatterbox.mtl_tts import ChatterboxMultilingualTTS
            model = ChatterboxMultilingualTTS.from_local(
                ROOT / "models" / "chatterbox", device="cuda",
                t3_model="v3")
            sample_rate = model.sr
        torch.cuda.synchronize()
        metrics["load_s"] = time.perf_counter() - started
        metrics["load_peak_allocated_bytes"] = torch.cuda.max_memory_allocated()
        metrics["load_peak_reserved_bytes"] = torch.cuda.max_memory_reserved()
        torch.cuda.reset_peak_memory_stats()
        started = time.perf_counter()
        if candidate == "combinacao":
            # Only the synthetic, description-created Qwen output is eligible.
            reference = AUDIO / "qwen3.wav"
            assert reference.is_file()
            model.prepare_conditionals(str(reference))
        segments = []
        segment_seconds = []
        for index, paragraph in enumerate(paragraphs):
            if candidate == "qwen3":
                outputs, rate = model.generate_voice_design(
                    text=paragraph, language="Portuguese", instruct=DESCRIPTION)
                assert rate == sample_rate
                segment = np.asarray(outputs[0], dtype=np.float32).reshape(-1)
            else:
                segment = model.generate(paragraph, language_id="pt").squeeze().cpu().numpy()
            if not np.isfinite(segment).all() or segment.size == 0:
                raise RuntimeError("Áudio vazio ou não finito; teste interrompido.")
            segments.append(segment)
            segment_seconds.append(len(segment) / sample_rate)
            print(json.dumps({"paragraph_done": index + 1, "trial": trial}), flush=True)
        torch.cuda.synchronize()
        metrics["generation_s"] = time.perf_counter() - started
        metrics["generation_peak_allocated_bytes"] = torch.cuda.max_memory_allocated()
        metrics["generation_peak_reserved_bytes"] = torch.cuda.max_memory_reserved()
        gap = np.zeros(int(0.25 * sample_rate), dtype=np.float32)
        joined = []
        for index, segment in enumerate(segments):
            if index:
                joined.append(gap)
            joined.append(segment)
        samples = np.concatenate(joined)
        output = AUDIO / f"{candidate}-run{trial}.wav"
        with wave.open(str(output), "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(sample_rate)
            wav.writeframes((np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes())
        if trial == 1:
            (AUDIO / f"{candidate}.wav").write_bytes(output.read_bytes())
        metrics.update({"status": "ok", "sample_rate": sample_rate,
                        "audio_s": len(samples) / sample_rate,
                        "synthesized_s": sum(segment_seconds),
                        "inserted_silence_s": 0.25 * (len(segments) - 1),
                        "segment_seconds": segment_seconds,
                        "rtf_final": metrics["generation_s"] / (len(samples) / sample_rate),
                        "rtf": metrics["generation_s"] / sum(segment_seconds),
                        "seconds_per_minute": metrics["generation_s"] * 60 / sum(segment_seconds),
                        "peak_amplitude": float(np.abs(samples).max()),
                        "clipped_fraction": float(np.mean(np.abs(samples) >= 1)),
                        "watermark": "PerTh upstream enabled" if candidate != "qwen3" else "not declared"})
    except Exception as error:
        metrics.update({"status": "blocked", "error_type": type(error).__name__,
                        "error": str(error),
                        "failure_peak_allocated_bytes": torch.cuda.max_memory_allocated(),
                        "failure_peak_reserved_bytes": torch.cuda.max_memory_reserved()})
        raise
    finally:
        stop.set()
        monitor.join()
        metrics.update(peaks)
        (ROOT / "evidence" / f"{candidate}-run{trial}.json").write_text(
            json.dumps(metrics, indent=2, ensure_ascii=False), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("candidate", choices=("qwen3", "chatterbox", "combinacao"))
    parser.add_argument("--trial", type=int, choices=(1, 2, 3))
    args = parser.parse_args()
    if args.candidate in ("chatterbox", "combinacao"):
        raise SystemExit("Prova suspensa: dependência pykakasi GPL-3.0-or-later. Consulte docs/n5.0-prova.md.")
    if args.trial:
        worker(args.candidate, args.trial)
        return
    for trial in (1, 2, 3):
        log = ROOT / "evidence" / f"{args.candidate}-run{trial}.log"
        with log.open("w", encoding="utf-8") as stream:
            result = subprocess.run([sys.executable, __file__, args.candidate,
                                     "--trial", str(trial)], stdout=stream,
                                    stderr=subprocess.STDOUT)
        print(json.dumps({"candidate": args.candidate, "trial": trial,
                          "exit_code": result.returncode}), flush=True)
        if result.returncode:
            raise SystemExit("Candidato bloqueado; sem retentativa ou fallback.")


if __name__ == "__main__":
    main()
