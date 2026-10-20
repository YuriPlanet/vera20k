"""Execute concrete Foot shadow callers and the original retail VXL stencil.

VERA20K_AIRCRAFT_SHADOW_INPUTS=<extracted directory> python -m \
tools.voxel_oracle.aircraft_shadow --write / --check

VERA20K_UNIT_SHADOW_INPUTS=<extracted directory> python -m \
tools.voxel_oracle.aircraft_shadow --unit --write / --check

The map/object fixture and VM belong to aircraft_fire_location. Aircraft executes
floor, height, projection, dispatch and restoration, supplying Mark's external
display notification and the final Techno drawing boundary. Unit executes its
draw projection and post-body suffix with declared body-frame/mask inputs.
Retail files are identified by hash; only generated art is embedded.
"""
import hashlib
import os
from pathlib import Path
import struct
import sys

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESP,
)

from tools.native_oracle import SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.native_slope import initialize_slope_tables
from tools.spatial_oracle.aircraft_fire_location import Fixture, OWNER, TYPE, SCENARIO, cell, dwords
from tools.voxel_oracle.raster_fixtures import test_files
from tools.voxel_oracle.shadow_raster import render_shadow_native


def prepare(case):
    """Shared original map, camera and shadow inputs for concrete callers."""
    f = Fixture(dict(aircraft=case['xyz']))
    u = f.u
    if case.get('family') == 'Unit':
        u.mem_write(OWNER, dwords(0x7F5C70))
        u.mem_write(TYPE, dwords(0x7F6218))
        # Object5F6B90's threshold is two native level heights, not a
        # locomotor cruise height. The shared Aircraft caller never uses it.
        u.mem_write(0xAC13C8, dwords(104))
    initialize_slope_tables(u, f.sp, RET_MAGIC)
    f.call(0x49F2F0, 0, [])
    assert list(struct.iter_unpack('<hh', u.mem_read(0x89F688, 32))) == [
        (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)]
    u.reg_write(UC_X86_REG_ESP, f.sp)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, 0x52BDDD, 0x52BE0A, required_addresses=(0x5AE860, 0x754C00))
    run_checked(u, 0x7558CE, 0x755904, required_addresses=(0x5AE860, 0x5AEF60, 0x5AF1A0))
    loco, point = SCRATCH + 0x30000, SCRATCH + 0x32200
    # Fly's real constructor; Rocket supplies the inherited base-only shadow
    # interface with its original COM vtable, without claiming Rocket AI setup.
    if case['locomotor'] == 'Jumpjet':
        from tools.spatial_oracle.jumpjet_coordinates import initialize_jumpjet_levels
        initialize_jumpjet_levels(f.call)
        rules = SCRATCH + 0x60000
        u.mem_write(0x8871E0, dwords(rules))
        u.mem_write(rules + 0x40C, dwords(4))
    f.call(0x54AC40 if case['locomotor'] == 'Jumpjet' else 0x4CC9A0, loco, [])
    if case['locomotor'] == 'Rocket':
        u.mem_write(loco + 4, dwords(0x7F0B1C))
    u.mem_write(loco + 0xC, dwords(OWNER))
    u.mem_write(OWNER + 0x674, dwords(loco + 4))
    u.mem_write(OWNER + 0x6C, dwords(100))
    u.mem_write(TYPE + 0xA0, dwords(100))
    u.mem_write(OWNER + 0x8C, bytes([case.get('on_bridge', False)]))
    for offset, value in ((0x388, case['primary']), (0x3A0, case['secondary'])):
        f.call(0x4C91C0, OWNER + offset, [])
        u.mem_write(point, dwords(value))
        f.call(0x4C9300, OWNER + offset, [point])
    x, y = (n // 256 for n in case['xyz'][:2])
    u.mem_write(cell(x, y) + 0x11B, bytes([case.get('level', 0), case.get('slope', 0)]))
    u.mem_write(cell(x, y) + 0x140, dwords(case.get('flags', 0)))
    for dx, dy in ((0, -1), (-1, 0)):
        u.mem_write(cell(x + dx, y + dy) + 0x140, dwords(case.get('neighbor_flags', 0)))
    u.mem_write(TYPE + 0xC95, bytes([case.get('dropship', False)]))
    tactical = SCRATCH + 0x40000
    u.mem_write(0x887324, dwords(tactical))
    u.mem_write(tactical + 0xB0, dwords(-320, 200))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    for address, value in ((0xB0CEB4, 416), (0x89E7C0, 104), (0x889EBC, 416), (0xAC13BC, 416)):
        u.mem_write(address, dwords(value))
    u.mem_write(0x886FA0, dwords(0, 0, 640, 480))
    u.mem_write(0xB0CE30, dwords(640, 480))
    # Identity HVA/header seams only expose caller inputs. Physical file
    # loading, HVA composition and stencil are independently executed below.
    vxl, hva, matrix = SCRATCH + 0x50000, SCRATCH + 0x51000, SCRATCH + 0x52000
    u.mem_write(TYPE + 0xB0, dwords(vxl, hva))
    u.mem_write(hva, dwords(0, 1, 1, matrix))
    u.mem_write(matrix, struct.pack('<12f', 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0))
    u.mem_write(TYPE + 0xD96, b'\1')
    u.mem_write(OWNER + 0x6B6, b'\1')
    marks = []

    def mark_boundary(uc, pc, _size, _data):
        if pc != 0x4D3780:
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        ret, arg = struct.unpack('<2I', uc.mem_read(sp, 8))
        assert uc.reg_read(UC_X86_REG_ECX) == OWNER and arg in (0, 1)
        marks.append(arg)
        uc.mem_write(OWNER + 0x74, bytes([arg]))
        uc.reg_write(UC_X86_REG_EAX, 1)
        uc.reg_write(UC_X86_REG_EIP, ret)
        uc.reg_write(UC_X86_REG_ESP, sp + 8)

    u.hook_add(UC_HOOK_CODE, mark_boundary)
    return f, loco, marks


def execute(case, inspect_renderer=False):
    f, loco, marks = prepare(case)
    u = f.u
    before_rng = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    u.reg_write(UC_X86_REG_EBP, OWNER)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_EDI, 0)
    u.reg_write(UC_X86_REG_ESP, f.sp)
    point_entry = 0x55A8C0 if case['locomotor'] == 'Rocket' else 0x4CF940
    matrix_entry = 0x55A7D0 if case['locomotor'] == 'Rocket' else 0x4CFB00
    run_checked(u, 0x414687, 0x706BD0, count=300000,
                required_addresses=(0x578080, 0x6D2140, point_entry, matrix_entry, 0x4DB0D0))
    sp = u.reg_read(UC_X86_REG_ESP)
    args = struct.unpack('<12I', u.mem_read(sp, 48))
    result = dict(input=case, anchor=list(struct.unpack('<2i', u.mem_read(args[6], 8))),
                  temporary_xyz=list(struct.unpack('<3i', u.mem_read(OWNER + 0x9C, 12))),
                  matrix_bits=list(struct.unpack('<12I', u.mem_read(args[7], 48))))
    if inspect_renderer:
        context = u.context_save()
        run_checked(u, 0x706BD0, 0x753F90, count=300000,
                    required_addresses=(0x707280, 0x5AEA10))
        render_args = struct.unpack('<8I', u.mem_read(u.reg_read(UC_X86_REG_ESP), 32))
        result['renderer'] = dict(body_mask=render_args[5],
            viewer_bits=list(struct.unpack('<12I', u.mem_read(render_args[2], 48))))
        assert render_args[5] == 0
        u.context_restore(context)
    # The Techno draw is the output boundary here; restoration runs in the
    # original Aircraft caller after returning from that stdcall(11 args).
    u.reg_write(UC_X86_REG_ESP, sp + 48)
    run_checked(u, args[0], 0x4148C4, count=300000, required_addresses=(0x5F5FA0,))
    result['restored_xyz'] = list(struct.unpack('<3i', u.mem_read(OWNER + 0x9C, 12)))
    result['restored_6b6'] = u.mem_read(OWNER + 0x6B6, 1)[0]
    result['mark_modes'] = marks
    assert result['restored_xyz'] == case['xyz']
    if case['locomotor'] == 'Fly' and case['xyz'][2] == 0 and not case.get('slope', 0):
        body_matrix, body_key = SCRATCH + 0x53000, SCRATCH + 0x53100
        u.mem_write(body_key, dwords(0))
        f.call(0x4CF610, 0, [loco + 4, body_matrix, body_key])
        result['body_cache_key'] = struct.unpack('<I', u.mem_read(body_key, 4))[0]
    assert before_rng == bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    return result


