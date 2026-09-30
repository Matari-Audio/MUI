# mui-cut's Blender side (`mui-cut render --renderer blender`). Run as
#   blender -b --factory-startup --python mui-cut-blender.py -- JOB.json
# JOB.json is {"desc", "tex", "blend", "render"}: the scene described by
# mui-cut's src/blender.rs, the layer texture directory, the .blend to reuse
# or save, and [frame, png] pairs to render. Blender runs as its own
# process; mui-cut links none of it.
import json
import math
import os
import sys
import time

import bpy
from mathutils import Matrix

job = json.load(open(sys.argv[sys.argv.index("--") + 1]))
D = job["desc"]
O = D["options"]
scene = bpy.context.scene


def mat(a):
    return Matrix([a[0:4], a[4:8], a[8:12], a[12:16]]).transposed()


def setp(obj, name, value):
    """Set a setting that some Blender versions lack."""
    if name == "use_nodes" and bpy.app.version >= (5, 0):
        return  # always on, and deprecated
    try:
        setattr(obj, name, value)
    except (AttributeError, TypeError, ValueError):
        pass


def times(i):
    """Frame i's states run from frame i to half a frame on (the shutter)."""
    n = len(D["frames"][i])
    return [i + (0.5 * k / (n - 1) if n > 1 else 0) for k in range(n)]


def states():
    for i, frame in enumerate(D["frames"]):
        for t, s in zip(times(i), frame):
            yield t, s


def key(owner, path, value, t):
    setattr(owner, path, value)
    owner.keyframe_insert(path, frame=t)


def key_socket(socket, values):
    """Key a socket only if it changes."""
    values = list(values)
    socket.default_value = values[0][1]
    if any(v != values[0][1] for _, v in values):
        for t, v in values:
            socket.default_value = v
            socket.keyframe_insert("default_value", frame=t)


def key_pose(ob, t, m, prev):
    loc, rot, sca = mat(m).decompose()
    if prev is not None and rot.dot(prev) < 0:
        rot.negate()
    ob.location, ob.rotation_quaternion, ob.scale = loc, rot, sca
    for p in ("location", "rotation_quaternion", "scale"):
        ob.keyframe_insert(p, frame=t)
    return rot


def link(ob):
    scene.collection.objects.link(ob)
    ob.rotation_mode = "QUATERNION"
    return ob


def nodes_of(mat_):
    nt = mat_.node_tree
    return nt, nt.nodes, nt.links


def socket(node, identifier):
    return next(s for s in list(node.inputs) + list(node.outputs) if s.identifier == identifier)


def build():
    bpy.context.preferences.edit.keyframe_new_interpolation_type = "LINEAR"
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob)
    r = scene.render
    r.engine = O["engine"]
    r.resolution_x, r.resolution_y = O["size"]
    r.resolution_percentage = 100
    r.film_transparent = False
    r.image_settings.file_format = "PNG"
    r.image_settings.color_mode = "RGB"
    r.image_settings.color_depth = "8"
    # The house style: colours as painted, no filmic curve, no bloom.
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    frames = len(D["frames"])
    scene.frame_start, scene.frame_end = 0, max(frames - 1, 0)
    mb = O["mb"]
    r.use_motion_blur = mb > 1
    r.motion_blur_shutter = 0.5
    setp(r, "motion_blur_position", "START")
    e = scene.eevee
    setp(e, "taa_render_samples", O["samples"])
    setp(e, "use_raytracing", True)
    # Frosted glass up to fairly rough still traces the screen behind it.
    setp(e.ray_tracing_options, "trace_max_roughness", 0.8)
    setp(e, "use_shadows", True)
    setp(e, "shadow_ray_count", 2)
    setp(e, "shadow_step_count", 8)
    setp(e, "use_fast_gi", True)
    setp(e, "motion_blur_steps", mb)
    setp(e, "volumetric_end", 200)
    if O["engine"] == "CYCLES":
        scene.cycles.samples = O["samples"]
        setp(scene.cycles, "use_denoising", True)

    world()
    camera()
    lights()
    ground()
    layers()
    models()
    fog()


