"""q3map2 authoring bridge: Valve UV axes -> brush primitives -> baked BSP.

The generated brush-primitive file is compiler input, never an editable source.
Lighting flags match Xonotic's Stormkeep build (including its sRGB pipeline).
"""
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
from pathlib import Path

import numpy as np


STAGES = [
    ['-bsp', '-keeplights', '-meta', '-maxarea', '-samplesize', '8',
     '-mv', '1000000', '-mi', '6000000', '-sRGBtex', '-sRGBcolor'],
    ['-vis'],
    ['-light', '-lightmapsize', '1024', '-lightmapsearchpower', '4',
     '-fastallocate', '-deluxe', '-patchshadows', '-samples', '4',
     '-randomsamples', '-bounce', '8', '-fastbounce', '-bouncegrid',
     '-nobouncestore', '-dirty', '-dirtdepth', '64', '-dirtscale', '0.8',
     '-fill', '-backsplash', '0', '0', '-sRGBtex', '-sRGBcolor', '-sRGBlight'],
]


def primitive_uv(normal, distance, axes, scales, size):
    """Project explicit Valve axes onto q3map2's tangent basis, including offset."""
    n = np.where(np.abs(normal) < 1e-6, 0, normal)
    ry = -math.atan2(n[2], math.hypot(n[0], n[1]))
    rz = math.atan2(n[1], n[0])
    tx = np.array([-math.sin(rz), math.cos(rz), 0])
    ty = np.array([-math.sin(ry) * math.cos(rz), -math.sin(ry) * math.sin(rz), -math.cos(ry)])
    matrix = []
    for axis, scale, dimension in zip(axes, scales, size):
        direction = np.array(axis[:3]) / scale
        matrix.append([np.dot(direction, tx) / dimension, np.dot(direction, ty) / dimension,
                       (np.dot(direction, normal) * distance + axis[3]) / dimension])
    return np.array(matrix), tx, ty


def compiler_map(text, compiler):
    lines = []
    for line in text.splitlines():
        if '[' not in line or not line.lstrip().startswith('('):
            lines.append(line)
            continue
        if lines[-1].strip() == '{':
            lines[-1] = '{\nbrushDef\n{'
        groups = re.findall(r'\(([^)]+)\)|\[([^]]+)\]', line)
        points = [np.fromstring(a, sep=' ') for a, _ in groups[:3]]
        axes = [np.fromstring(b, sep=' ') for _, b in groups[3:]]
        name = line.split(')')[3].split('[')[0].strip()
        tail = line.rsplit(']', 1)[1].split()
        n = np.cross(points[2] - points[0], points[1] - points[0]); n /= np.linalg.norm(n)
        if compiler.visible(name):
            material = compiler.material(name)
            size = (material['width'], material['height'])
        else:
            size = (128, 128)  # Hidden compiler surfaces have no rendered UVs.
        matrix, _, _ = primitive_uv(n, np.dot(n, points[0]), axes, map(float, tail[1:3]), size)
        matrix_text = '( ' + ' '.join('( ' + ' '.join(f'{v:.12g}' for v in row) + ' )' for row in matrix) + ' )'
        lines.append(line[:line.index(')') + 1] + ''.join(' ( ' + ' '.join(f'{v:.12g}' for v in p) + ' )' for p in points[1:])
                     + ' ' + matrix_text + ' ' + name + ' ' + ' '.join(tail[3:]))
    # Every brushDef gets its own closing brace. Patch and entity braces stay intact.
    text = '\n'.join(lines)
    text = re.sub(r'(brushDef\n\{\n(?:[^\n]+\n)+?)(\})', r'\1}\n\2', text)
    # Hookrunner treats these two decorative portal models as static geometry.
    return text.replace('"classname" "misc_gamemodel"', '"classname" "misc_model"') + '\n'


def bake(compiler, executable):
    root = Path(__file__).resolve().parents[1]
    assets = root / 'assets/stormkeep'
    directory = assets / 'built/q3map2'
    directory.mkdir(parents=True, exist_ok=True)
    source = assets / 'source'
    input_text = compiler_map((assets / 'stormkeep.map').read_text(), compiler)
    digest = hashlib.sha256(input_text.encode() + json.dumps(STAGES).encode())
    for path in sorted(source.rglob('*')):
        if path.is_file():
            digest.update(str(path.relative_to(source)).encode()); digest.update(path.read_bytes())
    fingerprint = digest.hexdigest()
    manifest = directory / 'bake.json'
    bsp = directory / 'stormkeep.bsp'
    if manifest.exists() and bsp.exists():
        saved = json.loads(manifest.read_text())
        if saved['input_sha256'] == fingerprint and all(
            (directory / name).exists() and hashlib.sha256((directory / name).read_bytes()).hexdigest() == sha
            for name, sha in saved['outputs'].items()
        ):
            print('Baked lighting is current.', flush=True)
            return bsp
    executable = shutil.which(executable)
    if not executable:
        raise RuntimeError('Run scripts/setup-map-tools.sh to install q3map2, or set Q3MAP2 to its executable')
    manifest.unlink(missing_ok=True)
    bsp.unlink(missing_ok=True)
    for previous in (directory / 'stormkeep').glob('lm_*.tga'):
        previous.unlink()
    (directory / 'stormkeep.map').write_text(input_text)
    command = [executable, '-game', 'xonotic', '-fs_nohomepath', '-fs_nobasepath',
               '-fs_pakpath', str(source), '-threads', str(os.cpu_count())]
    for stage in STAGES:
        log = directory / (stage[0][1:] + '.log')
        print(f'Baking {stage[0][1:]} (log: {log.relative_to(root)})', flush=True)
        with log.open('w') as output:
            subprocess.run(command + stage + [str(directory / 'stormkeep.map')],
                           stdout=output, stderr=subprocess.STDOUT, check=True)
        if '************ ERROR ************' in log.read_text():
            raise RuntimeError(f'q3map2 failed: {log}')
    outputs = [bsp, *sorted((directory / 'stormkeep').glob('lm_*.tga'))]
    manifest.write_text(json.dumps(dict(compiler_revision='0c813a34', input_sha256=fingerprint,
                                       outputs={str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest() for p in outputs}), indent=2) + '\n')
    return bsp
