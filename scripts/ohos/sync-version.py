#!/usr/bin/env python3
"""Synchronize the checked-in HAP version with the selected release version."""
import json
import os
from pathlib import Path
import json5

root = Path(__file__).resolve().parents[2]
version = os.environ.get("PACKAGE_VERSION") or json.loads((root / "package.json").read_text())["version"]
path = root / "src-tauri/crates/tauritavern/gen/ohos/AppScope/app.json5"
config = json5.loads(path.read_text())
major, minor, patch = map(int, version.split("-")[0].split("."))
config["app"]["versionName"] = version
config["app"]["versionCode"] = major * 1_000_000 + minor * 1_000 + patch
path.write_text(json.dumps(config, indent=2) + "\n")
