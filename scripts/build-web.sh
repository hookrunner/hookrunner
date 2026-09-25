#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
bindgen="${WASM_BINDGEN:-wasm-bindgen}"
expected=$(python3 - <<'PY'
import tomllib
with open('Cargo.lock', 'rb') as file:
    print(next(p['version'] for p in tomllib.load(file)['package'] if p['name'] == 'wasm-bindgen'))
PY
)
if ! command -v "$bindgen" >/dev/null || [[ "$($bindgen --version)" != "wasm-bindgen $expected" ]]; then
  echo "Install the matching WASM tool: cargo install wasm-bindgen-cli --version $expected --locked" >&2
  exit 1
fi
cargo build --locked --profile web --target wasm32-unknown-unknown -p hookrunner-game
package_dir=$(mktemp -d "${TMPDIR:-/tmp}/hookrunner-web.XXXXXX")
trap 'rm -rf -- "$package_dir"' EXIT
"$bindgen" --target web --out-dir "$package_dir" --out-name hookrunner_web "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/web/hookrunner.wasm"
python3 - "$package_dir" <<'PY'
import hashlib
import json
import pathlib
import shutil
import sys

package = pathlib.Path(sys.argv[1])
files = sorted(package.iterdir())
digest = hashlib.sha256()
template = pathlib.Path('web/index.html').read_text()
build_sync = pathlib.Path('web/build-sync.js').read_text()
loading = pathlib.Path('web/loading.js').read_text()
digest.update(template.encode())
digest.update(build_sync.encode())
digest.update(loading.encode())
for path in files:
    digest.update(path.name.encode())
    with path.open('rb') as source:
        digest.update(hashlib.file_digest(source, 'sha256').digest())
asset_directories = {
    'stormkeep/built/textures': pathlib.Path('assets/stormkeep/built/textures'),
    'shaders': pathlib.Path('assets/shaders'),
    'fonts': pathlib.Path('assets/fonts'),
    'weapons/starter_pistol/built': pathlib.Path('assets/weapons/starter_pistol/built'),
}
for name, directory in asset_directories.items():
    for path in sorted(directory.iterdir()):
        digest.update(f'{name}/{path.name}'.encode())
        digest.update(hashlib.sha256(path.read_bytes()).digest())
version = digest.hexdigest()[:16]
destination = pathlib.Path('dist/pkg') / version
destination.mkdir(parents=True, exist_ok=True)
for path in files:
    shutil.copy2(path, destination / path.name)
for name, directory in asset_directories.items():
    shutil.copytree(directory, destination / 'assets' / name, dirs_exist_ok=True)

# Ship editable corresponding map sources, importer, and attribution with the build.
import zipfile
with zipfile.ZipFile(destination / 'stormkeep-source.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(pathlib.Path('assets/stormkeep').rglob('*')):
        if path.is_file() and 'built' not in path.parts:
            archive.write(path, path)
    for path in map(pathlib.Path, ['scripts/import-stormkeep.py', 'scripts/map_bake.py',
                                  'scripts/map_bsp.py', 'scripts/setup-map-tools.sh',
                                  'scripts/requirements-maps.txt']):
        archive.write(path, path)
    for path in sorted(pathlib.Path('editor/Hookrunner').iterdir()):
        archive.write(path, path)

# Include the editable weapon and exporter.
with zipfile.ZipFile(destination / 'starter-pistol-source.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(pathlib.Path('assets/weapons/starter_pistol').rglob('*')):
        if path.is_file() and 'built' not in path.parts:
            archive.write(path, path)
    for path in map(pathlib.Path, ['scripts/export-starter-pistol.py', 'LICENSE']):
        archive.write(path, path)

# Publish the page only after both files from the new build are ready.
page = template.replace('__HOOKRUNNER_WEB_MODULE__', f'./pkg/{version}/hookrunner_web.js')
page = page.replace('__HOOKRUNNER_ASSET_ROOT__', f'./pkg/{version}/assets')
page = page.replace('__HOOKRUNNER_BUILD__', version)
page = page.replace('__HOOKRUNNER_BUILD_SYNC__', build_sync)
page = page.replace('__HOOKRUNNER_LOADING__', loading)
page = page.replace('__HOOKRUNNER_WASM_URL__', f'./pkg/{version}/hookrunner_web_bg.wasm')
page = page.replace('__HOOKRUNNER_WASM_BYTES__', str((package / 'hookrunner_web_bg.wasm').stat().st_size))
staged_page = pathlib.Path('dist/.index.html.tmp')
staged_page.write_text(page)
staged_page.replace('dist/index.html')
staged_manifest = pathlib.Path('dist/.build.json.tmp')
staged_manifest.write_text(json.dumps({'build': version}) + '\n')
staged_manifest.replace('dist/build.json')
# The project supports only the current build. Cached clients must reload.
for previous in destination.parent.iterdir():
    if previous == destination:
        continue
    if previous.is_dir() and not previous.is_symlink():
        shutil.rmtree(previous)
    else:
        previous.unlink()
print(f'Build: {version}')
PY
echo 'Browser build ready. Serve with: python3 scripts/serve-web.py'
