"""Original area-damage bridge admission and cell-target aim coordinates.

The four outer blocks and RandomRanged execute unchanged. Driver return values
and optional callback writes are supplied boundary conditions: they establish
outer continuation, not bridge-driver, collapse, or whole-game parity.
"""
from functools import lru_cache
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    load_image, run_checked, finish_vectors, provenance, STACK_BASE, STACK_SIZE,
    RET_MAGIC, NATIVE_FPCW,
)
from tools.rmg_oracle.gen_rng_vectors import seeded_struct
from tools.projectile_oracle.ordinary_collision import slope_matrices

MEM, TABLE = 0x21000000, 0x21100000
CELL, ANCHOR, RULES, SCENARIO, WARHEAD, ION, IMPACT, BULLET = (
    MEM + offset for offset in (0, 0x200, 0x1000, 0x4000, 0x6000, 0x7000, 0x8000, 0x9000)
)
MAP, DUMMY = 0x87F7E8, 0xABDC50
SP = STACK_BASE + STACK_SIZE - 0x2000
BRIDGE_BASE, WOOD_BASE, MIDDLE1, MIDDLE2 = 1000, 2000, 20, 40
BLOCK_CALLS = {0x48A00E: 'A', 0x48A032: 'A', 0x48A192: 'B',
               0x48A1B6: 'B', 0x48A25A: 'C', 0x48A2B4: 'D'}
DRIVERS = {0x587180, 0x57BAA0, 0x57CCF0}


def words(*values):
    return struct.pack('<' + 'I' * len(values), *(value & 0xFFFFFFFF for value in values))


def read32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def signed(value):
    return struct.unpack('<i', words(value))[0]


@lru_cache(maxsize=None)
def seed_bytes(seed):
    return seeded_struct(seed)


def call(u, address, this=0, args=()):
    u.mem_write(SP, words(RET_MAGIC, *args))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, this)
    run_checked(u, address, RET_MAGIC, count=20000)
    return u.reg_read(UC_X86_REG_EAX)


