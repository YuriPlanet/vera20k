"""Whole original SelectAction Super clicks and CanFire component controls.

Uses the existing sidebar VM. Prepared object/rules fields are explicit inputs;
only audio-device emitters and OS timeGetTime supply boundary returns.
"""
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP,
                              UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP)

from tools.native_oracle import STACK_BASE, STACK_SIZE, finish_vectors, provenance, run_checked
from tools.sidebar_oracle.cameo_order import Sidebar, SIDEBAR, STRIPS, item, put32, read
from tools.spatial_oracle.map_queries import dwords

SELECT_ACTION = 0x6AAD00
CAN_FIRE = 0x6CC360
CODE_SPANS = ((SELECT_ACTION, 0xC84), (CAN_FIRE, 0x24))


def execute(case):
    fixture = Sidebar([item('SUPER', kind=31)])
    uc = fixture.u
    control, key = fixture.alloc(0x40), fixture.alloc(4)
    mouse = fixture.alloc(0x100)
    fixture.invoke(0x7B8730, mouse, (), ends=0x7B8856)
    put32(uc, 0x887640, mouse)
    # Mouse5BDA40 installs this original primary vtable at 5BDA4A.
    # Repair/Sell use its +48 cursor reset (5BDA80 -> 5BDC80). Supply
    # the native no-cursor-image state; that leaf returns before drawing.
    put32(uc, SIDEBAR, 0x7E1964)
    uc.mem_write(0xABF2DD, b'\1')
    put32(uc, 0xABF294, 0)
    super_ptr = read(uc, fixture.super_instances)[0]
    type_ptr = fixture.pointers['SUPER']
    fixture.invoke(0x6AACB0, control)
    put32(uc, control + 0x2C, STRIPS[1])
    put32(uc, control + 0x30, 0)
    put32(uc, STRIPS[1] + 0x54, 1)
    uc.mem_write(STRIPS[1] + 0x58, dwords(0, 31, 0, 0))
    put32(uc, super_ptr + 0x28, type_ptr)
    uc.mem_write(super_ptr + 0x6F, bytes([
        case.get('ready', True), case.get('held', False)]))
    uc.mem_write(type_ptr + 0xE5, bytes([case.get('drain', False)]))
    put32(uc, super_ptr + 0x7C, case.get('state', 0))
    uc.mem_write(super_ptr + 0x6D, bytes([case.get('granted', True)]))
    put32(uc, type_ptr + 0xBC, case.get('action', 1))
    put32(uc, type_ptr + 0x98, 0)
    put32(uc, 0x8809A0, case.get('selected', -1))
    put32(uc, fixture.rules + 0x18C, 42)
    put32(uc, 0xA8ECC8, 0)
    for address, value in zip((0x88098C, 0x880990, 0x880994),
                              (super_ptr, type_ptr, 7)):
        put32(uc, address, value)
    uc.mem_write(0x88099A, b'\1\1\1')
    # Sidebar base87F7E8 +11B0/+11B1: Repair4AC8C0 and Sell4AC660.
    # The three bytes above are different pointer modes cleared by Select.
    uc.mem_write(0x880998, bytes([
        case.get('repair', True), case.get('sell', True)]))
    put32(uc, 0xA802C8, 0)
    put32(uc, 0xA802D0, 0)
    emitted = []
    original_code = [bytes(uc.mem_read(a, n)) for a, n in CODE_SPANS]

    def observe(machine, pc, size, _):
        if pc in fixture.factory_queries or pc == 0x4F7870:
            raise RuntimeError('Unexpected substituted factory/CanBuild branch')
        if bytes(machine.mem_read(pc, size)) == b'\xff\x15\x30\x15\x7e\x00':
            machine.reg_write(UC_X86_REG_EAX, 0)  # OS timeGetTime boundary
            machine.reg_write(UC_X86_REG_EIP, pc + size)
        if pc == 0x750920:
            emitted.append({'kind': 'GUI', 'sound': machine.reg_read(UC_X86_REG_ECX)})
            fixture.native_return(0, 8)
        if pc == 0x752700:
            text_ptr = machine.reg_read(UC_X86_REG_ECX)
            name = bytes(machine.mem_read(text_ptr, 80)).split(b'\0')[0].decode('ascii')
            emitted.append({'kind': 'EVA', 'name': name})
            # The shared VM also observes EVA; never pop the caller twice.
            if machine.reg_read(UC_X86_REG_EIP) == pc:
                fixture.native_return(0, 4)

    uc.hook_add(UC_HOOK_CODE, observe)
    fixture.invoke(CAN_FIRE, super_ptr)
    can_fire = bool(uc.reg_read(UC_X86_REG_EAX) & 255)
    fixture.invoke(SELECT_ACTION, control,
                   (0x10 if case.get('right') else 1, key, 0), context=case)
    assert original_code == [bytes(uc.mem_read(a, n)) for a, n in CODE_SPANS]
    event_count = read(uc, 0xA802C8)[0]
    return dict(
        input=case,
        expected=dict(
            can_fire=can_fire,
            selected=read(uc, 0x8809A0)[0],
            emitted=emitted,
            placement=[read(uc, a)[0] for a in (0x88098C, 0x880990, 0x880994)],
            modes=list(uc.mem_read(0x88099A, 3)),
            repair=bool(uc.mem_read(0x880998, 1)[0]),
            sell=bool(uc.mem_read(0x880999, 1)[0]),
            event_count=event_count,
            event_hex=bytes(uc.mem_read(0xA802D4, 17)).hex() if event_count else None,
        ),
        code_unchanged=True,
    )


