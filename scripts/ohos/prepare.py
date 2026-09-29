#!/usr/bin/env python3
"""Apply pinned OHOS core/plugins in a disposable build checkout only."""
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parents[2]
source = Path(os.environ['OHOS_TAURI_SOURCES']).resolve()
pins = json.loads((root / 'scripts/ohos/tauri-pins.json').read_text())
for name, pin in pins.items():
    revision = subprocess.check_output(['git', '-C', str(source / name), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != pin['revision']:
        raise ValueError(f'Unexpected image {name} revision: {revision}')

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
patches = helper.prepare_sources(external / 'core')

host = root / 'src-tauri/crates/tauritavern'
manifest = host / 'Cargo.toml'
text = manifest.read_text()
# Desktop-only dependencies still participate in Cargo resolution. Keep them out
# of this experimental checkout without changing any normal-platform manifest.
for name in ('tauri-plugin-single-instance', 'tauri-plugin-window-state', 'tauri-plugin-pilot'):
    text = re.sub(rf'^{name} = .*\n', '', text, flags=re.MULTILINE)
text = text.replace('devtools-pilot = ["dep:tauri-plugin-pilot"]', 'devtools-pilot = []')
text += '\n[target.\'cfg(target_env = "ohos")\'.dependencies]\nnapi-ohos = "=1.2.0"\nnapi-derive-ohos = "=1.2.0"\n'
manifest.write_text(text)
helper.patch_application(root / 'src-tauri', host, patches)

# Stable Tauri does not know this platform enum, so extend capabilities only here.
for name in ('mobile-barcode-scanner', 'system-file-picker'):
    path = host / 'capabilities' / f'{name}.json'
    data = json.loads(path.read_text())
    data['platforms'].append('openHarmony')
    path.write_text(json.dumps(data, indent=2)+'\n')
path = host / 'capabilities/default.json'
data = json.loads(path.read_text())
for index, permission in enumerate(data['permissions']):
    if isinstance(permission, dict) and permission.get('identifier') == 'fs:allow-write-file':
        # External writes use only picker-authorized URIs. Standard sandbox paths
        # retain the runtime data scope and app-cache/temp scopes installed by TT.
        data['permissions'][index] = 'fs:allow-write-file'
path.write_text(json.dumps(data, indent=2)+'\n')
config = host / 'tauri.conf.json'
data = json.loads(config.read_text())
data['bundle']['resources'] = {}
config.write_text(json.dumps(data, indent=2)+'\n')
(host / 'gen').mkdir(exist_ok=True)
(host / 'gen/ohos-plugins-source').write_text(str(plugins)+'\n')
