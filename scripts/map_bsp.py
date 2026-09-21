"""Read q3map2's current IBSP output, preserving diffuse/lightmap UVs and colors."""
import re
import struct
from collections import defaultdict

import numpy as np
from PIL import Image


def srgb_to_linear(color):
    color = np.asarray(color, dtype=float)
    return np.where(color <= 0.04045, color / 12.92, ((color + .055) / 1.055) ** 2.4)


def skybox(source, built):
    """Convert Quake's six environment images into Bevy's cube-map convention."""
    # DarkPlaces' cube vertices/UV convention, before the Quake suffix transforms.
    corners = np.array([
        [[1,-1,1],[1,-1,-1],[1,1,-1]], [[-1,1,1],[-1,1,-1],[-1,-1,-1]],
        [[1,1,1],[1,1,-1],[-1,1,-1]], [[-1,-1,1],[-1,-1,-1],[1,-1,-1]],
        [[-1,-1,1],[1,-1,1],[1,1,1]], [[1,-1,-1],[-1,-1,-1],[-1,1,-1]],
    ])
    coords = np.array([[[0,1],[1,1],[1,0]], [[1,0],[0,0],[0,1]],
                       [[1,1],[1,0],[0,0]], [[0,0],[0,1],[1,1]],
                       [[0,1],[1,1],[1,0]], [[0,1],[1,1],[1,0]]])
    faces = []
    for name, flipx, flipy, transpose in [('rt',False,False,True), ('lf',True,True,True),
                                        ('bk',False,True,False), ('ft',True,False,False),
                                        ('up',False,False,True), ('dn',False,False,True)]:
        pixels = np.asarray(Image.open(source / f'env/extragalactic/asteroids_{name}.tga').convert('RGB'))
        if flipx: pixels = pixels[:, ::-1]
        if flipy: pixels = pixels[::-1]
        if transpose: pixels = pixels.transpose(1, 0, 2)
        faces.append(pixels)
    size = faces[0].shape[0]
    u, v = np.meshgrid((np.arange(size) + .5) * 2 / size - 1, (np.arange(size) + .5) * 2 / size - 1)
    one = np.ones_like(u)
    directions = [(one,-v,-u), (-one,-v,u), (u,one,v), (u,-one,-v), (u,-v,one), (-u,-v,-one)]
    output = []
    for direction in directions:
        # Bevy negates world Z before sampling; Q3 -> Bevy is (x,z,-y).
        q = np.stack(direction, axis=-1)[..., [0,2,1]]
        axis = np.argmax(np.abs(q[size//2,size//2]))
        face = axis * 2 + int(q[size//2,size//2,axis] < 0)
        a, b, c = corners[face]
        basis = np.stack([b-a, c-a], axis=1)
        coefficients = (q-a) @ np.linalg.pinv(basis).T
        uv = coords[face,0] + coefficients @ (coords[face,1:] - coords[face,0])
        xy = np.clip((uv * size).astype(int), 0, size-1)
        output.append(faces[face][xy[...,1], xy[...,0]])
    Image.fromarray(np.concatenate(output)).save(built / 'textures/sky.png')
    return 'sky.png'


def index_vertices(vertices):
    """Share only bit-identical GPU vertices, preserving UV/normal/color seams."""
    data = np.asarray(vertices, dtype='<f4').reshape(-1, 14)
    records = data.view(np.dtype((np.void, data.dtype.itemsize * 14))).ravel()
    _, first, inverse = np.unique(records, return_index=True, return_inverse=True)
    order = np.argsort(first)
    remap = np.empty(len(first), dtype='<u4')
    remap[order] = np.arange(len(first), dtype='<u4')
    return data[first[order]], remap[inverse]


class Bsp:
    def __init__(self, path):
        self.path = path
        self.bytes = path.read_bytes()
        if self.bytes[:8] != b'IBSP.\0\0\0':
            raise ValueError('Expected q3map2 Xonotic IBSP 46')
        self.lumps = [struct.unpack_from('<2I', self.bytes, 8 + i * 8) for i in range(17)]
        self.entities = [dict(re.findall(r'"([^"\n]*)"\s+"([^"\n]*)"', block))
                         for block in re.findall(r'\{([^}]+)\}', self.lump(0).decode())]
        self.shaders = [name.split(b'\0')[0].decode() for name, _, _ in struct.iter_unpack('<64s2i', self.lump(1))]
        self.models = list(struct.iter_unpack('<6f4i', self.lump(7)))
        self.surfaces = list(struct.iter_unpack('<12i12f2i', self.lump(13)))
        self.vertices = np.array([(*v[:10], *(srgb_to_linear(np.array(v[10:13]) / 255)), v[13] / 255)
                                  for v in struct.iter_unpack('<10f4B', self.lump(10))])
        self.indices = np.frombuffer(self.lump(11), dtype='<i4')

    def lump(self, index):
        offset, size = self.lumps[index]
        return self.bytes[offset:offset + size]

    def render(self, compiler, built, rotate, scale):
        meshes = defaultdict(list)
        lightmaps = []
        for path in sorted(self.path.with_suffix('').glob('lm_*.tga')):
            # q3map2 -deluxe interleaves irradiance and direction images.
            if int(path.stem[3:]) % 2:
                continue
            filename = path.with_suffix('.png').name
            Image.open(path).save(built / 'textures' / filename)
            lightmaps.append(filename)
        if not lightmaps:
            raise ValueError('The q3map2 bake produced no external lightmaps')
        owners = {0: self.entities[0]}
        owners.update({int(e['model'][1:]): e for e in self.entities if e.get('model', '').startswith('*')})
        for model_index, model in enumerate(self.models):
            props = owners[model_index]
            if props.get('gametypefilter', '').startswith('+') and 'dm' not in props['gametypefilter']:
                continue
            origin = np.fromstring(props.get('origin', '0 0 0'), sep=' ')
            for surface in self.surfaces[model[6]:model[6] + model[7]]:
                shader, _, kind, first, count, start, index_count, lm = surface[:8]
                name = self.shaders[shader]
                if not compiler.visible(name.removeprefix('textures/')):
                    continue
                material = compiler.material(name)
                # BSP vertices: position3, textureUV2, lightmapUV2, normal3, color4.
                verts = self.vertices[first:first + count].copy()
                verts[:, :3] = (verts[:, :3] + origin) @ rotate.T * scale
                verts[:, 7:10] = verts[:, 7:10] @ rotate.T
                if lm >= 0:
                    verts[:, 10:14] = 1
                    if lm % 2 or lm // 2 >= len(lightmaps):
                        raise ValueError(f'Invalid irradiance lightmap index {lm}')
                if material['emissive']:
                    verts[:, 10:14] = 1
                    lm = -1
                key = material['id'], lm // 2 if lm >= 0 else -1

                def triangle(vertices):
                    vertices = np.array(vertices)
                    cross = np.cross(vertices[1, :3] - vertices[0, :3], vertices[2, :3] - vertices[0, :3])
                    if np.linalg.norm(cross) < 1e-9:
                        return
                    # IBSP faces are clockwise. Also handle compiler patch ordering
                    # using authored outward normals, preserving the wall fix.
                    if np.dot(cross, vertices[:, 7:10].mean(axis=0)) < 0:
                        vertices = vertices[[0, 2, 1]]
                    for v in vertices:
                        n = v[7:10]; n /= max(np.linalg.norm(n), 1e-10)
                        meshes[key].append([*v[:3], *n, *v[3:7], *v[10:14]])

                if kind in (1, 3):
                    for indices in self.indices[start:start + index_count].reshape(-1, 3):
                        triangle(verts[indices])
                elif kind == 2:
                    width, height = surface[-2:]
                    controls = verts.reshape(height, width, 14)
                    for y in range(0, height - 2, 2):
                        for x in range(0, width - 2, 2):
                            control = controls[y:y + 3, x:x + 3]
                            weights = np.array([[(1 - u) ** 2, 2 * u * (1 - u), u ** 2] for u in np.linspace(0, 1, 5)])
                            grid = np.einsum('ai,bj,ijk->abk', weights, weights, control)
                            for a in range(4):
                                for b in range(4):
                                    triangle([grid[a,b], grid[a+1,b], grid[a+1,b+1]])
                                    triangle([grid[a,b], grid[a+1,b+1], grid[a,b+1]])
                else:
                    raise ValueError(f'Unsupported visible BSP surface type {kind}')
        used = sorted({material for material, _ in meshes})
        remap = {material: index for index, material in enumerate(used)}
        compiler.materials = {name: dict(material, id=remap[material['id']])
                              for name, material in compiler.materials.items() if material['id'] in remap}
        with (built / 'render.bin').open('wb') as output:
            output.write(struct.pack('<I', len(meshes)))
            for (material, lightmap), vertices in sorted(meshes.items()):
                unique, indices = index_vertices(vertices)
                output.write(struct.pack('<IiII', remap[material], lightmap, len(unique), len(indices)))
                output.write(unique.tobytes())
                output.write(indices.tobytes())
        return lightmaps, sum(len(v) // 3 for v in meshes.values())