def world():
    """The camera sees the background (or the environment); everything else
    is lit by the ambient light plus the environment."""
    w = bpy.data.worlds.new("world")
    scene.world = w
    setp(w, "use_nodes", True)
    nt = w.node_tree
    nt.nodes.clear()
    amb = nt.nodes.new("ShaderNodeBackground")
    bg = nt.nodes.new("ShaderNodeBackground")
    bg.inputs["Color"].default_value = (*D["background"], 1)
    path = nt.nodes.new("ShaderNodeLightPath")
    mix = nt.nodes.new("ShaderNodeMixShader")
    out = nt.nodes.new("ShaderNodeOutputWorld")
    nt.links.new(path.outputs["Is Camera Ray"], mix.inputs[0])
    nt.links.new(mix.outputs[0], out.inputs["Surface"])
    key_socket(amb.inputs["Color"], ((t, (*s["ambient"], 1)) for t, s in states()))
    W = D["world"]
    if not W:
        nt.links.new(amb.outputs[0], mix.inputs[1])
        nt.links.new(bg.outputs[0], mix.inputs[2])
        return
    # The environment image on the view direction, turned about z.
    tc = nt.nodes.new("ShaderNodeTexCoord")
    mp = nt.nodes.new("ShaderNodeMapping")
    mp.vector_type = "POINT"
    img = nt.nodes.new("ShaderNodeTexEnvironment")
    img.image = bpy.data.images.load(os.path.join(job["tex"], W["image"]), check_existing=True)
    img.projection = "EQUIRECTANGULAR"
    nt.links.new(tc.outputs["Generated"], mp.inputs["Vector"])
    nt.links.new(mp.outputs["Vector"], img.inputs["Vector"])
    env = nt.nodes.new("ShaderNodeBackground")
    nt.links.new(img.outputs["Color"], env.inputs["Color"])
    key_socket(env.inputs["Strength"], ((t, s["env"][0]) for t, s in states()))
    key_socket(mp.inputs["Rotation"], ((t, (0, 0, s["env"][1])) for t, s in states()))
    lit = nt.nodes.new("ShaderNodeAddShader")
    nt.links.new(amb.outputs[0], lit.inputs[0])
    nt.links.new(env.outputs[0], lit.inputs[1])
    nt.links.new(lit.outputs[0], mix.inputs[1])
    nt.links.new((env if W["background"] else bg).outputs[0], mix.inputs[2])


def camera():
    cd = bpy.data.cameras.new("camera")
    cd.sensor_fit = "VERTICAL"
    cd.sensor_height = 24
    cd.clip_start, cd.clip_end = 0.1, 2000
    ob = link(bpy.data.objects.new("camera", cd))
    scene.camera = ob
    cd.dof.use_dof = any(s["camera"]["focus"] > 0 for _, s in states())
    prev = None
    for t, s in states():
        c = s["camera"]
        prev = key_pose(ob, t, c["m"], prev)
        key(cd, "lens", c["lens"], t)
        if cd.dof.use_dof:
            key(cd.dof, "focus_distance", max(c["focus"], 0.01), t)
            key(cd.dof, "aperture_fstop", max(c["fstop"], 0.1), t)


def lights():
    for n, L in enumerate(D["lights"]):
        ld = bpy.data.lights.new(L["id"], L["kind"])
        ld.use_shadow = L["shadow"]
        setp(ld, "use_shadow_jitter", True)
        ob = link(bpy.data.objects.new(L["id"], ld))
        ranged = any(s["lights"][n]["range"] > 0 for _, s in states())
        setp(ld, "use_custom_distance", ranged)
        prev = None
        for t, s in states():
            l = s["lights"][n]
            prev = key_pose(ob, t, l["m"], prev)
            key(ld, "color", l["color"], t)
            key(ld, "energy", l["energy"], t)
            if L["kind"] == "SUN":
                key(ld, "angle", l["size"], t)
            else:
                key(ld, "shadow_soft_size", l["size"], t)
            if L["kind"] == "SPOT":
                key(ld, "spot_size", max(l["spot"], 0.02), t)
                key(ld, "spot_blend", l["blend"], t)
            if ranged:
                key(ld, "cutoff_distance", l["range"] if l["range"] > 0 else 1e4, t)


