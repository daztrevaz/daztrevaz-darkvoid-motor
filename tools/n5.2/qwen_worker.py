"""Offline Qwen Base adapter. First-party controller; dependencies stay in the venv.

One JSON request on stdin; JSON progress/result on stdout. No downloads or API.
"""

import contextlib
import hashlib
import json
import os
from pathlib import Path
import random
import socket
import sys
import time
import wave

ROOT = Path(r"D:\DARKVOID\modelos-teste")
PROFILES = {
    "en-en2": ("English", Path(r"D:\DARKVOID\provavoz\prova2\referencias-en\EN-2.wav"),
               "62239f0bd21491f851b06b6ecad0721759c8602edb7c6dc8919b8c6beb5018b4"),
}


def emit(**data):
    print(json.dumps(data, ensure_ascii=True), flush=True)


def main():
    request = json.loads(sys.stdin.buffer.readline(65537).decode("utf-8"))
    text = request["text"].strip()
    if not text or len(text) > 8000:
        raise ValueError("invalid_text")
    language, reference, expected_pcm = PROFILES[request["profile"]]
    output = Path(request["output"])
    if output.drive.upper() != "D:" or output.exists():
        raise ValueError("invalid_output")
    for key, directory in {
        "HF_HOME": "hf", "TORCH_HOME": "torch", "HF_HUB_CACHE": "hf/hub",
        "HUGGINGFACE_HUB_CACHE": "hf/hub", "TRANSFORMERS_CACHE": "hf/transformers",
        "XDG_CACHE_HOME": "cache", "NUMBA_CACHE_DIR": "numba-cache",
        "CUDA_CACHE_PATH": "cuda-cache", "TORCH_EXTENSIONS_DIR": "torch-extensions",
        "TORCHINDUCTOR_CACHE_DIR": "inductor-cache", "TRITON_CACHE_DIR": "triton-cache",
        "MPLCONFIGDIR": "matplotlib", "TEMP": "tmp", "TMP": "tmp",
    }.items():
        os.environ[key] = str(ROOT / directory)
    for key in ("HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "HF_HUB_DISABLE_TELEMETRY", "DO_NOT_TRACK"):
        os.environ[key] = "1"
    os.environ["GRADIO_ANALYTICS_ENABLED"] = "False"
    os.environ["WANDB_DISABLED"] = "true"

    def offline(*_args, **_kwargs):
        raise OSError("Rede desativada no processo de narração.")

    socket.socket.connect = offline
    emit(fraction=0.05, stage="Carregando modelo local")
    started = time.perf_counter()
    with contextlib.redirect_stdout(sys.stderr):
        import numpy as np
        import torch
        from qwen_tts import Qwen3TTSModel
        torch.set_num_threads(6)
        device = "cuda:0" if torch.cuda.is_available() else "cpu"
        dtype = torch.bfloat16 if device.startswith("cuda") else torch.float32
        seed = int(request.get("seed", 42))
        random.seed(seed)
        np.random.seed(seed % (2**32))
        torch.manual_seed(seed)
        if device.startswith("cuda"):
            torch.cuda.manual_seed_all(seed)
            torch.cuda.reset_peak_memory_stats()
        model = Qwen3TTSModel.from_pretrained(
            str(ROOT / "models" / "qwen3-base"), device_map=device,
            dtype=dtype, attn_implementation="sdpa", local_files_only=True)
    load_s = time.perf_counter() - started
    emit(fraction=0.20, stage="Preparando referência fixa")
    with wave.open(str(reference), "rb") as wav:
        if (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) != (1, 2, 24000):
            raise ValueError("invalid_reference")
        pcm = wav.readframes(wav.getnframes())
    if hashlib.sha256(pcm).hexdigest() != expected_pcm:
        raise ValueError("reference_changed")
    reference_audio = np.frombuffer(pcm, dtype="<i2").astype(np.float32) / 32768
    started = time.perf_counter()
    with contextlib.redirect_stdout(sys.stderr):
        prompt = model.create_voice_clone_prompt(
            ref_audio=(reference_audio, 24000), x_vector_only_mode=True)
    emit(fraction=0.30, stage="Gerando narração")
    with contextlib.redirect_stdout(sys.stderr):
        outputs, rate = model.generate_voice_clone(
            text=text, language=language, voice_clone_prompt=prompt)
        if device.startswith("cuda"):
            torch.cuda.synchronize()
    generation_s = time.perf_counter() - started
    raw = np.asarray(outputs[0], dtype=np.float32)
    if raw.ndim != 1 or not raw.size or not np.isfinite(raw).all() or rate != 24000:
        raise ValueError("invalid_audio")
    emit(fraction=0.95, stage="Salvando áudio")
    output.parent.mkdir(parents=True, exist_ok=True)
    part = output.with_suffix(".wav.part")
    with wave.open(str(part), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes((np.clip(raw, -1, 1) * 32767).astype("<i2").tobytes())
    part.replace(output)
    duration = raw.size / rate
    result = {"path": str(output), "duration": duration, "profile": request["profile"],
              "provider": "qwen-base", "device": device, "loadSeconds": load_s,
              "generationSeconds": generation_s, "secondsPerMinute": generation_s * 60 / duration,
              "peakVramBytes": torch.cuda.max_memory_allocated() if device.startswith("cuda") else 0,
              "reservedVramBytes": torch.cuda.max_memory_reserved() if device.startswith("cuda") else 0,
              "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
              "clippedSamples": int(np.count_nonzero(np.abs(raw) > 1)),
              "seed": seed, "cloneMode": "x_vector_only"}
    emit(fraction=1.0, result=result)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        # Do not echo user text, filesystem details or library tracebacks to the editor.
        print(type(error).__name__, file=sys.stderr)
        emit(error={"code": "narration_failed", "message": "Não foi possível gerar a narração local. Confira o modelo e as referências."})
        raise SystemExit(1)
