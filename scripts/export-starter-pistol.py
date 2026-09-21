"""Export the editable pistol currently open in Blender without changing its source.

blender --background assets/weapons/starter_pistol/starter_pistol.blend \
    --python scripts/export-starter-pistol.py
"""
from pathlib import Path
import bpy
import bmesh

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'assets/weapons/starter_pistol'


def export():
    root = bpy.data.objects['StarterPistol']
    parts = [obj for obj in root.children if obj.type == 'MESH']
    assert parts, 'StarterPistol must have editable mesh children'
    bpy.ops.object.select_all(action='DESELECT')
    for obj in parts:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.duplicate()
    # Apply the edited source's modifiers on disposable copies.
    bpy.ops.object.convert(target='MESH')
    copies = list(bpy.context.selected_objects)
    bpy.context.view_layer.objects.active = copies[0]
    bpy.ops.object.join()
    mesh = bpy.context.object
    mesh.name = 'StarterPistol_Mesh'
    mesh.parent = root
    bm = bmesh.new()
    bm.from_mesh(mesh.data)
    bmesh.ops.dissolve_degenerate(bm, dist=1e-7, edges=list(bm.edges))
    bmesh.ops.triangulate(bm, faces=list(bm.faces))
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.calc_area() < 1e-12], context='FACES')
    bm.to_mesh(mesh.data)
    bm.free()
    mesh.data.update()
    triangles = len(mesh.data.polygons)
    assert all(p.area > 1e-12 for p in mesh.data.polygons), 'Degenerate triangle'
    bpy.ops.object.select_all(action='DESELECT')
    mesh.select_set(True)
    root.select_set(True)
    for obj in root.children:
        if obj.type == 'EMPTY':
            obj.select_set(True)
    (OUT / 'built').mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=str(OUT / 'built/starter_pistol.glb'), export_format='GLB',
        use_selection=True, export_yup=True, export_animations=False,
        export_cameras=False, export_lights=False, export_extras=True,
    )
    bpy.data.objects.remove(mesh, do_unlink=True)
    print(f'Exported starter_pistol.glb: {triangles} triangles', flush=True)


if __name__ == '__main__':
    export()
