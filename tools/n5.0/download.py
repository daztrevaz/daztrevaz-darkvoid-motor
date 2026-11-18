"""Download explicitly approved, pinned official checkpoints outside Git.

Run after environment.ps1. No script or private audio is read by this tool.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import time
import urllib.request


ROOT = Path(r"D:\DARKVOID\modelos-teste")
MODELS = {
    "qwen3": (
        "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign",
        "5ecdb67327fd37bb2e042aab12ff7391903235d3",
        None,
    ),
    "chatterbox": (
        "ResembleAI/chatterbox",
        "5bb1f6ee58e50c3b8d408bc82a6d3740c2db6e18",
        {"ve.pt", "t3_mtl23ls_v3.safetensors", "s3gen.pt", "conds.pt",
         "grapheme_mtl_merged_expanded_v1.json", "Cangjie5_TC.json", "README.md"},
    ),
}


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("candidate", choices=MODELS)
    parser.add_argument("--confirmed", action="store_true", required=True)
    args = parser.parse_args()
    for key in ("HF_HOME", "TORCH_HOME", "PIP_CACHE_DIR", "TEMP"):
        path = Path(os.environ[key]).resolve()
        if not path.is_relative_to(ROOT.resolve()):
            raise SystemExit(f"Cache outside approved directory: {key}")
    repo, revision, selected = MODELS[args.candidate]
    url = f"https://huggingface.co/api/models/{repo}/revision/{revision}?blobs=true"
    with urllib.request.urlopen(url, timeout=60) as response:
        metadata = json.load(response)
    assert metadata["sha"] == revision
    expected_license = "mit" if args.candidate == "chatterbox" else "apache-2.0"
    if metadata.get("cardData", {}).get("license") != expected_license:
        raise SystemExit("Licença diferente da conferida; download interrompido.")
    evidence = ROOT / "evidence"
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / f"{args.candidate}-upstream.json").write_text(
        json.dumps(metadata, indent=2), encoding="utf-8")
    files = metadata["siblings"]
    if selected is not None:
        assert selected <= {item["rfilename"] for item in files}
        files = [item for item in files if item["rfilename"] in selected]
    manifest = {"repo": repo, "revision": revision, "files": []}
    for item in files:
        name = item["rfilename"]
        target = ROOT / "models" / args.candidate / name
        assert target.resolve().is_relative_to(ROOT.resolve())
        target.parent.mkdir(parents=True, exist_ok=True)
        expected = item.get("lfs", {}).get("sha256")
        started = time.perf_counter()
        if not target.exists():
            part = target.with_name(target.name + ".part")
            source = f"https://huggingface.co/{repo}/resolve/{revision}/{name}"
            print(f"Downloading {args.candidate}/{name}: {item.get('size', '?')} bytes", flush=True)
            with urllib.request.urlopen(source, timeout=120) as response, part.open("wb") as output:
                if not response.url.startswith("https://"):
                    raise RuntimeError("HTTPS redirect required")
                for block in iter(lambda: response.read(8 * 1024 * 1024), b""):
                    output.write(block)
            if expected and digest(part) != expected:
                raise RuntimeError(f"SHA-256 mismatch: {name}")
            part.replace(target)
        actual = digest(target)
        if expected and actual != expected:
            raise RuntimeError(f"SHA-256 mismatch: {name}")
        manifest["files"].append({"name": name, "bytes": target.stat().st_size,
                                  "sha256": actual, "upstream_sha256": expected,
                                  "download_and_verify_s": time.perf_counter() - started})
        (evidence / f"{args.candidate}-manifest.json").write_text(
            json.dumps(manifest, indent=2), encoding="utf-8")
    print(json.dumps({"candidate": args.candidate,
                      "bytes": sum(item["bytes"] for item in manifest["files"]),
                      "verified": True}), flush=True)


if __name__ == "__main__":
    main()
