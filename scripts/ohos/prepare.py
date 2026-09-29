#!/usr/bin/env python3
"""Apply experimental Tauri dependencies to a disposable OHOS checkout only."""
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

root = Path(__file__).resolve().parents[2]
source = Path(os.environ["OHOS_TAURI_SOURCES"]).resolve()
pins = json.loads((root / "scripts/ohos/tauri-pins.json").read_text())
for name, pin in pins.items():
    revision = subprocess.check_output(["git", "-C", str(source / name), "rev-parse", "HEAD"], text=True).strip()
    if revision != pin["revision"]:
        raise ValueError(f"Unexpected {name} revision: {revision}")
patches = {}
for manifest in (source / "tauri/crates").glob("*/Cargo.toml"):
    name = tomllib.loads(manifest.read_text()).get("package", {}).get("name", "")
    if name.startswith("tauri"):
        patches[name] = manifest.parent
for name in ("wry", "tao", "cargo-mobile2"):
    patches[name] = source / name
host = root / "src-tauri/crates/tauritavern"
manifest = host / "Cargo.toml"
text = manifest.read_text()
# Cargo resolves dependencies of other targets too. Exclude unavailable plugins
# from this disposable manifest without changing regular platform dependencies.
text = re.sub(r'^tauri-plugin-.*\n', '', text, flags=re.MULTILINE)
text = text.replace('devtools-pilot = ["dep:tauri-plugin-pilot"]', 'devtools-pilot = []')
for name in ("tauri", "tauri-build"):
    text = re.sub(rf'^{name} = \{{ version = "[^"]+"', f'{name} = {{ path = "{patches[name]}"', text, flags=re.MULTILINE)
manifest.write_text(text)
manifest = root / "src-tauri/Cargo.toml"
with manifest.open("a") as output:
    output.write('\n[patch.crates-io]\n')
    for name, path in patches.items():
        output.write(f'{name} = {{ path = "{path}" }}\n')
# Plugin permissions cannot be resolved without their plugin build scripts.
for capability in (host / "capabilities").glob("*.json"):
    capability.unlink()
(host / "capabilities/ohos.json").write_text(json.dumps({
    "identifier": "ohos", "windows": ["main"],
    "permissions": ["core:default", "core:window:allow-destroy"],
}, indent=2) + "\n")
config = host / "tauri.conf.json"
data = json.loads(config.read_text())
data["bundle"]["resources"] = {}
config.write_text(json.dumps(data, indent=2) + "\n")
