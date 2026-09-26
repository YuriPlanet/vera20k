"""Full native impact resolution, ordinary detonation/AoE, bridge draw, cluster successor.

Original code runs from 468D80 to its return. No detonation, area-damage, RNG,
coordinate, animation-selection, cell-lookup or air-collection body is stubbed.
Bridge driver leaves return supplied false; reached rendering-only projection/dirty calls are bounded.
"""
import json
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.spatial_oracle.bridge_damage_admission import (
    base, call, words, read32, signed, seed_bytes, slope_matrices,
    MEM, TABLE, CELL, ANCHOR, DUMMY, SCENARIO, RULES, WARHEAD, ION,
    BULLET, DRIVERS, SP, RET_MAGIC,
)
from tools.native_oracle import run_checked, finish_vectors, provenance

TYPE = MEM + 0xA000
DEFAULTS = dict(level=2, impact_z=624, seed=1, damage=2000, strength=1500,
                cluster=1, flags=0x100, anchor_overlay=0x18, wall=True,
                no_damage=False, destroyable=True, bridge_patch=False)
OBSERVE_ENTRIES = {
    0x468D80: 'impact_ladder', 0x4690B0: 'detonate', 0x489280: 'area_damage',
    0x489E87: 'bridge_blocks', 0x5657A0: 'get_cell', 0x578080: 'ground_z',
    0x486840: 'cell_ground_coords', 0x486890: 'cell_deck_coords',
    0x412B40: 'air_collection', 0x4137A0: 'air_first',
    0x48A4F0: 'select_animation', 0x49F420: 'cluster_coordinate',
    0x65C7E0: 'random_ranged', 0x65C780: 'random_raw',
}


def execute(case):
    case = dict(DEFAULTS, **case)
    u = base(case)
    u.mem_write(0x89E7C0, words(104))
    u.mem_write(0x87F914, words(64, 64))
    u.mem_write(DUMMY, words(0x7E4EEC))
    u.mem_write(DUMMY + 0x11B, bytes([2]))
    # A 7x7 patch covers every coordinate scattered at most512 leptons from
    # the fixed original center. Other objects and AirTracker buckets are empty.
    offset = 0x10000
    for y in range(17, 24):
        for x in range(7, 14):
            if (x, y) in ((10, 20), (9, 20)):
                continue
            pointer = MEM + offset
            offset += 0x200
            u.mem_write(pointer, words(0x7E4EEC))
            u.mem_write(pointer + 0x24, struct.pack('<hh', x, y))
            u.mem_write(pointer + 0x38, words(-1))
            u.mem_write(pointer + 0x44, words(-1))
            u.mem_write(pointer + 0x11B, bytes([2]))
            if case['bridge_patch']:
                u.mem_write(pointer + 0x2C, words(ANCHOR))
                u.mem_write(pointer + 0x140, words(case['flags']))
            u.mem_write(TABLE + (y * 512 + x) * 4, words(pointer))
    u.mem_write(ANCHOR + 0x11B, bytes([2]))
    # The anchor's non-wall, non-explosive supplied overlay type permits
    # ordinary AoE when later cluster coordinates fall into that cell.
    u.mem_write(0xA83D84, words(MEM + 0x20000))
    u.mem_write(MEM + 0x20000, words(*([MEM + 0x21000] * 256)))
    for index, matrix in enumerate(slope_matrices()):
        u.mem_write(0xB45188 + 48 * index, struct.pack('<12I', *matrix))
    scenario_flags = (0x8000 if case['destroyable'] else 0) | (0x20 if case['no_damage'] else 0)
    u.mem_write(SCENARIO, words(scenario_flags))
    u.mem_write(SCENARIO + 0x218, seed_bytes(case['seed']))
    u.mem_write(0xA8B230, words(SCENARIO))
    u.mem_write(0x8871E0, words(RULES))
    u.mem_write(RULES + 0x1740, words(case['strength']))
    u.mem_write(RULES + 0xFF0, words(ION))
    u.mem_write(WARHEAD + 0x144, bytes([case['wall']]))
    u.mem_write(BULLET, words(0x7E46E4))
    u.mem_write(BULLET + 0x9C, words(2688, 5248, case['impact_z']))
    u.mem_write(BULLET + 0xAC, words(TYPE))
    u.mem_write(BULLET + 0x128, words(WARHEAD))
    u.mem_write(BULLET + 0x10C, words(CELL))
    u.mem_write(BULLET + 0x90, bytes([1]))
    u.mem_write(BULLET + 0x6C, words(case['damage']))
    u.mem_write(BULLET + 0x150, words(256))
    u.mem_write(TYPE + 0x29B, bytes([1]))
    u.mem_write(TYPE + 0x2AC, words(case['cluster']))
    events, pending, raw_words, calls, substitutions = [], {}, [], {}, []

    def return_supplied(result, cleanup):
        sp = u.reg_read(UC_X86_REG_ESP)
        destination = read32(u, sp)
        u.reg_write(UC_X86_REG_EAX, result)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
        u.reg_write(UC_X86_REG_EIP, destination)

    def observe(_uc, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address in OBSERVE_ENTRIES:
            label = OBSERVE_ENTRIES[address]
            calls[label] = calls.get(label, 0) + 1
        if address in pending:
            event = pending.pop(address)
            result = u.reg_read(UC_X86_REG_EAX)
            if event['kind'] == 'cluster_coord':
                event['coord'] = list(struct.unpack('<3i', u.mem_read(result, 12)))
            else:
                event['result'] = result if event['kind'] == 'raw' else signed(result)
        if address == 0x4690B0:
            pointer = read32(u, sp + 4)
            events.append(dict(kind='detonate', coord=list(struct.unpack('<3i', u.mem_read(pointer, 12)))))
        elif address == 0x489280:
            pointer = u.reg_read(UC_X86_REG_ECX)
            events.append(dict(kind='area', coord=list(struct.unpack('<3i', u.mem_read(pointer, 12))),
                               damage=signed(u.reg_read(UC_X86_REG_EDX))))
        elif address == 0x65C7E0:
            event = dict(kind='range', caller=f'{read32(u, sp):08X}',
                         low=signed(read32(u, sp + 4)), high=signed(read32(u, sp + 8)))
            events.append(event)
            pending[read32(u, sp)] = event
        elif address == 0x65C780:
            event = dict(kind='raw', caller=f'{read32(u, sp):08X}')
            events.append(event)
            pending[read32(u, sp)] = event
        elif address in (0x65C84B, 0x65C79D):
            raw_words.append(u.reg_read(UC_X86_REG_ESI))
        elif address == 0x49F420:
            event = dict(kind='cluster_coord', distance=signed(read32(u, sp + 4)))
            events.append(event)
            pending[read32(u, sp)] = event
        elif address in DRIVERS:
            pointer = read32(u, sp + 4)
            event = dict(kind='driver_false', entry=f'{address:08X}',
                         coord=list(struct.unpack('<2h', u.mem_read(pointer, 4))))
            events.append(event)
            substitutions.append(f'{address:08X}')
            return_supplied(0, 4)
        elif address == 0x6D2140:
            u.mem_write(read32(u, sp + 8), words(0, 0))
            events.append(dict(kind='project_supplied_zero'))
            substitutions.append('006D2140')
            return_supplied(0, 8)
        elif address == 0x6D2790:
            events.append(dict(kind='dirty_rect', rect=list(struct.unpack('<4i', u.mem_read(sp + 4, 16)))))
            substitutions.append('006D2790')
            return_supplied(0, 20)

    handle = u.hook_add(UC_HOOK_CODE, observe)
    u.mem_write(SP, words(RET_MAGIC, 1))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, BULLET)
    run_checked(u, 0x468D80, RET_MAGIC, count=400000,
                required_addresses=(0x4690B0, 0x489280, 0x49F420, 0x65C7E0, 0x65C780)
                if case['cluster'] > 0 else ())
    assert not pending
    u.hook_del(handle)
    state = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    next_values = [call(u, 0x65C780, SCENARIO + 0x218) for _ in range(4)]
    return dict(input=case, events=events, raw_words=raw_words, calls=calls,
                substitutions=substitutions, rng_indices=list(struct.unpack_from('<2I', state, 4)),
                next_rng=next_values)


