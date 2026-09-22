"""Run the original Aircraft FindFireLocation and candidate admission chain.

Original range/tier, trig, approximate distance, map, reservation and Scenario
RNG instructions execute. Supplied object/map state is not a lifecycle proof.
No call, instruction or return value is substituted.
"""
from functools import lru_cache
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC, load_image, run_checked,
    finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords, packed

OWNER, TARGET, DEST, BLOCKER = [SCRATCH + n for n in (0x1000, 0x2000, 0x3000, 0x4000)]
TYPE, WEAPON, ELITE, PROJECTILE, SCENARIO, VECTOR = [SCRATCH + n for n in
    (0x8000, 0xA000, 0xB000, 0xC000, 0xD000, 0xE000)]
BLOCKER_TYPE = SCRATCH + 0xF000
TABLE, CELLS = SCRATCH + 0x100000, SCRATCH + 0x200000
MAP = 0x87F7E8
SIDE = 128


def i32(value):
    return struct.unpack('<i', dwords(value))[0]


def cell(x, y):
    assert 0 <= x < SIDE and 0 <= y < SIDE
    return CELLS + (y * SIDE + x) * 0x200


@lru_cache(maxsize=2)
def map_bytes(visible=True):
    table, cells = bytearray(0x100000), bytearray(SIDE * SIDE * 0x200)
    for y in range(SIDE):
        for x in range(SIDE):
            offset = (y * SIDE + x) * 0x200
            struct.pack_into('<I', table, (y * 512 + x) * 4, cell(x, y))
            struct.pack_into('<hh', cells, offset + 0x24, x, y)
            cells[offset + 0x12C] = 0x10 if visible else 0
    return bytes(table), bytes(cells)