def execute_unit(case, inspect_renderer=False):
    """Unit73C1D2 suffix, with its post-body anchor/frame/mask inputs.

    The original Unit vtable and Jumpjet constructor select inherited shadow
    virtuals. Intervening body composition is outside this caller boundary;
    the ordinary body anchor is produced by native Tactical projection.
    """
    assert case['family'] == 'Unit' and case['locomotor'] == 'Jumpjet'
    f, _loco, marks = prepare(case)
    u = f.u
    u.mem_write(OWNER + 0x74, bytes([case.get('marked', True)]))
    rect = SCRATCH + 0x54000
    u.mem_write(rect, dwords(0, 0, 640, 480))
    u.mem_write(OWNER + 0x80, b'\1\0')
    # Retained viewport writer, before the separate camera-clamp work. Unit's
    # actual draw entry then obtains its point through Object/Tactical.
    u.reg_write(UC_X86_REG_ESP, f.sp)
    u.reg_write(UC_X86_REG_ECX, SCRATCH + 0x40000)
    u.mem_write(f.sp, dwords(RET_MAGIC, rect))
    run_checked(u, 0x6D5F60, 0x6D5F8D)
    u.reg_write(UC_X86_REG_ESP, f.sp)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    u.mem_write(f.sp, dwords(RET_MAGIC, rect, 0, 0))
    run_checked(u, 0x73B0B0, 0x73CEC0,
                required_addresses=(0x5F4B10, 0x6D2140))
    point = struct.unpack('<I', u.mem_read(u.reg_read(UC_X86_REG_ESP) + 4, 4))[0]
    anchor = bytes(u.mem_read(point, 8))
    u.mem_write(TYPE + 0xD6A, bytes([case.get('balloon_hover', True)]))
    u.mem_write(TYPE + 0xD98, bytes([case.get('no_shadow', False)]))
    u.mem_write(OWNER + 0x220, dwords(case.get('cloak', 0)))
    u.mem_write(OWNER + 0x3CD, bytes([case.get('sinking', False)]))
    before_rng = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    u.reg_write(UC_X86_REG_ESP, f.sp)
    u.reg_write(UC_X86_REG_EBP, OWNER)
    u.reg_write(UC_X86_REG_EBX, TYPE)
    u.mem_write(f.sp + 0x1C8, anchor)
    u.mem_write(f.sp + 0x8C, dwords(case.get('body_frame', 0)))
    u.mem_write(f.sp + 0x18, dwords(SCRATCH + 0x55000))
    end = run_checked(u, 0x73C1D2, (0x706BD0, 0x73C5C9), count=300000)
    result = dict(input=case, body_anchor=list(struct.unpack('<2i', anchor)),
                  suffix_reached=end == 0x706BD0,
                  xyz=list(struct.unpack('<3i', u.mem_read(OWNER + 0x9C, 12))))
    if end == 0x706BD0:
        sp = u.reg_read(UC_X86_REG_ESP)
        args = struct.unpack('<12I', u.mem_read(sp, 48))
        result.update(anchor=list(struct.unpack('<2i', u.mem_read(args[6], 8))),
                      matrix_bits=list(struct.unpack('<12I', u.mem_read(args[7], 48))),
                      cache_key=args[3], body_mask_present=args[9] != 0)
        if inspect_renderer:
            end = run_checked(u, 0x706BD0, (0x753F90, 0x73C5C9), count=300000)
            result['draw_admitted'] = end == 0x753F90
            if end != 0x753F90:
                assert before_rng == bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
                return result
            render_args = struct.unpack('<8I', u.mem_read(u.reg_read(UC_X86_REG_ESP), 32))
            result['renderer'] = dict(body_mask=render_args[5],
                mask_offset=list(render_args[6:8]),
                viewer_bits=list(struct.unpack('<12I', u.mem_read(render_args[2], 48))))
    assert result['xyz'] == case['xyz'] and not marks
    assert before_rng == bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    return result