def base(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(MEM, 0x200000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    u.mem_write(0x822D80, struct.pack('<H', NATIVE_FPCW))
    u.mem_write(MAP + 0x13C, words(TABLE, 0x40000))
    for pointer, coord in ((CELL, (10, 20)), (ANCHOR, (9, 20))):
        u.mem_write(pointer, words(0x7E4EEC))
        u.mem_write(pointer + 0x24, struct.pack('<hh', *coord))
        u.mem_write(pointer + 0x44, words(-1))
        u.mem_write(TABLE + (coord[1] * 512 + coord[0]) * 4, words(pointer))
    u.mem_write(CELL + 0x2C, words(ANCHOR))
    u.mem_write(CELL + 0x38, words(case.get('tile', -1)))
    u.mem_write(CELL + 0x44, words(case.get('overlay', -1)))
    u.mem_write(CELL + 0x11B, bytes((case.get('level', 0) & 255, case.get('slope', 0))))
    u.mem_write(CELL + 0x140, words(case.get('flags', 0x100)))
    u.mem_write(ANCHOR + 0x44, words(case.get('anchor_overlay', 0x18)))
    u.mem_write(DUMMY, bytes(0x200))
    u.mem_write(DUMMY + 0x38, words(-1))
    u.mem_write(DUMMY + 0x44, words(-1))
    u.mem_write(0xAA0E28, words(case.get('bridge_base', BRIDGE_BASE)))
    u.mem_write(0xABAD1C, words(case.get('wood_base', WOOD_BASE)))
    u.mem_write(0xABAD30, words(MIDDLE1))
    u.mem_write(0xAA1028, words(MIDDLE2))
    # The process supplies104; execute its original deck-offset initializer.
    u.mem_write(0x89E870, words(104))
    call(u, 0x489100)
    assert read32(u, 0x89E864) == 416
    return u


def execute(case):
    u = base(case)
    seed = case.get('seed', 31)
    u.mem_write(SCENARIO, words(0x8000 if case.get('destroyable', True) else 0))
    u.mem_write(SCENARIO + 0x218, seed_bytes(seed))
    u.mem_write(0xA8B230, words(SCENARIO))
    u.mem_write(0x8871E0, words(RULES))
    u.mem_write(RULES + 0x1740, words(case.get('strength', 1500)))
    u.mem_write(RULES + 0xFF0, words(WARHEAD if case.get('ion', False) else ION))
    u.mem_write(WARHEAD + 0x144, bytes((case.get('wall', True),)))
    u.mem_write(IMPACT, words(10 * 256 + 128, 20 * 256 + 128, case.get('impact_z', 416)))
    u.mem_write(SP + 0x18, struct.pack('<hh', 10, 20))
    u.mem_write(SP + 0x24, words(case.get('damage', 2000)))
    u.mem_write(SP + 0x28, words(IMPACT))
    u.mem_write(SP + 0x1000 + 0xC, words(WARHEAD))
    u.reg_write(UC_X86_REG_EBP, SP + 0x1000)
    u.reg_write(UC_X86_REG_EBX, WARHEAD)
    u.reg_write(UC_X86_REG_ESP, SP)
    events, ranged, visits = [], [], []
    active_block = None
    block_counts = dict(A=0, B=0, C=0, D=0)
    raw_draw_count = 0
    range_return = None

    def finish(cleanup, result=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        destination = read32(u, sp)
        u.reg_write(UC_X86_REG_EAX, result)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
        u.reg_write(UC_X86_REG_EIP, destination)

    def observe(uc, address, _size, _data):
        nonlocal active_block, range_return, raw_draw_count
        if address in (0x489EDB, 0x48A0A5, 0x48A214, 0x48A26A):
            active_block = {0x489EDB: 'A', 0x48A0A5: 'B', 0x48A214: 'C', 0x48A26A: 'D'}[address]
        if address == 0x5657A0:
            coord = struct.unpack('<hh', uc.mem_read(read32(uc, uc.reg_read(UC_X86_REG_ESP) + 4), 4))
            events.append(dict(kind='lookup', coord=list(coord)))
        elif address == 0x65C7E0:
            sp = uc.reg_read(UC_X86_REG_ESP)
            range_return = read32(uc, sp)
            ranged.append(dict(block=active_block, low=signed(read32(uc, sp + 4)),
                               high=signed(read32(uc, sp + 8))))
        elif address == 0x65C84B:
            # Original generator transition, including rejection retries.
            raw_draw_count += 1
        elif range_return is not None and address == range_return:
            ranged[-1]['result'] = signed(uc.reg_read(UC_X86_REG_EAX))
            events.append(dict(kind='rng', **ranged[-1]))
            range_return = None
        if address in BLOCK_CALLS:
            active_block = BLOCK_CALLS[address]
        if address in DRIVERS:
            block = active_block
            index = block_counts[block]
            block_counts[block] += 1
            responses = case.get('returns', {}).get(block, [False])
            result = responses[min(index, len(responses) - 1)]
            sp = uc.reg_read(UC_X86_REG_ESP)
            coord = list(struct.unpack('<hh', uc.mem_read(read32(uc, sp + 4), 4)))
            assert coord == [10, 20]
            visits.append(block)
            events.append(dict(kind='driver', block=block, entry=f'{address:08X}',
                               coord=coord, returned=result))
            for mutation in case.get('mutations', []):
                if mutation['after_block'] == block and mutation.get('after_call', 0) == index:
                    pointer = CELL if mutation['target'] == 'cell' else ANCHOR
                    field = mutation['field']
                    offset = {'overlay': 0x44, 'flags': 0x140, 'tile': 0x38, 'level': 0x11B}[field]
                    blob = bytes((mutation['value'] & 255,)) if field == 'level' else words(mutation['value'])
                    uc.mem_write(pointer + offset, blob)
                    events.append(dict(kind='supplied_callback_write', **mutation))
            finish(4, int(result))
        elif address == 0x70D4A0:
            assert uc.reg_read(UC_X86_REG_ECX) == CELL
            events.append(dict(kind='detach', block=active_block))
            finish(0)
        elif address == 0x6D2140:
            sp = uc.reg_read(UC_X86_REG_ESP)
            uc.mem_write(read32(uc, sp + 8), words(0, 0))
            events.append(dict(kind='project', block=active_block))
            finish(8)
        elif address == 0x6D2790:
            sp = uc.reg_read(UC_X86_REG_ESP)
            events.append(dict(kind='dirty_rect', block=active_block,
                               rect=list(struct.unpack('<4i', uc.mem_read(sp + 4, 16)))))
            finish(20)

    hook = u.hook_add(UC_HOOK_CODE, observe)
    run_checked(u, 0x489E87, 0x48A2C4, count=100000, required_addresses=(0x5657A0,))
    u.hook_del(hook)
    state = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    next_values = [call(u, 0x65C780, this=SCENARIO + 0x218) for _ in range(4)]
    return dict(input=case, driver_calls=visits, ranged_calls=ranged,
                raw_draw_count=raw_draw_count, rng_indices=list(struct.unpack_from('<2I', state, 4)),
                next_rng=next_values, events=events,
                final=dict(overlay=signed(read32(u, CELL + 0x44)),
                           flags=read32(u, CELL + 0x140),
                           anchor_overlay=signed(read32(u, ANCHOR + 0x44))))


def specifications():
    result = []
    def add(name, **kwargs):
        result.append(dict(name=name, **kwargs))
    for anchor in (0x18, 0x19, 0xED, 0xEE, 0x17, 0x1A, -1):
        add(f'anchor_{anchor}', anchor_overlay=anchor)
    for overlay in (0x18, 0x19, 0xED, 0xEE, -1):
        add(f'self_{overlay}', flags=0x180, overlay=overlay, anchor_overlay=0x17)
    for level in (-128, -1, 0, 2, 127):
        for delta in (207, 208, 209, 416, 519, 520, 521):
            for anchor in (0x18, 0xED):
                add(f'z_{level}_{delta}_{anchor}', level=level, impact_z=level * 104 + delta,
                    anchor_overlay=anchor)
    for family, family_base in (('concrete', BRIDGE_BASE), ('wood', WOOD_BASE)):
        for middle in (MIDDLE1, MIDDLE2):
            for offset in (-1, 0, 1, 2, 3, 4):
                add(f'tile_{family}_{middle}_{offset}', tile=family_base + middle + offset - 1,
                    flags=0, impact_z=-99999, anchor_overlay=-1)
    for overlay in (0x49, 0x4A, 0x63, 0x64, 0xCC, 0xCD, 0xE6, 0xE7):
        add(f'direct_{overlay}', flags=0, overlay=overlay)
    for flags in (0, 0x80, 0x100, 0x180, 0x500, 0x1000):
        add(f'flag_{flags}', flags=flags, overlay=0x18, anchor_overlay=0xED)
    add('wall_false', wall=False)
    add('destroyable_false', destroyable=False)
    add('ion_all_fail', ion=True)
    add('ion_third_success', ion=True, returns={'A': [False, False, True]})
    add('ion_first_success', ion=True, returns={'A': [True]})
    add('one_range', strength=1, damage=2)
    for strength in (-32769, -1, 0, 1, 65535, 65536, 2147483647):
        add(f'strength_{strength}', strength=strength, damage=2147483647)
    for damage in (-2147483648, -1, 0, 1, 65535, 65536, 2147483647):
        add(f'damage_{damage}', damage=damage)
    # Both family tile bands deliberately overlap to exercise independent blocks.
    add('both_families', flags=0, tile=BRIDGE_BASE+MIDDLE1-1, wood_base=BRIDGE_BASE,
        returns={'A': [True], 'B': [True]})
    add('structural_no_tile_a_then_direct', overlay=0xCD,
        returns={'A': [True], 'D': [True]})
    add('callback_changes_anchor_for_b', returns={'A': [True], 'B': [True]},
        mutations=[dict(after_block='A', target='anchor', field='overlay', value=0xED)])
    add('callback_removes_structural_before_b', returns={'A': [True]}, mutations=[
        dict(after_block='A', target='anchor', field='overlay', value=0xED),
        dict(after_block='A', target='cell', field='flags', value=0),
        dict(after_block='A', target='cell', field='level', value=127)])
    add('callback_publishes_low_direct', returns={'A': [True], 'C': [True]},
        mutations=[dict(after_block='A', target='cell', field='overlay', value=0x4A)])
    add('callback_low_direct_becomes_high', flags=0, overlay=0x4A,
        returns={'C': [True], 'D': [True]},
        mutations=[dict(after_block='C', target='cell', field='overlay', value=0xCD)])
    # Strict signed damage > roll, capture the original roll first.
    first = execute(dict(name='roll_probe'))['ranged_calls'][0]['result']
    for damage in (first - 1, first, first + 1):
        add(f'damage_roll_boundary_{damage}', damage=damage)
    return result


def aim_cases():
    result = []
    matrices = slope_matrices()
    for level, slope, flags in ((0, 0, 0), (0, 0, 0x100), (2, 0, 0x100),
                                (-1, 0, 0x100), (-128, 0, 0x100), (127, 0, 0x100),
                                (2, 1, 0), (2, 1, 0x100), (2, 0, 0x400), (2, 0, 0x1100)):
        case = dict(level=level, slope=slope, flags=flags)
        u = base(case)
        u.mem_write(0x89E7C0, words(104))
        for index, matrix in enumerate(matrices):
            u.mem_write(0xB45188 + 48 * index, struct.pack('<12I', *matrix))
        call(u, 0x47B2C0)
        assert read32(u, 0x89E7B4) == 416
        # Ordinary cell-target branch after object-target pointer proved null.
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EDI, CELL)
        run_checked(u, 0x6FE1F6, 0x6FE21F, count=10000,
                    required_addresses=(0x486890, 0x486840, 0x47B3A0))
        launch = list(struct.unpack('<3i', u.mem_read(SP + 0x88, 12)))
        u.mem_write(BULLET + 0x10C, words(CELL))
        u.reg_write(UC_X86_REG_EBX, BULLET)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x4686FA, 0x46872E, count=10000,
                    required_addresses=(0x486890, 0x486840, 0x47B3A0))
        frozen = list(struct.unpack('<3i', u.mem_read(BULLET + 0x140, 12)))
        assert frozen == launch
        result.append(dict(input=case, fireat_aim=launch, bullet_frozen_target=frozen))
    return result


def ladder_cases():
    result = []
    projectile_type = MEM + 0xA000
    for dummy in (False, True):
        for delta in (0, 31, 32, 41, 42, 416, 417):
            u = base(dict(level=2, flags=0x100))
            u.mem_write(0x89E7C0, words(104))
            call(u, 0x47B2C0)
            target = DUMMY if dummy else CELL
            if dummy:
                u.mem_write(DUMMY, bytes(u.mem_read(CELL, 0x200)))
            location = [2688, 5248, 208 + delta]
            u.mem_write(BULLET, words(0x7E46E4))
            u.mem_write(BULLET + 0x9C, words(*location))
            u.mem_write(BULLET + 0xAC, words(projectile_type))
            u.mem_write(BULLET + 0x10C, words(target))
            u.mem_write(BULLET + 0x128, words(WARHEAD))
            u.mem_write(projectile_type + 0x29B, b'\x01')
            u.mem_write(projectile_type + 0x2AC, words(1))
            u.mem_write(SP, words(RET_MAGIC, 1))
            u.reg_write(UC_X86_REG_ECX, BULLET)
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x468D80, 0x4690B0, count=10000,
                        required_addresses=(0x486840, 0x5F6360))
            argument = read32(u, u.reg_read(UC_X86_REG_ESP) + 4)
            result.append(dict(dummy=dummy, coord=[10, 20], level=2, slope=0, flags=0x100,
                               arcing=1, cluster=1, impact_flag=1, inaccurate=0,
                               airburst=0, rot=0, em_effect=0, reference=[0, 0, 0], location=location,
                               impact=list(struct.unpack('<3i', u.mem_read(argument, 12)))))
    return result