class Fixture:
    def __init__(self, case):
        self.u = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(SCRATCH, 0xB00000)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        self.sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(TABLE, map_bytes(case.get('visible', True))[0])
        u.mem_write(CELLS, map_bytes(case.get('visible', True))[1])
        u.mem_write(MAP + 0xF4, dwords(64))
        u.mem_write(MAP + 0xFC, dwords(*case.get('local', [0, 0, 64, 64])))
        u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
        u.mem_write(0xA8B238, dwords(case.get('game_mode', 1)))
        u.mem_write(0xABDC50 + 0x11B, b'\0\0')
        for addr, xyz in ((OWNER, case.get('aircraft', [40 * 256 + 128, 64 * 256 + 128, 500])),
                          (TARGET, case.get('target', [64 * 256 + 128, 64 * 256 + 128, 0])),
                          (DEST, case.get('destination', [80 * 256 + 128, 64 * 256 + 128, 0])),
                          (BLOCKER, case.get('blocker', [1, 1, 0]))):
            u.mem_write(addr, dwords(0x7E22A4 if addr == OWNER else 0x7F5C70))
            u.mem_write(addr + 0x14, dwords(5))
            u.mem_write(addr + 0x74, b'\1')
            u.mem_write(addr + 0x9C, dwords(*xyz))
            u.mem_write(addr + 0x6C4, dwords(TYPE))
        u.mem_write(OWNER + 0x6C0, dwords(0x7E2250))
        u.mem_write(OWNER + 0x3D4, bytes([case.get('flag_3d4', False)]))
        u.mem_write(OWNER + 0x150, struct.pack('<f', case.get('veterancy', 0)))
        u.mem_write(TYPE, dwords(0x7E2868))
        u.mem_write(TYPE + 0x898, dwords(WEAPON if case.get('weapon', True) else 0))
        u.mem_write(TYPE + 0xA94, dwords(ELITE if case.get('elite_weapon', True) else 0))
        u.mem_write(TYPE + 0xE0D, bytes([case.get('airport_bound', False)]))
        u.mem_write(TYPE + 0xDFC, bytes([case.get('carryall', False)]))
        u.mem_write(TYPE + 0xD54, bytes([case.get('spawned', False)]))
        # Exercise GetRange7012C0's actual CargoClass head/+30 traversal and
        # each passenger's native turret-aware GetCurrentWeapon70E1A0.
        u.mem_write(TYPE + 0x5E4, bytes([case.get('open_topped', False)]))
        cargo = case.get('passengers', [])
        u.mem_write(OWNER + 0x118, dwords(SCRATCH + 0x90000 if cargo else 0))
        for n, passenger in enumerate(cargo):
            actor = SCRATCH + 0x90000 + n * 0x8000
            kind, gun, elite = actor + 0x2000, actor + 0x4000, actor + 0x5000
            u.mem_write(actor, dwords(0x7F5C70))
            u.mem_write(actor + 0x14, dwords(passenger.get('flags', 5)))
            u.mem_write(actor + 0x30, dwords(actor + 0x8000 if n + 1 < len(cargo) else 0))
            u.mem_write(actor + 0x6C4, dwords(kind))
            u.mem_write(actor + 0x138, dwords(passenger.get('current', 0)))
            u.mem_write(actor + 0x150, struct.pack('<f', passenger.get('veterancy', 0)))
            u.mem_write(kind, dwords(0x7F6218))
            u.mem_write(kind + 0x808, dwords(passenger.get('turrets', 0)))
            for slot, value in enumerate(passenger.get('ranges', [1024])):
                if value is not None:
                    address = gun + slot * 0x100
                    u.mem_write(kind + 0x898 + slot * 0x1C, dwords(address))
                    u.mem_write(address + 0xB4, dwords(value))
            for slot, value in enumerate(passenger.get('elite_ranges', [])):
                if value is not None:
                    address = elite + slot * 0x100
                    u.mem_write(kind + 0xA94 + slot * 0x1C, dwords(address))
                    u.mem_write(address + 0xB4, dwords(value))
        u.mem_write(BLOCKER + 0x6C4, dwords(BLOCKER_TYPE))
        u.mem_write(BLOCKER_TYPE, dwords(0x7F6218))
        u.mem_write(BLOCKER_TYPE + 0xD54, bytes([case.get('blocker_spawned', False)]))
        u.mem_write(BLOCKER + 0x2D0, dwords(int(case.get('blocker_spawn_manager', False))))
        for w, r in ((WEAPON, case.get('range', 1536)), (ELITE, case.get('elite_range', 2304))):
            u.mem_write(w + 0xA0, dwords(PROJECTILE))
            u.mem_write(w + 0xB4, dwords(r))
        u.mem_write(PROJECTILE + 0x2DC, dwords(case.get('rot', 3)))
        u.mem_write(PROJECTILE + 0x29E, bytes([case.get('inviso', False)]))
        u.mem_write(TARGET + 0x14, dwords(case.get('target_flags', 5)))
        u.mem_write(TARGET + 0x5A4, dwords(DEST if case.get('target_has_destination', False) else 0))
        u.mem_write(OWNER + 0x5A4, dwords(BLOCKER if case.get('carryall_target', False) else 0))
        for x, y in case.get('hidden', []):
            u.mem_write(cell(x, y) + 0x12C, b'\0')
        for x, y in case.get('revealed', []):
            u.mem_write(cell(x, y) + 0x12C, bytes([case.get('reveal_bits', 0x10)]))
        for x, y in case.get('blocker_cells', []):
            u.mem_write(cell(x, y) + 0xE4, dwords(BLOCKER))
        reservations = case.get('reserved', [])
        pointers = [OWNER] if case.get('include_self', True) else []
        for n, xy in enumerate(reservations):
            actor = SCRATCH + 0x10000 + n * 0x1000
            u.mem_write(actor, dwords(0x7F5C70))
            u.mem_write(actor + 0x74, bytes([case.get('reservation_marked', True)]))
            u.mem_write(actor + 0x81, bytes([case.get('reservation_limbo', False)]))
            u.mem_write(actor + 0x9C, dwords(1, 1, 0))
            u.mem_write(actor + 0x5A4, dwords(cell(*xy)))
            pointers.append(actor)
        if 'blocker' in case:
            pointers.append(BLOCKER)
        u.mem_write(VECTOR, dwords(*pointers))
        u.mem_write(0x8B3DC4, dwords(VECTOR))
        u.mem_write(0x8B3DD0, dwords(len(pointers)))
        u.mem_write(0xA8B230, dwords(SCENARIO))
        self.call(0x65C6D0, SCENARIO + 0x218, [case.get('seed', 31)])

    def call(self, entry, receiver, args):
        u = self.u
        u.mem_write(self.sp, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, self.sp)
        u.reg_write(UC_X86_REG_ECX, receiver)
        run_checked(u, entry, RET_MAGIC, count=500000)
        assert u.reg_read(UC_X86_REG_ESP) == self.sp + 4 * (len(args) + 1)
        return u.reg_read(UC_X86_REG_EAX)