def generate_unit():
    """Bounded Kirov Unit/Jumpjet controls; no Aircraft caller substitution."""
    from tools.voxel_oracle.shadow_fixtures import single_section

    root = Path(os.environ['VERA20K_UNIT_SHADOW_INPUTS'])
    retail = {name: (root / name).read_bytes() for name in ('ZEP.VXL', 'ZEP.HVA', 'VOXELS.VPL')}
    base = dict(family='Unit', locomotor='Jumpjet', xyz=[2688, 2688, 750],
                primary=0, secondary=0)
    controls = [dict(base, xyz=[2688, 2688, z]) for z in (0, 1, 100, 207, 208, 750)]
    controls += [dict(base, primary=0x3FF, secondary=0x400),
                 dict(base, primary=0x400, secondary=0x3FF),
                 dict(base, primary=0x9000, secondary=0x4000, slope=4, level=2),
                 dict(base, primary=0x4000, body_frame=3),
                 dict(base, on_bridge=True),
                 dict(base, flags=0x100, neighbor_flags=0x100),
                 dict(base, sinking=True), dict(base, no_shadow=True), dict(base, cloak=1),
                 dict(base, marked=False)]
    callers = [execute_unit(case, True) for case in controls]
    generated = single_section()
    body = bytes(27 if (x * 3 + y) % 7 < 3 else 0 for y in range(256) for x in range(256))
    cases = []
    for i in (0, 1, 2, 3, 4, 8, 15):
        caller = callers[i]
        renderer = caller['renderer']
        result = render_shadow_native(*generated, struct.pack('<12I', *caller['matrix_bits']),
            body if caller['body_mask_present'] else None, considered_aircraft=True,
            body_offset=renderer['mask_offset'])
        cases.append(dict(caller=i, **result))
    stock = []
    for i in (4, 6, 7, 8):
        caller = callers[i]
        result = render_shadow_native(retail['ZEP.VXL'], retail['ZEP.HVA'], retail['VOXELS.VPL'],
            struct.pack('<12I', *caller['matrix_bits']), considered_aircraft=True)
        stock.append(dict(caller=i, **result))
    return dict(callers=callers,
                generated=dict(vxl=generated[0].hex(), hva=generated[1].hex(),
                               vpl=generated[2].hex(), body=body.hex(), cases=cases),
                retail=dict(type_id='ZEP', image='ZEP',
                            sha256={name: hashlib.sha256(data).hexdigest() for name, data in retail.items()},
                            cases=stock))