def strength_cases():
    from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, SP as reader_sp
    result = []
    fixture = Fixture()
    u = fixture.u
    u.reg_write(UC_X86_REG_ESI, TYPE)
    run_checked(u, 0x6675DA, 0x6675E4, count=10)
    constructor_default = signed(read32(u, TYPE + 0x1740))
    for initial in (constructor_default, 1500, -7):
        for raw in (None, '', '1500', '0', '-1', '-32769', '65535', '65536',
                    '2147483647', '-2147483648', '2147483648', '4294967295',
                    '$10000', '10000h', '12junk', 'junk'):
            fixture.ini(0x83AD90, raw)
            fixture.write(INI + 4, 0x839E8C)
            fixture.write(TYPE + 0x1740, initial)
            u.reg_write(UC_X86_REG_ESP, reader_sp)
            u.reg_write(UC_X86_REG_ESI, TYPE)
            u.reg_write(UC_X86_REG_EDI, INI)
            run_checked(u, 0x66CD66, 0x66CD8C, count=10000,
                        required_addresses=(0x5276D0, 0x66CD86))
            assert u.reg_read(UC_X86_REG_ESP) == reader_sp
            result.append(dict(initial=initial, raw=raw, stored=signed(read32(u, TYPE + 0x1740))))
    return dict(constructor_default=constructor_default, cases=result)