def generate():
    cases = [
        {}, {'held': True}, {'ready': False}, {'selected': 0},
        {'right': True, 'held': True, 'selected': 0},
        {'right': True, 'ready': False, 'selected': 5},
        {'drain': True, 'ready': False, 'state': 1},
        {'drain': True, 'state': 0}, {'action': 0, 'selected': 5},
        {'held': True, 'selected': 5},
        {'drain': True, 'state': 2, 'ready': False},
        {'drain': True, 'state': -1, 'ready': False},
        {'drain': True, 'held': True, 'state': 1},
        {'granted': False},
        {'repair': True, 'sell': False},
        {'repair': False, 'sell': True},
        {'held': True, 'repair': True, 'sell': False},
        {'held': True, 'repair': False, 'sell': True},
    ]
    return dict(cases=[execute(case) for case in cases], pointer_targets=pointer_targets())


def pointer_target(case):
    """Original type target action and the SPECIAL_PLACE release suffix.

    The hovered identity and two House masks are supplied reader inputs.
    Building/Unit WhatAmI leaves and House4F9A50 execute unchanged. The
    release starts after its earlier map/placement processing, with that
    original calling frame supplied, and stops before the keyboard update.
    """
    fixture = Sidebar([item('TARGET_POWER', kind=31)])
    uc = fixture.u
    sw = fixture.pointers['TARGET_POWER']
    target, other, cell, clock = (fixture.alloc(size) for size in (0x800, 0x6000, 4, 16))
    put32(uc, 0xA8E340, 1)
    put32(uc, fixture.house + 0x30, 0)
    put32(uc, other + 0x30, 1)
    put32(uc, fixture.house + 0x5788, case['local_allies'])
    put32(uc, other + 0x5788, case['target_allies'])
    put32(uc, sw + 0xB4, case['super_kind'])
    put32(uc, sw + 0xBC, case['configured_action'])
    put32(uc, sw + 0x98, 0)
    put32(uc, target, 0x7E3EBC if case['target_kind'] == 6 else 0x7F5C70)
    put32(uc, target + 0x21C, fixture.house if case['target_owner'] == 0 else other)
    put32(uc, cell, 40 | (40 << 16))
    spans = ((0x6CEF80, 0x5E), (0x6CEEB0, 0x36), (0x4AC20C, 0x8F))
    original = [bytes(uc.mem_read(a, n)) for a, n in spans]
    fixture.invoke(0x6CEF80, sw, (cell, target if case['target_kind'] is not None else 0))
    action = uc.reg_read(UC_X86_REG_EAX)
    put32(uc, SIDEBAR, 0x7E1964)
    put32(uc, 0x8809A0, 0)
    put32(uc, 0xA802C8, 0)
    put32(uc, 0xA802D0, 0)
    uc.mem_write(SIDEBAR + 0x11D0, b'\1')
    trace = []

    def observe(machine, pc, _size, _):
        if pc == clock:
            fixture.native_return(0, 0)  # OS timeGetTime boundary
        elif pc in (0x6CEEB0, 0x4C6B60):
            trace.append(hex(pc))

    hook = uc.hook_add(UC_HOOK_CODE, observe)
    try:
        sp = STACK_BASE + STACK_SIZE - 0x2000
        uc.mem_write(sp, bytes(0x100))
        put32(uc, sp + 0x94, cell)
        put32(uc, sp + 0x9C, action)
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_EBP, SIDEBAR)
        uc.reg_write(UC_X86_REG_EBX, clock)
        run_checked(uc, 0x4AC20C, 0x4AC29B, required_addresses=(0x6CEEB0,))
    finally:
        uc.hook_del(hook)
    assert original == [bytes(uc.mem_read(a, n)) for a, n in spans]
    count = read(uc, 0xA802C8)[0]
    return dict(input=case, expected=dict(action=action, event_count=count,
        event_hex=bytes(uc.mem_read(0xA802D4, 17)).hex() if count else None,
        selected=read(uc, 0x8809A0)[0], capture=uc.mem_read(SIDEBAR + 0x11D0, 1)[0],
        calls=trace), code_unchanged=True)