def generate():
    root = Path(os.environ['VERA20K_AIRCRAFT_SHADOW_INPUTS'])
    # The retail type ORCA selects Image=FALC. Rust's production layered
    # reader and voxel catalog independently assert this binding.
    retail = {name: (root / name).read_bytes() for name in ('FALC.VXL', 'FALC.HVA', 'VOXELS.VPL')}
    base = dict(locomotor='Fly', xyz=[2688, 2688, 0], primary=0, secondary=0)
    controls = [dict(base, xyz=[2688, 2688, z]) for z in (0, 128, 1500, 640)]
    controls += [dict(base, primary=0x9000, secondary=0x4000, slope=4, level=2),
                 dict(base, primary=0x9000, secondary=0x4000, slope=4, level=2, dropship=True)]
    controls += [dict(base, locomotor=kind, primary=primary, secondary=secondary)
                 for kind in ('Fly', 'Rocket')
                 for primary, secondary in ((0x3FF, 0x400), (0x400, 0x3FF))]
    controls.append(dict(base, primary=0x9000, secondary=0x4000))
    for kind in ('Fly', 'Rocket'):
        for on_bridge, flags, neighbor, z in ((False, 0x900, 0x100, 1500),
                                             (True, 0x100, 0x100, 1500),
                                             (False, 0x100, 0, 1500),
                                             (False, 0x100, 0x100, 128)):
            controls.append(dict(base, locomotor=kind, xyz=[2688, 2688, z],
                                 on_bridge=on_bridge, flags=flags, neighbor_flags=neighbor))
    controls += [dict(base, locomotor='Rocket', xyz=[2688, 2688, 1500],
                      primary=0x9000, secondary=0x4000, slope=4, level=2)]
    callers = [execute(case, i == 0) for i, case in enumerate(controls)]
    generated = test_files()
    raster = []
    for i in (0, 4, 5, len(callers) - 1):
        matrix = struct.pack('<12I', *callers[i]['matrix_bits'])
        for aircraft in (False, True):
            for index in (0, 2):
                result = render_shadow_native(*generated, matrix, considered_aircraft=aircraft,
                                              shadow_index=index)
                raster.append(dict(caller=i, considered_aircraft=aircraft, shadow_index=index, **result))
    stock = []
    for i in (0, 1, 2, 3, 4, 5, len(callers) - 1):
        matrix = struct.pack('<12I', *callers[i]['matrix_bits'])
        result = render_shadow_native(retail['FALC.VXL'], retail['FALC.HVA'], retail['VOXELS.VPL'],
                                      matrix, considered_aircraft=True)
        stock.append(dict(caller=i, **result))
    return dict(callers=callers,
                generated=dict(vxl=generated[0].hex(), hva=generated[1].hex(), vpl=generated[2].hex(), cases=raster),
                retail=dict(type_id='ORCA', image='FALC',
                            sha256={name: hashlib.sha256(data).hexdigest() for name, data in retail.items()}, cases=stock))


