#!/usr/bin/env python3
"""Apply pinned OHOS core/plugins in a disposable build checkout only."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
pins = json.loads((root / 'scripts/ohos/tauri-pins.json').read_text())
pin = json.loads((root / 'scripts/ohos/plugins-pin.json').read_text())
external = Path(os.environ.get('RUNNER_TEMP', '/tmp')) / 'tauritavern-ohos-sources'
external.mkdir(parents=True, exist_ok=True)
plugins = external / 'plugins'
subprocess.run(['git', 'init', str(plugins)], check=True)
subprocess.run(['git', '-C', str(plugins), 'fetch', '--depth', '1', f"https://github.com/{pin['repository']}.git", pin['revision']], check=True)
subprocess.run(['git', '-C', str(plugins), 'checkout', '--detach', 'FETCH_HEAD'], check=True)
spec = importlib.util.spec_from_file_location('ohos_plugin_sources', plugins / 'shared/ohos/prepare.py')
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)
patches = helper.prepare_sources(external / 'runtime', pins)
host = root / 'src-tauri/crates/tauritavern'
manifest = host / 'Cargo.toml'
text = manifest.read_text()
# The fork's mobile_entry_point and Ability macros expand these crate paths in
# the application. Keep their versions aligned with the pinned Tauri fork.
text += '\n[target.\'cfg(target_env = "ohos")\'.dependencies]\nnapi-ohos = "=1.2.0"\nnapi-derive-ohos = "=1.2.0"\n'
manifest.write_text(text)
helper.patch_application(root / 'src-tauri', host, patches)

# Stable Tauri does not know this platform enum, so extend capabilities only here.
for name in ('mobile-barcode-scanner', 'system-file-picker'):
    path = host / 'capabilities' / f'{name}.json'
    data = json.loads(path.read_text())
    data['platforms'].append('openHarmony')
    path.write_text(json.dumps(data, indent=2)+'\n')
subprocess.run(['python3', str(plugins / 'shared/ohos/install.py'), str(host), '--sources-only'], check=True)
