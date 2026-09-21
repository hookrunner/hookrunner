#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Exact NetRadiant revision used by the official Stormkeep bake. No system install.
tool_root="$PWD/.tools/q3map2"
mkdir -p "$tool_root"
python3 - "$tool_root" <<'PY'
import hashlib
import pathlib
import sys
import tarfile
import urllib.request

root = pathlib.Path(sys.argv[1])
archive = root / 'netradiant.tar.gz'
url = 'https://gitlab.com/xonotic/netradiant/-/archive/0c813a34/netradiant-0c813a34.tar.gz'
expected = '8d90b68b4ec57b89c7e7613f5859ca779fc219e699c1ba1564292210a823947f'
if not archive.exists():
    urllib.request.urlretrieve(url, archive)
if hashlib.sha256(archive.read_bytes()).hexdigest() != expected:
    raise SystemExit('NetRadiant source checksum mismatch')
with tarfile.open(archive) as source:
    source.extractall(root, filter='data')
PY
# NetRadiant's CMake omits minizip's include flags on some distributions.
cmake -S "$tool_root/netradiant-0c813a34" -B "$tool_root/build" \
  -DBUILD_RADIANT=OFF -DBUILD_CRUNCH=OFF -DDOWNLOAD_GAMEPACKS=OFF \
  "-DCMAKE_C_FLAGS=$(pkg-config --cflags minizip) -I$(pkg-config --variable=includedir minizip)/minizip"
cmake --build "$tool_root/build" --target q3map2 --parallel
cp "$tool_root/build/q3map2" "$tool_root/q3map2"
echo "Map compiler ready: $tool_root/q3map2"