if __name__ == '__main__':
    unit = '--unit' in sys.argv[1:]
    if unit:
        finish_vectors(generate_unit, Path(__file__).with_name('unit_shadow.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--unit'],
            provenance=lambda: provenance(
                scope='Unit Jumpjet Kirov draw anchor, shadow suffix/gates/cache-key/mask offsets and stencil',
                assumptions=['original Unit/UnitType vtables, Jumpjet constructor; supplied post-construction XYZ/facings/type/lifecycle bytes',
                    'Unit73B0B0/Object projection produces the ordinary anchor; UnitDrawIt body-offset6B4 and parachute424 remain zero',
                    'interior73C1D2 begins after body composition with supplied body frame and a declared mask surface identity',
                    'patterned generated body is an explicit surface input, not a native Kirov composition proof',
                    'retail Kirov stencils cover unmasked controls; physical VXL/HVA hashes and half viewer are pinned',
                    'no RNG state changes, timer writes or detach calls in measured shadow boundaries; full flight/scene excluded'],
                substitutions=['Unit caller stops at final stencil output; no native call or instruction is replaced',
                    'shared raster file vtable IO and mapped heap allocation; generated body surface bytes supplied'],
                entry_points={'unit_draw': 0x73B0B0, 'unit_shadow_suffix': 0x73C1D2,
                    'jumpjet_ctor': 0x54AC40, 'base_shadow_matrix': 0x55A7D0,
                    'base_shadow_point': 0x55A8C0, 'high_flying': 0x5F6B90,
                    'foot_shadow': 0x4DB0D0, 'techno_shadow': 0x706BD0,
                    'quad': 0x753F90, 'crop': 0x754510}))
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Aircraft shadow caller/cleanup, Fly and Rocket base slots, generated selected-section and retail Harrier (ORCA type, FALC image) stencil',
        assumptions=['supplied original Aircraft/Type/Cell state, camera [-320,200], native startup matrices',
                     'identity HVA/header seam for caller; physical HVA/VXL loaded separately by original reader',
                     'Rocket uses its original COM vtable with supplied base owner; Rocket AI lifecycle is outside this fixture'],
        substitutions=['Foot Mark external display notification: records mode and supplies marked byte/success',
                       'Techno final drawing output boundary for caller cleanup; separate native core raster comparison',
                       'immutable file vtable IO and mapped heap allocation in shared shadow raster helper'],
        entry_points={'aircraft_caller': 0x414687, 'fly_body_matrix': 0x4CF610,
                      'fly_matrix': 0x4CFB00, 'fly_point': 0x4CF940,
                      'rocket_matrix': 0x55A7D0, 'rocket_point': 0x55A8C0, 'foot_shadow': 0x4DB0D0,
                      'techno_shadow': 0x706BD0, 'quad': 0x753F90, 'crop': 0x754510}))
