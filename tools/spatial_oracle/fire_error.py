"""Original GetFireError: TechnoClass::GetFireError 0x006FC0B0 through the class overrides.

Each row calls one class entry with (this, target, weapon_index, check_range), thiscall, the
function every cloned vtable still holds at slot +0x3C0:

    UnitClass     0x00740FD0 (vtable 0x007F5C70)    InfantryClass 0x0051C8B0 (vtable 0x007EB058)
    BuildingClass 0x00447F10 (vtable 0x007E3EBC)    AircraftClass 0x0041A9E0 (vtable 0x007E22A4)

Each override runs its prefix, calls the base directly and runs its suffix on a base 0, so the
base runs inside every class with that class's virtuals (vt+0x37C EMP, vt+0x380 paralysis,
vt+0x3F8 GetWeapon differ by class). A row records the returned code (a full int) and the
ordered names of the substituted calls. Every row asserts RET 0xC with a balanced stack, zero
RNG calls (0x0065C780, 0x0065C7E0), no _com_issue_error (0x007DC720) and no memory write outside
the stack, and checks its `covers` tags against the executed instructions (below).

Runs natively on the fixture: every field read; WhatAmI vt+0x2C; GetTechnoType vt+0x84/+0x88;
vt+0x1D4/+0x1D8/+0x1DC; Iron Curtain vt+0x160 (0x0041BF40); EMP vt+0x37C; paralysis vt+0x380;
GetWeapon vt+0x3F8 (0x0070E140, Building 0x004526F0 with vt+0x400 0x00458DD0); the target's
GetCoords vt+0x48; is-vehicle vt+0x80; mission vt+0x184 (0x005B3040); Building operational
vt+0x350 (0x004555D0); HasTurret vt+0x3FC (0x004527D0); Unit vt+0x4E4 (0x00736D50); damage
0x006F3970; 0x00746DB0; 0x0053A130; spawn counts 0x006B7D30/0x006B7D80; GetNthLink 0x0065AD40;
AsTechno 0x0040DD20; HealthRatio 0x005F5C60; FacingClass::Current 0x004C93D0; DrainingMe
0x0070FEC0; the COM smart-pointer helpers 0x0045AEA0/0x004B4D50; and, once per fixture, the
static initializers 0x006F28A0..0x006F2970 that set [0x00B0EB34] (T59's height unit, 104).

Substituted: each returns the row's `verdicts` value and appends its name to `calls`:
    in_range                firer vt+0x3A8 (target, weapon) -> bool            [T61]
    naval_selector          firer vt+0x2E8 (target) -> int, -1 = refuse        [T40]
    visual_state            target vt+0x68 (1, firer house) -> int             [T17]
    high_flying             target vt+0x54 -> bool                             [T18 T38 T40 T41]
    low_flying              target vt+0x50 -> bool; also U6 through Aircraft
                            vt+0x80 0x0041B910, which tail-jumps to vt+0x50    [T40 U6]
    firer_high_flying       firer vt+0x54 -> bool                              [T58]
    target_layer            target vt+0x78 -> int                              [T39]
    target_cell             target vt+0x1BC -> scratch cell {land_type +0xEC, flags +0x140}
    firer_cell              firer vt+0x1BC -> scratch cell, null, or "target" (the target
                            object itself, for I8's own-cell AreaFire)          [T40 T58 I8]
    sensor                  CellClass 0x004870D0 (ECX = target's cell, firer house index)
    allied                  HouseClass 0x004F9A50 (ECX = target house, firer house)   [T17]
    bridge_for_firing       0x00703B10 (ECX = firer)                           [T35]
    can_infect              0x0062A8E0 (ECX = *(firer+0x69C), target-as-Foot) [T50]
    can_capture             0x00471C90 (ECX = *(firer+0x2BC), target)          [T53]
    deploy_cell_ok          CellClass 0x00487C10 (ECX = the Unit's own cell); asked on every
                            Unit base 0, before the DeployToFire test           [U4]
    locomotor_moving        firer ILocomotion +0x80 Is_Moving_Now              [U8 I6]
    target_locomotor_moving target ILocomotion +0x80 Is_Moving_Now             [T29]
    locomotor_can_fire      firer ILocomotion +0x8C Can_Fire -> int            [U13 I10]
    jumpjet_locomotor       IPersist::GetClassID (+0xC) of the firer's locomotor: writes the
                            jumpjet CLSID at 0x007E9AC0 ({92612C46-F71F-11D1-AC9F-006008055BB5},
                            retail rulesmd.ini's Jumpjet Locomotor=) when true, the Drive
                            CLSID {4A582741-9839-11D1-B709-00A024DDAFD1} when false      [I6]
    fighter                 IFlyControl +0x1C (interface at Aircraft+0x6C0)     [A2]
    direction_to_target     0x005F3DB0 (ECX = firer, out, target) -> DirStruct  [U12 A2]
    turret_direction        Building vt+0x4E8 (out, target) -> DirStruct        [B7]
    occupants               Building vt+0x408 -> int. Logged as `occupants` from B1
                            (0x00447F37) and `occupants_via_get_weapon` from vt+0x400
                            0x00458DD0 inside Building GetWeapon 0x004526F0     [B1]
    power_fraction          HouseClass 0x004FCE30 -> binary64 in ST0            [B5]
Substituted silently (plumbing whose only consumer is a stub above): MapClass::GetCell
0x00565730 at its two direct call sites 0x006FC197 (the cell under the target's GetCoords,
consumed only by `sensor`) and 0x00741078 (the Unit's own cell, consumed only by
`deploy_cell_ok`), returning a scratch cell; the locomotor's IUnknown QueryInterface
(IID_IPersist 0x00818858 -> a scratch IPersist object), AddRef and Release.

Row schema. A row states only what it varies; `defaults` in the payload fills the rest. Dict
groups merge per key (cell verdicts one level deeper); `weapons` merges per slot, null = empty
slot. A float input may be a JSON number or "0x" + 16 hex digits of binary64 bits.
    class                 unit | infantry | aircraft | building (the firer)
    frame                 [0x00A8ED84] Frame (1000)
    weapon_index          arg 2 (0).  check_range  arg 3, low byte read (true)
    weapons[i]            TechnoType+0x898+0x1C*i -> WeaponTypeClass; W = weapons[weapon_index],
                          T37's O = weapons[weapon_index == 0 ? 1 : 0]; 0x006F3970 reads 0 and 1
      damage +0xA4 (50)  ambient_damage +0x98 (0)  burst +0x9C (1)  range +0xB4 (1024)
      use_fire_particles +0x129  use_spark_particles +0x12A  omni_fire +0x12B
      is_railgun +0x12D  is_sonic +0x130  spawner +0x131  decloak_to_fire +0x133 (1)
      fire_while_moving +0x141 (1)  drain_weapon +0x142  fire_in_transport +0x143
      ion_sensitive +0x14F  area_fire +0x150  is_mag_beam +0x15C   (bytes, default 0)
      every slot's Projectile +0xA0 and Warhead +0xAC point at the two objects below
    warhead               mind_control +0x155  ivan_bomb +0x157  parasite +0x159
                          temporal +0x15A  is_locomotor +0x15B  psychedelic +0x16D
                          bomb_disarm +0x16E (bytes, 0); verses +0xA0 11 x binary64 (1.0)
    projectile            aa +0x2A4 (1)  ag +0x2A5 (1)  rot +0x2DC (0)
    firer                 TechnoClass (every class):
      enslaved +0x2DC  warped_out +0x270  warping_in +0x271  robot_offline +0x1C8
      sinking +0x3CD  berserk +0x298  falling +0x8D  in_open_transport +0x82  on_bridge +0x8C
      z +0xA4 (Location.Z)  emp_remaining +0x504  fire_particles_live +0x304
      spark_particles_live +0x308  railgun_particles_live +0x314  wave_live +0x324
      rearm_start +0x2EC (-1)  rearm_left +0x2F4 (0)  burst_index +0x3B8  ammo +0x2FC (-1)
      cloak_state +0x220  current_weapon +0x138  health +0x6C (100)  draining_me +0x1D0
      mission +0xAC (1)  queued_mission +0xB4 (-1)  primary_facing +0x388
      secondary_facing +0x3A0 (FacingClass; ROT 0, so Current() returns the value)
      owner_is_human House+0x1EC  owner_blackout_start House+0x2A4 (-1)
      owner_blackout_left House+0x2AC  owner_drained_power_source House+0x577B
      locomotor_target +0x2AC, drain_target +0x1CC: null | "target" | "other"
      transporter +0x11C: null | "target" | "plain" | "warped_out" (+0x270) | "nested" (+0x11C)
      temporal +0x274: null | "idle" | "target" | "other" (TemporalClass +0x28)
      spawn_slots +0x2D0: null | [{state +0x4, child: null | {in_limbo +0x81,
                          missile_spawn Type+0xD68}}] (SpawnManager +0x3C items, +0x48 count)
    firer, Foot only:     magnetron_lifted +0x6AD  navcom +0x5A4  speed_fraction +0x578 (f64)
                          paralysis_start +0x6A0 (-1)  paralysis_left +0x6A8
    firer, Unit only:     death_frame_counter +0x6D8 (-1)  rocker_link +0x2A8  deploying +0x6E1
                          undeploying +0x6E2  tethered +0x418  firing_sequence +0x68D
                          turret_rotation_latch +0x6AF  firing_frame +0x6C0 (-1)
                          radio_link RadioLinks(+0xE4)[0]: null | "building" | "unit"
    firer, Infantry only: sequence +0x6C4 (0)
    firer, Aircraft only: paradrop_payload +0x6C9  has_passenger +0x118
    firer, Building only: online +0x660 (1)  tesla_chargers +0x67C  has_engineer +0x6CC
                          delayed_fire_counter +0x714
                          turret_upgrade (+0x702 = 1, +0x5EC[0] -> a type with Turret=yes)
    firer_type            TechnoTypeClass: natural +0x693  pushy +0x692  land_targeting +0x604
                          (dword)  mobile_fire +0x6AE (1)  turret_count +0x808 (dword)
                          turret +0xCA1  is_gattling +0xCD5  hunter_seeker +0xD27
                          balloon_hover +0xD6A  jumpjet +0xD94  organic +0xD97
                          UnitType: deploy_to_fire +0xE12  small_visceroid +0xE18
                          large_visceroid +0xE19  facings +0xE3C (8)
                          firing_sync_frame +0xE40 (2 dwords, -1)
                          InfantryType: jumpjet_turn +0xECB
                          BuildingType: power_drain +0xEE4 (dword)  needs_engineer +0x1552
                          powered +0x1573  powered_special +0x1574  can_be_occupied +0x157B
                          can_occupy_fire +0x157C  emp_pulse_cannon +0x16C3
                          turret_anim_is_voxel +0x16C5
    target                kind: unit | infantry | aircraft | building | cell | object
                          (TerrainClass, vtable 0x007F522C) | none (NULL). Techno kinds:
                          bomb +0x38  in_limbo +0x81  on_bridge +0x8C  z +0xA4  mission +0xAC (5)
                          iron_curtain_start +0x18C (-1)  iron_curtain_left +0x194
                          draining_me +0x1D0  warped_out +0x270  chrono_warp_latch +0x27C
                          bunkered +0x2E4  sinking +0x3CD  docked +0x418  health +0x6C (100)
                          Foot kinds: parasite_lock_until +0x698
                          unit: deploying +0x6E1  undeploying +0x6E2
                          rocker_link +0x2A8: null | "firer" | "other"
                          object: bomb +0x38.  cell: land_type +0xEC
    target_type           berserk_friendly +0x690  unnatural +0x694  drainable +0x5EF
                          immune_to_psionics +0xD35  spawned +0xD54  balloon_hover +0xD6A
                          jumpjet +0xD94  organic +0xD97  armor +0x9C  strength +0xA0 (100)
                          UnitType: is_simple_deployer +0xE13  non_vehicle +0xE1B
    verdicts              the stub values above
Fixed fixture state: Rules [0x008871E0]+0x16F8 = 1.0; firer house index 0, target house 1;
veterancy 0.0 (normal weapon slots); a Building's occupant vector count +0x694 equals the
`occupants` verdict and its fire index +0x69C sits at the end, so Building GetWeapon resolves
the type's own slots (OccupyWeapon selection is weapon selection, outside this oracle).

`covers`: "T45" = the row's code comes from T45; "T45/miss" = T45 ran and did not decide;
"T47/shadowed" = T47's condition holds but an earlier test decided, so T47 never ran. The
generator checks every tag: the deciding exit block (EXITS/SHARED_EXITS) and the first
instruction of each test (ENTRIES_OF) against the executed trace. "OK" is the fall-through.

Evidence limits: control flow, codes, verdict order and field offsets are native; the stubbed
leaves keep their own evidence. Excluded because they fault natively: Burst = 0 with weapon
index 0 on a Unit (T44 IDIV), a Spawner weapon without a SpawnManager (T35), a NULL warhead
(T36), a NULL target cell (T40), weapon index -1 and a NULL radio link while tethered (U5). A
NULL projectile is read unchecked wherever T38, T39, T41, U11 or U12 reads it; never used.
Rust consumer: src/sim/combat/fire_error_tests.rs (`original_fire_error_rows`).
"""
from __future__ import annotations

import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW, UC_X86_REG_FPTAG)

from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
                                 OracleError, finish_vectors, load_image, provenance, run_checked)

# ---- native identities ------------------------------------------------------------------------
BASE = 0x6FC0B0
ENTRIES = {"unit": 0x740FD0, "infantry": 0x51C8B0, "building": 0x447F10, "aircraft": 0x41A9E0}
VTABLES = {"unit": 0x7F5C70, "infantry": 0x7EB058, "aircraft": 0x7E22A4, "building": 0x7E3EBC,
           "cell": 0x7E4EEC, "object": 0x7F522C}
TYPE_AT = {"unit": 0x6C4, "infantry": 0x6C0, "aircraft": 0x6C4, "building": 0x520}
FLAGS = {"unit": 7, "infantry": 7, "aircraft": 7, "building": 3, "cell": 0, "object": 2}
CLASSES = ("unit", "infantry", "aircraft", "building")
FOOT = ("unit", "infantry", "aircraft")
FRAME, RULES_PTR, LEVEL_HEIGHT = 0xA8ED84, 0x8871E0, 0xB0EB34
LEVEL_HEIGHT_INITIALIZERS = (0x6F28A0, 0x6F28D0, 0x6F28F0, 0x6F2910, 0x6F2930, 0x6F2950, 0x6F2970)
CELL_GET_COORDS = 0x486840                   # CellClass vt+0x48
JUMPJET_CLSID, IID_IPERSIST = 0x7E9AC0, 0x818858
DRIVE_CLSID = bytes.fromhex("4127584a3998d111b70900a024ddafd1")
FORBIDDEN = {0x65C780: "Random", 0x65C7E0: "RandomRanged", 0x7DC720: "_com_issue_error"}
BODIES = ((0x6FC0B0, 0x6FCD38), (0x740FD0, 0x741335), (0x51C8B0, 0x51CB95),
          (0x447F10, 0x448065), (0x41A9E0, 0x41AA7F))
