"""Original UnitClass BalloonHover arms, one call per row.

- `approach`: UnitClass::Approach_Target 0x007414E0 as FootClass::Mission_Attack
  calls it (vt+0x53C with 0, 0x004D4E66), or with the row's flag. The Unit's
  vtable copy answers SelectWeapon (vt+0x2E4) with the row's slot; GetWeapon
  0x0070E140, HouseClass::IsControlledByHuman 0x0050B730 and HasWeaponAbility
  0x0070D0D0 run over the row's weapons, house and rookie veterancy, and
  Assign_Destination (vt+0x480) records its arguments. A row ends at the return
  or at 0x00741603, the next arm.
- `null`: UnitClass::Assign_Destination 0x00741970 with a NULL destination. The
  NavCom's WhatAmI (vt+0x2C), Direction_To 0x005F3DB0 on both Locations, the
  comparison 0x004D03D0, Get_Mission vt+0x184 0x005B3040 and Assign_Mission
  vt+0x1F0 0x005B2FD0 run; FacingClass::Current 0x004C93D0 on +0x388 answers
  the row's facing, set from the native Direction_To and the row's difference.
  A row ends at the return or at 0x00741A80, the ordinary NULL path.

A row's Target is the Target Unit, at `target_at` when the row gives one, or
the real Cell `target_cell`.

The Unit fixture is unit_source_scatter's: the real Unit vtable copied, a
constructed Drive and a 32x32 map of real Cells.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.unit_entry import CELL
from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, SP
from tools.spatial_oracle.unit_source_scatter import make_source_fixture, CELLS, VT, SET

MINE = SCRATCH + 0xA0000
TARGET, OTHER, HOUSE, WEAPONS, PROJECTILES, SELECT, OUT = (
    MINE, MINE + 0x1000, MINE + 0x2000, MINE + 0x3000, MINE + 0x4000, MINE + 0x5000,
    MINE + 0x6000)
APPROACH, NULL_SETTER = 0x7414E0, 0x741970
NEXT_ARM, ORDINARY_NULL = 0x741603, 0x741A80
ASSIGN_MISSION = 0x5B2FD0
TARGET_AT = [2688 + 700, 2688 - 300, 0]
OTHER_AT = [2688 - 900, 2688 + 400, 0]
# +0xB8 is a byte; +0xCC, the timer's middle dword, takes an uninitialised
# stack local (0x005B3023) and is not recorded.
MISSION_FIELDS = (0xAC, 0xB4, 0xB8, 0xBC, 0xC0, 0xC4, 0xC8, 0xD0)
PRESET = dict(zip(MISSION_FIELDS, (None, None, 1, 3, 7, 11, 13, 19)))


def target_of(case):
    if 'target_cell' in case:
        x, y = case['target_cell']
        return CELLS + (y * 32 + x) * 0x200
    return TARGET


def fixture(case):
    # The fixture indexes MissionControl by its mission; the row's own pair is
    # written after it.
    u, call, read32 = make_source_fixture(dict())
    u.mem_write(ACTOR + 0xAC, dwords(case.get('mission', 1), 0, case.get('queued', -1)))
    u.mem_map(MINE, 0x10000)
    target = target_of(case)
    for obj, at in ((TARGET, case.get('target_at', TARGET_AT)), (OTHER, OTHER_AT)):
        u.mem_write(obj, dwords(0x7F5C70))
        u.mem_write(obj + 0x14, dwords(7))
        u.mem_write(obj + 0x9C, dwords(*at))
    u.mem_write(HOUSE + 0x1EC, bytes([case.get('human', True), 0]))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(ACTOR + 0x14, dwords(7))
    u.mem_write(TYPE + 0xD6A, bytes([case.get('balloon', True)]))
    u.mem_write(TYPE + 0xD28, b'\0')
    u.mem_write(ACTOR + 0x2B4, dwords(target if case.get('target', True) else 0))
    nav = case.get('nav', 'none')
    u.mem_write(CELL + 0xE4, dwords({'cell_target': TARGET, 'cell_other': OTHER}.get(nav, 0)))
    u.mem_write(ACTOR + 0x5A4, dwords({'none': 0, 'target': target, 'other': OTHER}.get(nav, CELL)))
    for offset, value in PRESET.items():
        if value is not None:
            u.mem_write(ACTOR + offset, bytes([value]) if offset == 0xB8 else dwords(value))
    return u, call, read32


def approach(case):
    u, call, read32 = fixture(case)
    for slot, weapon in enumerate(case['weapons']):
        weapon_type = WEAPONS + slot * 0x200 if weapon != 'none' else 0
        u.mem_write(TYPE + 0x898 + slot * 0x1C, dwords(weapon_type))
        if weapon_type:
            projectile = PROJECTILES + slot * 0x400 if weapon != 'no_projectile' else 0
            u.mem_write(weapon_type + 0xA0, dwords(projectile))
            if projectile:
                u.mem_write(projectile + 0x2C0, bytes([weapon == 'vertical']))
    u.mem_write(VT + 0x2E4, dwords(SELECT))
    target = target_of(case)
    calls = []

    def observe(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == SELECT:
            calls.append(['select_weapon', read32(sp + 4) == target])
            u.reg_write(UC_X86_REG_EAX, case['slot'])
        elif address == SET:
            calls.append(['assign_destination', read32(sp + 4) == target, read32(sp + 8)])
            u.reg_write(UC_X86_REG_EAX, 0)
        else:
            return
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + (4 if address == SELECT else 8))

    u.hook_add(UC_HOOK_CODE, observe)
    u.mem_write(SP, dwords(RET_MAGIC, case.get('flag', 0)))
    from unicorn.x86_const import UC_X86_REG_ECX
    u.reg_write(UC_X86_REG_ECX, ACTOR)
    u.reg_write(UC_X86_REG_ESP, SP)
    end = run_checked(u, APPROACH, (RET_MAGIC, NEXT_ARM), count=200000,
                      required_addresses=[APPROACH])
    returned = end == RET_MAGIC
    if returned:
        assert u.reg_read(UC_X86_REG_ESP) == SP + 8
    return dict(input=case, path='returned' if returned else 'next_arm', calls=calls,
                answer=(u.reg_read(UC_X86_REG_EAX) == target) if returned else None)


def null(case):
    u, call, read32 = fixture(case)
    # Direction_To(this, Target) as the arm computes it, then the facing that
    # leaves the row's difference.
    call(0x5F3DB0, ACTOR, [OUT, target_of(case)])
    direction = struct.unpack('<H', u.mem_read(OUT, 2))[0]
    facing = (direction - case.get('difference', 0)) & 0xFFFF
    calls = []

    def observe(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x4C93D0:
            out = read32(sp + 4)
            u.mem_write(out, struct.pack('<H', facing))
            u.reg_write(UC_X86_REG_EAX, out)
            u.reg_write(UC_X86_REG_EIP, read32(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 8)
            calls.append('facing')
        elif address == ASSIGN_MISSION:
            calls.append(['assign_mission', read32(sp + 4)])

    u.hook_add(UC_HOOK_CODE, observe)
    u.mem_write(SP, dwords(RET_MAGIC, 0, 1))
    from unicorn.x86_const import UC_X86_REG_ECX
    u.reg_write(UC_X86_REG_ECX, ACTOR)
    u.reg_write(UC_X86_REG_ESP, SP)
    end = run_checked(u, NULL_SETTER, (RET_MAGIC, ORDINARY_NULL), count=200000,
                      required_addresses=[NULL_SETTER])
    kept = end == RET_MAGIC
    if kept:
        assert u.reg_read(UC_X86_REG_ESP) == SP + 12
    return dict(input=case, direction=direction, facing=facing,
                path='kept' if kept else 'ordinary', calls=calls,
                mission={hex(offset): (u.mem_read(ACTOR + offset, 1)[0] if offset == 0xB8 else
                                       struct.unpack('<i', u.mem_read(ACTOR + offset, 4))[0])
                         for offset in MISSION_FIELDS},
                nav=read32(ACTOR + 0x5A4) != 0)


def approach_rows():
    rows = []
    kinds = ('vertical', 'level', 'no_projectile', 'none')
    for human in (True, False):
        for nav in ('none', 'cell_empty'):
            for balloon in (True, False):
                for slot in (0, 1):
                    for weapon in kinds:
                        weapons = [weapon, 'level'] if slot == 0 else ['vertical', weapon]
                        for flag in (0, 1):
                            rows.append(dict(human=human, nav=nav, balloon=balloon,
                                             slot=slot, weapons=weapons, flag=flag))
    rows.append(dict(target=False, balloon=True, slot=0, weapons=['vertical', 'level'], flag=0))
    for weapon in ('vertical', 'level'):
        for flag in (0, 1):
            rows.append(dict(target_cell=[13, 9], balloon=True, slot=0, weapons=[weapon, 'level'],
                             flag=flag))
    return rows


def null_rows():
    rows = []
    missions = ((1, -1), (-1, 1), (5, -1), (5, 1), (2, -1), (0x1C, -1))
    for nav in ('none', 'target', 'cell_target', 'cell_other', 'cell_empty', 'other'):
        for difference in (0, 0x4000, 0x4001, -0x4000, -0x4001, 0x8000, 0x7FFF):
            for mission, queued in missions:
                rows.append(dict(nav=nav, difference=difference, mission=mission, queued=queued))
    for balloon, target in ((False, True), (True, False)):
        for nav in ('target', 'cell_target', 'other'):
            rows.append(dict(balloon=balloon, target=target, nav=nav, difference=0,
                             mission=5, queued=-1))
    for dx, dy in ((-500, -900), (60, 1200), (-1300, 20), (0, -700), (900, 900)):
        for nav in ('none', 'target', 'cell_target', 'other'):
            for difference in (0, 0x4000, 0x4001, -0x4000, -0x4001, 0x8000):
                rows.append(dict(target_at=[2688 + dx, 2688 + dy, 0], nav=nav,
                                 difference=difference, mission=5, queued=-1))
    # A Cell Target: the NavCom Cell (11, 10) itself or another.
    for target_cell in ([13, 9], [11, 10], [7, 14]):
        for nav in ('none', 'target', 'cell_other', 'cell_empty', 'other'):
            for difference in (0, 0x4000, 0x4001, 0x8000):
                for mission in (5, 1):
                    rows.append(dict(target_cell=target_cell, nav=nav, difference=difference,
                                     mission=mission, queued=-1))
    return rows


def generate():
    return dict(approach=[approach(row) for row in approach_rows()],
                null=[null(row) for row in null_rows()])


def metadata():
    return provenance(
        scope="UnitClass::Approach_Target 0x007414E0 from entry to its return or to the next "
              "arm at 0x00741603 (owner human or not, NavCom or none, BalloonHover +0xD6A, "
              "SelectWeapon's slot over two weapons whose type, projectile (+0xA0) and Vertical "
              "(+0x2C0) vary, the no-move flag, a Unit or a Cell Target), and UnitClass::Assign_Destination 0x00741970 "
              "with a NULL destination from entry to its return or to the ordinary NULL path "
              "at 0x00741A80 (BalloonHover, Target, NavCom none/the Target/a Cell whose first "
              "object (+0xE4) is the Target, another or none/another object, Direction_To "
              "minus the facing at and around +-0x4000 and 0x8000, current and queued "
              "missions, a Unit Target at six places or a Cell Target, the NavCom Cell or "
              "another). Path, recorded calls, the returned Target, the mission fields "
              "+0xAC..+0xD0 but +0xB0 and +0xCC, and NavCom afterwards. Not the head's crush arm (no Crusher=, "
              "rookie), the 0x00741603 arm or the ordinary NULL path's work.",
        assumptions=[
            "unit_source_scatter's Unit fixture (real Unit vtable 0x007F5C70 copied, "
            "constructed Drive, real Cells); the Target and the other object are Units on "
            "the real vtable with their Location and AbstractFlags 7, a Cell Target one of "
            "the fixture's real Cells; the house answers "
            "+0x1EC; mission fields +0xB8 (a byte), +0xBC, +0xC0, +0xC4, +0xC8 and +0xD0 "
            "preset to 1, 3, 7, 11, 13 and 19, and Frame 100.",
            "Per row: approach returns with RET 4 or stops at 0x00741603; null returns with "
            "RET 8 or stops at 0x00741A80.",
        ],
        substitutions=[
            "SelectWeapon vt+0x2E4 (0x00746CD0) answers the row's slot; Assign_Destination "
            "vt+0x480 records (Target, flag) and returns; FacingClass::Current 0x004C93D0 "
            "answers the row's facing.",
        ],
        entry_points={"approach": APPROACH, "null": NULL_SETTER, "direction_to": 0x5F3DB0,
                      "assign_mission": ASSIGN_MISSION})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
