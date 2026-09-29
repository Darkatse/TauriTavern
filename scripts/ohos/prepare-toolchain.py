#!/usr/bin/env python3
"""Fetch and pin the experimental OHOS tools for an OpenHarmony build."""
import subprocess
import json
import os
from pathlib import Path
import re

root = Path.cwd()
source = Path(os.environ["OHOS_TAURI_SOURCES"]).resolve()
pins = json.loads((root / "scripts/ohos/tauri-pins.json").read_text())
marker = source / "tauritavern-pins.json"

for name, pin in pins.items():
    checkout = source / name
    subprocess.run(["git", "init", str(checkout)], check=True)
    subprocess.run(["git", "-C", str(checkout), "remote", "add", "origin",
                    f"https://github.com/{pin['repository']}.git"], check=True)
    subprocess.run(["git", "-C", str(checkout), "fetch", "--depth", "1", "origin", pin["revision"]], check=True)
    subprocess.run(["git", "-C", str(checkout), "checkout", "--detach", "FETCH_HEAD"], check=True)
# Match the Ability revision in Tauri's own upstream lockfile.
for checkout in (source / "tauri", source / "wry", source / "tao"):
    for manifest in checkout.rglob("Cargo.toml"):
        text = manifest.read_text().replace(
            'git = "https://github.com/harmony-contrib/openharmony-ability.git"',
            'git = "https://github.com/harmony-contrib/openharmony-ability.git", rev = "' + pins["ability"]["revision"] + '"')
        manifest.write_text(text)
upstream = source / "tauri/Cargo.toml"
text = upstream.read_text()
for name in ("wry", "tao", "cargo-mobile2"):
    text = re.sub(rf'^{name} = .*$', f'{name} = {{ path = "{source / name}" }}', text, flags=re.MULTILINE)
upstream.write_text(text)
# Huawei's SDK has native directly below openharmony, without an API subdir.
env_file = source / "cargo-mobile2/src/open_harmony/env.rs"
text = env_file.read_text()
pattern = r"self\.ohos_home(?:\s*\.parent\(\)\s*\.unwrap\(\)){3}\s*\.as_os_str\(\)\s*\.to_os_string\(\)"
text, count = re.subn(pattern, 'std::env::var_os("DEVECO_SDK_HOME").expect("DEVECO_SDK_HOME is required")', text, count=1)
if count != 1:
    raise ValueError("Upstream OpenHarmony SDK layout helper changed")
env_file.write_text(text)
marker.write_text(json.dumps(pins, indent=2) + "\n")

