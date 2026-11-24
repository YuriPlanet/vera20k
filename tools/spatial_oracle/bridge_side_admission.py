"""Original overlay25 side-cell topology through Walk/Infantry and radar gates.

Run from the repository with PYTHONPATH=. and the shared native environment.
The native constructor produces topology. Actor/terrain/list/occupancy state is
explicitly supplied; original land readers, Walk query and admission execute.
"""
from pathlib import Path
import copy
import hashlib
import json
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle import bridge_constructor
from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor
from tools.spatial_oracle.astar_capture_neighbor import ASSETS, prepare
from tools.spatial_oracle.building_body_rules import SP, dwords

HERE = Path(__file__).resolve().parent
PROOF = ((0x75B59C, 0x75B6A0), (0x51BF90, 0x51C890),
         (0x4D9C60, 0x4D9E67), (0x5F5F00, 0x5F5F25))


def native_topology():
    n = OriginalBridgeConstructor('success', 25)
    result = n.run()
    cells = []
    for item in result['cells']:
        at = tuple(item['coord'])
        ptr = n.ptrs[at]
        cells.append(dict(item, coord=list(at),
                          anchor=list(item['anchor']) if item['anchor'] else None,
                          land=n.read_u32(ptr + 0xEC),
                          level=n.uc.mem_read(ptr + 0x11B, 1)[0],
                          slope=n.uc.mem_read(ptr + 0x11C, 1)[0]))
    return cells


def inputs(topology):
    cells = [dict(coord=c['coord'], overlay=c['overlay'],
                  occupation_owners=[-1, -1], land=c['land'], tube_index=-1,
                  level=c['level'], slope=c['slope'], occupation=[0, 0],
                  bridge_flags=c['flags'], state=c['state'], anchor=c['anchor'],
                  ground_objects=[], upper_objects=[]) for c in topology]
    base = dict(name='intact_side_deck',
                origin='Original constructor25 topology with supplied ordinary Infantry deck pose',
                native_size=[16, 16], local_size=[0, 0, 16, 16], frame=112,
                cells=cells,
                actor=dict(id=1, coord=[3712, 4224, 1040], current_mission=2,
                           queued_mission=-1, in_playfield=True, terrain_bypass=False,
                           team=None, slave_owner=None, nav_is_hut=False,
                           attack_is_hut=False, speed_type=0, is_train=False,
                           on_bridge=True),
                hut=dict(id=2, coord=[0, 0, 0], warping_out=False,
                         iron_curtain_start=-1, iron_curtain_duration=0),
                candidate=[15, 16], direction=2, **{'from': [14, 16]})
    yield base
    for name, updates in [
        ('intact_side_deck_ground_occupied', {'occupation': [0x20, 0]}),
        ('intact_side_deck_deck_occupied', {'occupation': [0, 0x20]}),
        ('side_raw100_clear_deck_height', {'clear_structural': True}),
        ('intact_side_ground_deck_occupied', {'occupation': [0, 0x20], 'ground_pose': True}),
        ('side_raw100_clear_ground_height', {'clear_structural': True, 'ground_pose': True}),
    ]:
        row = copy.deepcopy(base)
        row['name'] = name
        candidate = next(c for c in row['cells'] if c['coord'] == row['candidate'])
        if updates.get('clear_structural'):
            candidate['bridge_flags'] &= ~0x100
            row['origin'] += '; candidate bit0x100 cleared as a supplied control, not executed collapse'
        if 'occupation' in updates:
            candidate['occupation'] = updates['occupation']
        if updates.get('ground_pose'):
            row['actor']['on_bridge'] = False
            row['actor']['coord'][2] = 624
        yield row


def prepare_side(row):
    m, actor, cells, table, layers = prepare(row)
    u = m.u
    for c in row['cells']:
        ptr = cells[tuple(c['coord'])]
        u.mem_write(ptr + 0x11E, bytes([c['state']]))
        u.mem_write(ptr + 0x2C, dwords(cells[tuple(c['anchor'])] if c['anchor'] else 0))
    return m, actor, cells, table, layers


