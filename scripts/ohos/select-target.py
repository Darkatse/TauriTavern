#!/usr/bin/env python3
"""Persist the selected ABI for Hvigor, whose environment is filtered by Tauri."""
import json
import os
from pathlib import Path
import shutil

root = Path(__file__).resolve().parents[2]
abi = {
    "aarch64-unknown-linux-ohos": "arm64-v8a",
    "x86_64-unknown-linux-ohos": "x86_64",
}[os.environ["TARGET_TRIPLE"]]
entry = root / "src-tauri/crates/tauritavern/gen/ohos/entry"
profile = entry / "build-profile.json5"
config = json.loads(profile.read_text())
config["buildOption"]["externalNativeOptions"]["abiFilters"] = [abi]
profile.write_text(json.dumps(config, indent=2) + "\n")
# Staged libraries are generated outputs; never carry another ABI into this HAP.
libraries = entry / "libs"
if libraries.exists():
    shutil.rmtree(libraries)
print(f"Selected HAP ABI: {abi}")
