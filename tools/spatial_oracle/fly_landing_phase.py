"""Original Fly landing callback inside its complete Mark/Display phase.

Real Aircraft, Fly, Cell, air-tracker, neighbor-counter and destination callees
execute. Runtime map/type/contact state is supplied; no gameplay call is replaced.
This corpus covers ordinary landings and successful retry searches, not a complete Fly Process port.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, run_checked, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, OWNER, TYPE, SCENARIO, BLOCKER, cell, dwords
from tools.spatial_oracle.crate_ground_membership import LAYERS
from tools.spatial_oracle.map_queries import packed

LOCO, HOUSE, AIR_BUFFERS, RULES, DISPLAY_BUFFERS = [
    SCRATCH + n for n in (0x20000, 0x22000, 0x30000, 0x40000, 0x60000)
]
AIR_TRACKER = 0x887888


def fixture(case):
    x, y = case.get('cell', [64, 64])
    level, slope = case.get('level', 0), case.get('slope', 0)
    # Coordinates are world values; `z` is deliberately not an altitude cache.
    z = case.get('z', 0)
    f = Fixture(dict(aircraft=[x * 256 + 128, y * 256 + 128, z]))
    u = f.u
    # Tactical owns the dirty-cell list used by the changed-layer fog-border
    # suffix. Supply storage; keep the original updater and its calls intact.
    u.mem_write(0x887324, dwords(SCRATCH + 0x80000))
    u.mem_write(cell(x, y), dwords(0x7E4EEC))
    u.mem_write(cell(x, y) + 0x44, dwords(-1))
    u.mem_write(cell(x, y) + 0x11B, bytes([level, slope]))
    u.mem_write(cell(x, y) + 0x140, dwords(0x100 if case.get('bridge') else 0))
    f.call(0x4CC9A0, LOCO, [])
    u.mem_write(LOCO + 0xC, dwords(OWNER))
    u.mem_write(LOCO + 0x18, bytes([case.get('airport_bound', False)]))
    u.mem_write(LOCO + 0x1C, dwords(*case.get('destination', [x * 256 + 128, y * 256 + 128, 0])))
    u.mem_write(LOCO + 0x34, b'\1')
    u.mem_write(LOCO + 0x40, struct.pack('<dd', 0.75, 0.5))
    u.mem_write(LOCO + 0x50, bytes([0, case.get('landing', True), case.get('latched', False)]))
    u.mem_write(OWNER + 0x674, dwords(LOCO + 4))
    u.mem_write(OWNER + 0x6C, dwords(case.get('health', 100)))
    u.mem_write(OWNER + 0x94, dwords(-1))
    u.mem_write(OWNER + 0x90, b'\1')
    u.mem_write(OWNER + 0x8C, bytes([case.get('on_bridge', False)]))
    u.mem_write(OWNER + 0x21C, dwords(HOUSE))
    u.mem_write(OWNER + 0xE4, dwords(SCRATCH + 0x70000, 1))
    u.mem_write(OWNER + 0x2E8, struct.pack('<f', case.get('owner_float_2e8', 0)))
    u.mem_write(TYPE + 0xC95, bytes([case.get('dropship',False)]))
    u.mem_write(TYPE + 0x530, dwords(-1))  # no landing sound asset
    u.mem_write(TYPE + 0x52C, dwords(-1))  # no takeoff sound asset
    u.mem_write(TYPE + 0x618, dwords(-1))
    u.mem_write(TYPE + 0xE0D, bytes([case.get('airport_bound', False)]))
    u.mem_write(TYPE + 0xE0A, b'\1')  # Landable
    u.mem_write(TYPE + 0xDFC, b'\0')  # no Carryall animation/base
    for address, value in ((0x89E7C0, 104), (0xAC13C8, 104), (0xAC13BC, 416), (0x8B3CAC, 416)):
        u.mem_write(address, dwords(value))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x1768, dwords(11))
    u.mem_write(RULES + 0x7B4, dwords(1500))
    u.mem_write(0xA8ED84, dwords(100))

    # Original tracker-vector startup, stopping immediately before CRT atexit.
    # Preallocated storage avoids allocator growth; the actual Add/Remove run.
    u.mem_write(0x87F914, dwords(128, 128))
    u.reg_write(UC_X86_REG_ESP, f.sp)
    run_checked(u, 0x412870, 0x4128D6, count=10000)
    for bucket in range(400):
        u.mem_write(AIR_TRACKER + bucket * 24 + 4, dwords(AIR_BUFFERS + bucket * 64, 16))
    if case.get("air_registered", True):
        f.call(0x4134A0, AIR_TRACKER, [OWNER])
    else:
        u.mem_write(OWNER + 0x560, bytes(u.mem_read(0x8B3C58, 4)))
    f.call(0x49F2F0, 0, [])  # eight original packed neighbor deltas

    old = case.get('previous_neighbor_cell', [0, 0])
    u.mem_write(OWNER + 0x55C, packed(*old))
    watched = set()
    for center in ([x, y], old):
        if center == [0, 0]:
            continue
        for i in range(8):
            dx, dy = struct.unpack('<hh', u.mem_read(0x89F688 + i * 4, 4))
            watched.add((center[0] + dx, center[1] + dy))
    for xy in watched:
        u.mem_write(cell(*xy) + 0x122, bytes([case.get('neighbor_seed', 0)]))

    # Retain a registration from before the supplied physical descent. No
    # fake GetLayer return is used: the original submit reads actual object Z.
    u.reg_write(UC_X86_REG_ESP, f.sp)
    run_checked(u, 0x4A8630, 0x4A866D, count=100)
    for layer in range(5):
        u.mem_write(LAYERS + 24 * layer + 4, dwords(DISPLAY_BUFFERS + layer * 0x100, 16))
    u.mem_write(OWNER + 0xA4, dwords(case.get('registration_z', 900)))
    u.mem_write(OWNER + 0x74, b'\0')
    f.call(0x4D3780, OWNER, [1])
    f.call(0x4A9720, 0x87F7E8, [OWNER])
    u.mem_write(OWNER + 0xA4, dwords(z))

    def state():
        layers = []
        for layer in range(5):
            count = struct.unpack('<I', u.mem_read(LAYERS + 24 * layer + 16, 4))[0]
            assert count <= 16
            pointers = struct.unpack('<' + 'I' * count, u.mem_read(DISPLAY_BUFFERS + layer * 0x100, count * 4))
            assert all(pointer == OWNER for pointer in pointers)
            layers.append(len(pointers))
        return dict(
            coordinates=list(struct.unpack('<iii', u.mem_read(OWNER + 0x9C, 12))),
            phase=list(u.mem_read(LOCO + 0x50, 3)),
            speeds=list(struct.unpack('<dd', u.mem_read(LOCO + 0x40, 16))),
            destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x1C, 12))),
            moving=bool(u.mem_read(LOCO + 0x34, 1)[0]),
            marked=bool(u.mem_read(OWNER + 0x74, 1)[0]),
            on_bridge=bool(u.mem_read(OWNER + 0x8C, 1)[0]),
            neighbor_cell=list(struct.unpack('<hh', u.mem_read(OWNER + 0x55C, 4))),
            neighbors=[[a, b, u.mem_read(cell(a, b) + 0x122, 1)[0]] for a, b in sorted(watched)],
            air_members=sum(struct.unpack('<I', u.mem_read(AIR_TRACKER + i * 24 + 16, 4))[0] for i in range(400)),
            layers=layers,
            # CdTimer's middle word is unused storage; do not export the
            # caller's incidental stack value as semantic timer evidence.
            path_timer=[struct.unpack('<i', u.mem_read(OWNER + offset, 4))[0]
                        for offset in (0x668, 0x670)],
        )

    if case.get('blocking_unit'):
        u.mem_write(BLOCKER + 0x21C, dwords(HOUSE))
        u.mem_write(BLOCKER + 0x9C, dwords(x*256+128,y*256+128,0))
        u.mem_write(cell(x,y)+0xE4,dwords(BLOCKER))
    if case.get('reserved'):
        actor=SCRATCH+0x90000
        u.mem_write(actor+0x90,b'\1')
        u.mem_write(actor+0x5A4,dwords(cell(x,y)))
        u.mem_write(0xA8E394,dwords(SCRATCH+0x91000))
        u.mem_write(SCRATCH+0x91000,dwords(actor))
        u.mem_write(0xA8E3A0,dwords(1))
    if case.get('airport_bound'):
        # No contact building admits the destination: full landing executes
        # BeginTakeoff and FNPC. Supply the native Clear land cost table and
        # absent overlays so original search/projection callees can execute.
        u.mem_write(0x89EA40, struct.pack('<90f', *[1.0] * 90))
        for y in range(128):
            for x in range(128):
                u.mem_write(cell(x, y) + 0x44, dwords(-1))
    return f, state


def execute(case):
    f, state = fixture(case)
    u = f.u
    before, events = state(), []
    before_rng = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    labels = {0x4CE840: 'landing', 0x4CE680: 'takeoff', 0x4A9770: 'display_remove',
              0x4A9720: 'display_submit', 0x4196B0: 'aircraft_can_enter',
              0x4135D0: 'air_remove', 0x4CED34: 'owner_slot_544',
              0x4CEF88: 'landing_assign_destination', 0x7509E0: 'landing_sound',
              0x4CF950: 'begin_takeoff', 0x56DC20: 'nearby_cell_search',
              0x4CD3B6: 'phase_ground_admission', 0x4CD42C: 'phase_ground_radio'}

    def observe(_u, pc, _size, _data):
        if pc == 0x4D3780:
            arg = struct.unpack('<I', u.mem_read(u.reg_read(UC_X86_REG_ESP) + 4, 4))[0]
            events.append(['mark', arg])
        elif pc in labels:
            events.append([labels[pc]])

    u.hook_add(UC_HOOK_CODE, observe)
    returned = f.call(0x4CD2A0, LOCO, []) & 0xFF
    assert bytes(u.mem_read(SCENARIO + 0x218, 0x3F4)) == before_rng
    return dict(input=case, before=before, after=state(), returned=returned, events=events, rng_changed=False)


def cases():
    rows = [dict(name=f'height_{z}', z=z) for z in (-1, 0, 1, 299, 300, 301, 900)]
    rows += [dict(name=f'old_{old}_seed_{seed}', previous_neighbor_cell=old, neighbor_seed=seed)
             for old in ([0, 0], [64, 64], [63, 64], [60, 60]) for seed in (0, 1, 255)]
    rows += [dict(name='no_landing', landing=False), dict(name='already_latched', latched=True),
             dict(name='dead', health=0), dict(name='negative_health', health=-1),
             dict(name='ground_registration', registration_z=0),
             dict(name='positive_owner_float', owner_float_2e8=0.25),
             dict(name='negative_owner_float', owner_float_2e8=-0.25),
             dict(name='above_sloped_cell', level=2, slope=1, z=260),
             dict(name='already_on_bridge', bridge=True, on_bridge=True, z=416),
             dict(name='different_empty_destination', destination=[16768,16512,0]),
             dict(name='new_bridge_attachment', bridge=True, z=416),
             dict(name='bridge_ground_occupied',bridge=True,z=416,blocking_unit=True),
             dict(name='bridge_cell_reserved',bridge=True,z=416,reserved=True),
             dict(name='dropship_settle',dropship=True,latched=True,owner_float_2e8=0.25),
             dict(name='dropship_final_settle',dropship=True,latched=True,owner_float_2e8=0.01)]
    rows += [dict(name=f'pad_refusal_retry_{z}', z=z, airport_bound=True)
             for z in (0, 1, 299, 300, 900)]
    rows += [dict(name=f'bridge_pad_refusal_retry_{z}', z=z, airport_bound=True, bridge=True)
             for z in (416, 900)]
    return rows


def generate():
    result = [execute(case) for case in cases()]
    by_name = {row['input']['name']: row for row in result}
    assert by_name['height_0']['before']['layers'] == [0, 0, 0, 0, 1]
    assert by_name['height_0']['after']['layers'] == [0, 0, 1, 0, 0]
    assert by_name['height_0']['after']['air_members'] == 0
    assert by_name['height_1']['after']['air_members'] == 1
    assert ['landing_sound'] in by_name['height_299']['events']
    assert ['landing_sound'] not in by_name['height_300']['events']
    assert by_name['no_landing']['before'] == by_name['no_landing']['after']
    assert by_name['dead']['before'] == by_name['dead']['after']
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'phase': 0x4CD2A0, 'landing': 0x4CE840, 'can_enter': 0x4196B0,
                      'air_remove': 0x4135D0, 'air_init': 0x412870, 'neighbors_init': 0x49F2F0,
                      'display_init': 0x4A8630, 'mark': 0x4D3780},
        assumptions=['Real Aircraft and Fly vtables, original Fly constructor; initialized empty map and no Team, cargo, radio contacts, animations or target.',
                     'Landable type, zero Carryall; AirportBound type and retained Fly+18 only on retry rows, with all land costs1 and no overlays. Supplied+C95 only in pre-latched pitch-settle cases; sound indexes-1. Owner+2E8 samples are supplied, not proof of their producer.',
                     'Original air-tracker and Display constructor prefixes stop before CRT atexit; preallocated buffers avoid allocator growth.',
                     'Ground slope initializer input89E7C0=104 (47B3A0 lazily caches it); earlier29-row corpus omitted this and its lone slope row ran with an invalid zero slope scale.',
                     'Original Mark(PUT), Display submit and air Add establish initial membership. Supplied Z change models the preceding descent; full Process does not run.',
                     'Supplied Foot+55C neighbor source and Cell+122 seeds exercise wrapping and overlap; they are not constructor/lifecycle proofs. Rules+1768 path timer duration11, frame100.'],
        substitutions=[],
        scope='41 full landing-phase calls: retained Top-to-Ground resubmission, threshold/latch, air removal, neighbor migration, destination clear, path timer, unchanged Scenario RNG and already-OnBridge landing. Includes changed-layer bridge acceptance and occupant/reservation refusal, radio/sight/fog-border suffix, pre-latched Dropship settle, and AirportBound refusal through successful original FNPC/BeginTakeoff/MoveTo. Excludes failed-search/destruction, Carryall/type+C95 animation branches, radio docking lifecycle, complete Process, rendered output and Rust landing parity.',
    ))