def execute(case):
    f = Fixture(case)
    u = f.u
    candidates, ranked, draws = [], [], []
    original = bytes(u.mem_read(0x4197C0, 0x4B6))
    before_rng = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    weapon_range = i32(f.call(0x7012C0, OWNER, [0]))

    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x41991B:
            candidates.append([i32(u.reg_read(UC_X86_REG_EAX)), i32(u.reg_read(UC_X86_REG_ESI))])
        elif pc == 0x419A33:
            ranked.append(dict(candidate_index=len(candidates)-1,
                               cell=list(struct.unpack('<hh', u.mem_read(sp + 0xC, 4))),
                               distance=i32(u.reg_read(UC_X86_REG_EAX))))
        elif pc == 0x419AB8:
            draws.append(u.reg_read(UC_X86_REG_EAX))

    u.hook_add(UC_HOOK_CODE, observe)
    pointer = f.call(0x4197C0, OWNER, [0 if case.get('null_target', False) else TARGET])
    assert original == bytes(u.mem_read(0x4197C0, 0x4B6))
    if pointer == 0:
        result = None
    elif pointer == TARGET:
        result = 'target'
    else:
        assert CELLS <= pointer < CELLS + SIDE * SIDE * 0x200
        result = list(struct.unpack('<hh', u.mem_read(pointer + 0x24, 4)))
    rng_changed = before_rng != bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    # Pin continuation as well as the search's returned random value. A port
    # drawing on failed/direct-target paths would otherwise hide behind the
    # same selected cell in a later isolated comparison.
    next_random = f.call(0x65C780, SCENARIO + 0x218, [])
    return dict(input=case, result=result, candidates=candidates, ranked=ranked, draws=draws,
                weapon_range=weapon_range,
                rng_changed=rng_changed, next_random=next_random)


def inputs():
    cases = [dict(name='base'), dict(name='null', null_target=True),
             dict(name='strafe_direct', rot=0), dict(name='inviso_search', rot=0, inviso=True),
             dict(name='short', range=512), dict(name='no_weapon', weapon=False),
             dict(name='elite', veterancy=2), dict(name='elite_fallback', veterancy=2, elite_weapon=False),
             dict(name='target_destination', target_has_destination=True),
             dict(name='nonfoot_destination', target_has_destination=True, target_flags=1),
             dict(name='opposite_side', aircraft=[88 * 256 + 128, 64 * 256 + 128, 500]),
             dict(name='far_corner', aircraft=[50 * 256 + 1, 60 * 256 - 1, 500]),
             dict(name='campaign', game_mode=0)]
    cases += [dict(name=f'seed_{seed}', seed=seed) for seed in (0, 1, 2, 7, 99)]
    cases += [dict(name=f'range_{r}', range=r) for r in (-1, 0, 511, 513, 768, 769, 1537)]
    cases += [dict(name=f'rot_{r}', rot=r) for r in (-1, 1, 2)]
    cases += [dict(name='all_hidden', game_mode=0, visible=False),
              dict(name='flag_bypasses_hidden', game_mode=0, visible=False, flag_3d4=True),
              dict(name='mode_bypasses_hidden', game_mode=1, visible=False),
              dict(name='only_first', game_mode=0, visible=False, revealed=[[64, 59]]),
              dict(name='visible_bit_is_not_cache_open', game_mode=0, visible=False,
                   revealed=[[64, 59]], reveal_bits=8),
              dict(name='only_inner_ring', game_mode=0, visible=False, revealed=[[64, 60]]),
              dict(name='same_leptons', aircraft=[64 * 256 + 128, 64 * 256 + 128, 500]),
              dict(name='self_reserves_best', aircraft=[59 * 256 + 128, 64 * 256 + 128, 500], seed=2),
              dict(name='self_not_in_vector', aircraft=[59 * 256 + 128, 64 * 256 + 128, 500],
                   seed=2, include_self=False),
              dict(name='physical_blocker', blocker=[59 * 256 + 128, 64 * 256 + 128, 0], seed=2),
              dict(name='carryall_skips_its_unit', blocker=[59 * 256 + 128, 64 * 256 + 128, 0],
                   carryall=True, carryall_target=True, seed=2),
              dict(name='airport_ignores_blocker', blocker=[59 * 256 + 128, 64 * 256 + 128, 0],
                   airport_bound=True, seed=2)]
    for suffix, flags in [('live', {}), ('unmarked', {'reservation_marked': False}),
                          ('limbo', {'reservation_limbo': True})]:
        cases.append(dict(name='reserved_' + suffix, reserved=[[59, 64]], seed=2, **flags))
    first_ring = [[64,59],[66,59],[68,60],[69,62],[69,64],[69,66],[68,68],[66,69],
                  [64,69],[62,69],[60,68],[59,66],[59,64],[59,62],[60,60],[62,59]]
    cases += [dict(name='first_ring_reserved', reserved=first_ring),
              dict(name='all_rings_reserved', reserved=[[x,y] for y in range(59,70) for x in range(59,70)]),
              dict(name='empty_playfield', local=[0,0,0,0]),
              dict(name='clipped_ring', local=[30,30,8,8]),
              dict(name='record_history_not_second_nearest', target_has_destination=True,
                   destination=[69 * 256 + 128, 68 * 256 + 128, 0])]
    for suffix, flags in [('ordinary', {}), ('spawn_manager', {'blocker_spawn_manager': True}),
                          ('spawned', {'blocker_spawned': True})]:
        cases.append(dict(name='spawned_near_' + suffix, spawned=True,
                          blocker=[59 * 256 + 128, 64 * 256 + 128, 0],
                          blocker_cells=[[59,64]], seed=2, **flags))
    for name, passengers in [
        ('empty', []), ('unarmed', [dict(ranges=[None])]),
        ('short', [dict(ranges=[512])]), ('long', [dict(ranges=[2048])]),
        ('mixed', [dict(ranges=[2048]), dict(ranges=[769]), dict(ranges=[1024])]),
        ('negative', [dict(ranges=[-1])]), ('zero', [dict(ranges=[0])]),
        ('turret_slot', [dict(turrets=2, current=1, ranges=[2048, 768])]),
        ('no_turrets', [dict(current=1, ranges=[1024, 512])]),
        ('elite', [dict(veterancy=2, ranges=[1024], elite_ranges=[769])]),
        ('elite_fallback', [dict(veterancy=2, ranges=[1024], elite_ranges=[None])]),
    ]:
        cases.append(dict(name='cargo_' + name, open_topped=True, passengers=passengers))
    cases.append(dict(name='closed_cargo', open_topped=False, passengers=[dict(ranges=[512])]))
    return cases