def selector_cases():
    outcomes = {0x576BA0: 'A', 0x571490: 'B', 0x57BAA0: 'C', 0x57CCF0: 'D', 0x587388: None}
    unique = {}
    for case in specifications():
        fields = {key: value for key, value in case.items()
                  if key in ('flags', 'tile', 'overlay', 'level', 'anchor_overlay', 'bridge_base', 'wood_base')}
        key = tuple(sorted(fields.items()))
        unique.setdefault(key, dict(name=case['name'], **fields))
    result = []
    for case in unique.values():
        u = base(case)
        u.mem_write(IMPACT, struct.pack('<hh', 10, 20))
        u.mem_write(SP + 0x14, words(MAP))
        u.mem_write(SP + 0x30, words(IMPACT))
        u.reg_write(UC_X86_REG_EBP, MAP)
        u.reg_write(UC_X86_REG_ESP, SP)
        end = run_checked(u, 0x5871A5, tuple(outcomes), count=10000)
        if end != 0x587388:
            assert u.reg_read(UC_X86_REG_ECX) == MAP
            coord = read32(u, u.reg_read(UC_X86_REG_ESP) + 4)
            assert list(struct.unpack('<hh', u.mem_read(coord, 4))) == [10, 20]
        result.append(dict(input=case, driver=outcomes[end], endpoint=f'{end:08X}'))
    return result


