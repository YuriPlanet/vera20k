"""Original Bullet Render through physical120MM SHP clipping and RGB565 pixels.

Native Convert objects are independently initialized from physical palettes.
Prepared surfaces/circular A/Z buffers isolate the selected original draw path.
All Render, Draw, shape decode, clip, selector, rowwalker and leaf code runs.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP
from tools.native_oracle import NATIVE_SHA256, finish_vectors, provenance
from tools.projectile_oracle.bridge_render import setup, ints
from tools.projectile_oracle.bridge_render_inputs import assets_root, load_retail_type
from tools.projectile_oracle.bridge_render_inputs_palette import PaletteReader, initialize
from tools.spatial_oracle.bridge_damage_admission import MEM, call, read32, words

WIDTH, HEIGHT = 64, 96
PIXEL_COUNT = WIDTH * HEIGHT
SURFACE, ZOBJ, AOBJ, ZSURF, ASURF, PIXELS, Z, A = [MEM + offset for offset in
    (0x20000, 0x21000, 0x22000, 0x23000, 0x24000, 0x30000, 0x40000, 0x50000)]
LEAVES = {0x494B60: 'plain_body', 0x492D20: 'plain_shadow'}


def packed_words(values):
    return struct.pack('<' + 'H' * len(values), *values)


def prepare_surface(u, case, *, size=None, initialize_buffer_height=False):
    """Shared original BSurface/circular A/Z transport; caller owns mapping."""
    width,height=(WIDTH,HEIGHT) if size is None else size
    count=width*height
    def surface(address, buffer):
        # Original BSurface virtual methods are used for bounds, locks and pitch.
        u.mem_write(address, words(0x7E2070, width, height, 0, 2,
                                   buffer, count * 2, 0))

    surface(SURFACE, PIXELS)
    surface(ZSURF, Z)
    surface(ASURF, A)
    u.mem_write(0x887314, words(SURFACE))
    for address, backing, buffer, global_ in (
        (ZOBJ, ZSURF, Z, 0x887644), (AOBJ, ASURF, A, 0x87E8A4),
    ):
        # Original scanline functions read +14 backing surface, +18/+1c range,
        # +20 bytes, +24 depth baseline and +28 circular-buffer row width.
        u.mem_write(address, words(0, 0, width, height, 0, backing,
                                   buffer, buffer + count * 2,
                                   count * 2, case.get('depth_baseline',1024), width))
        u.mem_write(global_, words(address))
        if case.get('scroll_rows', 0):
            # Same physical circular-scroll transport as shroud.py's owner.
            u.mem_write(address + 0x10, words(case['scroll_rows'] * width * 2))
        if initialize_buffer_height or case.get('scroll_rows', 0):
            # Original4112D0 uses this height for a full backing-surface fill.
            # Legacy prepared-only draws retain their historical zero default.
            u.mem_write(address + 0x2c, words(height))
    background, old_z = case.get('background', 0xffff), case.get('old_z', 2000)
    initial = packed_words([background] * count)
    z_values = [old_z] * count
    for index, value in case.get('z_patches', []):
        z_values[index] = value
    depths = packed_words(z_values)
    u.mem_write(PIXELS, initial)
    u.mem_write(Z, depths)
    a_values = [case.get('a_value', 127)] * count
    for index, value in case.get('a_patches', []):
        a_values[index] = value
    lighting = packed_words(a_values)
    u.mem_write(A, lighting)
    return dict(background=background, depths=depths, lighting=lighting)


def surface_snapshot(u, state):
    raw = bytes(u.mem_read(PIXELS, PIXEL_COUNT * 2))
    changed = [[i % WIDTH, i // WIDTH, value[0]]
               for i, value in enumerate(struct.iter_unpack('<H', raw))
               if value[0] != state['background']]
    return dict(changed_pixels=changed, color_sha256=hashlib.sha256(raw).hexdigest(),
                z_unchanged=bytes(u.mem_read(Z, len(state['depths']))) == state['depths'])


def execute(case, native_type, raw_shp, palette):
    spec = dict(level=6, flags=256, z=1041, viewport=[0, 0, WIDTH, HEIGHT], camera=[0, 0])
    spec.update(case)
    state = setup(spec, native_type, raw_shp)
    u, obj, typ, shp, tactical, clip, point = state
    heap, globals_ = palette
    u.mem_map(0x24000000, 0x4000000)
    u.mem_write(0x24000000, heap)
    for address, blob in globals_:
        u.mem_write(address, blob)
    buffers = prepare_surface(u, case)
    # Derive the camera from an actual original projection, keeping the desired
    # body screen point an explicit fixture input rather than an expected result.
    call(u, 0x6D2140, tactical, (obj + 0x9C, point))
    world_point = ints(u, point, 2)
    center = case.get('body_screen', [32, 16])
    camera = [world_point[i] - center[i] for i in range(2)]
    u.mem_write(tactical + 0xB0, words(*camera))
    draws, leaves, reached = [], [], set()

    def snapshot():
        return surface_snapshot(u, buffers)

    def observe(_u, address, _size, _data):
        if address in (0x5F4B10, 0x468090, 0x4AED70, 0x490B90, 0x4373B0,
                       0x7BC040, *LEAVES):
            reached.add(address)
        if address == 0x4AED70:
            sp = u.reg_read(UC_X86_REG_ESP)
            args = ints(u, sp + 4, 14)
            draws.append(dict(caller=f'{read32(u,sp):08X}', frame=args[1],
                              point=ints(u,args[2],2), clip=ints(u,args[3],4),
                              flags=args[4], z_adjust=args[6], gradient=args[7],
                              brightness=args[8], convert=u.reg_read(UC_X86_REG_EDX),
                              surface=u.reg_read(UC_X86_REG_ECX), leaves=[]))
        elif address in LEAVES:
            sp = u.reg_read(UC_X86_REG_ESP)
            args = ints(u, sp + 4, 8)
            offset = (args[0] - PIXELS) // 2
            row = dict(leaf=LEAVES[address], x=offset % WIDTH, y=offset // WIDTH,
                       count=args[2], candidate=args[3], brightness=args[6],
                       source_indices=list(u.mem_read(args[1],args[2])))
            leaves.append(row)
            draws[-1]['leaves'].append(row)
        elif address in (0x468379, 0x468422) and draws:
            if draws[-1]['caller'] == f'{address:08X}':
                draws[-1]['after_draw'] = snapshot()

    hook = u.hook_add(UC_HOOK_CODE, observe)
    result = call(u, 0x5F4B10, obj, (clip, 1, 0)) & 255
    u.hook_del(hook)
    return dict(input=spec, actual_camera=camera, projection_before_camera=world_point,
                retained_xyz=ints(u,obj+0x9c,3), render_result=result,
                reached=[f'{p:08X}' for p in sorted(reached)], draws=draws, **snapshot())


def generate():
    reader, pointer = load_retail_type()
    native_type = reader.bullet_state(pointer)
    raw_shp = (assets_root() / '120mm.shp').read_bytes()
    m = initialize(PaletteReader({}))
    palette = (bytes(m.u.mem_read(0x24000000,m.cursor-0x24000000)),
               [(p,bytes(m.u.mem_read(p,n))) for p,n in
                ((0x87F6C0,8),(0x8A0DD0,28))])
    cases = [dict(name=f'height_{z}_flags{flags}',z=z,flags=flags)
             for z in (624,1039,1040,1041,1500) for flags in (0,256)]
    cases += [dict(name=f'depth_{z}',old_z=z) for z in (0,827,828,829,830,831,65535)]
    cases += [dict(name=f'clip_point_{x}_{y}',body_screen=[x,y]) for x,y in
              ((0,0),(1,1),(2,2),(63,95),(64,96),(-1,16),(65,16),(32,-1),(32,97))]
    cases += [dict(name='dirty_rebase',dirty=[9,11,32,40]),
              dict(name='on_bridge',on_bridge=1),
              dict(name='no_shadow',shadow=0),
              dict(name='inviso',inviso=1),
              dict(name='missing_image',missing_image=True),
              dict(name='anim_palette_control',anim_palette=1)]
    canonical = [dict(name=f'canonical_{name}',depth_baseline=32768,old_z=65535,**fields)
                 for name,fields in [('full',{}),('dirty',dict(dirty=[9,11,32,40])),
                     ('corner',dict(body_screen=[0,0])),('below_deck',dict(z=1039)),
                     ('collapsed',dict(flags=0))]]
    canonical += [dict(name=f'canonical_depth_{z}',depth_baseline=32768,old_z=z)
                  for z in (32571,32574,32575)]
    return dict(native_sha256=NATIVE_SHA256,
                selected_native_type=native_type, shp_sha256=hashlib.sha256(raw_shp).hexdigest(),
                palette_loads=m.asset_loaded,
                surface=dict(width=WIDTH,height=HEIGHT,bytes_per_pixel=2,
                             circular_depth_baseline=1024,a_value=127),
                rows=[execute(case,native_type,raw_shp,palette) for case in cases],
                canonical_depth_rows=[execute(case,native_type,raw_shp,palette) for case in canonical])


def metadata():
    return provenance(
        scope='32 baseline1024 and eight baseline32768 original full Object Render/Bullet Draw executions through physical120MM shape frame/clip/selector/plain-rowwalker/body-shadow pixels, using independently initialized native PALETTE.PAL/ANIM.PAL Convert tables. No whole scene, native Display traversal, fog or GPU parity claim.',
        assumptions=[
            'bridge_render_inputs.load_retail_type independently executes original selected Cannon ctor/layered readers on physical lexical INI caches. The physical48-byte120MM SHP is loaded unchanged. Prepared Bullet retainedXYZ and map cell are boundaries; no gameplay launch/lifecycle is executed here.',
            'bridge_render_inputs_palette.initialize executes original startup52BE61..52BFCE and complete Convert48E740 plus Blitter initializer48EBF0. Its reached heap and palette/format globals are copied byte-for-byte into each isolated draw machine; copied data is never reconstructed from expected colors.',
            'Prepared64x96 RGB565 BSurface, circular A/Z buffers, baseline1024 or32768 and A127; all original surface virtual methods, rectangle clipping, frame access, Convert selectors, rowwalking and pixel leaves execute without substitution. Background/oldZ/body screen points are explicit inputs. Camera is derived from original zero-camera projection.',
            'Captures retain per-shape arguments, reached addresses, original leaf candidates/source rows and RGB565 pixels after each shadow/body draw. Native depth arrays are compared byte-for-byte to their inputs. Cases cover clipping on each boundary, dirty rebasing, bridge deck thresholds, OnBridge, exact depth candidates, image/Inviso and palette controls.',
        ],
        substitutions=['Archive IO and allocator boundaries during upstream native input/Convert construction are documented by bridge_render_inputs and bridge_render_inputs_palette. No call is replaced during the rendered execution.'],
        entry_points={'object_render':0x5F4B10,'bullet_draw':0x468090,
                      'shape':0x4AED70,'plain_selector':0x490B90,
                      'plain_rowwalker':0x4373B0,'clip':0x7BC040,
                      'body_leaf':0x494B60,'shadow_leaf':0x492D20})


if __name__ == '__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
