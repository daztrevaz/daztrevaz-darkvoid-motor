"""Score generated WAVs with the existing Rust Whisper example; keep text private.

Reports strict WER using the motor's normalization. Full recognitions and error
alignments are private artifacts, never public report inputs.
"""

import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess


ROOT = Path(r"D:\DARKVOID\modelos-teste")
AUDIO = Path(r"D:\DARKVOID\provavoz")


def normalize(word):
    while word and not word[0].isalnum():
        word = word[1:]
    while word and not word[-1].isalnum():
        word = word[:-1]
    return word.lower()


def tokens(text):
    return [w for part in re.split(r"[\s-]+", text) if (w := normalize(part))]


def alignment(reference, hypothesis):
    rows = [[j for j in range(len(hypothesis) + 1)]]
    for i, word in enumerate(reference, 1):
        row = [i]
        for j, other in enumerate(hypothesis, 1):
            row.append(min(rows[i - 1][j] + 1, row[-1] + 1,
                           rows[i - 1][j - 1] + (word != other)))
        rows.append(row)
    i, j = len(reference), len(hypothesis)
    edits = []
    while i or j:
        if i and j and rows[i][j] == rows[i - 1][j - 1] + (reference[i - 1] != hypothesis[j - 1]):
            if reference[i - 1] != hypothesis[j - 1]:
                edits.append({"type": "S", "reference_index": i - 1,
                              "reference": reference[i - 1], "hypothesis": hypothesis[j - 1]})
            i -= 1
            j -= 1
        elif i and rows[i][j] == rows[i - 1][j] + 1:
            edits.append({"type": "D", "reference_index": i - 1, "reference": reference[i - 1]})
            i -= 1
        else:
            edits.append({"type": "I", "reference_index": i, "hypothesis": hypothesis[j - 1]})
            j -= 1
    return rows[-1][-1], list(reversed(edits))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("executable", type=Path)
    parser.add_argument("candidate", choices=("qwen3", "chatterbox", "combinacao", "auditions"))
    args = parser.parse_args()
    executable = args.executable.resolve()
    private = AUDIO / "n5.0-conferencia"
    private.mkdir(exist_ok=True)
    script = (AUDIO / "roteiro-teste.txt").read_text(encoding="utf-8-sig").strip()
    if args.candidate == "auditions":
        script = "\n\n".join(re.split(r"\n\s*\n", script)[:3])
        sources = sorted(AUDIO.glob("qwen3-voz-*.wav"))
    else:
        sources = [AUDIO / f"{args.candidate}-run{trial}.wav" for trial in (1, 2, 3)]
    inputs = []
    for source in sources:
        if not source.exists():
            continue
        target = private / f"{source.stem}.f32"
        subprocess.run([shutil.which("ffmpeg"), "-v", "error", "-y", "-i", str(source),
                        "-ac", "1", "-ar", "16000", "-f", "f32le", str(target)], check=True)
        target.with_suffix(".txt").write_text(script, encoding="utf-8")
        inputs.append(target)
    if not inputs:
        raise SystemExit("Sem áudio concluído para conferir.")
    log = private / f"{args.candidate}-whisper.txt"
    with log.open("w", encoding="utf-8") as stream:
        subprocess.run([str(executable), str(ROOT / "models" / "whisper-small"),
                        "--lang=pt", *map(str, inputs)], stdout=stream,
                       stderr=subprocess.STDOUT, check=True)
    output = log.read_text(encoding="utf-8")
    lines = re.findall(r"([^\n]+\.f32) \[pt ([\d.]+)s\]: (.*?)  \(WER ([\d.]+)%\)", output)
    if len(lines) != len(inputs):
        raise RuntimeError("Saída do Whisper inesperada; conferir log privado.")
    ref = tokens(script)
    summaries = []
    for path, seconds, hypothesis, motor_wer in lines:
        error_count, edits = alignment(ref, tokens(hypothesis))
        wer = error_count / len(ref)
        if abs(wer * 100 - float(motor_wer)) > 0.051:
            raise RuntimeError("WER diverge do motor; conferir normalização.")
        summary = {"run": Path(path).stem, "reference_words": len(ref),
                   "hypothesis_words": len(tokens(hypothesis)), "wer": wer,
                   "substitutions": sum(e["type"] == "S" for e in edits),
                   "deletions": sum(e["type"] == "D" for e in edits),
                   "insertions": sum(e["type"] == "I" for e in edits),
                   "transcription_s": float(seconds)}
        summaries.append(summary)
        (private / f"{Path(path).stem}-alignment.json").write_text(
            json.dumps({"summary": summary, "edits": edits, "hypothesis": hypothesis},
                       indent=2, ensure_ascii=False), encoding="utf-8")
    (ROOT / "evidence" / f"{args.candidate}-wer.json").write_text(
        json.dumps(summaries, indent=2), encoding="utf-8")
    print(json.dumps(summaries, indent=2))


if __name__ == "__main__":
    main()