def pointer_targets():
    cases = []
    for name, target, owner, local, other in [
        ('ground', None, 1, 0, 0), ('own_building', 6, 0, 0, 0),
        ('allied_building', 6, 1, 0, 1), ('mutual_allies', 6, 1, 2, 1),
        ('reverse_only', 6, 1, 2, 0), ('enemy_building', 6, 1, 0, 0),
        ('own_unit', 28, 0, 0, 0), ('allied_unit', 28, 1, 0, 1),
        ('enemy_unit', 28, 1, 0, 0),
    ]:
        cases.append(dict(name=name, super_kind=10, configured_action=69,
            target_kind=target, target_owner=owner, local_allies=local, target_allies=other))
    for kind in (0, 1):
        cases.append(dict(name=f'other_type_{kind}_ground', super_kind=kind, configured_action=1,
            target_kind=None, target_owner=1, local_allies=0, target_allies=0))
    return [pointer_target(case) for case in cases]


def metadata():
    return provenance(
        scope='Whole original Select6AAD00 Super branch and CanFire6CC360, plus Type6CEF80 pointer action and the original SPECIAL_PLACE release suffix, on supplied state.',
        assumptions=[
            'Shared Sidebar VM owns mapped bytes, House/type arrays and allocation. Original Select constructor6AACB0 and device Mouse constructor prefix7B8730..7B8856 run; rendering/device startup is excluded.',
            'The Sidebar/Mouse complete object is supplied with its original primary vtable7E1964 (installed by constructor5BDA40 at5BDA4A). Repair/Sell cursor reset executes5BDA80 and5BDC80 with the native no-cursor-image prior: ABF2DD=1 and ABF294=null, so the original leaf returns before drawing. The complete Mouse constructor and cursor image loading are not executed.',
            'One visible RTTI31 entry of index0 and prepared GUIBuildSound42, ready/hold/charge-drain bytes, selected index, placement references and pointer modes. Repair/Sell are separate bytes880998/880999 (Sidebar87F7E8+11B0/+11B1); the additional three mode bytes88099A..88099C are not proxies for them. These independent prior modes are cleanup controls, not a whole valid UI scenario.',
            'Original CanFire and whole SelectAction execute unchanged, including placement/repair/sell clearing, UnselectAll with an empty selection, immediate SPECIAL_PLACE construction/enqueue, right-click and refusal epilogues.',
            'Native charge-drain state is any signed nonzero value, including -1. The ungranted/ready control isolates CanFire having no granted test; it is not a visible-cameo producer claim.',
            'No native retail loader, power lifecycle, charge-drain lifecycle, event execution, rendered cursor, audible device output or full game replay is claimed. Rust integration separately checks the retail power transition.',
            'Eleven pointer controls supply an already picked object, original Building7E3EBC/Unit7F5C70 vtables, two House indexes/masks and one registered SW type. Whole Type6CEF80 and House4F9A50 execute. Action69 is ForceShield; action70 is NoForceShield in the native name table. Other-type controls deliberately supply action1. No type carries action70.',
            'Pointer releases execute only4AC20C..4AC29B with the original stack/register inputs supplied. Original FindFirstOfAction6CEEB0, SPECIAL_PLACE4C6B60 construction and OutList copying execute; the suffix clears capture and preserves the selected Super. Earlier map/placement processing, keyboard update731CF0, hover-object picking and event delivery are excluded.',
        ],
        substitutions=[
            'Audio750920 GUI sound and the shared752700 EVA sink record requests with their RET8/RET4 conventions; no audio device executes.',
            'Opaque OS timeGetTime imports supply zero without replacing their callers. Unexpected shared fixture FindFactory/CanBuild substitutions fail closed.',
            'The pointer release suffix supplies its already loaded EBX OS timeGetTime function pointer with a zero-return boundary; original release instructions are unchanged.',
        ],
        entry_points=dict(select_action=0x6AAD00, can_fire=0x6CC360,
                          impatient_click_noop=0x6CC070, select_ctor=0x6AACB0,
                          repair_mode=0x4AC8C0, sell_mode=0x4AC660,
                          cursor_reset=0x5BDA80, cursor_select=0x5BDC80,
                          pointer_action=0x6CEF80, allied_with=0x4F9A50,
                          release_suffix=0x4AC20C, find_first_action=0x6CEEB0,
                          special_place=0x4C6B60))


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata())
