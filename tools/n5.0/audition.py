"""Short original voice auditions requested after the baseline listening review.

These previews use the first three private paragraphs. They are not replacement
benchmark runs and do not use a reference recording or voice cloning.
"""

import json
import os
from pathlib import Path
import random
import re
import wave


ROOT = Path(r"D:\DARKVOID\modelos-teste")
AUDIO = Path(r"D:\DARKVOID\provavoz")
VOICES = (
    ("qwen3-voz-01", "baixo documental",
     "A native Brazilian Portuguese male documentary narrator. A very low, "
     "full-bodied bass voice, deep chest resonance, mature and authoritative. "
     "Calm, deliberate delivery with natural pauses. Native Brazilian vowels "
     "and consonants, Brazilian pronunciation, never European Portuguese."),
    ("qwen3-voz-02", "baixo profundo",
     "A native Brazilian Portuguese male narrator with an exceptionally deep "
     "basso profondo voice. Dark, sonorous, rich low frequencies and powerful "
     "chest resonance. Slow, weighty, controlled documentary narration. "
     "Clear natural Brazilian pronunciation, never a Portugal accent. "
     "Natural speech, no singing, no exaggerated vocal fry."),
    ("qwen3-voz-03", "grave sombrio",
     "A native Brazilian Portuguese male cinematic documentary narrator. "
     "Very deep low bass register, dark resonant timbre with a subtle rough "
     "texture. Restrained menace, quiet authority, measured suspenseful pauses. "
     "Clear native Brazilian Portuguese articulation, not European Portuguese. "
     "Natural spoken narration, no whispering, no singing."),
)


def main():
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    for key in ("HF_HOME", "TORCH_HOME", "TEMP", "PYTHONPYCACHEPREFIX"):
        if not Path(os.environ[key]).resolve().is_relative_to(ROOT.resolve()):
            raise SystemExit(f"Cache outside approved directory: {key}")
    import numpy as np
    import torch
    from qwen_tts import Qwen3TTSModel

    if not torch.cuda.is_available():
        raise SystemExit("GPU CUDA indisponível; sem fallback.")
    torch.set_num_threads(6)
    script = (AUDIO / "roteiro-teste.txt").read_text(encoding="utf-8-sig").strip()
    paragraphs = re.split(r"\n\s*\n", script)[:3]
    model = Qwen3TTSModel.from_pretrained(
        str(ROOT / "models" / "qwen3"), device_map="cuda:0",
        dtype=torch.bfloat16, attn_implementation="sdpa", local_files_only=True)
    report = []
    for identifier, label, description in VOICES:
        # Same seed across descriptions: this is an audition, not a stability test.
        random.seed(42)
        np.random.seed(42)
        torch.manual_seed(42)
        torch.cuda.manual_seed_all(42)
        segments = []
        for paragraph in paragraphs:
            outputs, rate = model.generate_voice_design(
                text=paragraph, language="Portuguese", instruct=description)
            if segments:
                segments.append(np.zeros(int(0.25 * rate), dtype=np.float32))
            segments.append(np.asarray(outputs[0], dtype=np.float32).reshape(-1))
        samples = np.concatenate(segments)
        if not np.isfinite(samples).all() or not samples.size:
            raise RuntimeError("Áudio inválido; audição interrompida.")
        with wave.open(str(AUDIO / f"{identifier}.wav"), "wb") as output:
            output.setnchannels(1)
            output.setsampwidth(2)
            output.setframerate(rate)
            output.writeframes((np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes())
        row = {"id": identifier, "intended_style": label, "description": description,
               "paragraphs": len(paragraphs), "audio_s": len(samples) / rate,
               "seed": 42, "benchmark": False}
        report.append(row)
        (ROOT / "evidence" / "auditions.json").write_text(
            json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
        print(json.dumps({"id": identifier, "audio_s": row["audio_s"]}), flush=True)


if __name__ == "__main__":
    main()