GET_CELL_SITES = (0x6FC19C, 0x74107D)        # return addresses of the two direct GetCell calls
OCCUPANTS_VIA_GET_WEAPON = 0x458DF2          # return address inside vt+0x400 0x00458DD0

# ---- scratch layout ---------------------------------------------------------------------------
REGION = 0x100000
SP = STACK_BASE + STACK_SIZE - 0x1000
(FIRER, TARGET, TRANSPORT, LINK, OTHER, TEMPORAL, DUMMY) = (SCRATCH + n * 0x1000 for n in range(7))
SPAWNER, SPAWN_ITEMS, SPAWN_NODES = SCRATCH + 0x8000, SCRATCH + 0x8400, SCRATCH + 0x8800
CHILDREN = SCRATCH + 0x9000                  # child i at +0x2000*i, its type at child+0x1000
FIRER_TYPE, TARGET_TYPE, UPGRADE_TYPE = SCRATCH + 0x20000, SCRATCH + 0x22000, SCRATCH + 0x24000
WEAPONS, WARHEAD, PROJECTILE = SCRATCH + 0x28000, SCRATCH + 0x29000, SCRATCH + 0x29800
FIRER_HOUSE, TARGET_HOUSE, RULES = SCRATCH + 0x30000, SCRATCH + 0x38000, SCRATCH + 0x40000
FIRER_CELL, TARGET_CELL, COORD_CELL = SCRATCH + 0x42000, SCRATCH + 0x42400, SCRATCH + 0x42800
FIRER_VT, TARGET_VT = SCRATCH + 0x50000, SCRATCH + 0x50800
LOCO, PERSIST, TARGET_LOCO = SCRATCH + 0x51000, SCRATCH + 0x51100, SCRATCH + 0x51200
LOCO_VT, PERSIST_VT, TARGET_LOCO_VT, FLY_VT = (SCRATCH + 0x51400 + n * 0x100 for n in range(4))
PARASITE, CAPTURE, RADIO_LINKS = SCRATCH + 0x51800, SCRATCH + 0x51900, SCRATCH + 0x51A00
POWER_SLOT, POWER_CODE = SCRATCH + 0x51B00, SCRATCH + 0x51B10
STUBS = SCRATCH + 0x60000                    # one INT3 per stub, 0x10 apart

# stub name -> stack bytes its native original pops (callee cleanup)
STUB_POPS = {
    "in_range": 8, "naval_selector": 4, "visual_state": 8, "high_flying": 0, "low_flying": 0,
    "firer_high_flying": 0, "target_layer": 0, "target_cell": 0, "firer_cell": 0,
    "occupants": 0, "turret_direction": 8, "locomotor_moving": 4, "target_locomotor_moving": 4,
    "locomotor_can_fire": 4, "jumpjet_locomotor": 8, "fighter": 4,
    "query_interface": 0xC, "add_ref": 4, "release": 4,
}
STUB_AT = {name: STUBS + 0x10 * n for n, name in enumerate(STUB_POPS)}
STUB_NAME = {address: name for name, address in STUB_AT.items()}
NATIVE_STUBS = {0x4870D0: ("sensor", 4), 0x4F9A50: ("allied", 4), 0x703B10: ("bridge_for_firing", 0),
                0x62A8E0: ("can_infect", 4), 0x471C90: ("can_capture", 4),
                0x487C10: ("deploy_cell_ok", 0), 0x5F3DB0: ("direction_to_target", 8),
                0x4FCE30: ("power_fraction", 0), 0x565730: ("get_cell", 4)}
FIRER_SLOTS = {0x3A8: "in_range", 0x2E8: "naval_selector", 0x54: "firer_high_flying",
               0x1BC: "firer_cell"}
BUILDING_SLOTS = {0x408: "occupants", 0x4E8: "turret_direction"}
TARGET_SLOTS = {0x50: "low_flying", 0x54: "high_flying", 0x68: "visual_state",
                0x78: "target_layer", 0x1BC: "target_cell"}
COM_SLOTS = {LOCO_VT: {0x0: "query_interface", 0x4: "add_ref", 0x8: "release",
                       0x80: "locomotor_moving", 0x8C: "locomotor_can_fire"},
             PERSIST_VT: {0x0: "query_interface", 0x4: "add_ref", 0x8: "release",
                          0xC: "jumpjet_locomotor"},
             TARGET_LOCO_VT: {0x0: "query_interface", 0x4: "add_ref", 0x8: "release",
                              0x80: "target_locomotor_moving"},
             FLY_VT: {0x1C: "fighter"}}

# ---- inputs: group -> field -> (offset, kind, default, applies to) -----------------------------
ALL, UNIT, INF, AIR, BLD = CLASSES, ("unit",), ("infantry",), ("aircraft",), ("building",)
TECHNO_KINDS = CLASSES
H = "house"
FIRER_FIELDS = {
    "enslaved": (0x2DC, "ptr", False, ALL), "warped_out": (0x270, "u8", 0, ALL),
    "warping_in": (0x271, "u8", 0, ALL), "robot_offline": (0x1C8, "u8", 0, ALL),
    "sinking": (0x3CD, "u8", 0, ALL), "berserk": (0x298, "u8", 0, ALL),
    "falling": (0x8D, "u8", 0, ALL), "in_open_transport": (0x82, "u8", 0, ALL),
    "on_bridge": (0x8C, "u8", 0, ALL), "z": (0xA4, "i32", 0, ALL),
    "emp_remaining": (0x504, "i32", 0, ALL),
    "fire_particles_live": (0x304, "ptr", False, ALL),
    "spark_particles_live": (0x308, "ptr", False, ALL),
    "railgun_particles_live": (0x314, "ptr", False, ALL),
    "wave_live": (0x324, "ptr", False, ALL),
    "rearm_start": (0x2EC, "i32", -1, ALL), "rearm_left": (0x2F4, "i32", 0, ALL),
    "burst_index": (0x3B8, "i32", 0, ALL), "ammo": (0x2FC, "i32", -1, ALL),
    "cloak_state": (0x220, "i32", 0, ALL), "current_weapon": (0x138, "i32", 0, ALL),
    "health": (0x6C, "i32", 100, ALL), "draining_me": (0x1D0, "ptr", False, ALL),
    "mission": (0xAC, "i32", 1, ALL), "queued_mission": (0xB4, "i32", -1, ALL),
    "primary_facing": (0x388, "facing", 0, ALL), "secondary_facing": (0x3A0, "facing", 0, ALL),
    "owner_is_human": ((H, 0x1EC), "u8", 0, ALL),
    "owner_blackout_start": ((H, 0x2A4), "i32", -1, ALL),
    "owner_blackout_left": ((H, 0x2AC), "i32", 0, ALL),
    "owner_drained_power_source": ((H, 0x577B), "u8", 0, ALL),
    "magnetron_lifted": (0x6AD, "u8", 0, FOOT), "navcom": (0x5A4, "ptr", False, FOOT),
    "speed_fraction": (0x578, "f64", 0.0, FOOT),
    "paralysis_start": (0x6A0, "i32", -1, FOOT), "paralysis_left": (0x6A8, "i32", 0, FOOT),
    "death_frame_counter": (0x6D8, "i32", -1, UNIT), "rocker_link": (0x2A8, "ptr", False, UNIT),
    "deploying": (0x6E1, "u8", 0, UNIT), "undeploying": (0x6E2, "u8", 0, UNIT),
    "tethered": (0x418, "u8", 0, UNIT), "firing_sequence": (0x68D, "u8", 0, UNIT),
    "turret_rotation_latch": (0x6AF, "u8", 0, UNIT), "firing_frame": (0x6C0, "i32", -1, UNIT),
    "sequence": (0x6C4, "i32", 0, INF),
    "paradrop_payload": (0x6C9, "u8", 0, AIR), "has_passenger": (0x118, "ptr", False, AIR),
    "online": (0x660, "u8", 1, BLD), "tesla_chargers": (0x67C, "i32", 0, BLD),
    "has_engineer": (0x6CC, "u8", 0, BLD), "delayed_fire_counter": (0x714, "i32", 0, BLD),
}
FIRER_STRUCTURED = {"locomotor_target": (None, ALL), "drain_target": (None, ALL),
                    "transporter": (None, ALL), "temporal": (None, ALL),
                    "spawn_slots": (None, ALL), "radio_link": (None, UNIT),
                    "turret_upgrade": (False, BLD)}
FIRER_TYPE_FIELDS = {
    "natural": (0x693, "u8", 0, ALL), "pushy": (0x692, "u8", 0, ALL),
    "land_targeting": (0x604, "i32", 0, ALL), "mobile_fire": (0x6AE, "u8", 1, ALL),
    "turret_count": (0x808, "i32", 0, ALL), "turret": (0xCA1, "u8", 0, ALL),
    "is_gattling": (0xCD5, "u8", 0, ALL), "hunter_seeker": (0xD27, "u8", 0, ALL),
    "balloon_hover": (0xD6A, "u8", 0, ALL), "jumpjet": (0xD94, "u8", 0, ALL),
    "organic": (0xD97, "u8", 0, ALL),
    "deploy_to_fire": (0xE12, "u8", 0, UNIT), "small_visceroid": (0xE18, "u8", 0, UNIT),
    "large_visceroid": (0xE19, "u8", 0, UNIT), "facings": (0xE3C, "i32", 8, UNIT),
    "firing_sync_frame": (0xE40, "i32x2", [-1, -1], UNIT),
    "jumpjet_turn": (0xECB, "u8", 0, INF),
    "power_drain": (0xEE4, "i32", 0, BLD), "needs_engineer": (0x1552, "u8", 0, BLD),
    "powered": (0x1573, "u8", 0, BLD), "powered_special": (0x1574, "u8", 0, BLD),
    "can_be_occupied": (0x157B, "u8", 0, BLD), "can_occupy_fire": (0x157C, "u8", 0, BLD),
    "emp_pulse_cannon": (0x16C3, "u8", 0, BLD), "turret_anim_is_voxel": (0x16C5, "u8", 0, BLD),
}
WEAPON_FIELDS = {
    "damage": (0xA4, "i32", 50), "ambient_damage": (0x98, "i32", 0), "burst": (0x9C, "i32", 1),
    "range": (0xB4, "i32", 1024), "use_fire_particles": (0x129, "u8", 0),
    "use_spark_particles": (0x12A, "u8", 0), "omni_fire": (0x12B, "u8", 0),
    "is_railgun": (0x12D, "u8", 0), "is_sonic": (0x130, "u8", 0), "spawner": (0x131, "u8", 0),
    "decloak_to_fire": (0x133, "u8", 1), "fire_while_moving": (0x141, "u8", 1),
    "drain_weapon": (0x142, "u8", 0), "fire_in_transport": (0x143, "u8", 0),
    "ion_sensitive": (0x14F, "u8", 0), "area_fire": (0x150, "u8", 0),
    "is_mag_beam": (0x15C, "u8", 0),
}
WARHEAD_FIELDS = {
    "verses": (0xA0, "f64x11", [1.0] * 11), "mind_control": (0x155, "u8", 0),
    "ivan_bomb": (0x157, "u8", 0), "parasite": (0x159, "u8", 0), "temporal": (0x15A, "u8", 0),
    "is_locomotor": (0x15B, "u8", 0), "psychedelic": (0x16D, "u8", 0),
    "bomb_disarm": (0x16E, "u8", 0),
}
PROJECTILE_FIELDS = {"aa": (0x2A4, "u8", 1), "ag": (0x2A5, "u8", 1), "rot": (0x2DC, "i32", 0)}
TARGET_FIELDS = {
    "bomb": (0x38, "ptr", False, TECHNO_KINDS + ("object",)),
    "in_limbo": (0x81, "u8", 0, TECHNO_KINDS), "on_bridge": (0x8C, "u8", 0, TECHNO_KINDS),
    "z": (0xA4, "i32", 0, TECHNO_KINDS), "mission": (0xAC, "i32", 5, TECHNO_KINDS),
    "iron_curtain_start": (0x18C, "i32", -1, TECHNO_KINDS),
    "iron_curtain_left": (0x194, "i32", 0, TECHNO_KINDS),
    "draining_me": (0x1D0, "ptr", False, TECHNO_KINDS),
    "warped_out": (0x270, "u8", 0, TECHNO_KINDS),
    "chrono_warp_latch": (0x27C, "u8", 0, TECHNO_KINDS),
    "bunkered": (0x2E4, "ptr", False, TECHNO_KINDS), "sinking": (0x3CD, "u8", 0, TECHNO_KINDS),
    "docked": (0x418, "u8", 0, TECHNO_KINDS), "health": (0x6C, "i32", 100, TECHNO_KINDS),
    "parasite_lock_until": (0x698, "i32", 0, FOOT),
    "deploying": (0x6E1, "u8", 0, UNIT), "undeploying": (0x6E2, "u8", 0, UNIT),
    "land_type": (0xEC, "i32", 0, ("cell",)),
}
TARGET_STRUCTURED = {"kind": ("unit", CLASSES + ("cell", "object", "none")),
                     "rocker_link": (None, UNIT)}
TARGET_TYPE_FIELDS = {
    "armor": (0x9C, "i32", 0, TECHNO_KINDS), "strength": (0xA0, "i32", 100, TECHNO_KINDS),
    "drainable": (0x5EF, "u8", 0, TECHNO_KINDS),
    "berserk_friendly": (0x690, "u8", 0, TECHNO_KINDS),
    "unnatural": (0x694, "u8", 0, TECHNO_KINDS),
    "immune_to_psionics": (0xD35, "u8", 0, TECHNO_KINDS),
    "spawned": (0xD54, "u8", 0, TECHNO_KINDS), "balloon_hover": (0xD6A, "u8", 0, TECHNO_KINDS),
    "jumpjet": (0xD94, "u8", 0, TECHNO_KINDS), "organic": (0xD97, "u8", 0, TECHNO_KINDS),
    "is_simple_deployer": (0xE13, "u8", 0, UNIT), "non_vehicle": (0xE1B, "u8", 0, UNIT),
}
VERDICTS = {
    "in_range": True, "naval_selector": 0, "visual_state": 0, "high_flying": False,
    "low_flying": True, "firer_high_flying": False, "target_layer": 2,
    "target_cell": {"land_type": 0, "flags": 0}, "firer_cell": {"land_type": 0, "flags": 0},
    "sensor": False, "allied": False, "bridge_for_firing": False, "can_infect": True,
    "can_capture": True, "deploy_cell_ok": True, "locomotor_moving": False,
    "target_locomotor_moving": False, "locomotor_can_fire": 0, "jumpjet_locomotor": False,
    "fighter": False, "direction_to_target": 0, "turret_direction": 0, "occupants": 1,
    "power_fraction": 1.0,
}


