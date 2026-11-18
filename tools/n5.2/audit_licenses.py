"""Inventory the installed Qwen dependency closure without importing its runtime."""

import importlib.metadata as metadata
import json
from pathlib import Path
import sys

from packaging.requirements import Requirement


def main():
    queue = [("qwen-tts", set())]
    visited = set()
    rows = {}
    errors = []
    while queue:
        name, extras = queue.pop(0)
        name = name.lower().replace("_", "-")
        key = (name, tuple(sorted(extras)))
        if key in visited:
            continue
        visited.add(key)
        distribution = metadata.distribution(name)
        info = distribution.metadata
        license_name = info.get("License-Expression") or "; ".join(
            value.rsplit(" :: ", 1)[-1] for value in info.get_all("Classifier", [])
            if value.startswith("License ::")) or (info.get("License") or "UNKNOWN").splitlines()[0]
        required = []
        for raw in distribution.requires or []:
            requirement = Requirement(raw)
            if requirement.marker and not any(
                requirement.marker.evaluate({"extra": extra}) for extra in extras | {""}
            ):
                continue
            required.append(str(requirement))
            installed = metadata.version(requirement.name)
            if requirement.specifier and installed not in requirement.specifier:
                errors.append(f"{name}: {requirement}; installed {installed}")
            queue.append((requirement.name, set(requirement.extras)))
        rows[name] = {"name": info["Name"], "version": distribution.version,
                      "license": license_name, "requires": required}
    forbidden = [row["name"] for row in rows.values()
                 if "GPL" in row["license"].upper().replace("LGPL", "")
                 and not any(exception in row["license"] for exception in
                             ("GCC-exception", "Classpath-exception"))]
    result = {"root": "qwen-tts", "python": sys.version.split()[0],
              "packages": sorted(rows.values(), key=lambda row: row["name"].lower()),
              "forbidden_package_licenses": forbidden, "version_errors": errors,
              "native_components": [
                  {"component": "libsndfile / soxr", "license": "LGPL-2.1-or-later"},
                  {"component": "NumPy/SciPy GCC runtime", "license": "GPL-3.0-or-later WITH GCC-exception-3.1"},
                  {"component": "PyTorch CUDA/cuDNN", "license": "NVIDIA redistributable runtime terms"}],
              "scope": "Installed Windows dependency closure, no optional development extras. Native notices reviewed separately; no SoX/FFmpeg executable required by the selected 12Hz clone path."}
    output = Path(sys.argv[1])
    output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"packages": len(rows), "forbidden": forbidden, "version_errors": errors}))
    if forbidden or errors:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