def execute(row):
    m, actor, cells, _, layers = prepare_side(row)
    u = m.u
    m.invoke(0x49F3A0, 0)
    loco = m.alloc(0x80)
    m.invoke(0x75AA90, loco)
    u.mem_write(loco + 0xC, dwords(actor))
    stack = SP - 0x1000
    u.mem_write(stack, bytes(0x80))
    u.mem_write(stack + 0x14, dwords(row['direction']))
    u.reg_write(UC_X86_REG_ESP, stack)
    u.reg_write(UC_X86_REG_EBP, loco)
    u.reg_write(UC_X86_REG_EAX, actor)
    candidate = cells[tuple(row['candidate'])]
    calls, fields, args = [], [], []
    code = [bytes(u.mem_read(a, b - a)) for a, b in PROOF]

    def observe(_u, pc, _size, _data):
        if pc in (0x7C8E17, 0x7C8B3D, 0x7D140B, 0x5B40B0, 0x65C780, 0x65C7E0):
            raise AssertionError(('unexpected measured seam or RNG', hex(pc)))
        if pc in (0x5F5F00, 0x5F6960, 0x51BF90, 0x4D9C60, 0x51C11A, 0x51C237, 0x51C243):
            calls.append(hex(pc))
        if pc == 0x51BF90:
            sp = u.reg_read(UC_X86_REG_ESP)
            raw = struct.unpack('<5I', u.mem_read(sp + 4, 20))
            assert raw[0] == candidate
            args.append(dict(candidate=row['candidate'], direction=raw[1],
                             height=raw[2], previous_null=raw[3] == 0, flag=raw[4]))

    def read(_u, _access, address, size, _value, _data):
        for offset, label in ((0x44, 'own_overlay'), (0x54, 'ground_owner'),
                              (0x58, 'deck_owner'), (0x11B, 'ground_level'),
                              (0x11E, 'state'), (0x124, 'ground_occupation'),
                              (0x128, 'deck_occupation'), (0x140, 'raw_flags')):
            if candidate + offset <= address < candidate + offset + (1 if offset in (0x11B, 0x11E) else 4):
                fields.append(dict(field=label, pc=hex(u.reg_read(UC_X86_REG_EIP)), size=size))

    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_READ, read)]
    end = run_checked(u, 0x75B59C, (0x75BC13, 0x75B6A0), count=200000,
                      required_addresses=(0x5F5F00, 0x5F6960, 0x51BF90, 0x4D9C60))
    for hook in hooks:
        u.hook_del(hook)
    assert u.reg_read(UC_X86_REG_ESP) == stack
    assert code == [bytes(u.mem_read(a, b - a)) for a, b in PROOF]
    return dict(input=row, native_entry_arguments=args,
                can_enter_class=u.reg_read(UC_X86_REG_ESI),
                outcome='head_selection' if end == 0x75BC13 else 'refusal_response',
                boundary=hex(end), prospective_xyz=list(struct.unpack('<3i', u.mem_read(stack + 0x3C, 12))),
                reached=calls, candidate_reads=fields, native_layers=layers,
                code_unchanged=True, measured_substitutions=[])


def radar(row):
    m, _, cells, _, _ = prepare_side(row)
    u = m.u
    cell = cells[tuple(row['candidate'])]
    overlay_types, bridge_type, tile_types, tile, output = [m.alloc(n) for n in (0x100, 0x300, 4, 0x400, 32)]
    u.mem_write(0xA83D84, dwords(overlay_types))
    u.mem_write(overlay_types + 24 * 4, dwords(bridge_type))
    u.mem_write(bridge_type, dwords(0x7EF600))
    u.mem_write(bridge_type + 0x294, dwords(24))
    u.mem_write(0xA8ED2C, dwords(tile_types))
    u.mem_write(tile_types, dwords(tile))
    stack = SP - 0x1000
    u.mem_write(stack, dwords(RET_MAGIC, output, output + 16))
    u.reg_write(UC_X86_REG_ESP, stack)
    u.reg_write(UC_X86_REG_ECX, cell)
    entry = bytes(u.mem_read(0x47C060, 0x1ED))
    end = run_checked(u, 0x47C060, (0x5FED00, 0x47C24A), count=20000,
                      required_addresses=(0x47C060, 0x47C4D0, 0x47C0AE))
    assert entry == bytes(u.mem_read(0x47C060, 0x1ED))
    result = dict(name=row['name'], raw_flags=m.read32(cell + 0x140), own_overlay=-1,
                  state=u.mem_read(cell + 0x11E, 1)[0],
                  anchor=list(struct.unpack('<2h', u.mem_read(m.read32(cell + 0x2C) + 0x24, 4))),
                  boundary=hex(end), code_unchanged=True, measured_substitutions=[])
    if end == 0x5FED00:
        assert u.reg_read(UC_X86_REG_ECX) == bridge_type
        sp = u.reg_read(UC_X86_REG_ESP)
        result.update(outcome='structural_bridge_color', overlay_type_index=24, frame=m.read32(sp + 8))
    else:
        result.update(outcome='ground_tmp_color')
    return result