def _defaults(fields, structured=None):
    out = {name: spec[2] for name, spec in fields.items()}
    out.update({name: spec[0] for name, spec in (structured or {}).items()})
    return out


WEAPON_DEFAULT = _defaults(WEAPON_FIELDS)
DEFAULTS = {
    "class": "unit", "frame": 1000, "weapon_index": 0, "check_range": True,
    "firer": _defaults(FIRER_FIELDS, FIRER_STRUCTURED),
    "firer_type": _defaults(FIRER_TYPE_FIELDS),
    "weapons": [WEAPON_DEFAULT, WEAPON_DEFAULT],
    "warhead": _defaults(WARHEAD_FIELDS), "projectile": _defaults(PROJECTILE_FIELDS),
    "target": _defaults(TARGET_FIELDS, TARGET_STRUCTURED),
    "target_type": _defaults(TARGET_TYPE_FIELDS), "verdicts": VERDICTS,
}
APPLIES = {"firer": {**{k: v[3] for k, v in FIRER_FIELDS.items()},
                     **{k: v[1] for k, v in FIRER_STRUCTURED.items()}},
           "firer_type": {k: v[3] for k, v in FIRER_TYPE_FIELDS.items()},
           "target": {**{k: v[3] for k, v in TARGET_FIELDS.items()},
                      **{k: v[1] for k, v in TARGET_STRUCTURED.items()}},
           "target_type": {k: v[3] for k, v in TARGET_TYPE_FIELDS.items()}}

# ---- exits and test entries (for checking `covers`) -------------------------------------------
EXITS = {  # first instruction of a block that returns that test's code
    0x6FC0DF: "T3", 0x6FC0F6: "T4", 0x6FC168: "T10", 0x6FC640: "T35", 0x6FC720: "T38",
    0x6FC806: "T41", 0x6FC8AF: "T42", 0x6FC8E6: "T43", 0x6FC940: "T44", 0x6FC972: "T45",
    0x6FC995: "T46", 0x6FC9B8: "T46", 0x6FC9DB: "T46", 0x6FC9FE: "T46", 0x6FCA17: "T47",
    0x6FCA4F: "T48", 0x6FCA72: "T49", 0x6FCAB6: "T50", 0x6FCAEB: "T51", 0x6FCB15: "T52",
    0x6FCB44: "T53", 0x6FCB7E: "T54", 0x6FCB9E: "T55", 0x6FCBBE: "T56", 0x6FCBD7: "T57",
    0x6FCC4E: "T58", 0x6FCCAE: "T59", 0x6FCCDF: "T60", 0x6FCD0E: "T61", 0x6FCD1D: "OK",
    0x740FE3: "U1", 0x741005: "U2", 0x74101E: "U3", 0x7410A8: "U4", 0x7410E2: "U5",
    0x74113A: "U6", 0x741163: "U7", 0x741190: "U8", 0x7411CA: "U8", 0x7411F7: "U9",
    0x74121A: "U10", 0x74124D: "U11", 0x7412F1: "U12",
    0x51C8F0: "I1", 0x51C939: "I2", 0x51C98C: "I3", 0x51CABF: "I6", 0x51CB20: "I8",
    0x51CB53: "I9",
    0x447F64: "B3", 0x447FA3: "B5", 0x447FB8: "B6", 0x448047: "B7",
    0x41AA13: "A1", 0x41AA6C: "A2",
}
SHARED_EXITS = {  # shared return block -> {the instruction executed just before it: test}
    0x6FC86A: {0x6FC0BF: "T1", 0x6FC0CD: "T2", 0x6FC111: "T5", 0x6FC11F: "T6", 0x6FC12D: "T7",
               0x6FC139: "T8", 0x6FC145: "T9", 0x6FC1B0: "T11", 0x6FC1C6: "T12",
               0x6FC1E9: "T13", 0x6FC216: "T14", 0x6FC224: "T15", 0x6FC247: "T16",
               0x6FC2CC: "T18", 0x6FC2DA: "T19", 0x6FC36C: "T23", 0x6FC38D: "T23",
               0x6FC3BF: "T24", 0x6FC3EE: "T25", 0x6FC41F: "T26", 0x6FC445: "T27",
               0x6FC47E: "T28", 0x6FC4E5: "T29", 0x6FC546: "T30", 0x6FC577: "T31",
               0x6FC58F: "T32", 0x6FC5AD: "T32", 0x6FC5CF: "T33", 0x6FC600: "T34",
               0x6FC683: "T36", 0x6FC75C: "T39", 0x6FC7CA: "T40",
               0x6FC868: "T40|T41"},   # LandTargeting, shared: T40 via 0x006FC7E9, T41 via 0x006FC855
    0x6FCD29: {0x6FC283: "T17", 0x6FC29D: "T17", 0x6FC2F8: "T20", 0x6FC316: "T20",
               0x6FC333: "T21", 0x6FC350: "T22", 0x6FC619: "T35", 0x6FC62B: "T35",
               0x6FC6B7: "T37", 0x6FC6CF: "T37", 0x6FC6E7: "T37", 0x6FC6FF: "T37"},
    0x74132B: {0x741325: "U13"}, 0x51CAFA: {0x51C9C9: "I4", 0x51C9ED: "I5", 0x51CAF8: "I7"},
    0x51CB8C: {0x51CB86: "I10"},
    0x44805A: {0x447F2D: "B1", 0x447F3F: "B1", 0x447F4E: "B2", 0x447F7C: "B4", 0x447F8F: "B4"},
}
ENTRIES_OF = {  # first instruction of each test (lane sections 2 and 3; T40 after its Techno branch)
    "T1": 0x6FC0BA, "T2": 0x6FC0C5, "T3": 0x6FC0D3, "T4": 0x6FC0EE, "T5": 0x6FC105,
    "T6": 0x6FC117, "T7": 0x6FC125, "T8": 0x6FC133, "T9": 0x6FC13F, "T10": 0x6FC14B,
    "T11": 0x6FC19C, "T12": 0x6FC1BE, "T13": 0x6FC1CC, "T14": 0x6FC1EF, "T15": 0x6FC21C,
    "T16": 0x6FC22A, "T17": 0x6FC24D, "T18": 0x6FC2A3, "T19": 0x6FC2D2, "T20": 0x6FC2E0,
    "T21": 0x6FC31C, "T22": 0x6FC339, "T23": 0x6FC356, "T24": 0x6FC393, "T25": 0x6FC3C5,
    "T26": 0x6FC3F4, "T27": 0x6FC425, "T28": 0x6FC44B, "T29": 0x6FC484, "T30": 0x6FC4EF,
    "T31": 0x6FC54C, "T32": 0x6FC57D, "T33": 0x6FC5B3, "T34": 0x6FC5D5, "T35": 0x6FC606,
    "T36": 0x6FC64F, "T37": 0x6FC689, "T38": 0x6FC705, "T39": 0x6FC73C, "T40": 0x6FC76A,
    "T41": 0x6FC7EB, "T42": 0x6FC879, "T43": 0x6FC8C2, "T44": 0x6FC8F5, "T45": 0x6FC94F,
    "T46": 0x6FC981, "T47": 0x6FCA0D, "T48": 0x6FCA26, "T49": 0x6FCA5E, "T50": 0x6FCA81,
    "T51": 0x6FCAC5, "T52": 0x6FCB02, "T53": 0x6FCB24, "T54": 0x6FCB53, "T55": 0x6FCB8D,
    "T56": 0x6FCBAD, "T57": 0x6FCBCD, "T58": 0x6FCBE6, "T59": 0x6FCC5D, "T60": 0x6FCCBD,
    "T61": 0x6FCCEE, "OK": 0x6FCD1D,
    "U1": 0x740FD9, "U2": 0x740FF2, "U3": 0x741014, "U4": 0x741050, "U5": 0x7410C3,
    "U6": 0x7410EC, "U7": 0x741149, "U8": 0x741172, "U9": 0x7411D9, "U10": 0x741206,
    "U11": 0x741229, "U12": 0x74125C, "U13": 0x741300,
    "I1": 0x51C8B8, "I2": 0x51C8FE, "I3": 0x51C947, "I4": 0x51C9B8, "I5": 0x51C9CF,
    "I6": 0x51C9F3, "I7": 0x51CAD1, "I8": 0x51CB08, "I9": 0x51CB2E, "I10": 0x51CB61,
    "B1": 0x447F15, "B2": 0x447F45, "B3": 0x447F54, "B4": 0x447F6F, "B5": 0x447F95,
    "B6": 0x447FAE, "B7": 0x447FDF, "A1": 0x41A9FF, "A2": 0x41AA1E,
}


def u32(value):
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def f64(value):
    """A JSON number, or "0x" + 16 hex digits of binary64 bits (NaN, exact neighbours)."""
    if isinstance(value, str):
        return struct.pack("<Q", int(value, 16))
    return struct.pack("<d", float(value))


def resolve(row):
    """Merge a sparse row over DEFAULTS; reject unknown or inapplicable fields."""
    extra = set(row) - set(DEFAULTS) - {"name", "covers"}
    if extra:
        raise ValueError(f"{row.get('name')}: unknown inputs {sorted(extra)}")
    full = {}
    for key, default in DEFAULTS.items():
        given = row.get(key)
        if key == "weapons":
            given = given or []
            full[key] = [None if n < len(given) and given[n] is None
                         else {**WEAPON_DEFAULT, **(given[n] if n < len(given) else {})}
                         for n in range(max(2, len(given)))]
            for slot in given:
                if slot and set(slot) - set(WEAPON_DEFAULT):
                    unknown = sorted(set(slot) - set(WEAPON_DEFAULT))
                    raise ValueError(f"{row.get('name')}: unknown weapon fields {unknown}")
        elif isinstance(default, dict):
            merged = dict(default)
            for field, value in (given or {}).items():
                if field not in default:
                    raise ValueError(f"{row.get('name')}: unknown {key}.{field}")
                both = isinstance(default[field], dict) and isinstance(value, dict)
                merged[field] = {**default[field], **value} if both else value
            full[key] = merged
        else:
            full[key] = default if given is None else given
    cls, kind = full["class"], full["target"]["kind"]
    if not isinstance(full["verdicts"]["target_cell"], dict):   # T40 reads it unchecked
        raise ValueError(f"{row.get('name')}: target_cell must be a cell")
    if full["verdicts"]["firer_cell"] not in (None, "target") and not isinstance(
            full["verdicts"]["firer_cell"], dict):
        raise ValueError(f"{row.get('name')}: firer_cell must be a cell, null or \"target\"")
    for group, subject in (("firer", cls), ("firer_type", cls), ("target", kind),
                           ("target_type", kind)):
        for field in row.get(group, {}):
            if field != "kind" and subject not in APPLIES[group][field]:
                raise ValueError(f"{row.get('name')}: {group}.{field} does not apply to {subject}")
    return full