def ground():
    g = D["ground"]
    if not g:
        return
    s = g["radius"] * 4
    me = bpy.data.meshes.new("ground")
    me.from_pydata([(-s, -s, 0), (s, -s, 0), (s, s, 0), (-s, s, 0)], [], [(0, 1, 2, 3)])
    ob = link(bpy.data.objects.new("ground", me))
    ob.location.z = g["z"]
    m = bpy.data.materials.new("ground")
    setp(m, "use_nodes", True)
    nt, N, Lk = nodes_of(m)
    bsdf = N["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*g["color"], 1)
    bsdf.inputs["Roughness"].default_value = g["roughness"]
    bsdf.inputs["Specular IOR Level"].default_value = g["specular"]
    # Melts into the background, by 1/e at `radius`: exp(-(r/radius)^2)
    # of the lit floor over the background colour (no alpha, no dither).
    tc = N.new("ShaderNodeTexCoord")
    ln = N.new("ShaderNodeVectorMath")
    ln.operation = "LENGTH"
    Lk.new(tc.outputs["Object"], ln.inputs[0])
    q = N.new("ShaderNodeMath")
    q.operation = "DIVIDE"
    q.inputs[1].default_value = g["radius"]
    Lk.new(ln.outputs["Value"], q.inputs[0])
    sq = N.new("ShaderNodeMath")
    sq.operation = "MULTIPLY"
    Lk.new(q.outputs[0], sq.inputs[0])
    Lk.new(q.outputs[0], sq.inputs[1])
    ng = N.new("ShaderNodeMath")
    ng.operation = "MULTIPLY"
    ng.inputs[1].default_value = -1
    Lk.new(sq.outputs[0], ng.inputs[0])
    ex = N.new("ShaderNodeMath")
    ex.operation = "EXPONENT"
    Lk.new(ng.outputs[0], ex.inputs[0])
    bg = N.new("ShaderNodeEmission")
    bg.inputs["Color"].default_value = (*D["background"], 1)
    mix = N.new("ShaderNodeMixShader")
    Lk.new(ex.outputs[0], mix.inputs[0])
    Lk.new(bg.outputs[0], mix.inputs[1])
    Lk.new(bsdf.outputs[0], mix.inputs[2])
    Lk.new(mix.outputs[0], N["Material Output"].inputs["Surface"])
    me.materials.append(m)


def slab(name, v):
    """A variant's geometry: a card, or the outline extruded `depth` back
    from its face at z = 0 with a small bevel."""
    w, h = v["size"]
    if not v["rings"]:
        me = bpy.data.meshes.new(name)
        me.from_pydata(
            [(-w / 2, -h / 2, 0), (w / 2, -h / 2, 0), (w / 2, h / 2, 0), (-w / 2, h / 2, 0)],
            [],
            [(0, 1, 2, 3)],
        )
        return me
    cu = bpy.data.curves.new(name, "CURVE")
    cu.dimensions = "2D"
    cu.fill_mode = "BOTH"
    b = v["bevel"]
    cu.extrude = max(v["depth"] / 2 - b, 0.01)
    cu.bevel_depth = b
    cu.bevel_resolution = 2
    cu.offset = -b if b > 0 else 0
    for ring in v["rings"]:
        sp = cu.splines.new("POLY")
        sp.points.add(len(ring) - 1)
        for p, (x, y) in zip(sp.points, ring):
            p.co = (x, y, 0, 1)
        sp.use_cyclic_u = True
    tmp = bpy.data.objects.new(name, cu)
    scene.collection.objects.link(tmp)
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(tmp.evaluated_get(dg))
    bpy.data.objects.remove(tmp)
    bpy.data.curves.remove(cu)
    me.transform(Matrix.Translation((0, 0, -v["depth"] / 2)))
    return me


def material(name, v, tex_dir):
    """The texture on the face; the edge colour on walls, bevel and back."""
    m = bpy.data.materials.new(name)
    setp(m, "use_nodes", True)
    setp(m, "surface_render_method", "DITHERED")
    nt, N, Lk = nodes_of(m)
    bsdf = N["Principled BSDF"]
    tc = N.new("ShaderNodeTexCoord")
    mp = N.new("ShaderNodeMapping")
    sx, sy, ox, oy = v["uv"]
    mp.inputs["Scale"].default_value = (sx, sy, 1)
    mp.inputs["Location"].default_value = (ox, oy, 0)
    Lk.new(tc.outputs["Object"], mp.inputs["Vector"])
    img = N.new("ShaderNodeTexImage")
    img.image = bpy.data.images.load(os.path.join(tex_dir, v["tex"]), check_existing=True)
    img.extension = "EXTEND"
    Lk.new(mp.outputs["Vector"], img.inputs["Vector"])
    opacity = N.new("ShaderNodeValue")
    opacity.outputs[0].default_value = 1
    alpha = N.new("ShaderNodeMath")
    alpha.operation = "MULTIPLY"
    Lk.new(opacity.outputs[0], alpha.inputs[1])
    Lk.new(alpha.outputs[0], bsdf.inputs["Alpha"])
    if not v["rings"]:
        Lk.new(img.outputs["Color"], bsdf.inputs["Base Color"])
        Lk.new(img.outputs["Alpha"], alpha.inputs[0])
        return m, opacity
    alpha.inputs[0].default_value = 1
    # Face: its normal points out of the front (+z).
    sep = N.new("ShaderNodeSeparateXYZ")
    Lk.new(tc.outputs["Normal"], sep.inputs[0])
    front = N.new("ShaderNodeMath")
    front.operation = "GREATER_THAN"
    front.inputs[1].default_value = 0.9
    Lk.new(sep.outputs["Z"], front.inputs[0])
    f = N.new("ShaderNodeMath")
    f.operation = "MULTIPLY"
    Lk.new(front.outputs[0], f.inputs[0])
    Lk.new(img.outputs["Alpha"], f.inputs[1])
    mix = N.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    Lk.new(f.outputs[0], socket(mix, "Factor_Float"))
    socket(mix, "A_Color").default_value = (*v["edge"], 1)
    Lk.new(img.outputs["Color"], socket(mix, "B_Color"))
    Lk.new(socket(mix, "Result_Color"), bsdf.inputs["Base Color"])
    return m, opacity


SOCKETS = (
    ("metallic", "Metallic"),
    ("roughness", "Roughness"),
    ("transmission", "Transmission Weight"),
    ("ior", "IOR"),
    ("dispersion", "Dispersion"),  # not in Blender 5.2's Principled BSDF
)


def surface(m, mats, slab):
    """`mat` at each instant (all set on a layer; on a model only what its
    layer overrides) on m's Principled BSDF. Returns its most transmission:
    EEVEE refracts only with raytraced refraction on, through a thickness
    the stage's (object space; 0 on a flat card: a thin wall)."""
    nt, N, Lk = nodes_of(m)
    bsdf = next((n for n in N if n.type == "BSDF_PRINCIPLED"), None)
    out = next((n for n in N if n.type == "OUTPUT_MATERIAL"), None)
    if bsdf is None or out is None:
        return 0
    first = mats[0][1] if mats else {}
    tw = bsdf.inputs["Transmission Weight"]
    most = max(v.get("transmission", tw.default_value) for _, v in mats) if mats else tw.default_value
    if tw.is_linked:
        most = 1
    for k, name in SOCKETS:
        if k in first and name in bsdf.inputs:
            key_socket(bsdf.inputs[name], ((t, v[k]) for t, v in mats))
    if "tint" in first:
        mix = N.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        mix.blend_type = "MULTIPLY"
        socket(mix, "Factor_Float").default_value = 1
        base = bsdf.inputs["Base Color"]
        if base.is_linked:
            Lk.new(base.links[0].from_socket, socket(mix, "A_Color"))
        else:
            socket(mix, "A_Color").default_value = base.default_value
        Lk.new(socket(mix, "Result_Color"), base)
        key_socket(socket(mix, "B_Color"), ((t, (*v["tint"], 1)) for t, v in mats))
    thin = slab and all(v["thickness"] == 0 for _, v in mats)
    if "thickness" in first and not thin:
        th = N.new("ShaderNodeValue")
        Lk.new(th.outputs[0], out.inputs["Thickness"])
        key_socket(th.outputs[0], ((t, v["thickness"]) for t, v in mats))
    if most > 0:
        setp(bsdf.inputs["Thin Wall"], "default_value", thin)
        setp(m, "thickness_mode", "SLAB" if slab else "SPHERE")
        setp(m, "use_raytrace_refraction", True)
        setp(m, "use_transparent_shadow", True)
    return most


def layers():
    tex = job["tex"]
    for n, L in enumerate(D["layers"]):
        for vi, v in enumerate(L["variants"]):
            name = L["id"] if len(L["variants"]) == 1 else f"{L['id']}.{vi}"
            me = slab(name, v)
            m, opacity = material(name, v, tex)
            me.materials.append(m)
            ob = link(bpy.data.objects.new(name, me))
            glass = surface(m, [(t, s["layers"][n]["mat"]) for t, s in states()], True)
            # As the stage: mostly clear glass casts no shadow.
            setp(ob, "visible_shadow", L["shadow"] and glass < 0.5)
            prev = None
            alphas = []
            for t, s in states():
                ls = s["layers"][n]
                shown = ls["v"] == vi
                if shown:
                    prev = key_pose(ob, t, ls["m"], prev)
                ob.hide_render = not shown
                ob.keyframe_insert("hide_render", frame=t)
                alphas.append((t, ls["a"] if shown else 0))
            key_socket(opacity.outputs[0], alphas)


def models():
    for n, M in enumerate(D["models"]):
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=M["path"])
        parts = [o for o in bpy.data.objects if o not in before]
        root = link(bpy.data.objects.new(M["id"], None))
        over = [(t, s["models"][n]["mat"]) for t, s in states() if "mat" in s["models"][n]]
        done = {}
        for o in parts:
            for slot in o.material_slots:
                if slot.material and slot.material.name not in done:
                    done[slot.material.name] = surface(slot.material, over, False)
            glass = max((done.get(s.material.name, 0) for s in o.material_slots if s.material), default=0)
            setp(o, "visible_shadow", M["shadow"] and glass < 0.5)
            if o.parent is None:
                o.parent = root
        prev = None
        for t, s in states():
            ms = s["models"][n]
            prev = key_pose(root, t, ms["m"], prev)
            for o in parts:
                o.hide_render = not ms["show"]
                o.keyframe_insert("hide_render", frame=t)