def generate():
    assert (ASSETS / 'RULESMD.INI').is_file(), 'Set VERA20K_PROJECTILE_RENDER_ASSETS to extracted retail inputs'
    topology = native_topology()
    reference = Path(bridge_constructor.__file__).with_suffix('.json')
    original = next(row for row in json.loads(reference.read_text())['cases']
                    if row['kind'] == 'success' and row['overlay_id'] == 25 and row['requested'] == [16, 16])
    assert [{key: row[key] for key in ('coord', 'flags', 'overlay', 'state', 'anchor')}
            for row in topology] == original['cells']
    cases = list(inputs(topology))
    return dict(topology=topology, admission=[execute(row) for row in cases],
                radar=[radar(row) for row in (cases[0], cases[3])],
                constructor_reference=dict(file=reference.name, sha256=hashlib.sha256(reference.read_bytes()).hexdigest(),
                                           fields_equal=['coord', 'flags', 'overlay', 'state', 'anchor']))


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'bridge_side_admission.json', provenance=lambda: provenance(
        scope=__doc__, assumptions=[
            'Original Overlay constructor5FC380 and reached bridge setters produce overlay25 topology in the existing native constructor fixture. Its resulting flags/anchor/state/overlay/level/slope/land fields are projected into separate admission and radar machines; full constructor-to-mover lifetime is not executed.',
            'Ordinary Infantry uses original vtable7EB058, supplied Foot SpeedType0, source14,16, candidate15,16, east direction2, Move mission2, no Team/slave/objects, empty owner indices-1, no tubes. Physical on-deck pose uses Z1040 versus ground624, both from level6. Actual applicable land-speed INI reader674000 runs during setup.',
            'Original Walk75B59C query producer, physical height5F5F00, Infantry51BF90 and Foot height gate4D9C60 execute. Native CanEnter return is never supplied or replaced. Stop before head selection or refusal response. No full movement, map/INI loader, repair/collapse lifecycle or persistence proof.',
            'Raw100-clear controls change only candidate flags and retain native state9/overlay-1/anchor; they isolate structural-bit consumption and do not execute bridge destruction. Ground/deck raw occupation0x20 inputs distinguish the selected plane, with empty lists as explicitly supplied state.',
            'Radar47C060 runs through actual empty-ground Building lookup47C4D0 and raw100 gate. Supplied OverlayTypes[24] identity and tile-table storage allow observing the chosen native branch. Stop at real overlay color entry5FED00 or ground TMP branch47C24A; no color-return substitution, color value, pixel rendering or palette proof.'],
        substitutions=['Inherited INI setup allocator/free/TLS/file seams only; none is reached in the measured admission corridor. No measured gameplay return value or code is patched.'],
        entry_points={'overlay_constructor':0x5FC380,'cell_direction_initializer':0x49F2F0,
                      'lepton_direction_initializer':0x49F3A0,'land_speed_reader':0x674000,
                      'walk_constructor':0x75AA90,'walk_query':0x75B59C,
                      'physical_height':0x5F5F00,'infantry_can_enter':0x51BF90,
                      'foot_height_gate':0x4D9C60,'radar':0x47C060,
                      'radar_structural_gate':0x47C0AE,'overlay_color_boundary':0x5FED00,
                      'ground_color_boundary':0x47C24A}))
