"""Original Unit PerCell common overlay tail, with explicit receiver stubs.

Executes unmodified 73AFD4..73B074. Map, ability, GetCoords, sound and damage
callees are supplied receiver boundaries; all gates, call sites, x87 arithmetic
and owner stores execute original instructions. This is not full PerCell, wall
damage, ability, map, audio or rocking integration parity.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

ENTRY, END = 0x73AFD4, 0x73B074
MAP, MAP_LOOKUP, ABILITY = 0x87F7E8, 0x5657A0, 0x70D0D0
SOUND, DAMAGE, UNIT_VTABLE = 0x7509E0, 0x480CB0, 0x7F5C70
OVERLAY_REGISTRY, ADDEND = 0xA83D84, 0x7F4E38
OWNER, TYPE, CELL, OVERLAY, REGISTRY = (
    SCRATCH + n for n in (0x1000, 0x2000, 0x4000, 0x5000, 0x6000)
)
SP = STACK_BASE + STACK_SIZE - 0x1000
VELOCITY, CRUSH_FLAG = 0x334, 0x6B5
ADDEND_BITS = 0x3CA3D70A
FPCW = 0x0E7F


def unsigned(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def coord(u, address):
    return list(struct.unpack('<iii', u.mem_read(address, 12)))


def case(name, **changes):
    row = dict(name=name, reason=2, regular_crusher=1, ability_17=0,
               overlay_index=3, overlay_crushable=1, overlay_wall=1,
               type_movement_zone=0, sound_index=17, damage_return=1,
               entry_cell=[10, 11], live_coords=[2688, 3456, -731],
               velocity_bits='00000000', crush_flag=1)
    row.update(changes)
    return row


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.reg_write(UC_X86_REG_FPCW, FPCW)
    original_code = bytes(u.mem_read(ENTRY, END - ENTRY))
    original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
    original_addend = bytes(u.mem_read(ADDEND, 4))
    assert unsigned(u, ADDEND) == ADDEND_BITS
    get_coords = unsigned(u, UNIT_VTABLE + 0x48)
    u.mem_write(OWNER, dwords(UNIT_VTABLE))
    u.mem_write(OWNER + 0x6C4, dwords(TYPE))
    u.mem_write(OWNER + 0x9C, dwords(*row['live_coords']))
    u.mem_write(OWNER + VELOCITY, dwords(int(row['velocity_bits'], 16)))
    u.mem_write(OWNER + CRUSH_FLAG, bytes((row['crush_flag'],)))
    u.mem_write(TYPE + 0xD28, bytes((row['regular_crusher'],)))
    u.mem_write(TYPE + 0x5B4, dwords(row['type_movement_zone']))
    u.mem_write(CELL + 0x44, dwords(row['overlay_index']))
    u.mem_write(OVERLAY + 0x22D, bytes((row['overlay_crushable'],)))
    u.mem_write(OVERLAY + 0x2A8, bytes((row['overlay_wall'],)))
    u.mem_write(OVERLAY + 0x1F0, dwords(row['sound_index']))
    u.mem_write(OVERLAY_REGISTRY, dwords(REGISTRY))
    if row['overlay_index'] != -1:
        u.mem_write(REGISTRY + row['overlay_index'] * 4, dwords(OVERLAY))
    u.mem_write(SP + 0x1C, struct.pack('<hh', *row['entry_cell']))
    # Original argument slot at this interior boundary. The tail must not read
    # it; paired 0/2 rows exercise identical supplied post-prefix state.
    u.mem_write(SP + 0x44, dwords(row['reason']))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EBP, OWNER)
    before_owner = bytearray(u.mem_read(OWNER, 0x800))
    before_type = bytes(u.mem_read(TYPE, 0x1000))
    before_cell = bytes(u.mem_read(CELL, 0x200))
    before_overlay = bytes(u.mem_read(OVERLAY, 0x300))
    events, visits, owner_writes = [], [], []

    def return_from_stub(uc, result, expected_return, argument_bytes):
        esp = uc.reg_read(UC_X86_REG_ESP)
        assert unsigned(uc, esp) == expected_return
        uc.reg_write(UC_X86_REG_EAX, result & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + argument_bytes)
        uc.reg_write(UC_X86_REG_EIP, expected_return)

    def observe(uc, address, _size, _data):
        if ENTRY <= address < END:
            visits.append(address)
        esp = uc.reg_read(UC_X86_REG_ESP)
        ecx = uc.reg_read(UC_X86_REG_ECX)
        if address == MAP_LOOKUP:
            assert ecx == MAP
            pointer = unsigned(uc, esp + 4)
            assert pointer == SP + 0x1C
            sampled = list(struct.unpack('<hh', uc.mem_read(pointer, 4)))
            assert sampled == row['entry_cell']
            events.append(dict(kind='map', packed_cell=sampled))
            return_from_stub(uc, CELL, 0x73AFE3, 4)
        elif address == ABILITY:
            assert ecx == OWNER
            ability = signed(uc, esp + 4)
            assert ability == 17
            events.append(dict(kind='ability', ability=ability,
                               returned_al=row['ability_17']))
            return_from_stub(uc, row['ability_17'], 0x73AFFE, 4)
        elif address == get_coords:
            assert ecx == OWNER
            pointer = unsigned(uc, esp + 4)
            assert pointer == SP + 0x30
            sampled = coord(uc, OWNER + 0x9C)
            uc.mem_write(pointer, dwords(*sampled))
            events.append(dict(kind='get_coords', coords=sampled))
            return_from_stub(uc, pointer, 0x73B045, 4)
        elif address == SOUND:
            sampled = coord(uc, uc.reg_read(UC_X86_REG_EDX))
            sound_index = struct.unpack('<i', struct.pack('<I', ecx))[0]
            extra = signed(uc, esp + 4)
            assert sound_index == row['sound_index'] and extra == 0
            assert sampled == row['live_coords']
            events.append(dict(kind='sound', sound_index=sound_index,
                               coords=sampled, extra=extra))
            return_from_stub(uc, 0, 0x73B052, 4)
        elif address == DAMAGE:
            assert ecx == CELL
            damage = signed(uc, esp + 4)
            assert damage == -1
            events.append(dict(kind='damage', packed_cell=row['entry_cell'],
                               damage=damage, returned_eax=row['damage_return'],
                               velocity_before_bits=f'{unsigned(uc, OWNER + VELOCITY):08x}',
                               crush_flag_before=uc.mem_read(OWNER + CRUSH_FLAG, 1)[0]))
            # Intentionally no wall mutation/side effects: prove this caller's
            # continuation independently of the real receiver's implementation.
            return_from_stub(uc, row['damage_return'], 0x73B05B, 4)

    def observe_write(uc, _access, address, size, value, _data):
        if OWNER <= address < OWNER + 0x800:
            write = dict(offset=address - OWNER, size=size,
                         bits=f'{value & ((1 << (size * 8)) - 1):0{size * 2}x}',
                         instruction=f'{uc.reg_read(UC_X86_REG_EIP):08X}')
            owner_writes.append(write)
            events.append(dict(kind='owner_write', **write))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    run_checked(u, ENTRY, END, count=200,
                required_addresses=(ENTRY, MAP_LOOKUP))
    assert u.reg_read(UC_X86_REG_ESP) == SP
    assert u.reg_read(UC_X86_REG_FPCW) == FPCW
    assert u.reg_read(UC_X86_REG_EIP) == END
    applied = 0x73B05B in visits
    expected_kinds = ['map']
    if row['regular_crusher'] == 0:
        expected_kinds.append('ability')
    if applied:
        expected_kinds += ['get_coords', 'sound', 'damage',
                           'owner_write', 'owner_write']
        assert [x['offset'] for x in owner_writes] == [CRUSH_FLAG, VELOCITY]
        assert [(x['size'], x['instruction']) for x in owner_writes] == [
            (1, '0073B067'), (4, '0073B06E')]
        assert owner_writes[0]['bits'] == '00'
        assert all(a in visits for a in (0x73B042, 0x73B04D, 0x73B056,
                                        0x73B05B, 0x73B061, 0x73B067, 0x73B06E))
    else:
        assert owner_writes == []
    assert [e['kind'] for e in events] == expected_kinds
    after_owner = bytearray(u.mem_read(OWNER, 0x800))
    velocity_bits = f'{unsigned(u, OWNER + VELOCITY):08x}'
    flag = u.mem_read(OWNER + CRUSH_FLAG, 1)[0]
    after_owner[VELOCITY:VELOCITY + 4] = before_owner[VELOCITY:VELOCITY + 4]
    after_owner[CRUSH_FLAG] = before_owner[CRUSH_FLAG]
    assert after_owner == before_owner
    assert bytes(u.mem_read(TYPE, 0x1000)) == before_type
    assert bytes(u.mem_read(CELL, 0x200)) == before_cell
    assert bytes(u.mem_read(OVERLAY, 0x300)) == before_overlay
    assert bytes(u.mem_read(ENTRY, END - ENTRY)) == original_code
    assert bytes(u.mem_read(UNIT_VTABLE, 0x600)) == original_vtable
    assert bytes(u.mem_read(ADDEND, 4)) == original_addend
    assert signed(u, SP + 0x44) == row['reason']
    return dict(input=row, output=dict(
        applied=applied, velocity_bits=velocity_bits, crush_flag=flag,
        events=events, owner_writes=owner_writes,
        boundary='before_full_cell_crush_tail', boundary_address=f'{END:08X}',
        original_code_vtable_addend_unchanged=True,
        other_owner_type_cell_overlay_unchanged=True))


def generate():
    rows = []
    # Each gate dimension is crossed with both reasons. A reason label alone
    # is not a claim that its upstream mission receivers execute here.
    for n, (reason, crusher, ability, present, crushable, wall, zone) in enumerate(
            product((0, 2), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 12, 13))):
        rows.append(case(f'gate_{n:03}', reason=reason, regular_crusher=crusher,
                         ability_17=ability, overlay_index=3 if present else -1,
                         overlay_crushable=crushable, overlay_wall=wall,
                         type_movement_zone=zone, damage_return=0,
                         velocity_bits='bdcccccd', crush_flag=1))
    velocities = (
        '00000000', '80000000', '00000001', '80000001',
        '007fffff', '807fffff', '00800000', '80800000',
        '3ca3d709', '3ca3d70a', '3ca3d70b',
        'bca3d709', 'bca3d70a', 'bca3d70b',
        '3dcccccd', 'bdcccccd', '3f800000', 'bf800000',
        '3f800001', 'bf800001', '47800000', 'c7800000',
        '7f7fffff', 'ff7fffff',
    )
    for bits, reason, damage_return in product(velocities, (0, 2), (0, 1)):
        rows.append(case(f'numeric_{bits}_r{reason}_d{damage_return}',
                         reason=reason, velocity_bits=bits,
                         damage_return=damage_return,
                         # Deliberately non-Wall: the native damage body rejects
                         # this, but neither sound nor owner writes are gated.
                         overlay_wall=0, crush_flag=0 if damage_return else 255,
                         entry_cell=[-7, 13], live_coords=[4097, -513, 731]))
    rows += [
        case('ability_truthy_byte', regular_crusher=0, ability_17=255,
             overlay_crushable=0, type_movement_zone=12),
        case('regular_truthy_byte', regular_crusher=2, ability_17=0),
        case('negative_zone_reject', overlay_crushable=0, type_movement_zone=-1),
        case('zone_11_reject', overlay_crushable=0, type_movement_zone=11),
        case('sound_invalid_still_called', sound_index=-1, damage_return=0),
        case('damage_negative_return_ignored', damage_return=-1),
        case('retained_cell_live_coords_a', entry_cell=[4, 9], live_coords=[1, 2, 3]),
        case('retained_cell_live_coords_b', entry_cell=[4, 9], live_coords=[-769, 65535, -731]),
    ]
    result = [execute(row) for row in rows]
    assert len(result) == 296
    by_name = {row['input']['name']: row for row in result}
    assert len(by_name) == len(result)
    # Comparing paired native results is not a Python arithmetic model.
    for bits in velocities:
        baseline = by_name[f'numeric_{bits}_r0_d0']['output']
        assert baseline['applied'] and baseline['crush_flag'] == 0
        for reason, damage_return in product((0, 2), (0, 1)):
            output = by_name[f'numeric_{bits}_r{reason}_d{damage_return}']['output']
            assert output['applied'] and output['crush_flag'] == 0
            assert output['velocity_bits'] == baseline['velocity_bits']
    # Finite branch matrix; paired boundary states differ only in reason.
    assert sum(row['output']['applied'] for row in result[:192]) == 42
    for index in range(96):
        assert result[index]['output'] == result[index + 96]['output']
    return result


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    get_coords = unsigned(u, UNIT_VTABLE + 0x48)
    result = provenance(
        scope='296 executions of original Unit PerCell common overlay tail 73AFD4..73B074: native capability/overlay/zone branches, receiver call order and binary32 owner stores; explicit external receiver stubs',
        assumptions=[
            'Pinned retail gamemd.exe instructions, Unit vtable7F5C70 and addend7F4E38 are never patched. The original virtual+48 slot selects the observed GetCoords stub address. Code, vtable and addend bytes are checked unchanged per row',
            'Interior boundary EBP=owner and ESP=live local-frame base; supplied stack+1C is the signed packed entry cell captured upstream at739EE2, while supplied owner+9C is the current XYZ exposed by the GetCoords stub. Upstream capture and callbacks are not executed',
            'Both original reason argument values0/2 are seeded at stack+44. The selected common tail does not inspect that slot. This establishes reason independence for identical boundary state, not reason-specific prefix reachability or whole PerCell equivalence',
            '192 gate rows cross reasons0/2, Crusher0/1, ability return0/1, overlay absent/present, Crushable0/1, Wall0/1 and type movement zone0/12/13. Eight additional rows cover truthy bytes, zone-1/11, invalid sound index, negative damage return and two distinct live XYZ samples with the same retained cell',
            '96 numeric rows cross24 binary32 patterns with reasons0/2 and damage returns0/1. Patterns include signed zero, signed min/max subnormals, min normals, addend and adjacent ULPs with both signs, fractions, unit magnitudes, large values and max finite. Nonfinite input is excluded',
            'x87 FPCW0E7F (53-bit precision, truncate) is supplied from the established project native runtime contract. Original FLD/FADD/FSTP execute and outputs are raw binary32 bits. No Python float arithmetic computes expected results',
            'Owner writes are observed at original73B067 then73B06E; damage is called before either write. Non-Wall numeric rows deliberately supply damage return0/1 to isolate the unconditional continuation. A return1 for non-Wall is a counterfactual receiver result, not a claim about480CB0',
            'Stops before73B074 full-cell crush and Foot PerCell. Wall mutation, recursion/RNG, pointer expiry, lifecycle, map/dummy rules, audio playback, ability resolution, upstream MCV/missions, rocking update and Rust integration are outside this corpus',
        ],
        substitutions=[
            'Map5657A0: assert ECX=87F7E8 and argument=&retained stack cell; record signed cell, return fixture Cell pointer with RET4 semantics; no native lookup/dummy behavior executes',
            'HasWeaponAbility70D0D0: assert owner and literal argument17; record call and return supplied AL with RET4 semantics; rank/type ability bodies are excluded',
            f'GetCoords{get_coords:08X}: called through untouched original Unit vtable+48; copy supplied live owner+9C XYZ to caller output buffer and return it with RET4 semantics',
            'Sound7509E0: record actual ECX sound index, EDX XYZ and stack argument0; return0 with RET4 semantics; no audio receiver, RNG or playback executes, including invalid sound index-1',
            'Damage480CB0: assert retained Cell receiver and literal damage-1; record pre-write owner state and return supplied EAX with RET4 semantics; no overlay/owner mutation or damage body executes',
        ],
        entry_points=dict(entry=ENTRY, before_full_cell_crush=END,
                          map_lookup=MAP_LOOKUP, ability_17=ABILITY,
                          get_coords=get_coords, sound=SOUND, damage=DAMAGE,
                          original_addend=ADDEND, flag_store=0x73B067,
                          velocity_store=0x73B06E),
    )
    result['x87_control_word'] = f'{FPCW:04X}'
    result['addend_binary32_bits'] = f'{unsigned(u, ADDEND):08X}'
    result['original_code_range_sha256'] = {
        f'{ENTRY:08X}..{END:08X}': hashlib.sha256(bytes(u.mem_read(ENTRY, END - ENTRY))).hexdigest(),
    }
    return result


def drive_crush_history(row):
    """Retain one Foot across original crush, speed, rocking and reset bodies."""
    from tools.spatial_oracle.locomotor_force_track import OriginalForceTrack, FOOT, LOCO, SP as FRAME_SP
    from tools.spatial_oracle.track_speed_native import seed_getter_state, TYPE as SPEED_TYPE, HOUSE, HOUSE_TYPE, RULES
    from unicorn.x86_const import UC_X86_REG_EBX

    native = OriginalForceTrack({})
    u = native.uc
    seed_getter_state(u, dict(raw=15, rank=0, crate=1, applied=1), FOOT,
                      SPEED_TYPE, HOUSE, HOUSE_TYPE, RULES)
    cell = native.cell_address(10, 10)
    overlay, table, other = SCRATCH + 0xC000, SCRATCH + 0xD000, SCRATCH + 0xE000
    u.mem_write(FOOT + 0x9C, dwords(2688, 2688, 0))
    u.mem_write(FOOT + 0x14, dwords(5))
    u.mem_write(FOOT + 0x6C, dwords(300))
    u.mem_write(FOOT + 0x5E0, dwords(-1))
    u.mem_write(FOOT + CRUSH_FLAG, bytes((row['initial_flag'],)))
    initial_rocking = [0, 0x3E99999A, 0, 0x3D23D70A]
    u.mem_write(FOOT + 0x328, dwords(*initial_rocking))
    u.mem_write(SPEED_TYPE + 0xA0, dwords(300))
    u.mem_write(SPEED_TYPE + 0x5B4, dwords(row['zone']))
    for offset, key in ((0xD28, 'crusher'), (0xD2B, 'tilts'), (0xDBD, 'accelerates')):
        u.mem_write(SPEED_TYPE + offset, bytes((row[key],)))
    u.mem_write(SPEED_TYPE + 0x2F8, dwords(500))
    u.mem_write(SPEED_TYPE + 0x300, struct.pack('<d', 0.002))
    u.mem_write(SPEED_TYPE + 0x308, struct.pack('<d', 0.03))
    u.mem_write(LOCO + 0x34, dwords(5248, 2688, 0))
    u.mem_write(LOCO + 0x50, struct.pack('<d', 1))
    u.mem_write(LOCO + 0x58, dwords(0))
    u.mem_write(LOCO + 0x64, bytes((row['straight'],)))
    u.mem_write(0xA8E9A0, bytes((row['game_active'],)))
    # GetUnit(false) must traverse the original selected list, including self.
    if row['occupant'] == 'self':
        u.mem_write(cell + 0xE4, dwords(FOOT))
    elif row['occupant'] == 'deck':
        u.mem_write(cell + 0xE8, dwords(FOOT))
    elif row['occupant'] == 'infantry':
        u.mem_write(other, dwords(0x7EB058))  # original Infantry vtable
        u.mem_write(cell + 0xE4, dwords(other))
    u.mem_write(0xA83D84, dwords(table))
    u.mem_write(table, dwords(overlay))
    u.mem_write(cell + 0x44, dwords(0 if row['overlay'] else -1))
    u.mem_write(overlay + 0x22D, bytes((row['crushable'],)))
    u.mem_write(overlay + 0x2A8, bytes((row['wall'],)))
    bodies = ((0x4B1999, 0x4B1A77), (0x4B0F20, 0x4B1297),
              (ENTRY, END), (0x70B570, 0x70BCAA))
    originals = [bytes(u.mem_read(a, b - a)) for a, b in bodies]
    original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
    stores, calls = [], []

    def observe_write(uc, _access, address, size, value, _data):
        if address in (FOOT + CRUSH_FLAG, FOOT + VELOCITY):
            stores.append(dict(instruction=f'{uc.reg_read(UC_X86_REG_EIP):08X}',
                               offset=address - FOOT, size=size,
                               bits=f'{value & ((1 << (size * 8)) - 1):0{size * 2}x}'))

    def observe_call(uc, pc, _size, _data):
        assert pc not in (0x65C780, 0x65C7E0), 'unexpected RNG draw'
        if pc not in (SOUND, DAMAGE):
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        calls.append(dict(entry=f'{pc:08X}', arg=signed(uc, sp + 4)))
        # These external effects are already independently tested by the
        # existing Unit tail corpus. No wall mutation or sound RNG is claimed.
        uc.reg_write(UC_X86_REG_EIP, unsigned(uc, sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 8)
        uc.reg_write(UC_X86_REG_EAX, 0)

    def state():
        return dict(crush_flag=u.mem_read(FOOT + CRUSH_FLAG, 1)[0],
                    rocking_bits=[f'{native.unsigned(FOOT + offset):08x}'
                                  for offset in (0x328, 0x32C, 0x330, 0x334)],
                    target_bits=native.double_bits(LOCO + 0x50),
                    applied_bits=native.double_bits(FOOT + 0x578))

    def prefix():
        u.mem_write(FRAME_SP, dwords(0, 0))
        u.reg_write(UC_X86_REG_ECX, LOCO)
        u.reg_write(UC_X86_REG_ESP, FRAME_SP)
        run_checked(u, 0x4B0F20, 0x4B1297, count=3000,
                    required_addresses=(0x4DB1A0,))
        return state()

    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    u.hook_add(UC_HOOK_CODE, observe_call)
    u.reg_write(UC_X86_REG_EBP, LOCO)
    u.reg_write(UC_X86_REG_EBX, cell)
    u.reg_write(UC_X86_REG_ESP, FRAME_SP)
    run_checked(u, 0x4B1999, 0x4B1A77, count=3000)
    pre_entry = state()
    pre_entry_stores = list(stores)
    speed = prefix()
    native.invoke(0x70B570, FOOT)
    rocking = state()
    u.mem_write(FRAME_SP + 0x1C, struct.pack('<hh', 10, 10))
    u.reg_write(UC_X86_REG_EBP, FOOT)
    u.reg_write(UC_X86_REG_ESP, FRAME_SP)
    run_checked(u, ENTRY, END, count=3000, required_addresses=(MAP_LOOKUP,))
    per_cell = state()
    retained_speed = prefix()
    # Explicit new candidate target, as published by Process_Movement; no
    # path search or fresh admission is executed in this component history.
    u.mem_write(LOCO + 0x50, struct.pack('<d', 1))
    refreshed_speed = prefix()
    assert [bytes(u.mem_read(a, b - a)) for a, b in bodies] == originals
    assert bytes(u.mem_read(UNIT_VTABLE, 0x600)) == original_vtable
    return dict(input=row, initial_rocking_bits=[f'{x:08x}' for x in initial_rocking],
                pre_entry=pre_entry, pre_entry_stores=pre_entry_stores,
                speed=speed, rocking=rocking, per_cell=per_cell,
                retained_speed=retained_speed, refreshed_speed=refreshed_speed,
                calls=calls, stores=stores)


def generate_drive_crush():
    changes = [dict(name='bfrt_wall'), dict(name='bfrt_no_tilt', tilts=False),
               dict(name='normal_tank_wall', zone=0),
               dict(name='not_straight', straight=False),
               dict(name='empty_ground', occupant='none'),
               dict(name='deck_only', occupant='deck'),
               dict(name='infantry_only', occupant='infantry'),
               dict(name='inactive', game_active=False),
               dict(name='zone11', zone=11),
               dict(name='no_overlay', overlay=False),
               dict(name='nonwall_overlay', wall=False, crushable=False),
               dict(name='noncrusher', crusher=False),
               dict(name='nonaccelerating', accelerates=False),
               dict(name='retain_until_overlay', overlay=False, straight=False, initial_flag=True)]
    base = dict(zone=12, crusher=True, tilts=True, accelerates=True, straight=True,
                game_active=True, occupant='self', overlay=True, wall=True,
                crushable=True, initial_flag=False)
    return [drive_crush_history(dict(base, **change)) for change in changes]


def drive_crush_metadata():
    return provenance(
        scope='14 original Drive pre-entry -> speed prefix -> RockingUpdate -> Unit overlay tail -> speed histories in one retained Foot',
        assumptions=[
            'Reuses OriginalForceTrack map/constructor and track_speed_native getter inputs. Supplied healthy Unit, type speed15, crate/house1, far destination, selector0, applied/target1 and four binary32 rocking words. No retail type reader, path admission, paid-point placement or Mark body executes.',
            'Original GetUnit(false) traverses supplied ground/deck lists using original RTTI, including actor itself after a supplied Mark boundary. Original type/ability getters, map lookup, GetCoords, speed setter/getter and RockingUpdate execute. GameActive, straight latch and zone are explicit boundary inputs.',
            'Foot+6B5 is not reset between bodies. New target1 is supplied only for the last speed visit to model the documented next-candidate publication boundary. No numeric expected value is computed in Python.',
            'All three RNG raw/ranged entry calls are rejected. No timer, detach, destruction or rendering comparison is claimed. Existing unit_per_cell_overlay corpus independently covers all overlay-tail gates and binary32 recoil.',
        ],
        substitutions=['Sound7509E0 and CellDamage480CB0 return0 with RET4; record calls without wall mutation, audio or downstream RNG effects.'],
        entry_points=dict(pre_entry=0x4B1999, prefix=0x4B0F20,
                          rocking=0x70B570, unit_overlay=ENTRY))


if __name__ == '__main__':
    if '--drive-crush' in sys.argv:
        finish_vectors(generate_drive_crush,
                       Path(__file__).with_suffix('.drive_crush.json'),
                       provenance=drive_crush_metadata,
                       argv=[arg for arg in sys.argv[1:] if arg != '--drive-crush'])
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