def fog():
    """Mist: linear from `start` over `depth`, mixed toward the fog colour
    in the compositor."""
    f = D["fog"]
    if not f:
        return
    ms = scene.world.mist_settings
    ms.start, ms.depth, ms.falloff = f["start"], f["depth"], "LINEAR"
    bpy.context.view_layer.use_pass_mist = True
    ng = bpy.data.node_groups.new("fog", "CompositorNodeTree")
    ng.interface.new_socket("Image", in_out="OUTPUT", socket_type="NodeSocketColor")
    rl = ng.nodes.new("CompositorNodeRLayers")
    mix = ng.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    out = ng.nodes.new("NodeGroupOutput")
    ng.links.new(rl.outputs["Mist"], socket(mix, "Factor_Float"))
    ng.links.new(rl.outputs["Image"], socket(mix, "A_Color"))
    socket(mix, "B_Color").default_value = (*f["color"], 1)
    ng.links.new(socket(mix, "Result_Color"), out.inputs[0])
    scene.compositing_node_group = ng
    setp(scene.render, "use_compositing", True)


blend = job["blend"]
if os.path.exists(blend):
    bpy.ops.wm.open_mainfile(filepath=blend)
    scene = bpy.context.scene
else:
    t0 = time.time()
    build()
    bpy.ops.wm.save_as_mainfile(filepath=blend + ".part.blend", compress=True)
    os.replace(blend + ".part.blend", blend)
    print(f"mui-cut: blender scene built in {time.time() - t0:.2f} s", file=sys.stderr)

todo = job["render"]
for n, (frame, png) in enumerate(todo):
    t0 = time.time()
    scene.frame_set(frame)
    part = png + ".part.png"
    scene.render.filepath = part
    bpy.ops.render.render(write_still=True)
    os.replace(part, png)
    print(
        f"mui-cut: blender frame {n + 1}/{len(todo)} ({frame}) in {time.time() - t0:.2f} s",
        file=sys.stderr,
        flush=True,
    )