def generate():
    return dict(cases=[execute(case) for case in specifications()], aim_cases=aim_cases(),
                ladder_cases=ladder_cases(), strength=strength_cases(), selector_cases=selector_cases())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original489E87..48A2C4 bridge admission, four independent blocks, real lookup/RNG; ordinary cell-target FireAt/BulletFire and actual Cell impact ladder; BridgeStrength constructor/store/INI reader',
        assumptions=[
            'Interior state follows ordinary nonzero area damage, after receiver and Rocker phases; those phases and their mutations are excluded',
            'Scenario flag, Wall byte, signed damage/BridgeStrength, tile-family globals, explicit impact/anchor cells, original packed coordinate and lepton Z are supplied',
            'OriginalRandomSeed supplies Scenario+218; every RandomRanged instruction and rejection retry executes; next four original RandomNext values record continuation',
            'Actual GetCellClass5657A0 reads a populated fixed-stride table; cells and retained anchor relationships are synthetic accepted inputs, not native-loaded map witnesses',
            'Synthetic callback mutations establish dispatcher continuation reads, not stock driver/callback reachability or correctness',
            'Coordinate fixtures execute originalCellGetTargetCoords/GetCoords/ComputeGroundHeight with supplied104 and original deck initializers; only listed scalar/slope cases are covered',
            'Impact-ladder cases use actualCell and Bullet vtables, flat level2 structural cells, arcing1/cluster1, zero other flags and stop at first DetonateAtCoord; duplicated Cell scalar state supplies the shared dummy cases',
            'OriginalBridgeStrength constructor store and CombatDamage read/store run with supplied cached INI indexes built using original key CRC; nativeReadInt executes unchanged',
            'Inner-selector cases start5871A5 after the dirty vector clear, with EBP=Map and original stack locals; actual lookup/selection runs to the selected leaf entry or no-match tail, before any driver executes',
            'Signed out-of-retail-range strength/damage/level cases characterize accepted arithmetic domain, not retail map reachability',
        ], substitutions=[
            'Drivers587180/57BAA0/57CCF0 return supplied booleans and apply only declared callback writes; their gameplay bodies are excluded',
            'Detach70D4A0 and screenDirty6D2790 are recorded sinks; CoordsToClient6D2140 writes0,0 to its output; no other calls are replaced',
        ], entry_points={'area_bridge_blocks':0x489E87, 'area_deck_initializer':0x489100,
                         'lookup':0x5657A0, 'random_seed':0x65C6D0, 'random_ranged':0x65C7E0,
                         'fireat_cell_aim':0x6FE1F6, 'bullet_target_copy':0x4686FA,
                         'cell_target_coords':0x486890, 'cell_deck_initializer':0x47B2C0,
                         'impact_ladder':0x468D80, 'strength_default_store':0x6675DA,
                         'strength_read':0x66CD66, 'integer_reader':0x5276D0,
                         'inner_selector':0x5871A5}))