def generate():
    rows = [execute(case) for case in inputs()]
    by_name = {row['input']['name']: row for row in rows}
    # Coverage assertions establish that supplied states actually distinguish
    # the intended native branches. Outputs are still exclusively native.
    assert by_name['strafe_direct']['result'] == 'target'
    assert by_name['all_hidden']['result'] is None
    assert by_name['flag_bypasses_hidden']['result'] == by_name['base']['result']
    assert len(by_name['only_inner_ring']['candidates']) == 32
    assert by_name['all_rings_reserved']['result'] is None
    for name in ('null', 'strafe_direct', 'short', 'all_hidden', 'all_rings_reserved'):
        assert not by_name[name]['rng_changed'] and not by_name[name]['draws']
    assert by_name['self_reserves_best']['result'] != by_name['self_not_in_vector']['result']
    assert by_name['reserved_live']['result'] != by_name['reserved_unmarked']['result']
    assert by_name['reserved_unmarked']['result'] == by_name['reserved_limbo']['result']
    assert by_name['spawned_near_ordinary']['result'] != by_name['spawned_near_spawn_manager']['result']
    assert by_name['spawned_near_spawn_manager']['result'] == by_name['spawned_near_spawned']['result']
    second_nearest = sorted(by_name['opposite_side']['ranked'], key=lambda x: x['distance'])[1]['cell']
    assert by_name['opposite_side']['result'] != second_nearest
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'search': 0x4197C0, 'candidate_admission': 0x419B00,
                      'range': 0x7012C0, 'get_weapon': 0x70E140,
                      'sin': 0x4CACB0, 'cos': 0x4CAD00, 'sqrt': 0x4CAC40,
                      'rng': 0x65C7E0, 'rng_next': 0x65C780,
                      'playfield': 0x578460, 'cell_lookup': 0x5657A0},
        assumptions=['Supplied Aircraft/Unit object bytes, original vtables and retained destination identities.',
                     '128x128 allocated flat cells; global map width64 and supplied LocalSize; supplied Cell+12C bits.',
                     'Supplied Foot vector, native marked/limbo bytes and physical/destination reservations.',
                     'Scenario RNG initialized by original65C6D0; PC53/chop ambient state.',
                     'Supplied Cell ground list and Techno/Foot class flags for Spawned candidate admission.',
                     'Twelve cargo contrasts execute GetRange through original Cargo head/+30 traversal and turret-aware GetCurrentWeapon, including elite fallback and signed range.'],
        substitutions=[],
        scope='63 original full FindFireLocation calls including range/tier, sixteen-angle rings, native trig/distance, map/visibility, candidate admission, conditional RNG and its next draw. Supplied-state comparison; excludes initialization, reinforcement writers, AssignDestination, Fly movement and full Mission_Attack.',
    ))