def specifications():
    for seed in (1, 31, 42, 12345):
        for damage in (-65536, -1, 0, 1, 2000, 65536):
            for cluster in (1, 3):
                yield dict(name=f's{seed}_d{damage}_c{cluster}', seed=seed, damage=damage, cluster=cluster)
    for seed in (1, 42):
        for label, variation in (
                ('wall_no', dict(wall=False)), ('no_damage', dict(no_damage=True)),
                ('indestructible', dict(destroyable=False)), ('ground_z', dict(impact_z=208)),
                ('no_structural', dict(flags=0)), ('wood_anchor', dict(anchor_overlay=0xED)),
                ('cluster_zero', dict(cluster=0)), ('cluster_negative', dict(cluster=-1)),
                ('patch_c3', dict(bridge_patch=True, cluster=3)),
                ('strength_negative', dict(strength=-1, damage=-1)),
                ('strength_zero', dict(strength=0)), ('strength_65536', dict(strength=65536)),
        ):
            yield dict(name=f's{seed}_{label}', seed=seed, **variation)


def generate():
    return [execute(case) for case in specifications()]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='72 full original 468D80 ordinary impact-to-detonation-to-area/bridge-to-cluster cases; original Scenario RNG and continuation.',
        assumptions=[
            'Synthetic 64x64 map with an empty 7x7 local patch; Cell10,20 level2 raw structural0x100 and anchor9,20 overlay0x18 (or case variation).',
            'Bullet ordinary Arcing=true, impact flag1, Q8 multiplier256, no source; special warhead flags, CellSpread, AnimList, MaxDebris, Shrapnel, Rocker and Inviso zero; empty object and AirTracker lists.',
            'Zero-initialized supplied Rules/Warhead/BulletType fields except declared inputs; BridgeStrength supplied independently of its separately established native INI reader.',
            'Original Cell virtuals, MapClass lookup/height, slope matrices, Sqrt_Approx, ftol, animation selection, AirTracker and math execute; x87PC53/chop.',
            'bridge_patch=true supplies structural/anchor fields over surrounding cells for repeated-admission ordering; it is not a topology-validity or driver proof.',
        ],
        substitutions=['Bridge drivers587180/57BAA0/57CCF0 returnfalse without mutation; projection6D2140 writes0,0 and dirty6D2790 no-op. Every reached substitution recorded. No other body replaced.'],
        entry_points={'impact_ladder':0x468D80,'detonate':0x4690B0,'area_damage':0x489280,
                      'bridge_blocks':0x489E87,'cluster_distance_call':0x469057,
                      'cluster_coord':0x49F420,'rng_ranged':0x65C7E0,'rng_raw':0x65C780},
    ))