class Fixture:
    def __init__(self):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, REGION)
        u.mem_map(RET_MAGIC, 0x1000)
        for cls, entry in ENTRIES.items():
            assert self.word(VTABLES[cls] + 0x3C0) == entry, cls
        # T59's height unit is a file-static zeroed in the image: run its initializers in order.
        for initializer in LEVEL_HEIGHT_INITIALIZERS:
            u.reg_write(UC_X86_REG_FPCW, 0x027F)          # CRT startup control word
            u.mem_write(SP, u32(RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, initializer, RET_MAGIC, count=20_000)
        self.level_height = struct.unpack("<i", u.mem_read(LEVEL_HEIGHT, 4))[0]
        # A cell target's GetCoords 0x00486840 calls 0x0047B3A0, whose function-local statics
        # (guard byte 0x0089E770) initialise on first use: warm them once, before the write
        # guard, so no row writes outside its stack. The coordinate only feeds the GetCell stub.
        u.mem_write(SP, u32(RET_MAGIC) + u32(COORD_CELL))
        u.reg_write(UC_X86_REG_ECX, TARGET_CELL)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, CELL_GET_COORDS, RET_MAGIC, count=20_000)
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUBS, end=STUBS + 0xFFF)
        for address in NATIVE_STUBS:
            u.hook_add(UC_HOOK_CODE, self.on_native, begin=address, end=address)
        for address in FORBIDDEN:
            u.hook_add(UC_HOOK_CODE, self.on_forbidden, begin=address, end=address)
        for low, high in BODIES:
            u.hook_add(UC_HOOK_CODE, self.on_body, begin=low, end=high - 1)
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)
        self.row = None

    # -- memory helpers
    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def put(self, address, kind, value):
        u = self.u
        if kind == "u8":
            u.mem_write(address, bytes([int(value) & 0xFF]))
        elif kind == "i32":
            u.mem_write(address, u32(value))
        elif kind == "f64":
            u.mem_write(address, f64(value))
        elif kind == "ptr":
            u.mem_write(address, u32(DUMMY if value else 0))
        elif kind == "i32x2":
            u.mem_write(address, u32(value[0]) + u32(value[1]))
        elif kind == "f64x11":
            assert len(value) == 11
            u.mem_write(address, b"".join(f64(v) for v in value))
        elif kind == "facing":            # desired, start, timer (-1, 0, 0), ROT 0
            u.mem_write(address, u32(value & 0xFFFF) * 2 + u32(-1) + u32(0) * 2 + b"\0\0")
        else:
            raise ValueError(kind)

    def put_fields(self, base, fields, values, subject):
        for name, (offset, kind, _, applies) in fields.items():
            if subject in applies:
                address = (FIRER_HOUSE + offset[1]) if isinstance(offset, tuple) else base + offset
                self.put(address, kind, values[name])

    def clone_vtable(self, destination, source, slots):
        self.u.mem_write(destination, bytes(self.u.mem_read(source, 0x600)))
        for offset, name in slots.items():
            self.u.mem_write(destination + offset, u32(STUB_AT[name]))

    def reference(self, value):
        return {None: 0, "target": TARGET, "other": OTHER, "firer": FIRER}[value]

    # -- fixture
    def build(self, row):
        u = self.u
        cls, firer, target = row["class"], row["firer"], row["target"]
        kind, verdicts = target["kind"], row["verdicts"]
        u.mem_write(SCRATCH, bytes(REGION))
        u.mem_write(SP - 0x3000, bytes(0x3100))
        u.mem_write(STUBS, b"\xCC" * 0x1000)
        u.mem_write(FRAME, u32(row["frame"]))
        u.mem_write(RULES_PTR, u32(RULES))
        u.mem_write(RULES + 0x16F8, f64(1.0))
        u.mem_write(FIRER_HOUSE + 0x30, u32(0))
        u.mem_write(TARGET_HOUSE + 0x30, u32(1))
        # The firer.
        self.clone_vtable(FIRER_VT, VTABLES[cls],
                          {**FIRER_SLOTS, **(BUILDING_SLOTS if cls == "building" else {})})
        u.mem_write(FIRER, u32(FIRER_VT))
        u.mem_write(FIRER + 0x14, bytes([FLAGS[cls]]))
        u.mem_write(FIRER + 0x9C, u32(10 * 256 + 128) + u32(10 * 256 + 128))
        u.mem_write(FIRER + 0x21C, u32(FIRER_HOUSE))
        u.mem_write(FIRER + TYPE_AT[cls], u32(FIRER_TYPE))
        u.mem_write(FIRER + 0x2BC, u32(CAPTURE))
        if cls in FOOT:
            u.mem_write(FIRER + 0x674, u32(LOCO))
            u.mem_write(FIRER + 0x69C, u32(PARASITE))
        if cls == "aircraft":
            u.mem_write(FIRER + 0x6C0, u32(FLY_VT))
        if cls == "building":
            u.mem_write(FIRER + 0x694, u32(verdicts["occupants"]) + u32(0) + u32(verdicts["occupants"]))
        self.put_fields(FIRER, FIRER_FIELDS, firer, cls)
        u.mem_write(FIRER + 0x2AC, u32(self.reference(firer["locomotor_target"])))
        u.mem_write(FIRER + 0x1CC, u32(self.reference(firer["drain_target"])))
        transporter = firer["transporter"]
        if transporter == "target":
            u.mem_write(FIRER + 0x11C, u32(TARGET))
        elif transporter is not None:
            u.mem_write(FIRER + 0x11C, u32(TRANSPORT))
            u.mem_write(TRANSPORT, u32(VTABLES["unit"]))
            u.mem_write(TRANSPORT + 0x14, bytes([FLAGS["unit"]]))
            u.mem_write(TRANSPORT + 0x270, bytes([transporter == "warped_out"]))
            u.mem_write(TRANSPORT + 0x11C, u32(DUMMY if transporter == "nested" else 0))
            assert transporter in ("plain", "warped_out", "nested"), transporter
        if firer["temporal"] is not None:
            u.mem_write(FIRER + 0x274, u32(TEMPORAL))
            temporal = {"idle": None}.get(firer["temporal"], firer["temporal"])
            u.mem_write(TEMPORAL + 0x28, u32(self.reference(temporal)))
        slots = firer["spawn_slots"]
        if slots is not None:
            u.mem_write(FIRER + 0x2D0, u32(SPAWNER))
            u.mem_write(SPAWNER + 0x3C, u32(SPAWN_ITEMS))
            u.mem_write(SPAWNER + 0x48, u32(len(slots)))
            for n, slot in enumerate(slots):
                node = SPAWN_NODES + 0x10 * n
                u.mem_write(SPAWN_ITEMS + 4 * n, u32(node))
                u.mem_write(node + 4, u32(slot["state"]))
                child = slot.get("child")
                if child is not None:
                    address = CHILDREN + 0x2000 * n
                    u.mem_write(node, u32(address))
                    u.mem_write(address, u32(VTABLES["aircraft"]))
                    u.mem_write(address + 0x14, bytes([FLAGS["aircraft"]]))
                    u.mem_write(address + 0x81, bytes([child["in_limbo"]]))
                    u.mem_write(address + TYPE_AT["aircraft"], u32(address + 0x1000))
                    u.mem_write(address + 0x1000 + 0xD68, bytes([child["missile_spawn"]]))
        if firer["radio_link"] is not None:
            u.mem_write(FIRER + 0xE4, u32(RADIO_LINKS))
            u.mem_write(RADIO_LINKS, u32(LINK))
            u.mem_write(LINK, u32(VTABLES[firer["radio_link"]]))
            u.mem_write(LINK + 0x14, bytes([FLAGS[firer["radio_link"]]]))
        if firer["turret_upgrade"]:
            u.mem_write(FIRER + 0x702, bytes([1]))
            u.mem_write(FIRER + 0x5EC, u32(UPGRADE_TYPE))
            u.mem_write(UPGRADE_TYPE + 0xCA1, bytes([1]))
        self.put_fields(FIRER_TYPE, FIRER_TYPE_FIELDS, row["firer_type"], cls)
        # Weapons, warhead, projectile.
        for n, weapon in enumerate(row["weapons"]):
            if weapon is None:
                continue
            address = WEAPONS + 0x400 * n
            u.mem_write(FIRER_TYPE + 0x898 + 0x1C * n, u32(address))
            u.mem_write(address + 0xA0, u32(PROJECTILE))
            u.mem_write(address + 0xAC, u32(WARHEAD))
            for name, (offset, kind_, _) in WEAPON_FIELDS.items():
                self.put(address + offset, kind_, weapon[name])
        for fields, base, values in ((WARHEAD_FIELDS, WARHEAD, row["warhead"]),
                                     (PROJECTILE_FIELDS, PROJECTILE, row["projectile"])):
            for name, (offset, kind_, _) in fields.items():
                self.put(base + offset, kind_, values[name])
        # The target.
        if kind in CLASSES:
            self.clone_vtable(TARGET_VT, VTABLES[kind], TARGET_SLOTS)
            u.mem_write(TARGET, u32(TARGET_VT))
            u.mem_write(TARGET + 0x14, bytes([FLAGS[kind]]))
            u.mem_write(TARGET + 0x9C, u32(12 * 256 + 128) + u32(10 * 256 + 128))
            u.mem_write(TARGET + 0x21C, u32(TARGET_HOUSE))
            u.mem_write(TARGET + TYPE_AT[kind], u32(TARGET_TYPE))
            if kind in FOOT:
                u.mem_write(TARGET + 0x674, u32(TARGET_LOCO))
            self.put_fields(TARGET, TARGET_FIELDS, target, kind)
            self.put_fields(TARGET_TYPE, TARGET_TYPE_FIELDS, row["target_type"], kind)
            if kind == "unit":
                u.mem_write(TARGET + 0x2A8, u32(self.reference(target["rocker_link"])))
        elif kind in ("cell", "object"):
            self.clone_vtable(TARGET_VT, VTABLES[kind], TARGET_SLOTS)
            u.mem_write(TARGET, u32(TARGET_VT))
            u.mem_write(TARGET + 0x14, bytes([FLAGS[kind]]))
            if kind == "cell":
                u.mem_write(TARGET + 0x24, struct.pack("<hh", 12, 10))
            else:
                u.mem_write(TARGET + 0x9C, u32(12 * 256 + 128) + u32(10 * 256 + 128))
            self.put_fields(TARGET, TARGET_FIELDS, target, kind)
        # Cells returned by the cell verdicts, the locomotors and the Aircraft fly control.
        for cell, value in ((TARGET_CELL, verdicts["target_cell"]), (FIRER_CELL, verdicts["firer_cell"])):
            if isinstance(value, dict):
                u.mem_write(cell + 0xEC, u32(value["land_type"]))
                u.mem_write(cell + 0x140, u32(value["flags"]))
        for vtable, slots in COM_SLOTS.items():
            for offset, name in slots.items():
                u.mem_write(vtable + offset, u32(STUB_AT[name]))
        for interface, vtable in ((LOCO, LOCO_VT), (PERSIST, PERSIST_VT), (TARGET_LOCO, TARGET_LOCO_VT)):
            u.mem_write(interface, u32(vtable))
        u.mem_write(POWER_SLOT, f64(verdicts["power_fraction"]))
        u.mem_write(POWER_CODE, b"\xDD\x05" + u32(POWER_SLOT) + b"\xC3")   # FLD qword [slot]; RET

    # -- hooks
    def returns(self, value, pops):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, int(value) & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.word(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def arg(self, n):
        return self.word(self.u.reg_read(UC_X86_REG_ESP) + 4 * (n + 1))

    def fail(self, message):
        self.violations.append(message)
        self.u.emu_stop()

    def on_stub(self, u, address, _size, _data):
        name = STUB_NAME.get(address)
        if name is None:
            return self.fail(f"unknown stub 0x{address:08X}")
        this = u.reg_read(UC_X86_REG_ECX)
        verdicts = self.row["verdicts"]
        expected_this = {"in_range": FIRER, "naval_selector": FIRER, "firer_high_flying": FIRER,
                         "firer_cell": FIRER, "occupants": FIRER, "turret_direction": FIRER,
                         "high_flying": TARGET, "low_flying": TARGET, "visual_state": TARGET,
                         "target_layer": TARGET, "target_cell": TARGET}.get(name)
        if expected_this is not None and this != expected_this:
            return self.fail(f"{name} called on 0x{this:08X}")
        if name == "query_interface":
            if bytes(u.mem_read(self.arg(1), 16)) != bytes(u.mem_read(IID_IPERSIST, 16)):
                return self.fail("QueryInterface for an interface other than IPersist")
            u.mem_write(self.arg(2), u32(PERSIST))
            return self.returns(0, STUB_POPS[name])
        if name in ("add_ref", "release"):
            return self.returns(1, STUB_POPS[name])
        if name == "occupants" and self.word(u.reg_read(UC_X86_REG_ESP)) == OCCUPANTS_VIA_GET_WEAPON:
            self.calls.append("occupants_via_get_weapon")
        else:
            self.calls.append(name)
        value = verdicts[name]
        if name == "target_cell":
            value = TARGET_CELL
        elif name == "firer_cell":
            value = FIRER_CELL if isinstance(value, dict) else {None: 0, "target": TARGET}[value]
        elif name == "turret_direction":
            u.mem_write(self.arg(0), u32(value & 0xFFFF))
            value = self.arg(0)
        elif name == "jumpjet_locomotor":
            clsid = bytes(u.mem_read(JUMPJET_CLSID, 16)) if value else DRIVE_CLSID
            u.mem_write(self.arg(1), clsid)
            value = 0
        return self.returns(value, STUB_POPS[name])

    def on_native(self, u, address, _size, _data):
        name, pops = NATIVE_STUBS[address]
        if name == "get_cell":
            if self.word(u.reg_read(UC_X86_REG_ESP)) not in GET_CELL_SITES:
                return self.fail(f"GetCell from 0x{self.word(u.reg_read(UC_X86_REG_ESP)):08X}")
            return self.returns(COORD_CELL, pops)
        self.calls.append(name)
        value = self.row["verdicts"][name]
        if name == "power_fraction":        # binary64 result in ST0: run FLD [slot]; RET
            return u.reg_write(UC_X86_REG_EIP, POWER_CODE)
        if name == "direction_to_target":
            u.mem_write(self.arg(0), u32(value & 0xFFFF))
            value = self.arg(0)
        return self.returns(value, pops)

    def on_forbidden(self, u, address, _size, _data):
        self.fail(f"{FORBIDDEN[address]} called from 0x{self.word(u.reg_read(UC_X86_REG_ESP)):08X}")

    def on_body(self, _u, address, _size, _data):
        self.trace.append(address)

    def on_write(self, u, _access, address, size, _value, _data):
        if not (STACK_BASE <= address and address + size <= STACK_BASE + STACK_SIZE):
            self.fail(f"write outside the stack at 0x{address:08X} from 0x{u.reg_read(UC_X86_REG_EIP):08X}")

    # -- one row
    def decider(self):
        decided = None
        for n, address in enumerate(self.trace):
            if address in EXITS:
                decided = EXITS[address]
            elif address in SHARED_EXITS:
                test = SHARED_EXITS[address].get(self.trace[n - 1])
                if test == "T40|T41":
                    test = "T40" if 0x6FC7E9 in self.trace[:n] else "T41"
                decided = test or decided
        return decided

    def check_covers(self, row, name):
        decided, ran = self.decider(), set(self.trace)
        hits = [tag for tag in row["covers"] if "/" not in tag]
        if hits != [decided]:
            raise OracleError(f"{name}: decided by {decided}, tagged {hits}")
        for tag in row["covers"]:
            test, _, outcome = tag.partition("/")
            evaluated = ENTRIES_OF[test] in ran
            if outcome == "miss" and not evaluated:
                raise OracleError(f"{name}: {test} tagged miss but never ran")
            if outcome == "shadowed" and evaluated:
                raise OracleError(f"{name}: {test} tagged shadowed but ran")
            if outcome not in ("", "miss", "shadowed"):
                raise OracleError(f"{name}: bad tag {tag}")

    def execute(self, sparse):
        row = self.row = resolve(sparse)
        self.calls, self.trace, self.violations = [], [], []
        self.build(row)
        u = self.u
        target = 0 if row["target"]["kind"] == "none" else TARGET
        u.mem_write(SP, u32(RET_MAGIC) + u32(target) + u32(row["weapon_index"])
                    + u32(int(row["check_range"])))
        for register in (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDX, UC_X86_REG_ESI,
                         UC_X86_REG_EDI, UC_X86_REG_EBP):
            u.reg_write(register, 0)
        u.reg_write(UC_X86_REG_ECX, FIRER)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
        try:
            run_checked(u, ENTRIES[row["class"]], RET_MAGIC, count=200_000)
        except OracleError as error:
            if self.violations:
                raise OracleError(f"{sparse['name']}: {self.violations}") from error
            raise OracleError(f"{sparse['name']}: {error}") from error
        if self.violations:
            raise OracleError(f"{sparse['name']}: {self.violations}")
        if u.reg_read(UC_X86_REG_ESP) != SP + 0x10:
            raise OracleError(f"{sparse['name']}: not RET 0xC / unbalanced stack")
        self.check_covers(sparse, sparse["name"])
        code = struct.unpack("<i", u32(u.reg_read(UC_X86_REG_EAX)))[0]
        return {"input": sparse, "code": code, "calls": list(self.calls)}


# ---- rows -------------------------------------------------------------------------------------
def case(name, hit, *, miss=(), shadowed=(), cls="unit", **inputs):
    row = {"name": name, "covers": [hit, *(f"{t}/miss" for t in miss),
                                     *(f"{t}/shadowed" for t in shadowed)], "class": cls}
    row.update(inputs)
    return row


F = DEFAULTS["frame"]
EFFECTS = (("use_fire_particles", "fire_particles_live"), ("is_railgun", "railgun_particles_live"),
           ("use_spark_particles", "spark_particles_live"), ("is_sonic", "wave_live"))
HEALER = [{"damage": -50}, {"damage": -50}]
CURTAIN = {"iron_curtain_start": F - 10, "iron_curtain_left": 100}


def verses(value, armor=0):
    table = [1.0] * 11
    table[armor] = value
    return table


def base_rows():
    """Lane section 2, through every class the test can reach (the class virtuals differ)."""
    out = []

    def each(name, hit, classes=CLASSES, **kw):
        out.extend(case(f"{name}.{c}", hit, cls=c, **kw) for c in classes)

    def healable(c):   # a target the Unit/Infantry healer prefixes accept (U6, I2)
        return {"kind": "infantry" if c == "infantry" else "unit", "health": 50}

    each("T1.null_target", "T1", target={"kind": "none"})
    each("T1.twin.target", "OK", miss=["T1"])
    for test, field, value in (("T2", "enslaved", True), ("T3", "warping_in", 1),
                               ("T5", "warped_out", 1), ("T6", "robot_offline", 1),
                               ("T7", "sinking", 1), ("T19", "falling", 1)):
        each(f"{test}.{field}", test, firer={field: value})
        each(f"{test}.twin.not_{field}", "OK", miss=[test])
    each("T3_T5.warping_in_and_out", "T3", shadowed=["T5"], firer={"warping_in": 1, "warped_out": 1})
    each("T4.lifting_the_target", "T4", firer={"locomotor_target": "target"})
    each("T4.twin.lifting_another", "OK", miss=["T4"], firer={"locomotor_target": "other"})
    each("T8.draining_the_target", "T8", firer={"drain_target": "target"})
    each("T8.twin.draining_another", "OK", miss=["T8"], firer={"drain_target": "other"})
    each("T9.target_is_my_transport", "T9", firer={"transporter": "target"})
    each("T9.twin.other_transport", "OK", miss=["T9"], firer={"transporter": "plain"})
    each("T10.temporal_holds_target", "T10", firer={"temporal": "target"})
    each("T10.twin.temporal_holds_another", "OK", miss=["T10"], firer={"temporal": "other"})
    each("T10.twin.temporal_idle", "OK", miss=["T10"], firer={"temporal": "idle"})
    each("T11.magnetron_lifted", "T11", FOOT, firer={"magnetron_lifted": 1})
    each("T11.twin.not_lifted", "OK", FOOT, miss=["T11"])
    each("T12.target_in_limbo", "T12", target={"in_limbo": 1})
    each("T12.twin.target_on_map", "OK", miss=["T12"])
    each("T13.berserk_vs_berserk_friendly", "T13", firer={"berserk": 1},
         target_type={"berserk_friendly": 1})
    each("T13.twin.berserk_only", "OK", miss=["T13"], firer={"berserk": 1})
    each("T13.twin.berserk_friendly_only", "OK", miss=["T13"], target_type={"berserk_friendly": 1})
    each("T14.natural_vs_unnatural", "T14", firer_type={"natural": 1}, target_type={"unnatural": 1})
    each("T14.twin.natural_only", "OK", miss=["T14"], firer_type={"natural": 1})
    each("T14.twin.unnatural_only", "OK", miss=["T14"], target_type={"unnatural": 1})
    each("T15.target_chrono_warp_latch", "T15", target={"chrono_warp_latch": 1})
    each("T15.twin.no_latch", "OK", miss=["T15"])
    each("T16.computer_vs_iron_curtain", "T16", target=CURTAIN)
    each("T16.curtain_one_frame_left", "T16", target={"iron_curtain_start": F - 99, "iron_curtain_left": 100})
    each("T16.curtain_stopped_time_left", "T16", target={"iron_curtain_start": -1, "iron_curtain_left": 5})
    each("T16.twin.human_vs_iron_curtain", "OK", miss=["T16"], firer={"owner_is_human": 1}, target=CURTAIN)
    each("T16.twin.curtain_expired", "OK", miss=["T16"],
         target={"iron_curtain_start": F - 100, "iron_curtain_left": 100})
    each("T16.twin.curtain_stopped_negative", "OK", miss=["T16"],
         target={"iron_curtain_start": -1, "iron_curtain_left": -5})
    # T17: 0x006F3970(-1) is Damage+AmbientDamage averaged over slots 0/1 (truncating IDIV).
    cloaked, forgiven = {"visual_state": 5}, {"visual_state": 5, "allied": True}
    each("T17.cloaked_unsensed", "T17", verdicts=cloaked)
    each("T17.twin.sensed", "OK", miss=["T17"], verdicts={**cloaked, "sensor": True})
    each("T17.twin.visual_state_4", "OK", miss=["T17"], verdicts={"visual_state": 4})
    each("T17.twin.visual_state_6", "OK", miss=["T17"], verdicts={"visual_state": 6})
    each("T17.zero_damage_not_allied", "T17", verdicts=cloaked, weapons=[{"damage": 0}, {"damage": 0}])
    each("T17.twin.zero_damage_allied", "OK", miss=["T17"], verdicts=forgiven,
         weapons=[{"damage": 0}, {"damage": 0}])
    for c in CLASSES:
        out.append(case(f"T17.healer_not_allied.{c}", "T17", cls=c, verdicts=cloaked,
                        weapons=HEALER, target=healable(c)))
        out.append(case(f"T17.twin.healer_allied.{c}", "OK", miss=["T17"], cls=c, verdicts=forgiven,
                        weapons=HEALER, target=healable(c)))
        out.append(case(f"T17.twin.turret_count_uses_current_weapon.{c}", "OK", miss=["T17"],
                        cls=c, verdicts=forgiven, firer_type={"turret_count": 1},
                        firer={"current_weapon": 0}, weapons=[{"damage": -5}, {"damage": 5}],
                        target=healable(c)))
    for c in CLASSES:   # single-slot damage -1/0/+1 x allied x sensor
        for damage in (-1, 0, 1):
            for sensed, allied in ((False, False), (False, True), (True, False)):
                hit = not sensed and (damage > 0 or not allied)
                name = f"T17.single_slot_damage_{damage}_sensor_{int(sensed)}_allied_{int(allied)}.{c}"
                out.append(case(name,
                                "T17" if hit else "OK", miss=[] if hit else ["T17"], cls=c,
                                verdicts={"visual_state": 5, "sensor": sensed, "allied": allied},
                                weapons=[{"damage": damage}, None], target=healable(c)))
    each("T17.twin.average_truncates_to_zero", "OK", miss=["T17"], verdicts=forgiven,
         weapons=[{"damage": 1}, {"damage": 0}])
    each("T17.twin.negative_average_truncates_to_zero", "OK", miss=["T17"], verdicts=forgiven,
         weapons=[{"damage": -1}, {"damage": 0}])
    each("T17.average_one", "T17", verdicts=forgiven, weapons=[{"damage": 2}, {"damage": 0}])
    each("T17.single_slot", "T17", verdicts=forgiven, weapons=[{"damage": 1}, None])
    each("T17.ambient_damage_counts", "T17", verdicts=forgiven,
         weapons=[{"damage": 0, "ambient_damage": 1}, None])
    each("T17.twin.ambient_damage_cancels", "OK", miss=["T17"], verdicts=forgiven,
         weapons=[{"damage": 1, "ambient_damage": -1}, None])
    each("T17.turret_count_uses_current_weapon", "T17", verdicts=forgiven,
         firer_type={"turret_count": 1}, firer={"current_weapon": 1},
         weapons=[{"damage": -5}, {"damage": 5}])
    each("T17.twin.gattling_averages", "OK", miss=["T17"], verdicts=forgiven,
         firer_type={"turret_count": 1, "is_gattling": 1}, firer={"current_weapon": 1},
         weapons=[{"damage": -5}, {"damage": 5}])
    each("T17_T21.no_weapons_cloaked", "T17", shadowed=["T21"], verdicts=cloaked, weapons=[None, None])
    each("T17_T21.no_weapons_cloaked_allied", "T21", miss=["T17"], verdicts=forgiven, weapons=[None, None])
    balloon = {"target": {"docked": 1}, "target_type": {"balloon_hover": 1}}
    each("T18.docked_balloon_low", "T18", **balloon)
    each("T18.twin.docked_balloon_high", "OK", miss=["T18"], verdicts={"high_flying": True}, **balloon)
    each("T18.twin.docked_not_balloon", "OK", miss=["T18"], target={"docked": 1})
    each("T18.twin.balloon_not_docked", "OK", miss=["T18"], target_type={"balloon_hover": 1})
    # T20: a Building's EMP is caught by its own B5 first (see building_rows).
    each("T20.emp", "T20", FOOT, firer={"emp_remaining": 1})
    each("T20.twin.emp_zero", "OK", FOOT, miss=["T20"], firer={"emp_remaining": 0})
    each("T20.twin.emp_negative", "OK", FOOT, miss=["T20"], firer={"emp_remaining": -1})
    each("T20_T21.emp_without_weapon", "T20", FOOT, shadowed=["T21"], firer={"emp_remaining": 1},
         weapons=[None])
    out.append(case("T20.unit_death_counter_minus_two", "T20", miss=["U1"],
                    firer={"death_frame_counter": -2}))
    out.append(case("T20.twin.unit_emp_large_visceroid", "OK", miss=["T20"],
                    firer={"emp_remaining": 1}, firer_type={"large_visceroid": 1}))
    out.append(case("T20.twin.unit_emp_small_visceroid", "OK", miss=["T20"],
                    firer={"emp_remaining": 1}, firer_type={"small_visceroid": 1}))
    each("T21.no_weapon", "T21", weapons=[None])
    each("T21.slot_one_empty", "T21", weapon_index=1, weapons=[{}, None])
    each("T21.twin.weapon", "OK", miss=["T21"])
    each("T22.twin.ion_sensitive_is_dead_code", "OK", miss=["T22"], weapons=[{"ion_sensitive": 1}])
    each("T22.twin.not_ion_sensitive", "OK", miss=["T22"])
    drain = [{"drain_weapon": 1}]
    each("T23.second_drainer", "T23", weapons=drain, target={"draining_me": True},
         target_type={"drainable": 1})
    each("T23.not_drainable", "T23", weapons=drain)
    each("T23.twin.drainable", "OK", miss=["T23"], weapons=drain, target_type={"drainable": 1})
    each("T23.twin.cell_target", "OK", miss=["T23"], weapons=drain, target={"kind": "cell"})
    bunker = {"bunkered": True}
    each("T24.bunkered_tank_range_383", "T24", target=bunker, weapons=[{"range": 383}])
    each("T24.bunkered_tank_negative_range", "T24", target=bunker, weapons=[{"range": -1}])
    each("T24.twin.range_384", "OK", miss=["T24"], target=bunker, weapons=[{"range": 384}])
    each("T24.twin.bunkered_infantry", "OK", miss=["T24"], target={"kind": "infantry", **bunker},
         weapons=[{"range": 383}])
    out.append(case("T25.deploying", "T25", firer={"deploying": 1}))
    out.append(case("T25.undeploying", "T25", firer={"undeploying": 1}))
    out.append(case("T25.twin.settled", "OK", miss=["T25"]))
    gas = {"psychedelic": 1}
    each("T26.gas_vs_psionic_immune", "T26", warhead=gas, target_type={"immune_to_psionics": 1})
    each("T26.twin.gas_vs_normal", "OK", miss=["T26", "T27"], warhead=gas)
    each("T26.twin.immune_no_gas", "OK", miss=["T26"], target_type={"immune_to_psionics": 1})
    each("T27.gas_vs_bunkered_infantry", "T27", miss=["T24", "T26"], warhead=gas,
         target={"kind": "infantry", **bunker})
    each("T27.gas_vs_bunkered_tank_long_range", "T27", miss=["T24"], warhead=gas, target=bunker,
         weapons=[{"range": 384}])
    magnet = {"is_locomotor": 1}
    each("T28.magnet_vs_deploying", "T28", warhead=magnet, target={"deploying": 1})
    each("T28.magnet_vs_undeploying", "T28", warhead=magnet, target={"undeploying": 1})
    each("T28.twin.magnet_vs_settled", "OK", miss=["T28", "T29", "T30", "T31"], warhead=magnet)
    each("T28.twin.deploying_no_magnet", "OK", miss=["T28"], target={"deploying": 1})
    each("T29.magnet_vs_moving_jumpjet", "T29", warhead=magnet, target_type={"jumpjet": 1},
         verdicts={"target_locomotor_moving": True})
    each("T29.magnet_vs_moving_jumpjet_infantry", "T29", warhead=magnet,
         target={"kind": "infantry"}, target_type={"jumpjet": 1},
         verdicts={"target_locomotor_moving": True})
    each("T29.twin.jumpjet_parked", "OK", miss=["T29"], warhead=magnet, target_type={"jumpjet": 1})
    each("T29.twin.building_target", "OK", miss=["T29"], warhead=magnet,
         target={"kind": "building"}, target_type={"jumpjet": 1},
         verdicts={"target_locomotor_moving": True})
    each("T30.magnet_vs_unloading_simple_deployer", "T30", warhead=magnet, target={"mission": 0x10},
         target_type={"is_simple_deployer": 1})
    each("T30.twin.simple_deployer_guarding", "OK", miss=["T30"], warhead=magnet,
         target_type={"is_simple_deployer": 1})
    each("T30.twin.unloading_not_simple", "OK", miss=["T30"], warhead=magnet, target={"mission": 0x10})
    each("T31.magnet_vs_organic", "T31", warhead=magnet, target={"kind": "infantry"},
         target_type={"organic": 1})
    each("T31.twin.organic_no_magnet", "OK", miss=["T31"], target={"kind": "infantry"},
         target_type={"organic": 1})
    riding = {"in_open_transport": 1, "transporter": "plain"}
    each("T32.open_topped_no_fire_in_transport", "T32", firer=riding)
    each("T32.transport_warped_out", "T32", firer={**riding, "transporter": "warped_out"},
         weapons=[{"fire_in_transport": 1}])
    each("T32.twin.fire_in_transport", "OK", miss=["T32", "T33"], firer=riding,
         weapons=[{"fire_in_transport": 1}])
    each("T32.twin.closed_transport", "OK", miss=["T32"], firer={"transporter": "plain"})
    each("T33.transport_inside_transport", "T33", firer={**riding, "transporter": "nested"},
         weapons=[{"fire_in_transport": 1}])
    each("T33.twin.no_transporter", "OK", miss=["T32", "T33"], firer={"in_open_transport": 1},
         weapons=[{"fire_in_transport": 1}])
    each("T34.warped_target", "T34", target={"warped_out": 1})
    each("T34.twin.temporal_warhead", "OK", miss=["T34", "T36"], warhead={"temporal": 1},
         target={"warped_out": 1})
    spawner, one_spawn = [{"spawner": 1}], [{"state": 0}]
    each("T35.spawner_on_bridge", "T35", weapons=spawner, firer={"spawn_slots": one_spawn},
         verdicts={"bridge_for_firing": True})
    each("T35.all_spawns_regenerating", "T35", weapons=spawner,
         firer={"spawn_slots": [{"state": 7}, {"state": 7}]})
    each("T35.no_spawn_slots", "T35", weapons=spawner, firer={"spawn_slots": []})
    each("T35.twin.one_spawn_ready", "OK", miss=["T35"], weapons=spawner,
         firer={"spawn_slots": [{"state": 7}, {"state": 0}]})
    each("T35.paralysed", "T35", FOOT, weapons=spawner,
         firer={"spawn_slots": one_spawn, "paralysis_start": F - 10, "paralysis_left": 11})
    each("T35.twin.paralysis_ended", "OK", FOOT, miss=["T35"], weapons=spawner,
         firer={"spawn_slots": one_spawn, "paralysis_start": F - 10, "paralysis_left": 10})
    chrono = {"temporal": 1}
    each("T36.chrono_vs_spawned_aircraft", "T36", warhead=chrono, target={"kind": "aircraft"},
         target_type={"spawned": 1})
    each("T36.twin.not_spawned", "OK", miss=["T36"], warhead=chrono, target={"kind": "aircraft"})
    each("T36.twin.spawned_unit", "OK", miss=["T36"], warhead=chrono, target_type={"spawned": 1})
    for flag, live in EFFECTS:
        each(f"T37.other_slot_{flag}_live", "T37", weapons=[{}, {flag: 1}], firer={live: True})
        each(f"T37.twin.other_slot_{flag}_idle", "OK", miss=["T37"], weapons=[{}, {flag: 1}])
        each(f"T37.twin.{live}_no_flag", "OK", miss=["T37"], firer={live: True})
        each(f"T46.own_{flag}_live", "T46", weapons=[{flag: 1}], firer={live: True})
        each(f"T46.twin.own_{flag}_idle", "OK", miss=["T46"], weapons=[{flag: 1}])
    each("T37.slot1_fires_slot0_busy", "T37", weapon_index=1, weapons=[{"is_sonic": 1}, {}],
         firer={"wave_live": True})
    each("T37.slot2_fires_slot0_busy", "T37", weapon_index=2, weapons=[{"is_sonic": 1}, {}, {}],
         firer={"wave_live": True})
    each("T37.twin.slot2_fires_slot1_busy", "OK", miss=["T37"], weapon_index=2,
         weapons=[{}, {"is_sonic": 1}, {}], firer={"wave_live": True})
    each("T37.twin.other_slot_empty", "OK", miss=["T37"], weapons=[{}, None], firer={"wave_live": True})
    each("T37_T45.other_busy_while_rearming", "T37", shadowed=["T45"], weapons=[{}, {"is_sonic": 1}],
         firer={"wave_live": True, "rearm_start": -1, "rearm_left": 5})
    each("T38.high_target_no_aa", "T38", projectile={"aa": 0}, verdicts={"high_flying": True})
    each("T38.twin.high_target_aa", "OK", miss=["T38"], verdicts={"high_flying": True})
    each("T38.twin.low_target_no_aa", "OK", miss=["T38", "T39"], projectile={"aa": 0})
    each("T4_T38.lifted_high_target", "T4", shadowed=["T38"], firer={"locomotor_target": "target"},
         projectile={"aa": 0}, verdicts={"high_flying": True})
    each("T39.off_ground_layer_no_aa", "T39", projectile={"aa": 0}, verdicts={"target_layer": 3})
    each("T39.twin.ground_layer", "OK", miss=["T39"], projectile={"aa": 0})
    each("T39.twin.aa_projectile", "OK", miss=["T39"], verdicts={"target_layer": 3})
    each("T39.twin.building_target", "OK", miss=["T39"], projectile={"aa": 0},
         target={"kind": "building"}, verdicts={"target_layer": 3})
    water = {"target_cell": {"land_type": 2}}
    each("T40.naval_refused_on_water", "T40", verdicts={**water, "naval_selector": -1})
    each("T40.naval_refused_on_beach", "T40", verdicts={"target_cell": {"land_type": 6},
                                                        "naval_selector": -1})
    each("T40.twin.naval_accepted", "OK", miss=["T40"], verdicts=water)
    each("T40.twin.water_but_high", "OK", miss=["T40"],
         verdicts={**water, "naval_selector": -1, "high_flying": True})
    each("T40.twin.water_but_on_bridge", "OK", miss=["T40"], target={"on_bridge": 1},
         verdicts={**water, "naval_selector": -1})
    each("T40.land_targeting_refuses_ground", "T40", firer_type={"land_targeting": 1})
    each("T40.twin.land_targeting_target_not_low", "OK", miss=["T40"],
         firer_type={"land_targeting": 1}, verdicts={"low_flying": False})
    each("T40.twin.land_targeting_on_water", "OK", miss=["T40"], firer_type={"land_targeting": 1},
         verdicts=water)
    each("T40.twin.land_targeting_2", "OK", miss=["T40"], firer_type={"land_targeting": 2})
    cell, obj = {"kind": "cell"}, {"kind": "object"}
    each("T41.cell_needs_ag", "T41", target=cell, projectile={"ag": 0})
    each("T41.object_needs_ag", "T41", target=obj, projectile={"ag": 0})
    each("T41.twin.object_high", "OK", miss=["T41"], target=obj, projectile={"ag": 0},
         verdicts={"high_flying": True})
    each("T41.cell_land_targeting", "T41", target=cell, firer_type={"land_targeting": 1})
    each("T41.twin.water_cell_land_targeting", "OK", miss=["T41"], target={**cell, "land_type": 2},
         firer_type={"land_targeting": 1})
    each("T41.twin.beach_cell_land_targeting", "OK", miss=["T41"], target={**cell, "land_type": 6},
         firer_type={"land_targeting": 1})
    each("T41.twin.object_land_targeting", "OK", miss=["T41"], target=obj,
         firer_type={"land_targeting": 1})
    each("T41.twin.cell_ag", "OK", miss=["T41"], target=cell)
    beam = [{"is_mag_beam": 1}]
    each("T42.beam_busy_on_another", "T42", weapons=beam,
         firer={"locomotor_target": "other", "wave_live": True})
    each("T42.twin.building_target", "OK", miss=["T42", "T43"], weapons=beam,
         firer={"locomotor_target": "other", "wave_live": True}, target={"kind": "building"})
    each("T42.twin.no_wave", "OK", miss=["T42"], weapons=beam, firer={"locomotor_target": "other"})
    each("T42.twin.beam_idle", "OK", miss=["T42", "T43"], weapons=beam, firer={"wave_live": True})
    each("T43.dominated_by_T4", "T4", shadowed=["T43"], weapons=beam,
         firer={"locomotor_target": "target", "wave_live": True})
    each("T45.stopped_time_left", "T45", firer={"rearm_start": -1, "rearm_left": 5})
    each("T45.stopped_negative_left", "T45", firer={"rearm_start": -1, "rearm_left": -5})
    each("T45.one_frame_left", "T45", firer={"rearm_start": F - 9, "rearm_left": 10})
    each("T45.started_in_the_future", "T45", firer={"rearm_start": F + 5, "rearm_left": 10})
    each("T45.frame_wrap", "T45", frame=5, firer={"rearm_start": 0x7FFFFFF0, "rearm_left": 10})
    each("T45.twin.stopped_zero", "OK", miss=["T45"], firer={"rearm_start": -1, "rearm_left": 0})
    each("T45.twin.elapsed_equals_delay", "OK", miss=["T45"], firer={"rearm_start": F - 10, "rearm_left": 10})
    each("T45.twin.elapsed_past_delay", "OK", miss=["T45"], firer={"rearm_start": F - 11, "rearm_left": 10})
    each("T45_T47.rearming_no_ammo", "T45", shadowed=["T47"],
         firer={"rearm_start": -1, "rearm_left": 5, "ammo": 0})
    each("T45_T61.rearming_out_of_range", "T45", shadowed=["T61"],
         firer={"rearm_start": -1, "rearm_left": 5}, verdicts={"in_range": False})
    each("T47.no_ammo", "T47", firer={"ammo": 0})
    for ammo in (1, -1, -2):
        each(f"T47.twin.ammo_{ammo}", "OK", miss=["T47"], firer={"ammo": ammo})
    each("T47_T48.no_ammo_cloaked", "T47", shadowed=["T48"], firer={"ammo": 0, "cloak_state": 2})
    for state in (1, 2, 3):
        for c in CLASSES:
            hits = state == 2 or c != "aircraft"
            out.append(case(f"T48.cloak_state_{state}.{c}", "T48" if hits else "OK",
                            miss=[] if hits else ["T48"], cls=c, firer={"cloak_state": state}))
    each("T48.twin.uncloaked", "OK", miss=["T48"])
    each("T48.twin.no_decloak_to_fire", "OK", miss=["T48"], firer={"cloak_state": 2},
         weapons=[{"decloak_to_fire": 0}])
    each("T48_T49.cloaked_hunter_seeker", "T48", shadowed=["T49"], firer={"cloak_state": 2},
         firer_type={"hunter_seeker": 1})
    each("T49.hunter_seeker", "T49", firer_type={"hunter_seeker": 1})
    each("T49.hunter_seeker_without_range_check", "T49", check_range=False,
         firer_type={"hunter_seeker": 1})
    each("T49.twin.not_hunter_seeker", "OK", miss=["T49"], check_range=False)
    leech = {"parasite": 1}
    each("T50.cannot_infect", "T50", FOOT, warhead=leech, verdicts={"can_infect": False})
    each("T50.cannot_infect_building", "T50", FOOT, warhead=leech, target={"kind": "building"},
         verdicts={"can_infect": False})
    each("T50.twin.can_infect", "OK", FOOT, miss=["T50", "T51", "T52"], warhead=leech)
    out.append(case("T50.twin.building_is_not_foot", "OK", cls="building", miss=["T50"],
                    warhead=leech, verdicts={"can_infect": False}))
    each("T51.target_launch_locked", "T51", warhead=leech, target={"parasite_lock_until": F + 1})
    each("T51.twin.lock_ends_this_frame", "OK", miss=["T51"], warhead=leech,
         target={"parasite_lock_until": F})
    each("T52.parasite_vs_iron_curtain", "T52", warhead=leech, firer={"owner_is_human": 1},
         target=CURTAIN)
    each("T52.twin.no_curtain", "OK", miss=["T52"], warhead=leech)
    each("T53.cannot_capture", "T53", warhead={"mind_control": 1}, verdicts={"can_capture": False})
    each("T53.twin.can_capture", "OK", miss=["T53"], warhead={"mind_control": 1})
    each("T54.verses_zero", "T54", warhead={"verses": verses(0.0)})
    each("T54.verses_negative_zero", "T54", warhead={"verses": verses(-0.0)})
    each("T54.verses_nan", "T54", warhead={"verses": verses("0x7ff8000000000000")})
    each("T54.armor_3_zero", "T54", warhead={"verses": verses(0.0, 3)}, target_type={"armor": 3})
    each("T54.twin.smallest_subnormal", "OK", miss=["T54"],
         warhead={"verses": verses("0x0000000000000001")})
    each("T54.twin.negative", "OK", miss=["T54"], warhead={"verses": verses(-0.5)})
    each("T54.twin.other_armor_zero", "OK", miss=["T54"], warhead={"verses": verses(0.0, 3)})
    each("T55.disarm_unbombed", "T55", warhead={"bomb_disarm": 1})
    each("T55.twin.disarm_bombed", "OK", miss=["T55"], warhead={"bomb_disarm": 1}, target={"bomb": True})
    each("T56.ivan_bomb_bombed", "T56", warhead={"ivan_bomb": 1}, target={"bomb": True})
    each("T56.twin.ivan_bomb_unbombed", "OK", miss=["T56"], warhead={"ivan_bomb": 1})
    each("T57.target_sinking", "T57", target={"sinking": 1})
    each("T57.twin.afloat", "OK", miss=["T57"])
    deck = {"target_cell": {"flags": 0x100}, "firer_cell": {"flags": 0x100}}
    upper = {"on_bridge": 1}
    each("T58.firer_below_target_on_bridge", "T58", target=upper, verdicts=deck)
    each("T58.firer_on_bridge_target_below", "T58", firer=upper, verdicts=deck)
    each("T58.twin.both_on_bridge", "OK", miss=["T58"], firer=upper, target=upper, verdicts=deck)
    each("T58.twin.firer_cell_no_bridge", "OK", miss=["T58", "T59"], target=upper,
         verdicts={**deck, "firer_cell": {"flags": 0}})
    each("T58.twin.firer_cell_null", "OK", miss=["T58", "T59"], target=upper,
         verdicts={**deck, "firer_cell": None})
    each("T58.twin.firer_high", "OK", miss=["T58", "T59"], target=upper,
         verdicts={**deck, "firer_high_flying": True})
    each("T58.twin.target_cell_no_bridge", "OK", miss=["T58", "T59"], target=upper,
         verdicts={**deck, "target_cell": {"flags": 0}})
    each("T58.twin.flag_0x800_only", "OK", miss=["T58"], target=upper,
         verdicts={"target_cell": {"flags": 0x800}, "firer_cell": {"flags": 0x800}})
    each("T59.parasite_across_deck_above", "T59", miss=["T58"], warhead=leech,
         target={"on_bridge": 1, "z": 209})
    each("T59.parasite_across_deck_below", "T59", miss=["T58"], warhead=leech,
         target={"on_bridge": 1, "z": -209})
    each("T59.twin.two_levels", "OK", miss=["T58", "T59"], warhead=leech, target={"on_bridge": 1, "z": 208})
    each("T59.twin.no_parasite", "OK", miss=["T59"], target={"on_bridge": 1, "z": 400})
    each("T60.organic_paralysed", "T60", FOOT, firer_type={"organic": 1},
         firer={"paralysis_start": F - 10, "paralysis_left": 11})
    each("T60.organic_stopped_negative_paralysis", "T60", FOOT, firer_type={"organic": 1},
         firer={"paralysis_start": -1, "paralysis_left": -5})
    each("T60.twin.paralysis_ended", "OK", FOOT, miss=["T60"], firer_type={"organic": 1},
         firer={"paralysis_start": F - 10, "paralysis_left": 10})
    each("T60.twin.paralysed_not_organic", "OK", FOOT, miss=["T60"],
         firer={"paralysis_start": F - 10, "paralysis_left": 11})
    out.append(case("T60.twin.organic_building", "OK", cls="building", miss=["T60"],
                    firer_type={"organic": 1}))
    each("T61.out_of_range", "T61", verdicts={"in_range": False})
    each("T61.twin.range_not_checked", "OK", miss=["T61"], check_range=False, verdicts={"in_range": False})
    each("T61.twin.in_range", "OK", miss=["T61"])
    return out


def unit_rows():
    out = []

    def add(name, hit, **kw):
        out.append(case(name, hit, cls="unit", **kw))

    add("U1.death_counter_zero", "U1", firer={"death_frame_counter": 0})
    add("U1.death_counter_five", "U1", firer={"death_frame_counter": 5})
    add("U1_T1.dying_null_target", "U1", shadowed=["T1"], firer={"death_frame_counter": 0},
        target={"kind": "none"})
    add("U1.twin.counter_minus_one", "OK", miss=["U1"])
    add("U1.twin.counter_minus_two_null_target", "T1", miss=["U1"], firer={"death_frame_counter": -2},
        target={"kind": "none"})
    missile = {"state": 2, "child": {"in_limbo": 0, "missile_spawn": 1}}
    add("U2.spawn_launching", "U2", firer={"spawn_slots": [{"state": 1}]})
    add("U2.missile_in_flight", "U2", firer={"spawn_slots": [{"state": 7}, missile]})
    add("U2_T1.busy_null_target", "U2", shadowed=["T1"], firer={"spawn_slots": [{"state": 1}]},
        target={"kind": "none"})
    add("U2.twin.missile_in_limbo", "OK", miss=["U2"],
        firer={"spawn_slots": [{"state": 2, "child": {"in_limbo": 1, "missile_spawn": 1}}]})
    add("U2.twin.plane_in_flight", "OK", miss=["U2"],
        firer={"spawn_slots": [{"state": 2, "child": {"in_limbo": 0, "missile_spawn": 0}}]})
    add("U2.twin.state_two_without_child", "OK", miss=["U2"], firer={"spawn_slots": [{"state": 2}]})
    add("U2.twin.idle_and_regenerating", "OK", miss=["U2"],
        firer={"spawn_slots": [{"state": 7}, {"state": 0}]})
    add("U2.twin.no_spawn_manager", "OK", miss=["U2"])
    add("U3.rocker_link", "U3", firer={"rocker_link": True})
    add("U3.twin.no_rocker_link", "OK", miss=["U3"])
    add("U4.must_deploy", "U4", firer_type={"deploy_to_fire": 1}, verdicts={"deploy_cell_ok": False})
    add("U4.twin.deploy_cell_ok", "OK", miss=["U4"], firer_type={"deploy_to_fire": 1})
    add("U4.twin.not_deploy_to_fire", "OK", miss=["U4"], verdicts={"deploy_cell_ok": False})
    add("T45_U4.base_nonzero_skips_suffix", "T45", shadowed=["U4"],
        firer={"rearm_start": -1, "rearm_left": 5}, firer_type={"deploy_to_fire": 1},
        verdicts={"deploy_cell_ok": False})
    add("U5.docked_at_building", "U5", firer={"tethered": 1, "radio_link": "building"})
    add("U5.twin.tethered_to_unit", "OK", miss=["U5"], firer={"tethered": 1, "radio_link": "unit"})
    add("U5.twin.untethered", "OK", miss=["U5"], firer={"radio_link": "building"})
    add("U6.healer_vs_full_health_vehicle", "U6", weapons=HEALER)
    add("U6.healer_above_full_health", "U6", weapons=HEALER, target={"health": 101})
    add("U6.healer_vs_non_vehicle", "U6", weapons=HEALER, target={"health": 50},
        target_type={"non_vehicle": 1})
    add("U6.healer_vs_infantry", "U6", weapons=HEALER, target={"kind": "infantry", "health": 50})
    add("U6.healer_vs_high_aircraft", "U6", weapons=HEALER, target={"kind": "aircraft", "health": 50},
        verdicts={"low_flying": False})
    add("U6.healer_vs_building", "U6", weapons=HEALER, target={"kind": "building", "health": 50})
    add("U6.healer_vs_cell", "U6", weapons=HEALER, target={"kind": "cell"})
    add("U6.healer_vs_object", "U6", weapons=HEALER, target={"kind": "object"})
    add("U6.healer_vs_zero_strength", "U6", weapons=HEALER, target={"health": 50},
        target_type={"strength": 0})
    add("U6.average_minus_one", "U6", weapons=[{"damage": -2}, {"damage": 0}])
    add("U6.twin.healer_vs_damaged_vehicle", "OK", miss=["U6"], weapons=HEALER, target={"health": 50})
    add("U6.twin.healer_vs_99_percent", "OK", miss=["U6"], weapons=HEALER, target={"health": 99})
    add("U6.twin.healer_vs_low_aircraft", "OK", miss=["U6"], weapons=HEALER,
        target={"kind": "aircraft", "health": 50})
    add("U6.twin.healer_ratio_nan", "OK", miss=["U6"], weapons=HEALER, target={"health": 0},
        target_type={"strength": 0})
    add("U6.twin.average_truncates_to_zero", "OK", miss=["U6"], weapons=[{"damage": -1}, {"damage": 0}])
    add("U7.no_mobile_fire_moving", "U7", firer={"navcom": True}, firer_type={"mobile_fire": 0})
    add("U7.twin.no_mobile_fire_parked", "OK", miss=["U7"], firer_type={"mobile_fire": 0})
    add("U7.twin.mobile_fire_moving", "OK", miss=["U7", "U8", "U9", "U10"], firer={"navcom": True})
    add("U8.no_fire_while_moving", "U8", firer={"navcom": True}, weapons=[{"fire_while_moving": 0}])
    add("U8.balloon_locomotor_moving", "U8", firer_type={"balloon_hover": 1},
        weapons=[{"fire_while_moving": 0}], verdicts={"locomotor_moving": True})
    add("U8.twin.no_fire_while_moving_parked", "OK", miss=["U8"], weapons=[{"fire_while_moving": 0}])
    add("U8.twin.balloon_still_with_navcom", "OK", miss=["U8"], firer={"navcom": True},
        firer_type={"balloon_hover": 1}, weapons=[{"fire_while_moving": 0}])
    add("U9.spark_particles_moving", "U9", firer={"navcom": True}, weapons=[{"use_spark_particles": 1}])
    add("U9.fire_particles_moving", "U9", firer={"navcom": True}, weapons=[{"use_fire_particles": 1}])
    add("U9.twin.spark_particles_parked", "OK", miss=["U9"], weapons=[{"use_spark_particles": 1}])
    add("U10.temporal_idle_moving", "U10", firer={"navcom": True, "temporal": "idle"})
    add("U10.temporal_holding_another_moving", "U10", firer={"navcom": True, "temporal": "other"})
    add("U10.twin.temporal_parked", "OK", miss=["U10"], firer={"temporal": "idle"})
    add("U11.turret_rotating_straight_projectile", "U11", firer={"turret_rotation_latch": 1})
    add("U11.twin.homing_projectile", "OK", miss=["U11"], firer={"turret_rotation_latch": 1},
        projectile={"rot": 1})
    add("U11.twin.firing_sequence_set", "OK", miss=["U11"],
        firer={"turret_rotation_latch": 1, "firing_sequence": 1})
    for rot, deltas in ((0, (0x7FF, 0x800, 0x801, 0xF801, 0xF800, 0xF7FF, 0x8000)),
                        (1, (0xFFF, 0x1000, 0x1001, 0xF001, 0xF000, 0xEFFF)), (-1, (0x1000, 0x1001))):
        tolerance = 0x1000 if rot else 0x800
        for delta in deltas:
            signed = delta - 0x10000 if delta & 0x8000 else delta
            hit = abs(signed) > tolerance
            add(f"U12.rot_{rot}_delta_{delta:#06x}", "U12" if hit else "OK",
                miss=[] if hit else ["U12"], firer={"primary_facing": delta}, projectile={"rot": rot})
    add("U12.turret_uses_secondary_facing", "U12", firer={"secondary_facing": 0x4000},
        firer_type={"turret": 1})
    add("U12.direction_off", "U12", firer={"primary_facing": 0x1234},
        verdicts={"direction_to_target": 0x1234 + 0x801})
    add("U12.twin.turret_ignores_primary_facing", "OK", miss=["U12"], firer={"primary_facing": 0x4000},
        firer_type={"turret": 1})
    add("U12.twin.omni_fire", "OK", miss=["U12"], firer={"primary_facing": 0x4000},
        weapons=[{"omni_fire": 1}])
    add("U12.twin.small_visceroid", "OK", miss=["U12"], firer={"primary_facing": 0x4000},
        firer_type={"small_visceroid": 1})
    add("U12.twin.large_visceroid", "OK", miss=["U12"], firer={"primary_facing": 0x4000},
        firer_type={"large_visceroid": 1})
    add("U12.twin.direction_matches", "OK", miss=["U12"], firer={"primary_facing": 0x1234},
        verdicts={"direction_to_target": 0x1234})
    add("U13.locomotor_cannot_fire", "U13", verdicts={"locomotor_can_fire": 7})
    add("U13.twin.locomotor_can_fire", "OK", miss=["U13"])
    add("T45_U7.base_nonzero_skips_suffix", "T45", shadowed=["U7"],
        firer={"rearm_start": -1, "rearm_left": 5, "navcom": True}, firer_type={"mobile_fire": 0})
    add("T61_U12.range_before_facing", "T61", shadowed=["U12"], firer={"primary_facing": 0x4000},
        verdicts={"in_range": False})
    # T44 FiringSyncFrame: Unit only; weapon index 0; burst index % Burst (signed IDIV).
    sync = {"firing_sync_frame": [5, -1]}
    add("T44.sync_frame_mismatch", "T44", firer={"firing_frame": 3}, firer_type=sync)
    add("T44.second_sync_frame", "T44", firer={"burst_index": 1, "firing_frame": 4},
        firer_type={"firing_sync_frame": [-1, 7]}, weapons=[{"burst": 2}])
    add("T44.negative_remainder_reads_facings", "T44", firer={"burst_index": -1, "firing_frame": 3},
        weapons=[{"burst": 2}])
    add("T44.sync_frame_match_skips_rearm", "OK", miss=["T44"], shadowed=["T45"],
        firer={"firing_frame": 5, "rearm_start": -1, "rearm_left": 5}, firer_type=sync)
    add("T44.negative_remainder_matches_facings", "OK", miss=["T44"], shadowed=["T45"],
        firer={"burst_index": -1, "firing_frame": 8, "rearm_start": -1, "rearm_left": 5},
        weapons=[{"burst": 2}])
    add("T44.twin.no_firing_frame", "OK", miss=["T44"], firer={"firing_frame": -1}, firer_type=sync)
    add("T44.twin.no_sync_frame", "OK", miss=["T44"], firer={"firing_frame": 3})
    add("T44.twin.remainder_two", "OK", miss=["T44"], firer={"burst_index": 2, "firing_frame": 4},
        firer_type={"firing_sync_frame": [7, 7]}, weapons=[{"burst": 5}])
    add("T44.twin.weapon_index_one", "OK", miss=["T44"], weapon_index=1, firer={"firing_frame": 3},
        firer_type=sync)
    for c in ("infantry", "aircraft", "building"):
        out.append(case(f"T44.twin.burst_zero_not_a_unit.{c}", "OK", miss=["T44"], cls=c,
                        weapons=[{"burst": 0}]))
    return out


def infantry_rows():
    out = []

    def add(name, hit, **kw):
        out.append(case(name, hit, cls="infantry", **kw))

    for sequence in (0xB, 0xC, 0xD, 0xE, 0xF, 0x14, 0x15, 0x22, 0x23, 0x24):
        add(f"I1.dying_sequence_{sequence:#x}_null_target", "I1", shadowed=["T1"],
            firer={"sequence": sequence}, target={"kind": "none"})
    for sequence in (0xA, 0x10, 0x13, 0x16, 0x21, 0x25, -1):
        add(f"I1.twin.sequence_{sequence:#x}_null_target", "T1", miss=["I1"],
            firer={"sequence": sequence}, target={"kind": "none"})
    add("I1.dying_with_target", "I1", firer={"sequence": 0xB})
    add("I2.medic_null_target", "I2", shadowed=["T1"], weapons=HEALER, target={"kind": "none"})
    add("I2.medic_vs_vehicle", "I2", weapons=HEALER)
    add("I2.medic_vs_cell", "I2", weapons=HEALER, target={"kind": "cell"})
    add("I2.medic_vs_healthy_infantry", "I2", weapons=HEALER, target={"kind": "infantry"})
    add("I2.twin.medic_vs_wounded_infantry", "OK", miss=["I2"], weapons=HEALER,
        target={"kind": "infantry", "health": 50})
    add("I2.twin.not_a_medic", "OK", miss=["I2"], target={"kind": "infantry"})
    add("I3.pushy_vs_rocked_unit", "I3", firer_type={"pushy": 1}, target={"rocker_link": "other"})
    add("I3.twin.rocked_by_me", "OK", miss=["I3"], firer_type={"pushy": 1}, target={"rocker_link": "firer"})
    add("I3.twin.unrocked", "OK", miss=["I3"], firer_type={"pushy": 1})
    add("I3.twin.not_pushy", "OK", miss=["I3"], target={"rocker_link": "other"})
    add("I3.twin.infantry_target", "OK", miss=["I3"], firer_type={"pushy": 1}, target={"kind": "infantry"})
    add("I4.speed_above_threshold", "I4", firer={"speed_fraction": "0x3fb999999999999b"})
    add("I4.full_speed", "I4", firer={"speed_fraction": 1.0})
    add("I4.twin.speed_threshold", "OK", miss=["I4"], firer={"speed_fraction": 0.1})
    add("I4.twin.speed_below_threshold", "OK", miss=["I4"], firer={"speed_fraction": "0x3fb9999999999999"})
    add("I4.twin.speed_nan", "OK", miss=["I4"], firer={"speed_fraction": "0x7ff8000000000000"})
    for sequence in (5, 7, 0x1B, 0x1F, 0x20):
        add(f"I5.moving_in_action_{sequence:#x}", "I5", firer={"navcom": True, "sequence": sequence})
    add("I5.twin.moving_ready", "OK", miss=["I5"], firer={"navcom": True})
    add("I5.twin.moving_no_sequence", "OK", miss=["I5"], firer={"navcom": True, "sequence": -1})
    add("I5.twin.parked_in_action", "OK", miss=["I5"], firer={"sequence": 5})
    jumpjet = {"jumpjet": 1, "jumpjet_turn": 1}
    add("I6.jumpjet_turning_in_flight", "I6", firer_type=jumpjet,
        verdicts={"jumpjet_locomotor": True, "locomotor_moving": True})
    add("I6.twin.jumpjet_hovering", "OK", miss=["I6"], firer_type=jumpjet,
        verdicts={"jumpjet_locomotor": True})
    add("I6.twin.no_jumpjet_turn", "OK", miss=["I6"], firer_type={"jumpjet": 1},
        verdicts={"jumpjet_locomotor": True, "locomotor_moving": True})
    add("I6.twin.other_locomotor", "OK", miss=["I6"], firer_type=jumpjet,
        verdicts={"locomotor_moving": True})
    add("I6.twin.not_jumpjet_type", "OK", miss=["I6"], firer_type={"jumpjet_turn": 1},
        verdicts={"jumpjet_locomotor": True, "locomotor_moving": True})
    add("I7.fire_particles_moving", "I7", firer={"navcom": True}, weapons=[{"use_fire_particles": 1}])
    add("I7.twin.fire_particles_parked", "OK", miss=["I7"], weapons=[{"use_fire_particles": 1}])
    add("I7.twin.spark_particles_moving", "OK", miss=["I7"], firer={"navcom": True},
        weapons=[{"use_spark_particles": 1}])
    add("I8.area_fire_at_another_cell", "I8", weapons=[{"area_fire": 1}], target={"kind": "cell"})
    add("I8.area_fire_at_unit", "I8", weapons=[{"area_fire": 1}])
    add("I8.twin.area_fire_at_own_cell", "OK", miss=["I8"], weapons=[{"area_fire": 1}],
        target={"kind": "cell"}, verdicts={"firer_cell": "target"})
    add("I9.ivan_bomb_on_bombed_object", "I9", warhead={"ivan_bomb": 1},
        target={"kind": "object", "bomb": True})
    add("I9.twin.unbombed_object", "OK", miss=["I9"], warhead={"ivan_bomb": 1}, target={"kind": "object"})
    add("I9.twin.cell_target", "OK", miss=["I9"], warhead={"ivan_bomb": 1}, target={"kind": "cell"})
    add("T56_I9.bombed_unit_refused_by_base", "T56", shadowed=["I9"], warhead={"ivan_bomb": 1},
        target={"bomb": True})
    add("I10.locomotor_cannot_fire", "I10", verdicts={"locomotor_can_fire": 7})
    add("I10.twin.locomotor_can_fire", "OK", miss=["I10"])
    add("T45_I4.base_nonzero_skips_suffix", "T45", shadowed=["I4"],
        firer={"rearm_start": -1, "rearm_left": 5, "speed_fraction": 1.0})
    return out


def building_rows():
    out = []

    def add(name, hit, **kw):
        out.append(case(name, hit, cls="building", **kw))

    garrison = {"can_be_occupied": 1, "can_occupy_fire": 1}
    add("B1.garrison_cannot_fire", "B1", firer_type={"can_be_occupied": 1})
    add("B1.empty_garrison", "B1", firer_type=garrison, verdicts={"occupants": 0})
    add("B1.twin.manned_garrison", "OK", miss=["B1"], firer_type=garrison, verdicts={"occupants": 2})
    add("B1.twin.not_occupiable", "OK", miss=["B1"], verdicts={"occupants": 0})
    add("B2.drained", "B2", firer={"draining_me": True})
    add("B2.twin.not_drained", "OK", miss=["B2"])
    add("B3.emp_pulse_cannon", "B3", firer_type={"emp_pulse_cannon": 1})
    add("B3.twin.ordinary", "OK", miss=["B3"])
    add("B4.selling", "B4", firer={"mission": 0x13})
    add("B4.constructing", "B4", firer={"mission": 0x12})
    add("B4.queued_selling", "B4", firer={"mission": -1, "queued_mission": 0x13})
    add("B4.twin.sabotage", "OK", miss=["B4"], firer={"mission": 0x11})
    add("B4.twin.queued_attack", "OK", miss=["B4"], firer={"mission": -1, "queued_mission": 1})
    add("B4_B5.selling_before_operational", "B4", shadowed=["B5"], firer={"mission": 0x13, "online": 0})
    add("B5.offline", "B5", firer={"online": 0})
    add("B5.offline_one_charger", "B5", firer={"online": 0, "tesla_chargers": 1})
    add("B5.twin.offline_two_chargers", "OK", miss=["B5"], firer={"online": 0, "tesla_chargers": 2})
    add("B5_T20.emp", "B5", shadowed=["T20"], firer={"emp_remaining": 1})
    add("B5.twin.emp_zero", "OK", miss=["B5"], firer={"emp_remaining": 0})
    add("B5.dead", "B5", firer={"health": 0})
    powered = {"powered": 1, "power_drain": 10}
    add("B5.low_power", "B5", firer_type=powered, verdicts={"power_fraction": 0.5})
    add("B5.power_just_short", "B5", firer_type=powered,
        verdicts={"power_fraction": "0x3fefffffffffffff"})
    add("B5.twin.full_power", "OK", miss=["B5"], firer_type=powered)
    add("B5.twin.low_power_two_chargers", "OK", miss=["B5"], firer={"tesla_chargers": 2},
        firer_type=powered, verdicts={"power_fraction": 0.5})
    add("B5.twin.powered_without_drain", "OK", miss=["B5"], firer_type={"powered": 1},
        verdicts={"power_fraction": 0.5})
    add("B5.twin.drain_not_powered", "OK", miss=["B5"], firer_type={"power_drain": 10},
        verdicts={"power_fraction": 0.5})
    special = {"powered_special": 1}
    add("B5.blackout", "B5", firer={"owner_blackout_start": F - 1, "owner_blackout_left": 10},
        firer_type=special)
    add("B5.twin.blackout_over", "OK", miss=["B5"],
        firer={"owner_blackout_start": F - 10, "owner_blackout_left": 10}, firer_type=special)
    add("B5.drained_power_source", "B5", firer={"owner_drained_power_source": 1}, firer_type=special)
    add("B5.twin.blackout_not_special", "OK", miss=["B5"],
        firer={"owner_blackout_start": F - 1, "owner_blackout_left": 10})
    add("B5.needs_engineer", "B5", firer_type={"needs_engineer": 1})
    add("B5.twin.has_engineer", "OK", miss=["B5"], firer={"has_engineer": 1},
        firer_type={"needs_engineer": 1})
    add("B6.delayed_fire_pending", "B6", firer={"delayed_fire_counter": 1})
    add("B6_T1.pending_null_target", "B6", shadowed=["T1"], firer={"delayed_fire_counter": 1},
        target={"kind": "none"})
    add("B6.twin.none_pending", "OK", miss=["B6"])
    for voxel, deltas in ((0, (0x7FF, 0x800, 0x801, 0xF801, 0xF800, 0xF7FF, 0x8000)),
                          (1, (0, 1, 0xFFFF))):
        tolerance = 0 if voxel else 0x800
        for delta in deltas:
            signed = delta - 0x10000 if delta & 0x8000 else delta
            hit = abs(signed) > tolerance
            add(f"B7.voxel_{voxel}_delta_{delta:#06x}", "B7" if hit else "OK",
                miss=[] if hit else ["B7"], firer={"primary_facing": delta},
                firer_type={"turret": 1, "turret_anim_is_voxel": voxel})
    add("B7.turret_from_upgrade", "B7", firer={"primary_facing": 0x4000, "turret_upgrade": True})
    add("B7.twin.no_turret", "OK", miss=["B7"], firer={"primary_facing": 0x4000})
    add("B7.twin.turret_direction_matches", "OK", miss=["B7"], firer={"primary_facing": 0x4000},
        firer_type={"turret": 1}, verdicts={"turret_direction": 0x4000})
    add("T45_B7.base_nonzero_skips_suffix", "T45", shadowed=["B7"],
        firer={"primary_facing": 0x4000, "rearm_start": -1, "rearm_left": 5}, firer_type={"turret": 1})
    return out


def aircraft_rows():
    out = []

    def add(name, hit, **kw):
        out.append(case(name, hit, cls="aircraft", **kw))

    add("A1.payload_plane_empty", "A1", firer={"paradrop_payload": 1})
    add("A1.twin.payload_aboard", "OK", miss=["A1"], firer={"paradrop_payload": 1, "has_passenger": True})
    add("A1.twin.not_a_payload_plane", "OK", miss=["A1"])
    add("A1_A2.empty_payload_plane_facing_away", "A1", shadowed=["A2"],
        firer={"paradrop_payload": 1, "secondary_facing": 0x4000})
    for delta in (0x7FF, 0x800, 0x801, 0xF801, 0xF800, 0xF7FF, 0x8000):
        signed = delta - 0x10000 if delta & 0x8000 else delta
        hit = abs(signed) > 0x800
        add(f"A2.delta_{delta:#06x}", "A2" if hit else "OK", miss=[] if hit else ["A2"],
            firer={"secondary_facing": delta})
    add("A2.twin.fighter_ignores_facing", "OK", miss=["A2"], firer={"secondary_facing": 0x4000},
        verdicts={"fighter": True})
    add("A2.twin.primary_facing_ignored", "OK", miss=["A2"], firer={"primary_facing": 0x4000})
    add("T47_A2.base_nonzero_skips_suffix", "T47", shadowed=["A1", "A2"],
        firer={"ammo": 0, "paradrop_payload": 1, "secondary_facing": 0x4000})
    return out


def rows():
    return base_rows() + unit_rows() + infantry_rows() + building_rows() + aircraft_rows()


def generate():
    fields = [*DEFAULTS, *WEAPON_FIELDS, *VERDICTS, "occupants_via_get_weapon",
              *(field for group in APPLIES.values() for field in group)]
    undocumented = sorted({field for field in fields if field not in __doc__})
    if undocumented:
        raise OracleError(f"inputs missing from the docstring schema: {undocumented}")
    fixture = Fixture()
    if fixture.level_height != 104:
        raise OracleError(f"[0x00B0EB34] initialised to {fixture.level_height}, expected 104")
    all_rows = rows()
    names = [row["name"] for row in all_rows]
    if len(set(names)) != len(names):
        raise OracleError("duplicate row names")
    return {"defaults": DEFAULTS, "rows": [fixture.execute(row) for row in all_rows]}


def metadata():
    return provenance(
        scope="UnitClass 0x00740FD0, InfantryClass 0x0051C8B0, BuildingClass 0x00447F10 and "
              "AircraftClass 0x0041A9E0 GetFireError, each running TechnoClass::GetFireError "
              "0x006FC0B0: returned code and ordered substituted-call names per row, covering "
              "every test T1..T61, U1..U13, I1..I10, B1..B7, A1..A2 with a deciding row and a "
              "row where it runs and does not decide (T22 dead, T43 dominated by T4: see "
              "covers). Not consumer behaviour, weapon selection or the stubbed leaves.",
        assumptions=[
            "One emulator; per row the scratch region and stack frame are rewritten, general "
            "registers zeroed, FPCW 0x0E7F with an empty x87 stack; a write hook proves no "
            "native write outside the stack, so no state carries between rows.",
            "[0x00B0EB34] = 104 from its static initializers 0x006F28A0, 0x006F28D0, "
            "0x006F28F0, 0x006F2910, 0x006F2930, 0x006F2950, 0x006F2970 run in address order "
            "at FPCW 0x027F; 0x0047B3A0's local statics warmed once through CellClass "
            "GetCoords 0x00486840 before the write hook.",
            "Frame [0x00A8ED84] per row (1000 unless stated); Rules [0x008871E0]+0x16F8 = 1.0.",
            "Original vtables cloned to scratch with only the stubbed slots replaced; each "
            "clone's +0x3C0 equals the called class entry.",
            "Type pointer Unit/Aircraft +0x6C4, Infantry +0x6C0, Building +0x520; AbstractFlags "
            "+0x14 = 7 Foot, 3 Building, 2 TerrainClass object, 0 cell; firer house index 0, "
            "target house 1; veterancy 0.0, so GetWeapon returns TechnoType+0x898+0x1C*i.",
            "Building occupant vector count +0x694 = the occupants verdict with the fire index "
            "+0x69C at its end, so Building GetWeapon 0x004526F0 resolves the type's own slots.",
            "All weapon slots share one warhead and one projectile; only the fired weapon's are "
            "read. FacingClass fixtures have ROT 0, so Current 0x004C93D0 returns the value.",
            "Excluded because they fault natively: Burst 0 with weapon index 0 on a Unit (T44 "
            "IDIV), a Spawner weapon without SpawnManager (T35), a NULL warhead (T36), a NULL "
            "target cell (T40), a NULL radio link while tethered (U5), weapon index -1 "
            "(GetWeapon returns NULL, dereferenced at 0x006FC32B); a NULL projectile, read "
            "unchecked wherever T38, T39, T41, U11 or U12 reads it, is never used.",
            "covers tags are checked against the executed trace: the deciding return block "
            "and each test's first instruction (lane sections 2 and 3).",
        ],
        substitutions=[
            "Firer vt+0x3A8 in_range, vt+0x2E8 naval_selector, vt+0x54 firer_high_flying, "
            "vt+0x1BC firer_cell; Building vt+0x408 occupants, vt+0x4E8 turret_direction: "
            "return the row's verdict, logged.",
            "Target vt+0x50 low_flying, vt+0x54 high_flying, vt+0x68 visual_state, vt+0x78 "
            "target_layer, vt+0x1BC target_cell: return the row's verdict, logged.",
            "Direct calls CellClass 0x004870D0 sensor, HouseClass 0x004F9A50 allied, "
            "0x00703B10 bridge_for_firing, 0x0062A8E0 can_infect, 0x00471C90 can_capture, "
            "CellClass 0x00487C10 deploy_cell_ok, 0x005F3DB0 direction_to_target, HouseClass "
            "0x004FCE30 power_fraction (binary64 via FLD): the row's verdict, logged.",
            "Scratch ILocomotion objects: firer +0x80 locomotor_moving, +0x8C "
            "locomotor_can_fire; target +0x80 target_locomotor_moving; IPersist +0xC "
            "jumpjet_locomotor writes the CLSID at 0x007E9AC0 or the Drive CLSID; logged. "
            "IUnknown QueryInterface (IID_IPersist 0x00818858 only), AddRef, Release: silent.",
            "Aircraft IFlyControl at +0x6C0: scratch vtable, +0x1C fighter, logged.",
            "MapClass::GetCell 0x00565730 at its two direct sites (returns 0x006FC19C, "
            "0x0074107D): a scratch cell, silent; any other caller fails the row.",
        ],
        entry_points={**{f"{c}_get_fire_error": a for c, a in ENTRIES.items()},
                      "techno_get_fire_error": BASE, "damage_query": 0x6F3970,
                      "building_operational": 0x4555D0, "building_get_weapon": 0x4526F0,
                      "level_height_initializer": 0x6F2970})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
