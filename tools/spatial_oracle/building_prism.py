"""Original Prism Tower support: recruitment, bonus, support beam, cadence and reader.

Retail gamemd.exe under Unicorn (identity in the meta file). Groups A-D share
one building fixture; E uses building_body_rules' INI fixture.

Fixture (A-D). building_construction's building_fixture (the slave_manager
fixture's building, BuildingClass vtable 0x7E3EBC, Location (3200, 3200, 0),
owner HOUSE) is the first building: `master` (`supporter` in `beam` rows).
After its setup its 0x720 bytes (the BuildingClass size operator new
allocates, 0x44FAF8) are copied into each tower the row lists. Every building
then gets: Location = the first building's + the row's `offset` (leptons),
Health +0x6C (default 600), alive byte +0x90, type +0x520, Mission +0xAC
(default guard) and queued mission +0xB4 (default none), mission timer
+0xC0/+0xC8 = {frame0, 0, 0}, +0x6DD = 1, TarCom +0x2B4 (`target`, the fixture
unit, or 0), rearm timer +0x2EC/+0x2F0/+0x2F4 = {frame0 + `rearm`[0] (-1 for
null), 0, `rearm`[1]} (default {frame0 - 100, 0, 0}: run out), drain link
+0x1D0, Prism count +0x664, delayed-fire mode +0x704, payload
+0x708/+0x70C/+0x710 and countdown +0x714, and turret counter +0x148 = 0;
veterancy +0x150 is 0.0 (the master's is the row's `veterancy` when given).
The owner house's building vector (HOUSE+0x68: Items +0x6C, Capacity +0x70,
Count +0x78; its vtable is never called) holds the row's `vector` (default:
the row's `order`, else the first building then the towers in row order; null
is a null entry). The tower type (slave_manager's type) is PrismType
(Rules+0x498): Strength +0xA0 600, DelayedFireDelay +0x16EC 28, SpecialAnim
+0x11F4 "GAPRIS_A" and SpecialAnimDamaged +0x1204 "GAPRIS_AD" inline,
IsAnimDelayedFire +0x16A7 set, +0x16F0 -1, bytes
+0x16B8/+0x16C3/+0x16B5/+0x157B/+0xCA1/+0xCD5/+0x5E4 clear, Weapon[0] +0x898 =
PrismShot and Weapon[1] +0x8B4 = PrismSupport, supplied WeaponTypes with a
supplied Projectile and Range +0xB4 2048 (Weapon[1]: the row's
`support_range`; `elite_support_range` adds EliteWeapon[1] +0xAB0). A byte
copy of the type at another address is the non-Prism type (`type: other`).
Rules PrismSupportModifier/Max/Delay/Duration/Height (+0x49C..+0x4AC) are
150/8/45/15/420 unless the row's `rules` says otherwise; ConditionYellow
+0x1700 = 0.5 (double); MissionControl Rate/AARate as in
building_guard_attack. House LaserColor +0x56FC = (0x12, 0x34, 0x56). The
laser vector 0xABC878 = {Items, Capacity 16, Count 0}. FPCW 0x0E7F. Every anim
slot +0x55C..+0x5AF is empty (asserted). The fixture frame is 200 (`frame0`);
timer starts in rows are relative, in output absolute frames.

Common hooks (A-D). SelectWeapon 0x6F3330 answers weapon 0. GetFLH 0x453840
writes building n's sentinel (7001 + 100n, 7002 + 100n, 7003 + 100n) to its
out argument (n = 0 for the first building, then the towers in row order).
PlayAnim 0x451890 is recorded and answered. DestroyNthAnim 0x451E40 is
answered by the inherited refinery_dock observer (a no-op on empty slots) and
recorded at its Prism call sites 0x44B532/0x44B5CD. Is_Operational 0x4555D0
answers from the row's `powered` (default true).

Output conventions: buildings by name, the fixture unit `target`, 0 as null,
other pointers as hex strings; missions by building_guard_attack's names plus
`construction` (0x12), else the number; values are signed 32-bit. `state` =
{count +0x664, mode +0x704, payload [+0x708, +0x70C, +0x710], countdown
+0x714, turret_counter +0x148, rearm [+0x2EC start, +0x2F4 delay], mission
+0xAC, queued +0xB4, target +0x2B4}.

`recruit` rows (A). One native BuildingClass::Mission_Attack 0x44ACF0 on the
master (Mission Attack unless `master.mission` says otherwise, TarCom
`target`); BuildingClass::GetFireError 0x447F10 answered 0. Native: the
PrismType arm 0x44B2F8..0x44B62B with 0x459EF0, 0x70FEC0, Get_Mission
0x5B3040, Get_Weapon_Range 0x7012C0 -> 0x4526F0 -> 0x70E140 (veterancy +0x150
against 0x7E37B4), Sqrt_Approx 0x4CAC40 (table 0x8650BC), ftol 0x7C5F00, the
health ratio 0x5F5C60 and the tail 0x44B6D6. The 4 KiB below the entry stack
pointer holds 0xCCCCCCCC, so a stale stack word the arm copies reads
-858993460. Fields:
- `returns`: Mission_Attack's return (frames to the next dispatch).
- `walk`: [vector index, building, outcome] per index reached; outcome is the
  last test reached: null 0x44B380, dead 0x44B388, wrong_type 0x44B396,
  rearming 0x44B3AE, delayed_fire 0x44B3D4, drained 0x44B3E2, attacking
  0x44B3F1, self 0x44B404, out_of_range 0x44B40C (rejected by the distance),
  not_nearer 0x44B49E (admitted, not strictly nearer) or best 0x44B4A8
  (admitted, the new best).
- `distances`: [building, native distance, Get_Weapon_Range(1)] at 0x44B49A,
  leptons.
- `recruited`: EBX at 0x44B4CB, or null. `armed`: 0x44B595 reached.
- `calls`, in order: [select_weapon, target]; [fire_error, target, weapon,
  range flag, answer]; [flh, building, weapon index, base [x, y, z]];
  [destroy_anim, building, slot]; [play_anim, building, name, name offset in
  the type, slot, damaged, arg4, arg5].
- `master`, `towers` {name: ...}: `state` after the call.
- `random_indices`: {before, after}: the Scenario RNG's indices (RandomClass
  [0xA8B230]+0x218: +4, +8) around the call.

`bonus` rows (B). Native ProcessDelayedFire 0x4503F0 on the master (row
`master`: default mode 1, countdown 1, TarCom `target`, mission attack, count
per row). BuildingClass::GetFireError 0x447F10 answers the row's `fire_error`
(default 0); Fire_At 0x6FDD50 answers a fresh bullet buffer whose +0x150 holds
BulletClass::Construct's 0x100 (0x466546), or 0 for `bullet: false`. Fields:
`calls` ([fire_error, target, weapon, range flag, answer], [fire_at, target,
weapon]), `bullet_multiplier` (the buffer's +0x150 afterwards, 1/256 units;
null when none was handed out), `master` (`state`), `random_indices` (as A).

`damage` rows (B). DetonateAtCoord's block 0x469A56..0x469A83 native: ESI = a
bullet with +0x150 = `multiplier`, +0x6C = `damage`, +0xB0 = the master,
+0x128 = a warhead sentinel; [EBP+8] = a coordinate sentinel. Fields: `damage`
(EDX at the DamageArea call 0x469A83), `args` (its pushed [firer, warhead, 1,
house]), `coords` (ECX).

`beam` rows (C). Native ProcessDelayedFire 0x4503F0 on the first building,
`supporter` (default mode 2, countdown 1, payload (7101, 7102, 7103), the FLH
sentinel of a master in slot 1, count 2, rearm {frame0 - 50, 45}; +0x2F8 = 45
and +0x2F0 = 0x5A5A5A5A markers), through 0x44ABD0 and the LaserDrawClass
constructor 0x54FE60, both native. Operator new 0x7C8E17 answers a
16-byte-aligned heap cursor (0 for `new_fails`). Fields: `calls` ([new, size,
heap offset or null], [flh, ...] as A, [laser, {the constructor's arguments:
src, dst, z_adjust, flag, inner, outer, spread (RGB), duration, blinks, fades,
f1, f2}]); `laser`: the registered object (start frame +0x8,
field_c/_10/_14/_18, width +0x1C, house_color +0x20, field_21, src +0x24, dst
+0x30, z_adjust +0x3C, field_40, inner +0x41, outer +0x44, spread +0x47,
duration +0x4C, blinks +0x50, field_51, fades +0x52, f1 +0x54, f2 +0x58,
heap_offset) or null; `lasers_registered` (0xABC888); `supporter`: `state`
plus rearm_middle +0x2F0 and rof_copy +0x2F8; `random_indices` (as A).

`cadence` rows (D). Per frame k = 1..`frames` (Frame 0xA8ED84 = frame0 + k):
the row's events for k, then for each building in the row's Logic `order`
(default: the first building, then the towers) the ready-commence block
0x43FE27..0x43FE54, MissionClass::AI 0x5B3060 (Mission_Guard 0x4496B0 /
Mission_Attack 0x44ACF0), the ready-commence block 0x43FF91..0x43FFB4 and
ProcessDelayedFire 0x4503F0, all native. BState comes from Begin_Mode(1)
0x447780 on the first building before the towers are copied.
BuildingClass::GetFireError 0x447F10 runs natively. TechnoClass::GetFireError
0x6FC0B0 runs its prologue (and its null-target path) natively; with a target,
0x6FC0BF is redirected to the native rearm test 0x6FC94F (REARM tail
0x6FC972), and its not-rearming exit 0x6FC981 to the native RANGE tail
0x6FCD0E when the row has the building out of range and the range flag
argument is set, else to the OK tail 0x6FCD1D; the checks between are not run.
Fire_At 0x6FDD50 writes +0x2F8 = ROF, +0x2EC = Frame, +0x2F4 = ROF (the row's
`rof`, retail PrismShot 45) and answers a fresh bullet buffer (+0x150 =
0x100). Operator new is the heap cursor; the laser constructor runs natively.
TechnoClass::SetTarget 0x6FCDB0 writes +0x2B4; FacingClass::Set_Desired
0x4C9220, the direction 0x43ED40, StartUncloaking 0x7036C0 and ClearBibArea
0x449540 are answered; IsCloseEnough 0x6F7780 answers true, IsHumanPlayer
0x50B730 and the EMP test 0x70EFD0 false (as building_guard_attack). Events
[k, action, building]: acquire / lose (TarCom = `target` / 0: the passive
scan's or a detach's write), unpower / power (Is_Operational), out_of_range /
in_range (the RANGE answer above), stop (the IDLE event's
BuildingClass::SetTarget(0) 0x443B90, native; its other steps are not run).
Fields: `frames`: [{frame: k, towers: {building: {mission, count, mode,
countdown, rearm [start, delay], target, events}}}] after the building's turn;
events in order: guard / attack [return] (MissionClass::AI after the handler,
0x5B32F2 / 0x5B317C), fire_error [code] (Mission_Attack's GetFireError,
0x44B015), recruit [supporter] (0x44B4CB), arm (0x44B595), expiry_error [code]
(the expiry's GetFireError, 0x45047C), shot [target, weapon index, bullet
+0x150 after ProcessDelayedFire], beam [destination] (0x44ABD0), set_target
[target], draw [low, high] (RandomRanged 0x65C7E0, native), draws [n]
(Scenario RNG steps in the building's turn, when not 0).

`reader` rows (E). RulesClass::ReadGeneral's [General] gate 0x66D53C..0x66D558
and its Prism block 0x671130..0x6711FE, native on the building_body_rules INI
fixture after the CRT float-scanner initializer 0x7C8F5E. FPCW 0x0E7F, the
fixture's: the game's own ftol 0x7C5F00 loads [0x822D80] = 0x0E7F whenever the
control word differs and never restores it, and tube_startup_capture records
0x0E7F from the CRT initializers to WinMain, so ReadDouble's percent fmul
(0x52857E) and the x100 fmul (0x67116E) round toward zero at 53 bits: `35%`,
`14%` and `57%` read 34, 13 and 56 (35, 14 and 57 under a nearest-rounding
control word). Each pass supplies a
CCINIClass: a section index at +0x28 (native CRC 0x4A1DE0 keys, sorted)
holding [General] when the pass has one, and the section's entry index at
+0x2C holding the pass's keys; the reader's section cache (+0x4/+0x8) starts
empty. BuildingTypeClass::FindOrAllocate 0x4653C0 runs natively over a
supplied BuildingTypes vector (0xA83C6C/0xA83C78: GAPOWR, ATESLA). Fields:
`input` (`passes`: key -> raw string, or null for no [General];
`initial.modifier` written after the defaults), `defaults` (the RulesClass
constructor slice 0x665CE8..0x665D1D over 0x5A5A5A5A-filled fields), `passes`:
per pass {general (the gate let the block run), prism_type (a type name or
null), modifier, max, delay, duration, height} afterwards.

Usage: python -m tools.spatial_oracle.building_prism [--check|--write]
"""
from pathlib import Path
import hashlib
import sys
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.engineer_bridge_cursor_caller import TEXT_BEGIN, TEXT_SIZE
from tools.spatial_oracle import building_body_rules as body_rules
from tools.spatial_oracle import building_construction as bc
from tools.spatial_oracle import building_guard_attack as bga
from tools.spatial_oracle import slave_manager as sm
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import ACTOR, HOUSE, RULES
from tools.spatial_oracle.unit_source_scatter import SCENARIO
from tools.spatial_oracle.unit_scatter_state import SP
from tools.procedural_drawing_oracle import rally
from tools.input_oracle.fast_scroll import return_from_sink

MISSION_ATTACK, MISSION_GUARD, PROCESS_DELAYED_FIRE, SUPPORT_BEAM = 0x44ACF0, 0x4496B0, 0x4503F0, 0x44ABD0
GET_FIRE_ERROR, TECHNO_GET_FIRE_ERROR, FIRE_AT, SELECT_WEAPON = 0x447F10, 0x6FC0B0, 0x6FDD50, 0x6F3330
GET_FLH, PLAY_ANIM, OPERATOR_NEW, LASER_CTOR = 0x453840, 0x451890, 0x7C8E17, 0x54FE60
IS_OPERATIONAL, TECHNO_SET_TARGET, BUILDING_SET_TARGET = 0x4555D0, 0x6FCDB0, 0x443B90
SET_DESIRED, DIRECTION_TO, UNCLOAK, CLEAR_BIB = 0x4C9220, 0x43ED40, 0x7036C0, 0x449540
IS_CLOSE_ENOUGH, IS_HUMAN, UNDER_EMP, RANDOM_RANGED = 0x6F7780, 0x50B730, 0x70EFD0, 0x65C7E0
# The Prism arm's walk (0x44B370..0x44B4BD): the instruction each admission
# test starts at, named by the outcome when it is the last one reached.
WALK_TOP, DISTANCE_TEST = 0x44B370, 0x44B49A
WALK_TESTS = {0x44B380: 'null', 0x44B388: 'dead', 0x44B396: 'wrong_type', 0x44B3AE: 'rearming',
              0x44B3D4: 'delayed_fire', 0x44B3E2: 'drained', 0x44B3F1: 'attacking', 0x44B404: 'self',
              0x44B40C: 'out_of_range', 0x44B49E: 'not_nearer', 0x44B4A8: 'best'}
RECRUIT, MASTER_ARM, DESTROY_ANIM_CALLS = 0x44B4CB, 0x44B595, (0x44B532, 0x44B5CD)
# TechnoClass::GetFireError: after its prologue, the rearm test, its
# not-rearming exit, and the RANGE and OK tails.
TECHNO_AFTER_PROLOGUE, TECHNO_REARM_TEST, TECHNO_NOT_REARMING = 0x6FC0BF, 0x6FC94F, 0x6FC981
TECHNO_RANGE_TAIL, TECHNO_OK_TAIL = 0x6FCD0E, 0x6FCD1D
# MissionClass::AI after the Attack and Guard handlers return.
ATTACK_RETURNED, GUARD_RETURNED = 0x5B317C, 0x5B32F2
ATTACK_FIRE_ERROR, EXPIRY_FIRE_ERROR = 0x44B015, 0x45047C
DAMAGE_BLOCK = (0x469A56, 0x469A83)
LASERS = 0xABC878  # DynamicVectorClass: Items +4, Capacity +8, IsAllocated +0xD, Count +0x10, Growth +0x14
FRAME = bc.FRAME
BUILDING_SIZE = 0x720
PRISM, PRISM_SIZE = 0x23000000, 0x60000
TOWERS, TOWER_STRIDE = PRISM, 0x800  # up to 16 towers
VECTOR = PRISM + 0x8000
SHOT_WEAPON, SUPPORT_WEAPON, ELITE_SUPPORT_WEAPON, PROJECTILE = (PRISM + 0x9000 + n * 0x200 for n in range(4))
OTHER_TYPE, TYPE_SIZE = PRISM + 0xA000, 0x1798  # BuildingTypeClass size (0x465416)
WARHEAD, DRAINER, COORD = PRISM + 0xC000, PRISM + 0xC800, PRISM + 0xCC00
BULLETS, BULLET_STRIDE = PRISM + 0x10000, 0x200
HEAP, HEAP_END = PRISM + 0x20000, PRISM + 0x40000
LASER_ITEMS = PRISM + 0x40000
STALE = 0xCCCCCCCC
LASER_COLOR = (0x12, 0x34, 0x56)
MISSION = dict(bga.MISSION, construction=0x12)
MISSION_NAME = {value: name for name, value in MISSION.items()}
RETAIL_RULES = dict(modifier=150, max=8, delay=45, duration=15, height=420)


def signed(value):
    return struct.unpack('<i', struct.pack('<I', value & 0xFFFFFFFF))[0]


def flh_of(index):
    return [7001 + 100 * index, 7002 + 100 * index, 7003 + 100 * index]


class Scene:
    """The Prism fixture for groups A-D (module doc)."""

    def __init__(self, case, trace_walk=False):
        u, read32, master, frame, _calls = bc.building_fixture(
            dict(name=case['name'], control=[0, 1, 0], mission='none'))
        self.u, self.read32, self.frame0, self.case = u, read32, frame, case
        self.events, self.walk, self.distances = [], [], []
        self.last_bullet, self.recruited, self.armed = None, None, False
        self.shot_target = self.shot_weapon = None
        self.heap, self.bullets = HEAP, 0
        u.mem_map(PRISM, PRISM_SIZE)
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        kind = sm.YTYPE
        self.kind = kind
        # The Prism tower type (module doc).
        u.mem_write(kind + 0xA0, dwords(case.get('strength', 600)))
        u.mem_write(kind + 0x16EC, dwords(case.get('delayed_fire_delay', 28)))
        u.mem_write(kind + 0x11F4, b'GAPRIS_A'.ljust(16, b'\0'))
        u.mem_write(kind + 0x1204, b'GAPRIS_AD'.ljust(16, b'\0'))
        u.mem_write(kind + 0x16F0, dwords(-1))
        for offset, value in ((0x16A7, 1), (0x16B8, 0), (0x16C3, 0), (0x16B5, 0), (0x157B, 0), (0xCA1, 0),
                              (0xCD5, 0), (0x5E4, 0)):
            u.mem_write(kind + offset, bytes([value]))
        u.mem_write(kind + 0x898, dwords(SHOT_WEAPON))
        u.mem_write(kind + 0x8B4, dwords(SUPPORT_WEAPON))
        for weapon, weapon_range in ((SHOT_WEAPON, 2048), (SUPPORT_WEAPON, case.get('support_range', 2048)),
                                     (ELITE_SUPPORT_WEAPON, case.get('elite_support_range', 0))):
            u.mem_write(weapon + 0xA0, dwords(PROJECTILE))
            u.mem_write(weapon + 0xB4, dwords(weapon_range))
        if 'elite_support_range' in case:
            u.mem_write(kind + 0xA94 + 0x1C, dwords(ELITE_SUPPORT_WEAPON))
        u.mem_write(OTHER_TYPE, bytes(u.mem_read(kind, TYPE_SIZE)))
        for mission, (rate, aa_rate) in bga.RATES.items():
            entry = 0xA8E3A8 + MISSION[mission] * 0x20
            u.mem_write(entry + 0x10, struct.pack('<dd', bga.read_double(rate), bga.read_double(aa_rate)))
        rules = dict(RETAIL_RULES, **case.get('rules', {}))
        u.mem_write(RULES + 0x498, dwords(kind, rules['modifier'], rules['max'], rules['delay'], rules['duration'],
                                          rules['height']))
        u.mem_write(RULES + 0x1700, struct.pack('<d', 0.5))
        u.mem_write(HOUSE + 0x56FC, bytes(LASER_COLOR))
        # The master (the fixture building).
        self.first = first = case.get('fixture_name', 'master')
        self.address = {first: master}
        self.names = {master: first, ACTOR: 'target', 0: None}
        self.index = {first: 0}
        u.mem_write(master + 0x2D8, dwords(0))  # no slave manager
        u.mem_write(master + 0x6C, dwords(600))
        if case.get('begin_mode'):
            bc.invoke(u, bc.BEGIN_MODE, master, 1)
        self.place(master, case.get(first, {}), offset=None)
        if 'veterancy' in case.get(first, {}):
            u.mem_write(master + 0x150, struct.pack('<f', case[first]['veterancy']))
        template = bytes(u.mem_read(master, BUILDING_SIZE))
        location = struct.unpack('<3i', template[0x9C:0xA8])
        self.location = list(location)
        for number, spec in enumerate(case.get('towers', []), start=1):
            address = TOWERS + (number - 1) * TOWER_STRIDE
            u.mem_write(address, template)
            self.address[spec['name']] = address
            self.names[address] = spec['name']
            self.index[spec['name']] = number
            u.mem_write(address + 0x150, struct.pack('<f', 0.0))
            self.place(address, spec, offset=spec.get('offset', [0, 0, 0]))
        for address in self.address.values():
            assert not any(u.mem_read(address + 0x55C, 21 * 4)), 'anim slots must be empty'
        vector = case.get('vector', case.get('order', [first] + [spec['name'] for spec in case.get('towers', [])]))
        u.mem_write(VECTOR, dwords(*[self.address[name] if name else 0 for name in vector] or [0]))
        u.mem_write(HOUSE + 0x68, dwords(0, VECTOR, 64) + bytes([1, 1, 0, 0]) + dwords(len(vector), 0))
        # The laser vector (0xABC878): Items, Capacity 16, Count 0.
        u.mem_write(LASERS, dwords(0, LASER_ITEMS, 16) + bytes([1, 1, 0, 0]) + dwords(0, 0))
        self.install(trace_walk)

    def place(self, address, spec, offset):
        u, frame = self.u, self.frame0
        if offset is not None:
            u.mem_write(address + 0x9C, dwords(*[a + b for a, b in zip(self.location, offset)]))
            u.mem_write(address + 0x6C, dwords(spec.get('health', 600)))
        elif 'health' in spec:
            u.mem_write(address + 0x6C, dwords(spec['health']))
        u.mem_write(address + 0x90, bytes([spec.get('alive', True)]))
        u.mem_write(address + 0x520, dwords(OTHER_TYPE if spec.get('type') == 'other' else self.kind))
        u.mem_write(address + 0xAC, dwords(MISSION[spec.get('mission', 'guard')]))
        u.mem_write(address + 0xB4, dwords(MISSION[spec.get('queued', 'none')]))
        u.mem_write(address + 0xBC, dwords(0))
        u.mem_write(address + 0xC0, dwords(frame))
        u.mem_write(address + 0xC8, dwords(frame, 0, 0))
        u.mem_write(address + 0x6DD, bytes([1]))
        u.mem_write(address + 0x2B4, dwords(ACTOR if spec.get('target') else 0))
        start, delay = spec.get('rearm', [-100, 0])
        u.mem_write(address + 0x2EC, dwords(-1 if start is None else frame + start, 0, delay))
        u.mem_write(address + 0x1D0, dwords(DRAINER if spec.get('drained') else 0))
        u.mem_write(address + 0x664, dwords(spec.get('count', 0)))
        u.mem_write(address + 0x704, dwords(spec.get('mode', 0), *spec.get('payload', [0, 0, 0]),
                                            spec.get('countdown', 0)))
        u.mem_write(address + 0x148, dwords(0))

    # -- execution helpers ------------------------------------------------
    def invoke(self, entry, this, *args):
        return bc.invoke(self.u, entry, this, *args)

    def prefill_stack(self):
        self.u.mem_write(SP - 0x1000, dwords(STALE) * 0x400)

    def ret(self, cleanup, value=0):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def arg(self, n):
        return self.read32(self.u.reg_read(UC_X86_REG_ESP) + 4 * n)

    def name(self, pointer):
        return self.names.get(pointer, hex(pointer))

    def s32(self, address):
        return signed(self.read32(address))

    def event(self, *event):
        self.events.append(list(event))

    def random_indices(self):
        """The Scenario RNG's two indices (RandomClass at [0xA8B230]+0x218: +4, +8)."""
        return [self.s32(SCENARIO + 0x21C), self.s32(SCENARIO + 0x220)]

    def state(self, name):
        b = self.address[name]
        mission = self.s32(b + 0xAC)
        queued = self.s32(b + 0xB4)
        return dict(count=self.s32(b + 0x664), mode=self.s32(b + 0x704),
                    payload=[self.s32(b + 0x708), self.s32(b + 0x70C), self.s32(b + 0x710)],
                    countdown=self.s32(b + 0x714), turret_counter=self.s32(b + 0x148),
                    rearm=[self.s32(b + 0x2EC), self.s32(b + 0x2F4)],
                    mission=MISSION_NAME.get(mission, mission), queued=MISSION_NAME.get(queued, queued),
                    target=self.name(self.read32(b + 0x2B4)))

    def brief(self, name):
        b = self.address[name]
        mission = self.s32(b + 0xAC)
        return dict(mission=MISSION_NAME.get(mission, mission), count=self.s32(b + 0x664),
                    mode=self.s32(b + 0x704), countdown=self.s32(b + 0x714),
                    rearm=[self.s32(b + 0x2EC), self.s32(b + 0x2F4)], target=self.name(self.read32(b + 0x2B4)))

    def new_bullet(self):
        bullet = BULLETS + self.bullets * BULLET_STRIDE
        self.bullets += 1
        self.u.mem_write(bullet, bytes(BULLET_STRIDE))
        self.u.mem_write(bullet + 0x150, dwords(0x100))  # BulletClass::Construct 0x466546
        return bullet

    # -- hooks ------------------------------------------------------------
    def at(self, address, handler):
        self.u.hook_add(UC_HOOK_CODE, lambda _u, _a, _s, _d: handler(), begin=address, end=address)

    def install(self, trace_walk):
        u, case = self.u, self.case
        self.powered = {name: True for name in self.address}
        self.out_of_range = {name: False for name in self.address}
        for name, value in case.get('powered', {}).items():
            self.powered[name] = value
        mode = case.get('hooks', 'recruit')

        def select_weapon():
            if mode == 'recruit':
                self.event('select_weapon', self.name(self.arg(1)))
            self.ret(4, 0)

        def get_flh():
            this = u.reg_read(UC_X86_REG_ECX)
            out, index = self.arg(1), self.arg(2)
            base = struct.unpack('<3i', u.mem_read(u.reg_read(UC_X86_REG_ESP) + 12, 12))
            if mode != 'cadence':
                self.event('flh', self.name(this), index, list(base))
            u.mem_write(out, dwords(*flh_of(self.index[self.name(this)])))
            self.ret(0x14, out)

        def play_anim():
            this = u.reg_read(UC_X86_REG_ECX)
            pointer = self.arg(1)
            text = bytes(u.mem_read(pointer, 32)).split(b'\0')[0].decode('ascii')
            if mode != 'cadence':
                kind = self.read32(this + 0x520)
                self.event('play_anim', self.name(this), text, pointer - kind, self.arg(2), self.arg(3) & 0xFF,
                           self.arg(4), self.arg(5))
            self.ret(0x14, 0)

        def destroy_anim():
            if mode != 'cadence':
                self.event('destroy_anim', self.name(u.reg_read(UC_X86_REG_ECX)), self.arg(0))

        def is_operational():
            this = self.name(u.reg_read(UC_X86_REG_ECX))
            if mode != 'cadence':
                self.event('is_operational', this)
            self.ret(0, self.powered.get(this, True))

        self.at(SELECT_WEAPON, select_weapon)
        if not case.get('native_flh', False):
            self.at(GET_FLH, get_flh)
        self.at(PLAY_ANIM, play_anim)
        for site in DESTROY_ANIM_CALLS:
            self.at(site, destroy_anim)
        self.at(IS_OPERATIONAL, is_operational)
        if trace_walk:
            self.install_walk()
        if mode in ('recruit', 'bonus'):
            errors = list(case.get('fire_error', [0]))

            def get_fire_error():
                code = errors.pop(0) if len(errors) > 1 else errors[0]
                self.event('fire_error', self.name(self.arg(1)), self.arg(2), self.arg(3) & 0xFF, code)
                self.ret(12, code)
            self.at(GET_FIRE_ERROR, get_fire_error)
        if mode in ('bonus', 'cadence'):
            def fire_at():
                this = u.reg_read(UC_X86_REG_ECX)
                target, weapon = self.arg(1), self.arg(2)
                if mode == 'bonus':
                    self.event('fire_at', self.name(target), weapon)
                    bullet = self.new_bullet() if case.get('bullet', True) else 0
                else:
                    rof = case.get('rof', 45)
                    u.mem_write(this + 0x2F8, dwords(rof))
                    u.mem_write(this + 0x2EC, dwords(self.read32(FRAME)))
                    u.mem_write(this + 0x2F4, dwords(rof))
                    bullet = self.new_bullet()
                    self.shot_target, self.shot_weapon = self.name(target), weapon
                self.last_bullet = bullet or None
                self.ret(8, bullet)
            self.at(FIRE_AT, fire_at)
        if mode in ('beam', 'cadence', 'laser'):
            fails = case.get('new_fails', False)

            def operator_new():
                size = self.arg(1)
                answer = 0 if fails else self.heap
                if not fails:
                    self.heap += (size + 15) & ~15
                    assert self.heap <= HEAP_END
                if mode in ('beam', 'laser'):
                    self.event('new', size, None if fails else answer - HEAP)
                self.ret(0, answer)

            def laser_ctor():
                sp = u.reg_read(UC_X86_REG_ESP)
                raw = bytes(u.mem_read(sp + 4, 0x40))
                words = struct.unpack('<6ii4x', raw[:0x20])
                if mode in ('beam', 'laser'):
                    self.event('laser', dict(
                        src=list(words[0:3]), dst=list(words[3:6]), z_adjust=words[6], flag=raw[0x1C],
                        inner=list(raw[0x20:0x23]), outer=list(raw[0x24:0x27]), spread=list(raw[0x28:0x2B]),
                        duration=struct.unpack('<i', raw[0x2C:0x30])[0], blinks=raw[0x30], fades=raw[0x34],
                        f1=struct.unpack('<f', raw[0x38:0x3C])[0], f2=struct.unpack('<f', raw[0x3C:0x40])[0]))
            self.at(OPERATOR_NEW, operator_new)
            self.at(LASER_CTOR, laser_ctor)
        if mode == 'cadence':
            self.install_cadence()

    def install_walk(self):
        u = self.u

        def top():
            index = self.read32(u.reg_read(UC_X86_REG_ESP) + 0x14)
            self.walk.append([index, None, None])

        def test(stage):
            def handler():
                if stage == 'null':
                    self.walk[-1][1] = self.name(u.reg_read(UC_X86_REG_EDI))
                self.walk[-1][2] = stage
            return handler

        def distance():
            self.distances.append([self.name(u.reg_read(UC_X86_REG_EDI)), signed(u.reg_read(UC_X86_REG_EBP)),
                                   signed(u.reg_read(UC_X86_REG_EAX))])

        self.at(WALK_TOP, top)
        for address, stage in WALK_TESTS.items():
            self.at(address, test(stage))
        self.at(DISTANCE_TEST, distance)

        def recruit():
            self.recruited = self.name(u.reg_read(UC_X86_REG_EBX))

        def arm():
            self.armed = True
        self.at(RECRUIT, recruit)
        self.at(MASTER_ARM, arm)

    def install_cadence(self):
        u = self.u

        def after_prologue():
            if u.reg_read(UC_X86_REG_EBX):
                u.reg_write(UC_X86_REG_EIP, TECHNO_REARM_TEST)

        def not_rearming():
            flag = self.read32(u.reg_read(UC_X86_REG_ESP) + 0x2C) & 0xFF
            this = self.name(u.reg_read(UC_X86_REG_ESI))
            far = flag and self.out_of_range.get(this, False)
            u.reg_write(UC_X86_REG_EIP, TECHNO_RANGE_TAIL if far else TECHNO_OK_TAIL)

        def returned(kind):
            def handler():
                self.event(kind, signed(u.reg_read(UC_X86_REG_EAX)))
            return handler

        def fire_error(kind):
            def handler():
                self.event(kind, signed(u.reg_read(UC_X86_REG_EAX)))
            return handler

        def recruit():
            self.event('recruit', self.name(u.reg_read(UC_X86_REG_EBX)))

        def arm():
            self.event('arm')

        def beam():
            sp = u.reg_read(UC_X86_REG_ESP)
            self.event('beam', list(struct.unpack('<3i', u.mem_read(sp + 4, 12))))

        def techno_set_target():
            target = self.arg(1)
            self.event('set_target', self.name(target))
            u.mem_write(u.reg_read(UC_X86_REG_ECX) + 0x2B4, dwords(target))
            self.ret(4)

        def draw():
            self.event('draw', signed(self.arg(1)), signed(self.arg(2)))

        def answered(cleanup, value=0):
            return lambda: self.ret(cleanup, value)

        def direction_to():
            out = self.arg(1)
            u.mem_write(out, dwords(0))
            self.ret(8, out)

        self.at(TECHNO_AFTER_PROLOGUE, after_prologue)
        self.at(TECHNO_NOT_REARMING, not_rearming)
        self.at(ATTACK_RETURNED, returned('attack'))
        self.at(GUARD_RETURNED, returned('guard'))
        self.at(ATTACK_FIRE_ERROR, fire_error('fire_error'))
        self.at(EXPIRY_FIRE_ERROR, fire_error('expiry_error'))
        self.at(RECRUIT, recruit)
        self.at(MASTER_ARM, arm)
        self.at(SUPPORT_BEAM, beam)
        self.at(TECHNO_SET_TARGET, techno_set_target)
        self.at(RANDOM_RANGED, draw)
        self.at(SET_DESIRED, answered(4))
        self.at(DIRECTION_TO, direction_to)
        self.at(UNCLOAK, answered(4))
        self.at(CLEAR_BIB, answered(0))
        self.at(IS_CLOSE_ENOUGH, answered(4, 1))
        self.at(IS_HUMAN, answered(0, 0))
        self.at(UNDER_EMP, answered(0, 0))


# -- A: recruitment ------------------------------------------------------------
def recruit(case):
    s = Scene(dict(case, hooks='recruit'), trace_walk=True)
    s.u.mem_write(s.address['master'] + 0xAC, dwords(MISSION[case.get('master', {}).get('mission', 'attack')]))
    s.u.mem_write(s.address['master'] + 0x2B4, dwords(ACTOR))
    s.prefill_stack()
    before = s.random_indices()
    value = s.invoke(MISSION_ATTACK, s.address['master'])
    towers = {spec['name']: s.state(spec['name']) for spec in case.get('towers', [])}
    return dict(input=case, frame0=s.frame0, returns=signed(value), walk=s.walk, distances=s.distances,
                recruited=s.recruited, armed=s.armed, calls=s.events, master=s.state('master'), towers=towers,
                random_indices=dict(before=before, after=s.random_indices()))


def at_range(axis, delta, support_range=2048):
    offset = [0, 0, 0]
    offset[axis] = support_range + delta
    return offset


def recruit_cases():
    one = lambda name, **spec: dict(name=name, towers=[dict(dict(name='a', offset=[512, 0, 0]), **spec)])
    far9 = [dict(name=f'c{n}', offset=[[200 * n, 0, 0], [0, 200 * n, 0], [-200 * n, 0, 0]][n % 3])
            for n in (5, 2, 9, 1, 7, 4, 8, 3, 6)]
    cases = [
        # The admission tests, in order (0x44B380..0x44B404), each rejecting.
        dict(name='null_entry', towers=[dict(name='a', offset=[512, 0, 0])], vector=['master', None, 'a']),
        one('dead', alive=False),
        one('wrong_type', type='other'),
        one('rearm_running', rearm=[-10, 45]),
        one('rearm_one_left', rearm=[-44, 45]),
        one('rearm_just_done', rearm=[-45, 45]),
        one('rearm_never_started', rearm=[None, 45]),
        one('rearm_never_started_no_delay', rearm=[None, 0]),
        one('delayed_fire_countdown', countdown=1),
        one('delayed_fire_mode_only', mode=1),
        one('drained', drained=True),
        one('attacking', mission='attack'),
        one('queued_attack', mission='none', queued='attack'),
        one('selling_unpowered_with_target', mission='selling', target=True),
        dict(name='self_attack', vector=['master']),
        # A synthetic direct call with the master off Attack: the only caller
        # (MissionClass::AI 0x5B3176, mission 1) always has it on Attack.
        dict(name='self_guard', vector=['master'], master=dict(mission='guard')),
        # The range test: distance <= Get_Weapon_Range(1), native distance.
        *[one(f'{axis_name}_{label}', offset=at_range(axis, delta))
          for axis, axis_name in enumerate('xyz') for label, delta in (('range', 0), ('range_plus_1', 1),
                                                                      ('range_plus_2', 2))],
        # Sqrt_Approx: 2049.12 leptons measures 2048 (admitted), 2049.12 +
        # 0.0005 measures 2049 (refused); both exact isqrt 2049.
        one('sqrt_d2_4198910', offset=[2049, 5, 22]),
        one('sqrt_d2_4198912', offset=[2048, 48, 48]),
        dict(one('modded_range_3000', offset=[2500, 0, 0]), support_range=3000),
        dict(one('elite_range_1024', offset=[1500, 0, 0]), elite_support_range=1024,
             master=dict(veterancy=2.0)),
        # Selection: strictly nearer replaces; ties keep the lower index.
        dict(name='nearer_later', towers=[dict(name='a', offset=[1000, 0, 0]), dict(name='b', offset=[0, 600, 0])]),
        dict(name='nearer_first', towers=[dict(name='a', offset=[600, 0, 0]), dict(name='b', offset=[0, 1000, 0])]),
        dict(name='equal_distance', towers=[dict(name='a', offset=[700, 0, 0]), dict(name='b', offset=[0, -700, 0])]),
        # Cap: PrismSupportMax 8 against the master's count on entry.
        *[dict(name=f'nine_count{count}', towers=far9, master=dict(count=count)) for count in (0, 7, 8, 9)],
        dict(name='empty_vector', vector=[]),
        dict(name='max_zero', towers=[dict(name='a', offset=[512, 0, 0])], rules=dict(max=0)),
        # SpecialAnim vs SpecialAnimDamaged: Health/Strength <= ConditionYellow.
        *[one(f'supporter_health_{health}', health=health) for health in (301, 300, 299)],
        *[dict(name=f'master_health_{health}', vector=['master'], master=dict(health=health))
          for health in (301, 300, 299)],
    ]
    return cases


# -- B: bonus ------------------------------------------------------------------
def bonus(case):
    s = Scene(dict(case, hooks='bonus'))
    before = s.random_indices()
    s.invoke(PROCESS_DELAYED_FIRE, s.address['master'])
    multiplier = s.s32(s.last_bullet + 0x150) if s.last_bullet else None
    return dict(input=case, frame0=s.frame0, calls=s.events, bullet_multiplier=multiplier,
                master=s.state('master'), random_indices=dict(before=before, after=s.random_indices()))


def bonus_cases():
    armed = lambda name, count, **extra: dict(name=name, master=dict(dict(mode=1, countdown=1, target=True,
                                                                         mission='attack', count=count),
                                                                    **extra.pop('master', {})), **extra)
    cases = [armed(f'modifier{modifier}_count{count}', count, rules=dict(modifier=modifier))
             for modifier in (150, 100, 15000, 0, -50) for count in range(10)]
    cases += [
        armed('no_target', 3, master=dict(target=False)),
        armed('fire_error_range', 3, fire_error=[8]),
        armed('fire_error_rearm', 3, fire_error=[3]),
        armed('fire_error_cant', 3, fire_error=[6]),
        armed('fire_at_returns_null', 3, bullet=False),
        armed('countdown_2', 3, master=dict(countdown=2)),
        armed('countdown_0', 3, master=dict(countdown=0)),
        armed('mode_0', 3, master=dict(mode=0, countdown=5)),
        armed('mode_3', 3, master=dict(mode=3)),
        armed('weapon_index_1', 3, master=dict(payload=[1, 0, 0])),
    ]
    return cases


def damage(case):
    s = Scene(dict(case, hooks='damage'))
    u = s.u
    bullet = s.new_bullet()
    u.mem_write(bullet + 0x150, dwords(case['multiplier']))
    u.mem_write(bullet + 0x6C, dwords(case['damage']))
    u.mem_write(bullet + 0xB0, dwords(s.address['master']))
    u.mem_write(bullet + 0x128, dwords(WARHEAD))
    u.mem_write(COORD, dwords(1, 2, 3))
    frame = SP - 0x200
    u.mem_write(frame + 8, dwords(COORD))
    u.reg_write(UC_X86_REG_ESI, bullet)
    u.reg_write(UC_X86_REG_EBP, frame)
    u.reg_write(UC_X86_REG_ESP, SP - 0x400)
    run_checked(u, *DAMAGE_BLOCK, count=100)
    sp = u.reg_read(UC_X86_REG_ESP)
    names = {**s.names, WARHEAD: 'warhead', HOUSE: 'house', COORD: 'coord'}
    args = [names.get(s.read32(sp + 4 * n), s.read32(sp + 4 * n)) for n in range(4)]
    return dict(input=case, damage=signed(u.reg_read(UC_X86_REG_EDX)), args=args,
                coords=names.get(u.reg_read(UC_X86_REG_ECX)))


def damage_cases():
    multipliers = [0x100, 640, 1408, 3328, 3712, 0, 1, 255, 42949544, 0x7FFFFFFF, -256]
    damages = [120, -120, 1, 0, 1000000, 0x7FFFFFFF]
    return [dict(name=f'm{m}_d{d}', multiplier=m, damage=d) for m in multipliers for d in damages]


# -- C: support beam -----------------------------------------------------------
def beam(case):
    s = Scene(dict(case, hooks='beam', fixture_name='supporter'))
    u = s.u
    supporter = s.address['supporter']
    # FireAt's ROF copy and a marker in the timer's unused middle word.
    u.mem_write(supporter + 0x2F8, dwords(45))
    u.mem_write(supporter + 0x2F0, dwords(0x5A5A5A5A))
    before = s.random_indices()
    s.invoke(PROCESS_DELAYED_FIRE, supporter)
    after = s.random_indices()
    laser = None
    if s.read32(LASERS + 0x10):
        address = s.read32(LASER_ITEMS)
        raw = bytes(u.mem_read(address, 0x5C))
        i32 = lambda offset: struct.unpack('<i', raw[offset:offset + 4])[0]
        laser = dict(start=i32(0x8), field_c=i32(0xC), field_10=i32(0x10), field_14=i32(0x14), field_18=i32(0x18),
                     width=i32(0x1C), house_color=raw[0x20], field_21=raw[0x21],
                     src=list(struct.unpack('<3i', raw[0x24:0x30])), dst=list(struct.unpack('<3i', raw[0x30:0x3C])),
                     z_adjust=i32(0x3C), field_40=raw[0x40], inner=list(raw[0x41:0x44]), outer=list(raw[0x44:0x47]),
                     spread=list(raw[0x47:0x4A]), duration=i32(0x4C), blinks=raw[0x50], field_51=raw[0x51],
                     fades=raw[0x52], f1=struct.unpack('<f', raw[0x54:0x58])[0],
                     f2=struct.unpack('<f', raw[0x58:0x5C])[0], heap_offset=address - HEAP)
    state = dict(s.state('supporter'), rearm_middle=s.s32(supporter + 0x2F0), rof_copy=s.s32(supporter + 0x2F8))
    return dict(input=case, frame0=s.frame0, calls=s.events, laser=laser, lasers_registered=s.s32(LASERS + 0x10),
                supporter=state, random_indices=dict(before=before, after=after))


def beam_cases():
    # The stored payload is the FLH sentinel a master in tower slot 1 hands
    # over at recruitment (7101, 7102, 7103); the supporter's own is slot 0's.
    charged = lambda name, **extra: dict(name=name, supporter=dict(dict(
        mode=2, countdown=1, payload=flh_of(1), count=2, rearm=[-50, 45]), **extra.pop('supporter', {})),
        **extra)
    return [
        charged('beam'),
        charged('beam_modded_rules', rules=dict(delay=60, duration=30)),
        charged('beam_new_fails', new_fails=True),
        charged('beam_countdown_2', supporter=dict(countdown=2)),
        charged('beam_count_0', supporter=dict(count=0)),
        charged('beam_negative_y', supporter=dict(payload=[7101, -7102, 7103])),
        # No check on the way: drained, selling and unpowered.
        charged('beam_drained_selling_unpowered', supporter=dict(drained=True, mission='selling'),
                powered=dict(supporter=False)),
    ]


# -- D: cadence ----------------------------------------------------------------
def cadence(case):
    s = Scene(dict(case, hooks='cadence', begin_mode=True))
    u = s.u
    order = case.get('order', ['master'] + [spec['name'] for spec in case.get('towers', [])])
    script = {}
    for frame, action, name in case.get('events', []):
        script.setdefault(frame, []).append((action, name))
    frames = []
    for k in range(1, case['frames'] + 1):
        u.mem_write(FRAME, dwords(s.frame0 + k))
        log = {name: [] for name in order}
        for action, name in script.get(k, []):
            b = s.address[name]
            s.events = log[name]
            if action == 'acquire':
                u.mem_write(b + 0x2B4, dwords(ACTOR))
            elif action == 'lose':
                u.mem_write(b + 0x2B4, dwords(0))
            elif action in ('unpower', 'power'):
                s.powered[name] = action == 'power'
            elif action in ('out_of_range', 'in_range'):
                s.out_of_range[name] = action == 'out_of_range'
            elif action == 'stop':
                s.invoke(BUILDING_SET_TARGET, b, 0)
            else:
                raise ValueError(action)
        towers = {}
        for name in order:
            b = s.address[name]
            s.events, s.last_bullet = log[name], None
            before = s.random_indices()[0]
            bc.run_block(u, b, bc.READY_COMMENCE_UNLESS_BUILDING)
            s.invoke(bc.MISSION_AI, b)
            bc.run_block(u, b, bc.READY_COMMENCE)
            s.invoke(PROCESS_DELAYED_FIRE, b)
            if s.last_bullet:
                s.event('shot', s.shot_target, s.shot_weapon, s.s32(s.last_bullet + 0x150))
            draws = (s.random_indices()[0] - before) % 250  # the index wraps at 250 (0x65C7AB)
            if draws:
                s.event('draws', draws)
            towers[name] = dict(s.brief(name), events=log[name])
        frames.append(dict(frame=k, towers=towers))
    return dict(input=case, frame0=s.frame0, frames=frames)


# Supporters by distance from the master: 512, 768, 1024 leptons.
SUPPORTERS = [dict(name='s1', offset=[512, 0, 0]), dict(name='s2', offset=[0, 768, 0]),
              dict(name='s3', offset=[-1024, 0, 0])]


def cadence_cases():
    acquire = [[1, 'acquire', 'master']]
    nine = [dict(name=f'c{n}', offset=[[200 * n, 0, 0], [0, 200 * n, 0], [-200 * n, 0, 0]][n % 3])
            for n in (5, 2, 9, 1, 7, 4, 8, 3, 6)]
    two = SUPPORTERS[:2]
    return [
        dict(name='a_lone_two_cycles', frames=160, events=acquire),
        *[dict(name=f'b_supporters_after_{n}', frames=40, towers=SUPPORTERS[:n], events=acquire) for n in (1, 2, 3)],
        *[dict(name=f'c_supporters_before_{n}', frames=40, towers=SUPPORTERS[:n], events=acquire,
               order=[spec['name'] for spec in SUPPORTERS[:n]] + ['master']) for n in (1, 3)],
        dict(name='d_nine_candidates', frames=45, towers=nine, events=acquire),
        dict(name='e_target_lost_recruiting', frames=40, towers=SUPPORTERS, events=acquire + [[3, 'lose', 'master']]),
        dict(name='e_target_out_of_range_recruiting', frames=40, towers=SUPPORTERS,
             events=acquire + [[3, 'out_of_range', 'master']]),
        dict(name='f_target_lost_charging_reacquired', frames=45, towers=two,
             events=acquire + [[9, 'lose', 'master'], [12, 'acquire', 'master']]),
        dict(name='g_master_unpowered_charging', frames=45, towers=two, events=acquire + [[9, 'unpower', 'master']]),
        dict(name='h_supporter_own_target', frames=110, towers=two, events=acquire + [[6, 'acquire', 's1']]),
        dict(name='i_second_cycle', frames=170, towers=two, events=acquire),
        # The shot of master + 2 expires at frame 31 (b_supporters_after_2):
        # RANGE only for that expiry, then in range (the stale count arms the
        # next cycle), then a Stop, or RANGE kept.
        dict(name='j_stale_count_range_at_expiry', frames=70, towers=two,
             events=acquire + [[31, 'out_of_range', 'master'], [32, 'in_range', 'master']]),
        dict(name='j_stale_count_then_stop', frames=40, towers=two,
             events=acquire + [[31, 'out_of_range', 'master'], [32, 'in_range', 'master'], [32, 'stop', 'master']]),
        dict(name='j_range_kept_after_expiry', frames=40, towers=two,
             events=acquire + [[31, 'out_of_range', 'master']]),
        # A tower ahead of the master in the Logic order ends its own delayed
        # shot at frame 2: FireAt starts its rearm inside the visit
        # (0x6FF2B2), so the master's walk later in the frame refuses it.
        dict(name='k_shooter_rearms_before_a_later_walk', frames=32, master=dict(mission='attack', target=True),
             towers=[dict(name='m', offset=[512, 0, 0], mode=1, countdown=2), dict(name='x', offset=[0, 768, 0])],
             order=['m', 'master', 'x'], events=[[2, 'acquire', 'm']]),
    ]


# -- E: reader -------------------------------------------------------------------
READER = 0x24000000
INI = READER                                    # the pass's CCINIClass
SECTION_ITEMS, SECTION = READER + 0x100, READER + 0x200
ENTRY_ITEMS, ENTRIES, VALUES = READER + 0x400, READER + 0x800, READER + 0x1000
CRC_ENGINE, CRC_TEXT = READER + 0x2000, READER + 0x2100
TYPE_ITEMS, BUILDING_TYPES = READER + 0x3000, READER + 0x4000
BUILDING_TYPE_NAMES = ['GAPOWR', 'ATESLA']
GENERAL = 0x826278  # [0x7F0C9C], the "General" section name ReadGeneral passes
KEYS = {'PrismType': 0x83BBF4, 'PrismSupportModifier': 0x83BBDC, 'PrismSupportMax': 0x83BBCC,
        'PrismSupportDelay': 0x83BBB8, 'PrismSupportDuration': 0x83BBA0, 'PrismSupportHeight': 0x83BB8C}
CTOR_DEFAULTS = (0x665CE8, 0x665D1D)            # RulesClass ctor: +0x498..+0x4AC
GENERAL_GATE = (0x66D53C, (0x66D55E, 0x671E8E))  # ReadGeneral: [General] present?
PRISM_BLOCK = (0x671130, 0x6711FE)
READER_RULES = body_rules.RULES
READER_SP = body_rules.SP


class Reader:
    """building_body_rules' INI fixture with a whole [General] section."""

    def __init__(self):
        self.fixture = body_rules.Fixture()
        u = self.u = self.fixture.u
        u.mem_map(READER, 0x10000)
        # The CRT float scanner's initializer (as path_delay_rules runs it).
        u.mem_write(READER_SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, READER_SP)
        run_checked(u, 0x7C8F5E, RET_MAGIC)
        # The section and key names ReadGeneral passes (each pass supplies
        # its entries under these names).
        for text, address in [('General', GENERAL), *KEYS.items()]:
            assert bytes(u.mem_read(address, len(text) + 1)) == text.encode() + b'\0', text
        self.crcs = {}
        for index, name in enumerate(BUILDING_TYPE_NAMES):
            kind = BUILDING_TYPES + index * 0x1800
            u.mem_write(kind + 0x24, name.encode() + b'\0')
            u.mem_write(TYPE_ITEMS + index * 4, dwords(kind))
        u.mem_write(0xA83C6C, dwords(TYPE_ITEMS))
        u.mem_write(0xA83C78, dwords(len(BUILDING_TYPE_NAMES)))

    def read32(self, address):
        return struct.unpack('<I', self.u.mem_read(address, 4))[0]

    def crc(self, text):
        """The INI index key: the native CRC engine 0x4A1DE0 over the name."""
        if text not in self.crcs:
            u = self.u
            data = text.encode('ascii')
            u.mem_write(CRC_ENGINE, bytes(16))
            u.mem_write(CRC_TEXT, data + b'\0')
            u.mem_write(READER_SP, dwords(RET_MAGIC, CRC_TEXT, len(data)))
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            u.reg_write(UC_X86_REG_ECX, CRC_ENGINE)
            run_checked(u, 0x4A1DE0, RET_MAGIC, count=10_000)
            self.crcs[text] = u.reg_read(UC_X86_REG_EAX)
        return self.crcs[text]

    def supply(self, keys):
        """A CCINIClass with [General] holding `keys` (None: no [General])."""
        u = self.u
        u.mem_write(INI, bytes(0x40))
        u.mem_write(SECTION, bytes(0x40))
        if keys is None:
            return
        u.mem_write(SECTION_ITEMS, dwords(self.crc('General'), SECTION))
        # Section index +0x28: items, count, capacity, sorted, cached pair.
        u.mem_write(INI + 0x28, dwords(SECTION_ITEMS, 1, 1) + bytes([1, 0, 0, 0]) + dwords(0))
        pairs = []
        for number, (key, raw) in enumerate(keys.items()):
            entry, value = ENTRIES + number * 0x20, VALUES + number * 0x80
            u.mem_write(entry, bytes(0x20))
            u.mem_write(entry + 0x10, dwords(value))
            u.mem_write(value, raw.encode('ascii') + b'\0')
            pairs.append((signed(self.crc(key)), entry))
        pairs.sort()
        u.mem_write(ENTRY_ITEMS, b''.join(dwords(crc, entry) for crc, entry in pairs) or dwords(0))
        # Entry index +0x2C: items, count, capacity, sorted, cached pair.
        u.mem_write(SECTION + 0x2C, dwords(ENTRY_ITEMS, len(pairs), len(pairs)) + bytes([1, 0, 0, 0]) + dwords(0))

    def rules(self):
        values = struct.unpack('<6i', self.u.mem_read(READER_RULES + 0x498, 24))
        kinds = {BUILDING_TYPES + index * 0x1800: name for index, name in enumerate(BUILDING_TYPE_NAMES)}
        pointer = values[0] & 0xFFFFFFFF
        return dict(prism_type=kinds.get(pointer, None if pointer == 0 else hex(pointer)), modifier=values[1],
                    max=values[2], delay=values[3], duration=values[4], height=values[5])

    def defaults(self):
        u = self.u
        u.mem_write(READER_RULES + 0x498, dwords(0x5A5A5A5A) * 6)
        u.reg_write(UC_X86_REG_ESI, READER_RULES)
        u.reg_write(UC_X86_REG_EBX, 0)  # 0x665663
        run_checked(u, *CTOR_DEFAULTS, count=100)
        return self.rules()

    def read_general(self, keys):
        u = self.u
        self.supply(keys)
        frame = READER_SP - 0x100
        u.mem_write(frame + 8, dwords(INI))
        u.reg_write(UC_X86_REG_EBP, frame)
        u.reg_write(UC_X86_REG_ECX, READER_RULES)
        u.reg_write(UC_X86_REG_ESP, READER_SP - 0x400)
        gate = run_checked(u, GENERAL_GATE[0], GENERAL_GATE[1], count=10_000)
        if gate == GENERAL_GATE[1][0]:
            u.reg_write(UC_X86_REG_ESI, READER_RULES)
            u.reg_write(UC_X86_REG_EDI, INI)
            u.reg_write(UC_X86_REG_ESP, READER_SP - 0x400)
            run_checked(u, *PRISM_BLOCK, count=200_000)
        return dict(general=gate == GENERAL_GATE[1][0], **self.rules())


def reader(case):
    r = Reader()
    defaults = r.defaults()
    initial = case.get('initial')
    if initial:
        r.u.mem_write(READER_RULES + 0x49C, dwords(initial['modifier']))
    passes = [r.read_general(keys) for keys in case['passes']]
    return dict(input=case, defaults=defaults, passes=passes)


RETAIL_GENERAL = {'PrismType': 'ATESLA', 'PrismSupportModifier': '150%', 'PrismSupportMax': '8',
                  'PrismSupportDelay': '45;60', 'PrismSupportDuration': '15', 'PrismSupportHeight': '420'}


def reader_cases():
    modifier = lambda raw: [{'PrismSupportModifier': raw}]
    return [
        dict(name='retail', passes=[RETAIL_GENERAL]),
        *[dict(name=f'modifier_{label}', passes=modifier(raw))
          for label, raw in (('150pct', '150%'), ('1.5', '1.5'), ('150', '150'), ('33.3pct', '33.3%'),
                             ('minus50pct', '-50%'), ('junk', 'junk'),
                             # Rounding-sensitive: chopped products read one lower.
                             ('35pct', '35%'), ('14pct', '14%'), ('57pct', '57%'))],
        # [General] present without the key: ftol(current * 100).
        dict(name='general_without_keys_current_100', passes=[{}]),
        dict(name='general_without_keys_current_150', passes=[{}], initial=dict(modifier=150)),
        dict(name='retail_then_general_without_keys', passes=[RETAIL_GENERAL, {}]),
        dict(name='retail_then_general_without_keys_twice', passes=[RETAIL_GENERAL, {}, {}]),
        dict(name='retail_then_no_general', passes=[RETAIL_GENERAL, None]),
        dict(name='retail_twice', passes=[RETAIL_GENERAL, RETAIL_GENERAL]),
        dict(name='no_general', passes=[None]),
        dict(name='other_keys_modded', passes=[{'PrismSupportMax': '3', 'PrismSupportDelay': '60',
                                                'PrismSupportDuration': '30', 'PrismSupportHeight': '0'}]),
        dict(name='delay_45_60', passes=[{'PrismSupportDelay': '45;60'}]),
        dict(name='prism_type_none', passes=[RETAIL_GENERAL, {'PrismType': 'none'}]),
        dict(name='prism_type_bracketed_none', passes=[RETAIL_GENERAL, {'PrismType': '<none>'}]),
        dict(name='prism_type_gapowr', passes=[{'PrismType': 'GAPOWR'}]),
    ]


# -- F: laser creation, Logic lifetime and software drawing ----------------------
LASER_UPDATE, LASER_DRAW_ALL, LASER_DESTROY_ALL = 0x550150, 0x550240, 0x550000
LASER_DRAW, LASER_SPECIAL = 0x550260, 0x5509F0
LASER_ADDITIVE, LASER_PACKED = 0x4BDF00, 0x4BFD30
LASER_VECTOR_INIT, ATEXIT, OPERATOR_DELETE = 0x54FDC0, 0x7C978A, 0x7C8B3D
LASER_FIRE_ARM, LASER_FIRE_END = 0x6FF4CC, 0x6FF656
LASER_TACTICAL = COORD + 0x1000
DISK_RING_INIT, DISK_RING = 0x4A6CF0, 0x8A0180
GENERAL_OFFSETS = 0xABC738
DISK_CTOR, DISK_FIRE, DISK_AI, DISK_DETACH, DISK_SCALAR_DTOR = (
    0x4A7A30, 0x4A71A0, 0x4A7340, 0x4A7900, 0x4A7C90)
DISKS, PENDING_DELETE = 0x8A0208, 0xB0F698
DISK_LOGIC, DISK_EXPIRY = (0x55B5A1, 0x55B5BE), (0x725977, 0x725995)
DISK_SOURCE, DISK_TYPE, DISK_LOCO, DISK_TARGET, DISK_TARGET_TYPE, DISK_OUT = (
    PRISM + offset for offset in (0x48000, 0x49000, 0x4A000, 0x4B000, 0x4C000, 0x4D000))


def laser_rng_state(u, scenario):
    return {name: bytes(u.mem_read(pointer, 0x3F4)).hex()
            for name, pointer in (('main', 0x886B88), ('scenario', scenario + 0x218),
                                  ('mapgen', 0xABE890))}


def seed_laser_rngs(invoke, scenario, seed):
    # Existing native seed owner; never construct a host RNG or copy a VERA cursor.
    from tools.rmg_oracle.gen_rng_vectors import SEED_FN
    for pointer in (0x886B88, scenario + 0x218, 0xABE890):
        invoke(SEED_FN, pointer, seed)


def laser_state(u, address):
    raw = bytes(u.mem_read(address, 0x5C))
    i32 = lambda offset: struct.unpack_from('<i', raw, offset)[0]
    return dict(age=i32(0), changed=raw[4], timer_start=i32(8), timer_duration=i32(0x10),
                rate=i32(0x14), step=i32(0x18), width=i32(0x1C), house_color=raw[0x20],
                supported=raw[0x21], source=list(struct.unpack_from('<3i', raw, 0x24)),
                target=list(struct.unpack_from('<3i', raw, 0x30)), z_adjust=i32(0x3C),
                flag=raw[0x40], inner=list(raw[0x41:0x44]), outer=list(raw[0x44:0x47]),
                spread=list(raw[0x47:0x4A]), duration=i32(0x4C), blinks=raw[0x50],
                blink_state=raw[0x51], fades=raw[0x52],
                fade_start_bits=struct.unpack_from('<I', raw, 0x54)[0],
                fade_end_bits=struct.unpack_from('<I', raw, 0x58)[0])


class LaserScene(Scene):
    """Reuse Scene; added birth rows execute its formerly substituted GetFLH."""
    def __init__(self, case):
        super().__init__(dict(case, hooks='laser', native_flh=True))
        u = self.u
        if 'birth_frame' in case:
            self.frame0 = case['birth_frame']
            u.mem_write(FRAME, dwords(self.frame0))
        self.freed, self.rng_entries, self.getters = [], [], []
        self.text_sha256 = hashlib.sha256(u.mem_read(TEXT_BEGIN, TEXT_SIZE)).hexdigest()
        assert self.text_sha256 == '4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc'
        self.at(ATEXIT, lambda: self.ret(0))
        self.at(OPERATOR_DELETE, self.deleted)
        self.invoke(LASER_VECTOR_INIT, 0)
        u.mem_write(LASERS + 4, dwords(LASER_ITEMS, case.get('laser_capacity', 16)))
        u.mem_write(LASERS + 0xD, b'\0')
        # Native Tactical constructor's matrix initializer; no supplied inverse.
        u.mem_write(0x887324, dwords(LASER_TACTICAL))
        u.reg_write(UC_X86_REG_ESI, LASER_TACTICAL)
        u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(u, 0x6D1DC5, 0x6D1E1E, count=30)
        u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
        u.mem_write(0xB0CE30, dwords(160, 120))
        # Selected stock ATESLA/GAPRIS values; native readers live separately.
        u.mem_write(self.kind + 0xE44, dwords(*case.get('pixel_offset', [0, -4])))
        u.mem_write(self.kind + 0xEF0, dwords(0))  # 1x1
        u.mem_write(self.kind + 0x1764, bytes([case.get('primary_dual', True)]))
        u.mem_write(self.kind + 0x89C, dwords(*case.get('primary_flh', [0, 0, 378])))
        u.mem_write(self.kind + 0x8B8, dwords(*case.get('secondary_flh', [0, 0, 0])))
        u.mem_write(self.kind + 0x720, dwords(0))
        u.mem_write(self.kind + 0x16C5, b'\0\0')
        weapons = [(SHOT_WEAPON, case)]
        if 'current_weapon' in case:
            # Building FireAt6FF4EA re-reads GetCurrentWeapon after the shot's
            # IsLaser gate. Its current primary can differ from the selected
            # secondary in EBX; original70E1A0/Building GetWeapon execute.
            weapons.append((SUPPORT_WEAPON, case['current_weapon']))
            u.mem_write(self.kind + 0x898, dwords(SUPPORT_WEAPON))
            u.mem_write(self.kind + 0x8B4, dwords(SHOT_WEAPON))
        for weapon, fields in weapons:
            u.mem_write(weapon + 0x149, bytes([fields.get('is_laser', True)]))
            u.mem_write(weapon + 0x14C, bytes([fields.get('big_laser', False),
                        fields.get('house_color', True), fields.get('duration_byte', 15) & 255]))
            u.mem_write(weapon + 0x120, bytes(fields.get('inner', [100, 120, 140])))
            u.mem_write(weapon + 0x123, bytes(fields.get('outer', [10, 20, 30])))
            u.mem_write(weapon + 0x126, bytes(fields.get('spread', [0, 0, 0])))
        u.mem_write(HOUSE + 0x56FC, bytes(case.get('rgb', LASER_COLOR)))
        u.mem_write(0xB0EA90, dwords(0x7FFFFFFF, 0x7FFFFFFF, 0x7FFFFFFF))
        first = self.address[self.first]
        if case.get('non_prism'):
            u.mem_write(first + 0x520, dwords(OTHER_TYPE))
            u.mem_write(OTHER_TYPE + 0xE44, dwords(*case.get('pixel_offset', [0, -4])))
            u.mem_write(OTHER_TYPE + 0x1764, b'\0')
        if case.get('unit_boundary'):
            # Caller identity/width control, not a locomotion/Unit GetFLH port.
            # Native Unit vtable selects its ordinary Techno GetFLH; only that
            # already-owned coordinate boundary is supplied for this control.
            u.mem_write(first, dwords(0x7F5C70))
            def unit_flh():
                output = self.arg(1)
                u.mem_write(output, dwords(*case['unit_boundary']))
                self.events.append(['supplied_unit_flh', case['unit_boundary']])
                self.ret(20, output)
            self.at(0x6F3AD0, unit_flh)
        u.mem_write(first + 0x9C, dwords(*case.get('location', [3200, 3200, 0])))
        target = case.get('target_building')
        if target:
            u.mem_write(ACTOR, bytes(u.mem_read(first, BUILDING_SIZE)))
            u.mem_write(ACTOR + 0x14, dwords(self.read32(ACTOR + 0x14) | 2))
            u.mem_write(ACTOR + 0x520, dwords(OTHER_TYPE))
            u.mem_write(OTHER_TYPE + 0xEF0, dwords(target.get('foundation_index', 3)))
            u.mem_write(OTHER_TYPE + 0xEBC, dwords(*target.get('target_offset', [0, 0, 0])))
            u.mem_write(ACTOR + 0x9C, dwords(*target['location']))
        elif 'target_location' in case:
            u.mem_write(ACTOR + 0x9C, dwords(*case['target_location']))
        # Observe raw and ranged RNG entries regardless of stream; neither is
        # substituted. The complete stream bytes are also compared below.
        for pc in (0x65C780, 0x65C7E0):
            self.at(pc, lambda pc=pc: self.rng_entries.append(hex(pc)))
        for pc in (0x453840, 0x6F3AD0, 0x445E50, 0x459EF0, 0x4500A0, 0x447AC0, 0x41BDD0, 0x410540, 0x6D2070):
            self.at(pc, lambda pc=pc: self.getters.append(hex(pc)))

    def deleted(self):
        self.freed.append(self.arg(1) - HEAP)
        self.ret(0)

    def rng(self):
        assert hashlib.sha256(self.u.mem_read(TEXT_BEGIN, TEXT_SIZE)).hexdigest() == self.text_sha256
        return laser_rng_state(self.u, SCENARIO)

    def registered(self):
        return [self.read32(LASER_ITEMS + 4 * i) for i in range(self.read32(LASERS + 0x10))]

    def create(self, kind='main'):
        u, actor = self.u, self.address[self.first]
        if kind == 'support':
            master = self.address.get('receiver')
            if master:
                self.invoke(GET_FLH, master, COORD, 0, 0, 0, 0)
                payload = list(struct.unpack('<3i', u.mem_read(COORD, 12)))
            else:
                payload = self.case.get('payload', [3600, 2700, 0])
            u.mem_write(actor + 0x704, dwords(2, *payload, 1))
            self.invoke(PROCESS_DELAYED_FIRE, actor)
        else:
            u.mem_write(COORD, dwords(0, 0, 0, self.case.get('selected_slot', 0)))
            for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, COORD),
                               (UC_X86_REG_ESI, actor), (UC_X86_REG_EBX, SHOT_WEAPON),
                               (UC_X86_REG_EDI, ACTOR)):
                u.reg_write(reg, value)
            end = LASER_FIRE_END if self.case.get('is_laser', True) else 0x6FF57D
            run_checked(u, LASER_FIRE_ARM, end, count=200000)
            assert u.reg_read(UC_X86_REG_ESP) == SP
        return self.registered()[-1] if self.registered() else None


def laser_birth(case):
    s = LaserScene(case)
    before = s.rng()
    pointer = s.create(case.get('kind', 'main'))
    assert before == s.rng() and not s.rng_entries
    return dict(input=case, calls=s.events, getter_entries=s.getters,
                laser=laser_state(s.u, pointer) if pointer else None,
                registered=len(s.registered()), actor_after=s.state(s.first),
                rng_before=before, rng_after=s.rng(), rng_entries=s.rng_entries)


def laser_birth_cases():
    rows = [dict(name='main_' + str(count), master=dict(count=count))
            for count in (-1, 0, 1, 8)]
    rows += [dict(name='main_duration_' + str(n), duration_byte=n) for n in (0, 1, 10, 127, 128, 255)]
    rows += [dict(name='main_gate_disabled', is_laser=False),
             dict(name='main_new_fails', new_fails=True),
             dict(name='main_raised', location=[3200, 3200, 416], target_location=[3600, 2700, 208]),
             dict(name='support', kind='support'),
             dict(name='support_duration_30', kind='support', rules=dict(duration=30)),
             dict(name='support_new_fails', kind='support', new_fails=True),
             dict(name='production_main', location=[9856, 12416, 0], master=dict(count=1),
                  target_building=dict(location=[11648, 12416, 0], foundation_index=3)),
             dict(name='production_support', kind='support', location=[9856, 14208, 0],
                  towers=[dict(name='receiver', offset=[6656, 9216, 0])])]
    rows += [dict(name=f'{caller}_house{int(house)}_big{int(big)}',
                  house_color=house, big_laser=big,
                  **({'unit_boundary': [3011, 3123, 80]} if caller == 'unit'
                     else {'non_prism': True}))
             for caller in ('unit', 'other_building')
             for house in (False, True) for big in (False, True)]
    rows += [dict(name='building_secondary_uses_current_primary_explicit', selected_slot=1,
                  current_weapon=dict(is_laser=False, duration_byte=31, house_color=False,
                                      inner=[7, 19, 31], spread=[5, 9, 11])),
             dict(name='building_secondary_uses_current_primary_house', selected_slot=1,
                  duration_byte=31, house_color=False, current_weapon=dict(duration_byte=15)),
             dict(name='building_secondary_gate_uses_selected_weapon', selected_slot=1,
                  is_laser=False, current_weapon=dict(is_laser=True))]
    return rows


def laser_lifetime(case):
    s = LaserScene(case)
    pointer = s.create(case.get('kind', 'main'))
    assert pointer is not None
    before = s.rng()
    states = [dict(frame=s.frame0, phase='birth', registered=len(s.registered()),
                   laser=laser_state(s.u, pointer))]
    for frame in case['visits']:
        s.u.mem_write(FRAME, dwords(frame))
        s.invoke(LASER_UPDATE, 0)
        states.append(dict(frame=frame, phase='logic', registered=len(s.registered()),
                           laser=laser_state(s.u, pointer) if pointer in s.registered() else None))
    assert before == s.rng() and not s.rng_entries
    return dict(input=case, states=states, freed=s.freed, rng_unchanged=True, rng_entries=s.rng_entries)


def laser_lifetime_cases():
    return [dict(name='main_15', visits=list(range(200, 217))),
            dict(name='support_15', kind='support', visits=list(range(200, 217))),
            dict(name='repeated_and_skipped_frame', duration_byte=3,
                 visits=[200, 200, 201, 201, 208, 208, 220, 221]),
            dict(name='zero_duration', duration_byte=0, visits=[200]),
            dict(name='negative_byte_duration', duration_byte=255, visits=[200]),
            dict(name='signed_frame_wrap', birth_frame=0x7FFFFFFE, duration_byte=3,
                 visits=[0x7FFFFFFE, 0x7FFFFFFF, -0x80000000, -0x7FFFFFFF])]


def laser_flh(case):
    s = LaserScene(case)
    actor, u = s.address[s.first], s.u
    u.mem_write(actor + 0x3B8, dwords(case.get('burst', 0)))
    s.invoke(0x4C91E0, actor + 0x388, 0)
    u.mem_write(COORD + 64, dwords(case.get('heading', 0x2000)))
    s.invoke(0x4C9300, actor + 0x388, COORD + 64)
    before = s.rng()
    s.invoke(GET_FLH, actor, COORD, case.get('slot', 0), *case.get('base', [0, 0, 0]))
    assert before == s.rng() and not s.rng_entries
    return dict(input=case, source=list(struct.unpack('<3i', u.mem_read(COORD, 12))),
                getter_entries=s.getters, rng_unchanged=True)


def laser_flh_cases():
    return [dict(name=f'{flh}_dual{int(dual)}_slot{slot}_burst{burst}', primary_dual=dual,
                 slot=slot, burst=burst,
                 **(dict(primary_flh=[80, 40, 120], secondary_flh=[30, 25, 90],
                         pixel_offset=[6, -4], base=[7, 11, 13]) if flh == 'control' else {}))
            for flh in ('stock', 'control') for dual in (False, True)
            for slot in (0, 1) for burst in (0, 1)]


def disk_ring_init():
    s = LaserScene(dict(name='disk_ring_init'))
    seed_laser_rngs(s.invoke, SCENARIO, 31)
    before = s.rng()
    trig = bytes(s.u.mem_read(0x84F084, 40960))
    s.invoke(DISK_RING_INIT, 0)
    raw = bytes(s.u.mem_read(DISK_RING, 128))
    assert before == s.rng() and not s.rng_entries
    assert trig == bytes(s.u.mem_read(0x84F084, 40960))
    return dict(entry=hex(DISK_RING_INIT), table=hex(DISK_RING),
                xy=[list(pair) for pair in struct.iter_unpack('<2i', raw)],
                rng_before=before, rng_after=s.rng(),
                invariants=dict(text_unchanged=True, trig_table_unchanged=True,
                                rng_unchanged=True))


def disk_muzzle_cases():
    base = dict(source=[2688, 2688, 750], target=[2944, 2688, 0],
                marked=True, on_bridge=False, seed=31, frame=200,
                primary_flh=[0, 0, 75], turret_offset=0, has_turret=True,
                tilt_crash_jumpjet=True, rocking_bits=[0, 0],
                weapon_index=0, burst=0, base=[0, 0, 0])
    rows = [dict(base, name=f'stock_facing_{heading:04x}',
                 primary_facing=heading, secondary_facing=heading)
            for heading in (0, 0x4000, 0x6200, 0xC000)]
    rows.append(dict(base, name='unmarked_750', marked=False,
                     primary_facing=0x4000, secondary_facing=0x4000))
    rows.extend(dict(base, name=f'marked_{height}', source=[2688, 2688, height],
                     primary_facing=0x4000, secondary_facing=0x4000)
                for height in (207, 208))
    return rows


def prepare_disk_scene(case):
    """Shared retained Unit/Jumpjet inputs on the existing LaserScene owner.

    The inherited building/Drive is not reinterpreted as a Jumpjet. Spare
    storage in this same LaserScene VM holds the native-constructed locomotor
    and a retained Unit prestate; no getter/matrix/height answer is supplied.
    """
    from tools.spatial_oracle.jumpjet_coordinates import initialize_jumpjet_levels
    from tools.spatial_oracle.object_flight_height import initialize_object_scalars
    s = LaserScene(dict(name=case['name'], laser_capacity=case.get('laser_capacity', 16)))
    u = s.u
    assert 'unit_boundary' not in s.case
    for pointer, size in ((DISK_SOURCE, 0x700), (DISK_TYPE, 0x1000),
                          (DISK_LOCO, 0x98), (DISK_TARGET, 0x700),
                          (DISK_TARGET_TYPE, 0x1000)):
        u.mem_write(pointer, bytes(size))
    # Original Unit73543A and Foot4D34DA..4D34E2 establish the actual table
    # and Foot classification bit. These are retained object inputs, not a
    # full object constructor/Scenario loading claim.
    for pointer, typ, coord, health in ((DISK_SOURCE, DISK_TYPE, case['source'], 600),
                                        (DISK_TARGET, DISK_TARGET_TYPE, case['target'], 300)):
        u.mem_write(pointer, dwords(0x7F5C70))
        u.mem_write(pointer + 0x14, dwords(7))
        u.mem_write(pointer + 0x6C, dwords(health))
        u.mem_write(pointer + 0x90, b'\1')
        u.mem_write(pointer + 0x9C, dwords(*coord))
        u.mem_write(pointer + 0x6C4, dwords(typ))
        u.mem_write(pointer + 0x21C, dwords(HOUSE))
        u.mem_write(typ, dwords(0x7F6218))
    u.mem_write(DISK_SOURCE + 0x74, bytes([case['marked']]))
    u.mem_write(DISK_SOURCE + 0x8C, bytes([case['on_bridge']]))
    u.mem_write(DISK_SOURCE + 0x2B4, dwords(DISK_TARGET))
    u.mem_write(DISK_SOURCE + 0x328, dwords(*case['rocking_bits']))
    u.mem_write(DISK_SOURCE + 0x3B8, dwords(case['burst']))
    u.mem_write(DISK_TYPE + 0x898, dwords(SHOT_WEAPON, *case['primary_flh']))
    u.mem_write(DISK_TYPE + 0x720, dwords(case['turret_offset']))
    u.mem_write(DISK_TYPE + 0xCA1, bytes([case['has_turret']]))
    u.mem_write(DISK_TYPE + 0xD22, bytes([case['tilt_crash_jumpjet']]))
    # Stock DISK values copied by original Link54AD30. The differing-case
    # JumpJetTurnRate/JumpJetAccel keys leave native defaults4/2.0 intact.
    u.mem_write(DISK_TYPE + 0xD70, dwords(4, 16) + struct.pack('<ff', 8., 15.)
                + dwords(750) + struct.pack('<ff', 2., .1) + dwords(1, 15))
    object_initializers = initialize_object_scalars(
        u, lambda entry, receiver, args: s.invoke(entry, receiver, *args))
    initialize_jumpjet_levels(lambda entry, receiver, args: s.invoke(entry, receiver, *args))
    u.mem_write(FRAME, dwords(case['frame']))
    s.invoke(0x54AC40, DISK_LOCO)
    s.invoke(0x54AD30, 0, DISK_LOCO + 4, DISK_SOURCE)
    u.mem_write(DISK_SOURCE + 0x674, dwords(DISK_LOCO + 4))
    for offset, heading in ((0x388, case['primary_facing']), (0x3A0, case['secondary_facing'])):
        s.invoke(0x4C91E0, DISK_SOURCE + offset, 100)
        u.mem_write(DISK_OUT + 0xC0, dwords(heading))
        s.invoke(0x4C9300, DISK_SOURCE + offset, DISK_OUT + 0xC0)
    seed_laser_rngs(s.invoke, SCENARIO, case['seed'])
    assert s.read32(DISK_LOCO + 4) == 0x7ECD68
    assert s.read32(0x7ECD68 + 0x24) == 0x54DCC0
    assert s.read32(0x7F5C70 + 0xB0) == 0x6F3AD0
    return s, object_initializers


def disk_muzzle(case):
    """Actual Unit/Jumpjet GetFLH from declared post-read stock component inputs."""
    s, object_initializers = prepare_disk_scene(case)
    u = s.u
    tables = {pointer: bytes(u.mem_read(pointer, size)) for pointer, size in
              ((0x7F5C70, 0x600), (0x7F6218, 0x100), (0x7ECD68, 0xD0))}
    bodies = {pointer: bytes(u.mem_read(pointer, size)) for pointer, size in
              ((DISK_SOURCE, 0x700), (DISK_TYPE, 0x1000), (DISK_LOCO, 0x98),
               (DISK_TARGET, 0x700), (DISK_TARGET_TYPE, 0x1000))}
    trig = bytes(u.mem_read(0x84F084, 40960))
    entries = []
    reached = (0x6F3AD0, 0x54DCC0, 0x55A730, 0x5F65A0, 0x41BE00, 0x41BDD0,
               0x4DE620, 0x5F6B90, 0x5F5F40, 0x578080, 0x746E30, 0x70E140, 0x7C5F00)
    for entry in reached:
        s.at(entry, lambda entry=entry: entries.append(hex(entry)))
    before = s.rng()
    def coordinate(pointer, slot, output):
        s.invoke(s.read32(s.read32(pointer) + slot), pointer, output)
        return list(struct.unpack('<3i', u.mem_read(output, 12)))
    source = coordinate(DISK_SOURCE, 0x48, DISK_OUT)
    render = coordinate(DISK_SOURCE, 0xAC, DISK_OUT + 0x10)
    target = coordinate(DISK_TARGET, 0x48, DISK_OUT + 0x20)
    target_effect = coordinate(DISK_TARGET, 0xA4, DISK_OUT + 0x30)
    s.invoke(s.read32(s.read32(DISK_SOURCE) + 0xB0), DISK_SOURCE,
             DISK_OUT + 0x40, case['weapon_index'], *case['base'])
    muzzle = list(struct.unpack('<3i', u.mem_read(DISK_OUT + 0x40, 12)))
    s.invoke(s.read32(0x7ECD68 + 0x24), 0, DISK_LOCO + 4, DISK_OUT + 0x60, 0)
    matrix = [f'{word:08x}' for word in struct.unpack('<12I', u.mem_read(DISK_OUT + 0x60, 48))]
    height_begin = len(entries)
    height = signed(s.invoke(s.read32(0x7F5C70 + 0x1C8), DISK_SOURCE))
    height_entries = entries[height_begin:]
    high_begin = len(entries)
    high = bool(s.invoke(s.read32(0x7F5C70 + 0x54), DISK_SOURCE) & 255)
    high_entries = entries[high_begin:]
    assert before == s.rng() and not s.rng_entries
    assert all(raw == bytes(u.mem_read(pointer, len(raw))) for pointer, raw in tables.items())
    assert all(raw == bytes(u.mem_read(pointer, len(raw))) for pointer, raw in bodies.items())
    assert trig == bytes(u.mem_read(0x84F084, 40960))
    assert all(hex(entry) in entries for entry in (0x6F3AD0, 0x54DCC0, 0x55A730, 0x41BE00, 0x41BDD0))
    assert ('0x5f5f40' in high_entries) == case['marked']
    return dict(input=case, source_coords=source, render_coords=render, muzzle=muzzle,
                target_coords=target, target_effect_coords=target_effect,
                height=height, high_flying=high, draw_matrix_bits=matrix,
                getter_entries=entries, height_query_entries=height_entries,
                high_flying_entries=high_entries,
                object_initializers=[hex(entry) for entry in object_initializers],
                rng_before=before, rng_after=s.rng(),
                invariants=dict(text_unchanged=True, vtable_unchanged=True,
                                source_target_type_locomotor_unchanged=True,
                                trig_table_unchanged=True, rng_unchanged=True))


class LaserPixels:
    """Original laser draw on the existing rally surface/A/projection owner."""
    def __init__(self, case):
        self.case = case
        self.surface = rally.Rally(case, size=case.get('size', rally.SIZE))
        self.u = u = self.surface.u
        self.calls, self.rng_entries, self.draw_order, self.fade = [], [], [], []
        self.ranged_calls, self.pending_ranges = [], []
        self.raw_draws = []
        self.general = not case.get('house_color', True)
        u.mem_map(PRISM, PRISM_SIZE)
        if self.general:
            # Declared complete unrelated stream priors for this draw boundary.
            # GeneralDraw itself consumes only process Main through Ranged.
            self.scenario = PRISM + 0x50000
            u.mem_write(0xA8B230, dwords(self.scenario))
            seed_laser_rngs(self.call, self.scenario, case.get('seed', 31))
        self.pointer = HEAP
        # Only these DSurface draw entries replace BSurface's no-op slots;
        # the original BSurface lock/stride/unlock methods still execute.
        u.mem_write(rally.VTABLE + 0x34, dwords(LASER_PACKED))
        u.mem_write(rally.VTABLE + 0x40, dwords(LASER_ADDITIVE))
        self.surface_tables = {pointer: bytes(u.mem_read(pointer, 0x84))
                               for pointer in (rally.VTABLE, 0x7E2070)}
        self.z_surface, self.z_object, self.z_pixels = (rally.MEM + n for n in (0x60000, 0x60080, 0x70000))
        w, h = self.surface.size
        self.pixel_bytes = w * h * 2
        u.mem_write(self.z_surface, dwords(0x7E2070, w, h, 0, 2, self.z_pixels, self.pixel_bytes, 0))
        u.mem_write(self.z_object, dwords(0, 0, w, h, 0, self.z_surface, self.z_pixels,
                                          self.z_pixels + self.pixel_bytes, self.pixel_bytes, 32768, w))
        u.mem_write(0x887644, dwords(self.z_object))
        z = case.get('z', 65535)
        values = ([z] * (w * h) if isinstance(z, int)
                  else [z[(x // 11 + y // 7) % len(z)] for y in range(h) for x in range(w)])
        self.before_z = struct.pack('<' + 'H' * len(values), *values)
        u.mem_write(self.z_pixels, self.before_z)
        self.before_alpha = bytes(u.mem_read(rally.ALPHA, self.pixel_bytes))
        background = case.get('background', rally.BACKGROUND)
        self.before = struct.pack('<H', background) * (w * h)
        u.mem_write(rally.PIXELS, self.before)
        self.guard = bytes([0xA5]) * 32
        u.mem_write(rally.PIXELS - 32, self.guard)
        u.mem_write(rally.PIXELS + self.pixel_bytes, self.guard)
        u.mem_write(0xA8EB78, dwords(case.get('detail', 2)))
        u.mem_write(0xABCD44, dwords(case.get('fps', 60)))
        u.mem_write(0xABCD50, bytes([case.get('reduced', False)]))
        u.mem_write(0x8A0DF0, b'\0')
        u.mem_write(FRAME, dwords(200))
        u.mem_write(LASERS, dwords(0x7ECEDC, LASER_ITEMS, 16, 1, 0, 10))
        u.hook_add(UC_HOOK_CODE, self.observe)
        # Original constructor with supplied drawing controls. Stock crop
        # arguments are also asserted against original laser_birth below.
        rgb = case.get('rgb', LASER_COLOR)
        packed_rgb = rgb[0] | rgb[1] << 8 | rgb[2] << 16
        outer, spread = case.get('outer', [0, 0, 0]), case.get('spread', [0, 0, 0])
        packed_outer = outer[0] | outer[1] << 8 | outer[2] << 16
        packed_spread = spread[0] | spread[1] << 8 | spread[2] << 16
        self.call(LASER_CTOR, self.pointer,
                  *case.get('source', [3038, 3038, 0]), *case.get('target', [3600, 2700, 0]),
                  case.get('z_adjust', -4), 1, packed_rgb, packed_outer, packed_spread,
                  case.get('duration', 15), 0, case.get('fades', 1),
                  case.get('fade_start_bits', 0x3F800000), case.get('fade_end_bits', 0))
        u.mem_write(self.pointer, dwords(case.get('age', 0)))
        u.mem_write(self.pointer + 0x1C, dwords(case.get('width', 3)))
        u.mem_write(self.pointer + 0x20,
                    bytes([case.get('house_color', True), case.get('supported', False)]))
        self.pointers = [self.pointer]
        if case.get('second_laser'):
            second = self.pointer + 96
            self.call(LASER_CTOR, second, *case.get('target', [3600, 2700, 0]),
                      *case.get('source', [3038, 3038, 0]), -2, 1, 0x6496C8, 0, 0, 15,
                      0, 1, 0x3F800000, 0)
            u.mem_write(second + 0x20, b'\1')
            self.pointers.append(second)
        self.text_sha256 = hashlib.sha256(u.mem_read(TEXT_BEGIN, TEXT_SIZE)).hexdigest()
        assert self.text_sha256 == '4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc'

    def call(self, entry, this=0, *args):
        self.u.mem_write(SP, dwords(RET_MAGIC, *args))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ECX, this)
        run_checked(self.u, entry, RET_MAGIC, count=3000000)

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == ATEXIT:
            u.reg_write(UC_X86_REG_EIP, struct.unpack('<I', u.mem_read(sp, 4))[0])
            u.reg_write(UC_X86_REG_ESP, sp + 4)
            return
        if pc == LASER_DRAW:
            self.draw_order.append(u.reg_read(UC_X86_REG_ECX) - HEAP)
        if pc == 0x550C2C:
            self.fade.append(dict(octant=struct.unpack('<I', u.mem_read(sp + 0x28, 4))[0],
                                  intensity_bits=struct.unpack('<I', u.mem_read(sp + 0x44, 4))[0],
                                  toward_white=signed(u.reg_read(UC_X86_REG_EAX))))
        if self.general and pc == 0x550611:
            # Whole GeneralDraw's native float store precedes the detail test.
            # Detail0 records this intensity too, although packed drawing ignores it.
            self.fade.append(dict(octant=u.reg_read(UC_X86_REG_ESI),
                                  intensity_bits=struct.unpack('<I', u.mem_read(sp + 0x1C, 4))[0]))
        if pc in (LASER_PACKED, LASER_ADDITIVE):
            args = struct.unpack('<7I', u.mem_read(sp + 4, 28))
            row = dict(entry=hex(pc), clip=list(struct.unpack('<4i', u.mem_read(args[0], 16))),
                       from_point=list(struct.unpack('<2i', u.mem_read(args[1], 8))),
                       to_point=list(struct.unpack('<2i', u.mem_read(args[2], 8))))
            if pc == LASER_ADDITIVE:
                row.update(rgb=list(u.mem_read(args[3], 3)), intensity_bits=args[4],
                           z_start=signed(args[5]), z_end=signed(args[6]))
            else:
                row.update(color=args[3], z_start=signed(args[4]), z_end=signed(args[5]), write_z=args[6])
            self.calls.append(row)
        if pc in (0x65C780, 0x65C7E0):
            self.rng_entries.append(hex(pc))
        if self.general:
            assert pc != 0x4BC750, 'zero-outer AA drawing is outside these General controls'
            # The canonical RNG oracle (gen_rng_vectors.ranged_history)
            # observes these unmasked ESI stores: RandomRanged inlines its
            # raw draw, so entering RandomNext is not a draw-count receipt.
            if pc in (0x65C79D, 0x65C84B):
                self.raw_draws.append(dict(entry=hex(pc),
                                           word=u.reg_read(UC_X86_REG_ESI) & 0xFFFFFFFF))
            if self.pending_ranges and pc == self.pending_ranges[-1][0]:
                _, index = self.pending_ranges.pop()
                self.ranged_calls[index]['value'] = signed(u.reg_read(UC_X86_REG_EAX))
            if pc == 0x65C7E0:
                receiver = u.reg_read(UC_X86_REG_ECX)
                stream = {0x886B88: 'main', self.scenario + 0x218: 'scenario',
                          0xABE890: 'mapgen'}.get(receiver, hex(receiver))
                lo, hi = struct.unpack('<2i', u.mem_read(sp + 4, 8))
                self.ranged_calls.append(dict(stream=stream, lo=lo, hi=hi))
                self.pending_ranges.append((struct.unpack('<I', u.mem_read(sp, 4))[0],
                                            len(self.ranged_calls) - 1))

    def draw(self):
        before_laser = [bytes(self.u.mem_read(pointer, 0x5C)) for pointer in self.pointers]
        rng_before = (laser_rng_state(self.u, self.scenario) if self.general else
                      {name: bytes(self.u.mem_read(pointer, 0x3F4))
                       for name, pointer in (('main', 0x886B88), ('mapgen', 0xABE890))})
        self.call(LASER_DRAW_ALL)
        raw = bytes(self.u.mem_read(rally.PIXELS, self.pixel_bytes))
        assert self.before_z == bytes(self.u.mem_read(self.z_pixels, self.pixel_bytes))
        assert self.guard == bytes(self.u.mem_read(rally.PIXELS - 32, 32))
        assert self.guard == bytes(self.u.mem_read(rally.PIXELS + self.pixel_bytes, 32))
        assert before_laser == [bytes(self.u.mem_read(pointer, 0x5C)) for pointer in self.pointers]
        assert hashlib.sha256(self.u.mem_read(TEXT_BEGIN, TEXT_SIZE)).hexdigest() == self.text_sha256
        if self.general:
            rng_after = laser_rng_state(self.u, self.scenario)
            assert not self.pending_ranges
            assert len(self.ranged_calls) == 3
            assert all(row['stream'] == 'main' and 'value' in row for row in self.ranged_calls)
            assert rng_before['scenario'] == rng_after['scenario']
            assert rng_before['mapgen'] == rng_after['mapgen']
            if not any(self.case.get('spread', [0, 0, 0])):
                assert rng_before == rng_after
                assert not self.raw_draws
            assert self.before_alpha == bytes(self.u.mem_read(rally.ALPHA, self.pixel_bytes))
            assert all(blob == bytes(self.u.mem_read(pointer, len(blob)))
                       for pointer, blob in self.surface_tables.items())
        else:
            assert not self.rng_entries
            assert all(blob == bytes(self.u.mem_read(pointer, 0x3F4))
                       for name, pointer in (('main', 0x886B88), ('mapgen', 0xABE890))
                       for blob in (rng_before[name],))
        w, _ = self.surface.size
        result = dict(input=self.case, laser=laser_state(self.u, self.pointer), fade=self.fade,
                      segments=self.calls, draw_order=self.draw_order, rng_entries=self.rng_entries,
                      reduced_effects_after=self.u.mem_read(0xABCD50, 1)[0],
                      pixels=[[i % w, i // w, value[0]] for i, value in enumerate(struct.iter_unpack('<H', raw))
                              if raw[i * 2:i * 2 + 2] != self.before[i * 2:i * 2 + 2]],
                      pixel_sha256=hashlib.sha256(raw).hexdigest(), z_unchanged=True,
                      laser_unchanged=True, guard_unchanged=True)
        if self.general:
            result.update(rng_before=rng_before, rng_after=rng_after,
                          ranged_calls=self.ranged_calls,
                          raw_draws=self.raw_draws,
                          alpha_unchanged=True, text_unchanged=True, vtable_unchanged=True)
        return result


def laser_draw_cases():
    base = dict(camera=[-20, 300])
    rows = []
    for detail in (0, 2):
        for supported in (False, True):
            for age in (0, 1, 7, 13, 14):
                rows.append(dict(base, name=f'detail{detail}_support{int(supported)}_age{age}',
                                 detail=detail, supported=supported, width=5 if supported else 3, age=age))
        for dx, dy in ((512, 0), (512, 512), (0, 512), (-512, 512),
                       (-512, 0), (-512, -512), (0, -512), (512, -512)):
            rows.append(dict(base, name=f'octant_{detail}_{dx}_{dy}', detail=detail,
                             source=[3200, 3200, 0], target=[3200 + dx, 3200 + dy, 0], camera=[-80, 315]))
        for suffix, extra in (
                ('alpha_mixed', dict(alpha='mixed')), ('alpha_black', dict(alpha='black')),
                ('z_mixed', dict(z=[0, 32700, 65535])),
                ('raised', dict(source=[3038, 3038, 208], target=[3600, 2700, 416])),
                ('clip_left', dict(camera=[20, 300])), ('clip_right', dict(camera=[-100, 300])),
                ('clip_origin', dict(clip=[10, 12, 110, 85])),
                ('same_point', dict(target=[3038, 3038, 0])),
                ('saturation', dict(background=0xE73C, rgb=[240, 120, 200], supported=True, width=5)),
                ('zero_duration', dict(duration=0)), ('dark', dict(rgb=[1, 2, 3])),
                ('late', dict(age=15)), ('width_zero', dict(width=0))):
            rows.append(dict(base, name=f'detail{detail}_{suffix}', detail=detail, **extra))
    rows.extend([dict(base, name='fps_low', fps=0), dict(base, name='fps_recover', fps=60, reduced=True),
                 dict(base, name='reverse_draw_order', second_laser=True)])
    # Birth geometry is independently established by the stock GetFLH and
    # target-coordinate readers above. Viewport is a supplied160x120 crop.
    for detail in (0, 2):
        rows.extend([
            dict(name=f'stock_main_crop_detail{detail}', detail=detail,
                 source=[9694, 12254, 378], target=[11776, 12544, 0], z_adjust=-58,
                 width=5, supported=True, camera=[-330, 1220]),
            dict(name=f'stock_support_crop_detail{detail}', detail=detail,
                 source=[9694, 14046, 378], target=[9694, 12254, 378], z_adjust=0,
                 camera=[-550, 1220])])
    return rows


def disk_laser_draw_cases():
    base = dict(camera=[-20, 300], house_color=False, rgb=[216, 0, 184],
                outer=[80, 0, 88], spread=[0, 0, 0], width=1, supported=False,
                seed=31, fades=1, fade_start_bits=0x3F800000)
    rows = [dict(base, name=f'explicit_detail{detail}_age{age}_end{end:08x}',
                 detail=detail, age=age, duration=8, fade_end_bits=end)
            for detail, age, end in ((2, 4, 0x3F000000), (0, 4, 0x3F000000), (2, 7, 0))]
    for detail in (0, 2):
        for dx, dy in ((512, 0), (512, 512), (0, 512), (-512, 512),
                       (-512, 0), (-512, -512), (0, -512), (512, -512)):
            rows.append(dict(base, name=f'general_octant_{detail}_{dx}_{dy}',
                             source=[3200, 3200, 0], target=[3200 + dx, 3200 + dy, 0],
                             camera=[-80, 315], detail=detail, duration=8,
                             age=4, fade_end_bits=0x3F000000))
        for duration, age, end in ((1, 0, 0x3F000000), (8, 0, 0x3F000000),
                                   (8, 7, 0x3F000000), (15, 0, 0), (15, 7, 0), (15, 14, 0)):
            rows.append(dict(base, name=f'general_detail{detail}_duration{duration}_age{age}',
                             detail=detail, duration=duration, age=age, fade_end_bits=end))
        for suffix, extra in (
                ('alpha_mixed', dict(alpha='mixed')), ('alpha_black', dict(alpha='black')),
                ('z_mixed', dict(z=[0, 32700, 65535])),
                ('raised', dict(source=[3038, 3038, 208], target=[3600, 2700, 416])),
                ('clip_left', dict(camera=[20, 300])), ('clip_right', dict(camera=[-100, 300])),
                ('clip_origin', dict(clip=[10, 12, 110, 85]))):
            rows.append(dict(base, name=f'general_detail{detail}_{suffix}', detail=detail,
                             duration=8, age=4, fade_end_bits=0x3F000000, **extra))
    rows.append(dict(base, name='nonzero_spread_order', detail=2, duration=8,
                     age=4, fade_end_bits=0x3F000000, spread=[3, 5, 7]))
    return rows


def disk_lifecycle_priors():
    """Physical-informed post-read inputs, not a second Rules/SOUNDMD reader."""
    return dict(range_leptons=1792, inner=[216, 0, 184], outer=[80, 0, 88],
                spread=[0, 0, 0], duration_byte=15, is_house_color=False,
                warhead='disk_wh', charge_sound=0, report_sounds=[1],
                sound_indices_fixture_relative=True, source_report_selector=0,
                area_damage_boundary='0x489280', sound_boundary='0x7509e0')


def disk_lifecycle_cases():
    base = dict(disk_muzzle_cases()[1], laser_capacity=128, max_visits=64,
                births=[dict(name='shot', damage=90)], actions=[])
    rows = [dict(base, name='ordinary'),
            dict(base, name='current_coordinates', actions=[
                dict(frame=203, kind='coords', role='source', value=[2704, 2720, 750]),
                dict(frame=205, kind='coords', role='target', value=[2976, 2672, 0])]),
            dict(base, name='range_exit', actions=[
                dict(frame=202, kind='coords', role='target', value=[6000, 2688, 0])]),
            dict(base, name='owner_crashes', actions=[dict(frame=202, kind='crash')]),
            dict(base, name='owner_expiry', actions=[
                dict(frame=202, kind='expire', role='different'),
                dict(frame=203, kind='expire', role='source')]),
            dict(base, name='target_expiry', actions=[dict(frame=202, kind='expire', role='target')]),
            dict(base, name='duplicate_owner_expiry', actions=[
                dict(frame=202, kind='expire', role='source'),
                dict(frame=202, kind='expire', role='source')]),
            dict(base, name='reverse_birth_order', births=[
                dict(name='older', damage=90), dict(name='newer', damage=90)])]
    rows.append(dict(base, name='relative_angle_fire', births=[
        dict(name=f'angle_{dx}_{dy}', damage=90,
             target_coords=[2688 + dx, 2688 + dy, 0])
        for dx, dy in ((256, 0), (256, 256), (0, 256), (-256, 256),
                       (-256, 0), (-256, -256), (0, -256), (256, -256))],
        actions=[dict(frame=201, kind='expire', role='source')]))
    rows.append(dict(base, name='null_fire_inputs', births=[
        dict(name='null_owner', owner=None, damage=90),
        dict(name='null_target', target=None, damage=90),
        dict(name='null_weapon', weapon=None, damage=90)]))
    return rows


def disk_lifecycle(case, priors):
    """Same native source/target VM, actual Disk rungs and common queue drain.

    Damage and audio terminate at declared existing service boundaries. The
    original Disk, coordinates, LaserDraw construction/update, duplicate queue
    compaction, RTTI and scalar destruction execute; freed bytes are not state.
    """
    from tools.native_oracle import initialize_empty_windows_seh
    from tools.projectile_oracle.ifv_impact import retirement_transport
    s, _object_initializers = prepare_disk_scene(case)
    u = s.u
    roles = {None: 0, 'source': DISK_SOURCE, 'target': DISK_TARGET,
             'weapon': SHOT_WEAPON, 'warhead': WARHEAD, 'different': ACTOR}
    s.names.update({pointer: name for name, pointer in roles.items()})
    phase, current_disk = 'setup', None
    births, states, laser_births, ai_visits = [], [], [], []
    damage_calls, sound_calls, detach_calls, retire_calls = [], [], [], []
    pending_lasers, transports = {}, []
    disks, freed_seen, free_calls = {}, 0, []

    def registry(vector):
        count = s.read32(vector + 0x10)
        assert count <= s.read32(vector + 8)
        pointer = s.read32(vector + 4)
        return [s.read32(pointer + 4 * index) for index in range(count)]

    assert not registry(DISKS) and not registry(PENDING_DELETE) and not s.registered()
    initialize_empty_windows_seh(u)
    # Native vector startup and native Resize own all headers and insertion.
    # The existing Scene allocator supplies storage; no host queue is written.
    for initializer, vector in ((0x4A6C70, DISKS), (0x725850, PENDING_DELETE)):
        s.invoke(initializer, 0)
        assert s.invoke(s.read32(s.read32(vector) + 8), vector, 64, 0) & 255
        assert not registry(vector)
    s.invoke(DISK_RING_INIT, 0)
    u.mem_write(SHOT_WEAPON + 0xAC, dwords(WARHEAD))
    u.mem_write(SHOT_WEAPON + 0xB4, dwords(priors['range_leptons']))
    u.mem_write(SHOT_WEAPON + 0x120, bytes(priors['inner'] + priors['outer'] + priors['spread']))
    u.mem_write(SHOT_WEAPON + 0x14D, bytes([priors['is_house_color'], priors['duration_byte']]))
    u.mem_write(RULES + 0x28C, dwords(priors['charge_sound']))
    report_items = COORD + 0x200
    u.mem_write(report_items, dwords(*priors['report_sounds']))
    u.mem_write(SHOT_WEAPON + 0xBC, dwords(0, report_items, len(priors['report_sounds']),
                                         0, len(priors['report_sounds']), 0))
    u.mem_write(DISK_SOURCE + 0x3C8, struct.pack('<H', priors['source_report_selector']))
    cursor = s.read32(SCENARIO + 0x214)
    rng_before = s.rng()
    tables = {pointer: bytes(u.mem_read(pointer, size)) for pointer, size in (
        (0x7F5C70, 0x600), (0x7F6218, 0x100), (0x7ECD68, 0xD0),
        (0x7E5FB8, 0x90), (0x7E5EDC, 0x18), (0x7E91EC, 0x18),
        (DISK_RING, 128), (0x84F084, 40960))}
    retained = {pointer: bytearray(u.mem_read(pointer, size)) for pointer, size in (
        (DISK_SOURCE, 0x700), (DISK_TYPE, 0x1000), (DISK_LOCO, 0x98),
        (DISK_TARGET, 0x700), (DISK_TARGET_TYPE, 0x1000), (SHOT_WEAPON, 0x200))}

    def authored_write(pointer, offset, raw):
        retained[pointer][offset:offset + len(raw)] = raw
        u.mem_write(pointer + offset, raw)

    def disk_state(pointer):
        assert s.read32(pointer) == 0x7E5FB8
        return dict(native_id=s.s32(pointer + 0x10), owner=s.name(s.read32(pointer + 0x24)),
                    target=s.name(s.read32(pointer + 0x28)), weapon=s.name(s.read32(pointer + 0x2C)),
                    countdown=s.s32(pointer + 0x30), start_index=s.s32(pointer + 0x34),
                    step=s.s32(pointer + 0x38), damage=s.s32(pointer + 0x3C))

    def boundary(logic_frame=None):
        nonlocal freed_seen
        assert s.read32(SCENARIO + 0x214) == cursor
        assert rng_before == laser_rng_state(u, SCENARIO)
        assert all(raw == bytes(u.mem_read(pointer, len(raw))) for pointer, raw in tables.items())
        assert all(bytes(raw) == bytes(u.mem_read(pointer, len(raw))) for pointer, raw in retained.items())
        active, lasers = registry(DISKS), s.registered()
        for offset in s.freed[freed_seen:]:
            free_calls.append(dict(frame=s.read32(FRAME), phase=phase, object=s.name(HEAP + offset)))
        freed_seen = len(s.freed)
        row = dict(frame=s.read32(FRAME), logic_frame=logic_frame, phase=phase,
                   disk_order=[s.name(pointer) for pointer in active],
                   pending=[s.name(pointer) for pointer in registry(PENDING_DELETE)],
                   disks={name: disk_state(pointer) if pointer in active else None
                          for name, pointer in disks.items()},
                   laser_order=[s.name(pointer) for pointer in lasers],
                   laser_ages=[s.s32(pointer) for pointer in lasers],
                   native_id_cursor=cursor, rng_unchanged=True)
        assert all(state is None or state['native_id'] == -1 for state in row['disks'].values())
        states.append(row)
        return row

    def observe(_u, pc, _size, _data):
        nonlocal current_disk
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in pending_lasers:
            row, pointer = pending_lasers.pop(pc)
            row['laser'] = laser_state(u, pointer)
        if pc == DISK_AI:
            pointer = u.reg_read(UC_X86_REG_ECX)
            current_disk = s.name(pointer)
            ai_visits.append(dict(frame=s.read32(FRAME), shot=current_disk,
                                  countdown=s.s32(pointer + 0x30), step=s.s32(pointer + 0x38)))
        elif pc == LASER_CTOR:
            assert s.events[-1][0] == 'laser'
            pointer = u.reg_read(UC_X86_REG_ECX)
            name = f'beam_{len(laser_births)}'
            s.names[pointer] = name
            row = dict(frame=s.read32(FRAME), shot=current_disk, name=name,
                       constructor=s.events[-1][1])
            laser_births.append(row)
            pending_lasers[s.read32(sp)] = row, pointer
        elif pc in (0x4A776B, 0x4A76E8):
            # Observe before CALL: inherited building_fixture owns7509E0's
            # supplied request return and changes ESP before later entry hooks.
            sound_calls.append(dict(frame=s.read32(FRAME), shot=current_disk,
                call_site=hex(pc), entry='0x7509e0', sound_id=signed(u.reg_read(UC_X86_REG_ECX)),
                coords=list(struct.unpack('<3i', u.mem_read(u.reg_read(UC_X86_REG_EDX), 12))),
                handle=s.name(s.read32(sp))))
        elif pc == 0x489280:
            args = [s.read32(sp + offset) for offset in (4, 8, 12, 16)]
            damage_calls.append(dict(frame=s.read32(FRAME), shot=current_disk,
                entry=hex(pc), coords=list(struct.unpack('<3i', u.mem_read(u.reg_read(UC_X86_REG_ECX), 12))),
                damage=signed(u.reg_read(UC_X86_REG_EDX)), firer=s.name(args[0]),
                warhead=s.name(args[1]), affects_tiberium=args[2], house=s.name(args[3])))
            s.ret(16, 0)
        elif pc == DISK_DETACH:
            detach_calls.append(dict(frame=s.read32(FRAME), phase=phase,
                shot=s.name(u.reg_read(UC_X86_REG_ECX)), expired=s.name(s.read32(sp + 4))))
        elif pc in (DISK_SCALAR_DTOR, 0x4101F0):
            pointer = u.reg_read(UC_X86_REG_ECX)
            if pointer in disks.values():
                retire_calls.append(dict(frame=s.read32(FRAME), phase=phase,
                    entry=hex(pc), shot=s.name(pointer),
                    delete_flag=s.read32(sp + 4) if pc == DISK_SCALAR_DTOR else None))
        elif pc in (0x410230, 0x68BCB0, 0x65C79D, 0x65C84B):
            raise AssertionError(('unexpected identity/RNG producer', hex(pc)))
        retirement_transport(s, pc, transports)

    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        for birth in case['births']:
            phase = 'constructor'
            if 'target_coords' in birth:
                authored_write(DISK_TARGET, 0x9C, dwords(*birth['target_coords']))
            pointer = s.invoke(OPERATOR_NEW, 0, 0x40)
            assert pointer
            disks[birth['name']] = pointer
            s.names[pointer] = birth['name']
            assert s.invoke(DISK_CTOR, pointer) == pointer
            constructed = disk_state(pointer)
            boundary()
            phase = 'fire'
            s.invoke(DISK_FIRE, pointer, roles[birth.get('owner', 'source')],
                     roles[birth.get('target', 'target')], roles[birth.get('weapon', 'weapon')],
                     birth['damage'])
            births.append(dict(name=birth['name'], constructed=constructed,
                               after_fire=disk_state(pointer)))
            boundary()
        retired = False
        for visit in range(case['max_visits']):
            frame = case['frame'] + 1 + visit
            u.mem_write(FRAME, dwords(frame))
            for action in case['actions']:
                if action['frame'] != frame:
                    continue
                phase = 'action'
                if action['kind'] == 'coords':
                    authored_write(roles[action['role']], 0x9C, dwords(*action['value']))
                elif action['kind'] == 'crash':
                    authored_write(DISK_SOURCE, 0x425, b'\1')
                elif action['kind'] == 'expire':
                    u.mem_write(SP, dwords(RET_MAGIC))
                    u.reg_write(UC_X86_REG_ESP, SP)
                    u.reg_write(UC_X86_REG_ESI, roles[action['role']])
                    run_checked(u, *DISK_EXPIRY, count=200000,
                                context=dict(control='disk_lifecycle', case=case['name'],
                                             phase=phase, frame=frame))
                    assert u.reg_read(UC_X86_REG_ESP) == SP
                else:
                    raise ValueError(action)
                boundary(frame)
            phase = 'disk'
            u.mem_write(SP, dwords(RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, *DISK_LOGIC, count=2000000,
                        context=dict(control='disk_lifecycle', case=case['name'],
                                     phase=phase, frame=frame))
            assert u.reg_read(UC_X86_REG_ESP) == SP and not pending_lasers
            boundary(frame)
            phase = 'laser_update'
            s.invoke(LASER_UPDATE, 0)
            boundary(frame)
            # Actual Main_Tick advances Frame before common725C70. These
            # rows isolate the already-ordered rungs rather than all Logic.
            u.mem_write(FRAME, dwords(frame + 1))
            phase = 'drain'
            s.invoke(0x725C70, 0)
            final = boundary(frame)
            if not final['disk_order'] and not final['pending'] and not final['laser_order']:
                retired = True
                break
        assert retired, (case['name'], 'bounded lifecycle did not retire')
        assert not pending_lasers and all('laser' in row for row in laser_births)
        for name in disks:
            assert sum(row['shot'] == name and row['entry'] == hex(DISK_SCALAR_DTOR)
                       for row in retire_calls) == 1
            assert sum(row['shot'] == name and row['entry'] == '0x4101f0'
                       for row in retire_calls) == 1
            assert sum(row['object'] == name for row in free_calls) == 1
        assert not s.rng_entries and rng_before == s.rng()
    finally:
        u.hook_del(hook)
    return dict(input=case, births=births, states=states, ai_visits=ai_visits, laser_births=laser_births,
                damage_calls=damage_calls, sound_calls=sound_calls, detach_calls=detach_calls,
                retire_calls=retire_calls, free_calls=free_calls, transports=transports,
                rng_before=rng_before, rng_after=s.rng(), native_id_cursor=cursor,
                invariants=dict(text_unchanged=True, vtable_and_numeric_tables_unchanged=True,
                    retained_inputs_unchanged_except_actions=True, rng_unchanged=True,
                    native_id_cursor_unchanged=True, natural_retirement=True))


def generate_disk():
    ring = disk_ring_init()
    muzzle = [disk_muzzle(case) for case in disk_muzzle_cases()]
    draws, offsets = [], None
    for case in disk_laser_draw_cases():
        s = LaserPixels(case)
        draws.append(s.draw())
        actual = [list(row) for row in struct.iter_unpack('<4i', s.u.mem_read(GENERAL_OFFSETS, 128))]
        if offsets is None:
            offsets = actual
        else:
            assert offsets == actual
    priors = disk_lifecycle_priors()
    return dict(source='unicorn/gamemd.exe', ring_init=ring,
                general_offsets=offsets, muzzle=muzzle, laser_draw=draws,
                lifecycle_priors=priors,
                lifecycle=[disk_lifecycle(case, priors) for case in disk_lifecycle_cases()])


def disk_metadata():
    return provenance(
        scope='Selected stock DISK query prerequisite and explicit nonblinking nonzero-outer '
              'LaserDraw general software path; original ring/table initialization and joined '
              'Disk constructor/Fire/AI/expiry/common deferred retirement with copied LaserDraw '
              'lifetimes. Damage and sound are service boundaries; no whole Scenario or save/load replay.',
        entry_points={'disk_ring_initializer': DISK_RING_INIT, 'techno_get_flh': 0x6F3AD0,
                      'jumpjet_constructor': 0x54AC40, 'jumpjet_link': 0x54AD30,
                      'jumpjet_draw_matrix': 0x54DCC0, 'base_draw_matrix': 0x55A730,
                      'get_coords': 0x5F65A0, 'get_render_coords': 0x41BE00,
                      'get_target_coords': 0x41BDD0, 'high_flying': 0x4DE620,
                      'object_high_flying': 0x5F6B90, 'get_height': 0x5F5F40,
                      'object_crt_table': 0x8141D8, 'laser_constructor': LASER_CTOR,
                      'laser_draw_all': LASER_DRAW_ALL, 'laser_general_draw': LASER_DRAW,
                      'general_intensity_observer': 0x550611,
                      'dsurface_additive': LASER_ADDITIVE, 'dsurface_packed': LASER_PACKED,
                      'disk_vector_initializer': 0x4A6C70, 'disk_constructor': DISK_CTOR,
                      'disk_fire': DISK_FIRE, 'disk_ai': DISK_AI,
                      'disk_logic_block': DISK_LOGIC[0], 'disk_expiry_block': DISK_EXPIRY[0],
                      'disk_detach': DISK_DETACH, 'pending_initializer': 0x725850,
                      'pending_append': 0x4A7FE0, 'pending_drain': 0x725C70,
                      'disk_scalar_destructor': DISK_SCALAR_DTOR,
                      'abstract_destructor': 0x4101F0, 'laser_update': LASER_UPDATE,
                      'area_damage_boundary': 0x489280, 'sound_boundary': 0x7509E0},
        assumptions=['FPCW0E7F; original file trig and affine/ftol bodies. Ring16pairs '
                     'come from original4A6CF0, not a regular polygon or host trigonometry.',
                     'Muzzle uses original Unit7F5C70/type7F6218, separately constructed '
                     'Jumpjet ILoco7ECD68 at object+4 and original Link/Facing. Source/target '
                     'retained locations, normal rank, source marked/OnBridge and zero rocking '
                     'are declared component priors. Type post-read priors are stock DISK '
                     'FLH0,0,75/TurretOffset0/Turret=yes/TiltCrashJumpjet=yes and Jumpjet '
                     'parameters4/16/8/15/750/2/.1/no-wobbles/15. Native Rules/ART registration '
                     'and full Unit construction/flight are not executed by this query boundary.',
                     'The inherited sparse flat Cell/map fixture supplies level0/slope0; actual '
                     'Map floor/GetHeight executes. Original Object14-entry CRT and Jumpjet '
                     'startup run. No native getter/matrix or VERA-derived output is supplied.',
                     'General draw controls use original LaserDraw ctor, explicit stock '
                     'inner216,0,184/outer80,0,88/spread0 and fade1->0 or1->.5. '
                     'The one custom nonzero-spread control retains original request/response '
                     'and complete Main/Scenario/Mapgen states seeded by the existing native owner.',
                     'General offset tableABC738 is read after original whole GeneralDraw '
                     'initializes it. It is separate from Special offsets. Existing Rally '
                     'projection/BSurface/DSurface, RGB565, Z/A and clip/camera are retained '
                     'component inputs; no new raster or GPU/whole Display claim.',
                     'Joined lifecycle retains the SAME prepared Unit/Jumpjet and target VM. '
                     'Native vector initializers and virtual Resize allocate headers/backing; '
                     'actual55B5A1..55B5BD runs Disk AI in reverse birth order, then original '
                     'LaserUpdate. Frame increments before actual725C70. The max-visits bound '
                     'must reach empty Disk/pending/Laser rosters without host removal.',
                     'Range1792 is a physical-informed post-read input (exact-case Range7 in '
                     'RULESMD; absent LANGRULE and no selected Battle/XMP03T4 override), not '
                     'native whole-reader execution here. Source/target coordinate changes, '
                     'crash byte425 and pointer-expiry inputs are declared between-rung controls, '
                     'not execution of movement, crash production or complete Unit UnInit.',
                     'Actual725977..725994 calls Detach in reverse Disk order. Fire/Detach '
                     'append directly and AI calls original4A7FE0; duplicates are retained until '
                     '725C70 removes all matching tickets before one actual4A7C90 scalar '
                     'destructor. This scalar body inlines4A7B00 work, calls Abstract4101F0 '
                     'and frees through the inherited allocator transport. Freed bytes are '
                     'never emitted as live Disk state.',
                     'Disk ctor leaves nativeID+10=-1 and does not spend Scenario+214. '
                     'Each phase checks that cursor and all complete Main/Scenario/Mapgen '
                     'states. Earlier FireAt GetROF/rearm draws and downstream damage/audio '
                     'RNG lie outside these joined service boundaries.'],
        substitutions=['Inherited LaserScene transport/unused building fixture seams remain; '
                       'none substitutes a reached Unit GetFLH, coordinate, height, Facing or '
                       'DrawMatrix query. Retained Unit/type state is supplied, not constructed '
                       'by an entire native Scenario.',
                       'Surface backing/locking use the existing original BSurface methods; '
                       'its copied draw slots select existing original DSurface implementations. '
                       'Outer-zero4BC750 AA, blinking and held tilt/bridge/movement are excluded.',
                       'Joined original AreaDamage489280 terminates after recording its actual '
                       'coordinates, damage and four arguments; no receiver health outcome is '
                       'invented. Existing building_fixture7509E0 transport ends at the sound '
                       'request; fixture-relative IDs0/1 do not claim SOUNDMD registration or '
                       'playback. Original caller PCs retain actual sound coordinates/NULL handle. '
                       'Existing initialize_empty_windows_seh and ifv_impact.retirement_transport '
                       'supply only Windows SEH/readability boundaries during original CRT RTTI. '
                       'Save/load/swizzle and whole native FireAt/Logic/Scenario remain excluded.'])


def laser_reset():
    s = LaserScene(dict(name='scene_reset'))
    pointers = [s.create(), s.create('support'), s.create()]
    before = [laser_state(s.u, p) for p in pointers]
    # Actual scene-clear caller; other scene classes and IStream transport are
    # outside this bounded boundary. Load67E739 reaches Clear_Scene6851F0,
    # whose685297 call reaches this534949 call (original instruction trace).
    s.u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(s.u, 0x534949, 0x53494E, count=200000)
    return dict(lasers_before=before, registered_after=len(s.registered()), freed=s.freed)


def laser_type_inputs(name):
    """Original whole constructor and focused original field readers.

    The existing HutTypeReader owns allocation and INI caches. Physical retail
    strings are lexical inputs, not parsed scalar goldens. The Rules TargetCoord
    reader is deliberately distinct from ART's image-section pixel offsets.
    Layered Scenario loading and unrelated type fields do not execute here.
    """
    m = rally.HutTypeReader()
    typ, u = m.construct(name), m.u
    def fields():
        return dict(image=m.string(typ + 0x1F8),
                    foundation=m.read32(typ + 0xEF0),
                    primary_pixel=list(struct.unpack('<2i', u.mem_read(typ + 0xE44, 8))),
                    secondary_pixel=list(struct.unpack('<2i', u.mem_read(typ + 0xE4C, 8))),
                    target_offset=list(struct.unpack('<3i', u.mem_read(typ + 0xEBC, 12))),
                    primary_flh=list(struct.unpack('<3i', u.mem_read(typ + 0x89C, 12))),
                    turret_offset=struct.unpack('<i', u.mem_read(typ + 0x720, 4))[0],
                    primary_dual=u.mem_read(typ + 0x1764, 1)[0],
                    turret_voxel=u.mem_read(typ + 0x16C5, 1)[0],
                    barrel_voxel=u.mem_read(typ + 0x16C6, 1)[0],
                    can_be_occupied=u.mem_read(typ + 0x157B, 1)[0])
    constructor = fields()
    seams = sorted({row['pc'] for row in m.trace if row['kind'] == 'fixture_seam'})
    physical, slices, reads = {}, [], []
    def observe(_u, pc, _size, _data):
        if pc not in (0x528A10, 0x529CA0, 0x529880, 0x474DA0, 0x5295F0):
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x529CA0, 0x529880):
            section, key, default = [m.read32(sp + n) for n in (8, 12, 16)]
            default = list(struct.unpack('<3i' if pc == 0x529CA0 else '<2i',
                                         u.mem_read(default, 12 if pc == 0x529CA0 else 8)))
        else:
            section, key, default = [m.read32(sp + n) for n in (4, 8, 12)]
            if pc == 0x528A10:
                default = m.string(default)
            elif pc == 0x5295F0:
                default &= 255
        reads.append(dict(phase=m.phase, reader=hex(pc), section=m.string(section),
                          key=m.string(key), default=default))
    u.hook_add(UC_HOOK_CODE, observe)
    for layer, filename, keys, spans in (
            ('rules', 'rulesmd.ini', ('Image', 'TargetCoordOffset', 'TurretAnimIsVoxel', 'BarrelAnimIsVoxel'),
             [('image', 0x5F92F9, 0x5F9340, -4),
              ('target_offset', 0x460F50, 0x460F9C, 0),
              ('voxel_flags', 0x464638, 0x46466C, 0)]),
            ('art', 'artmd.ini', ('Foundation', 'PrimaryFirePixelOffset', 'SecondaryFirePixelOffset',
                                  'PrimaryFireDualOffset', 'PrimaryFireFLH', 'TurretOffset'),
             [('foundation', 0x461225, 0x46125D, 0),
              ('turret_offset', 0x715876, 0x71589A, 0),
              ('primary_flh', 0x715D94, 0x715DCF, 0),
              ('pixel_offsets', 0x4612EE, 0x461365, 0)])):
        path = Path('ini') / filename
        raw = path.read_bytes()
        section_name = name if layer == 'rules' else fields()['image']
        section = rally.sections(raw)[section_name]
        values = {key: section.get(key) for key in keys}
        physical[layer] = dict(path=str(path), bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                               section=section_name, keys=values)
        m.make_ini({section_name: {k: v for k, v in values.items() if v is not None}})
        for label, begin, end, delta in spans:
            m.phase = label
            mark, before = len(reads), fields()
            u.mem_write(rally.READER_SP, dwords(RET_MAGIC))
            u.mem_write(rally.READER_SP + 0x1B4, dwords(rally.READER_INI))
            for register, value in ((UC_X86_REG_ESP, rally.READER_SP), (UC_X86_REG_EBP, typ),
                                    (UC_X86_REG_EBX, typ if label == 'image' else typ + 0x24),
                                    (UC_X86_REG_ESI, typ + 0x1F8 if label == 'turret_offset' else rally.READER_INI),
                                    (UC_X86_REG_EDI, typ + 0x1F8)):
                u.reg_write(register, value)
            # First TargetCoord instruction also stores preceding Artillary AL.
            u.reg_write(UC_X86_REG_EAX, u.mem_read(typ + 0x16CA, 1)[0])
            m.trace = []
            run_checked(u, begin, end, count=200000)
            assert u.reg_read(UC_X86_REG_ESP) == rally.READER_SP + delta
            assert not any(row['kind'] == 'fixture_seam' for row in m.trace)
            m.unchanged()
            slices.append(dict(name=label, begin=hex(begin), end_exclusive=hex(end),
                               before=before, reads=reads[mark:], after=fields(), stack_delta=delta))
    return dict(type_id=name, physical_files=physical, constructor=constructor,
                constructor_fixture_seams=seams, reader_slices=slices, result=fields(),
                text_unchanged=True, text_sha256=m.original)


def laser_fps(case):
    """Original Throttle tail plus GetMinFrameRate, explicit OS clock only.

    Reuse Scene's VM and fast_scroll's return transport. The timeGetTime import
    is supplied; original6C8C40 performs SHR4. Each row adds a supplied number
    of Logic visits (original55AFB0..55AFD9 increment); the remaining Logic body
    and Main_Tick admission are outside this timing boundary.
    """
    s = LaserScene(dict(name=case['name']))
    u = s.u
    clock_address = COORD + 0x800
    u.mem_write(0x7E1530, dwords(clock_address))
    current_ms, reads = 0, []
    def clock():
        reads.append(current_ms)
        return_from_sink(u, 0, current_ms)
    s.at(clock_address, clock)
    u.mem_write(0xABCD40, dwords(case.get('count', 0), case.get('fps', 0), case.get('total', 0), case.get('buckets', 0)))
    u.mem_write(0xABCD50, bytes([case.get('reduced', False)]))
    u.mem_write(0xABCD88, dwords(*case.get('timer', [0, 0, 0])))
    u.mem_write(0xABCD94, bytes([case.get('initialized', False)]))
    u.mem_write(0x829FF4, dwords(case.get('minimum', 15), case.get('buffer', 5)))
    history = []
    for step in case['steps']:
        current_ms = step['milliseconds'] & 0xFFFFFFFF
        for _ in range(step.get('logic_visits', 0)):
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x55AFB0, 0x55AFD9, count=30)
        # Interior frame: POP EDI, then ESI/EBP/EBX, ADD ESP18 and RET.
        u.mem_write(SP, bytes(40) + dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, 0)
        u.reg_write(UC_X86_REG_EBX, 0xFFFFFFFF)
        mark = len(reads)
        run_checked(u, 0x55E33B, RET_MAGIC, count=3000)
        assert u.reg_read(UC_X86_REG_ESP) == SP + 44
        if 'configure' in step:
            s.invoke(0x55AF40, step['configure'][0])
            s.invoke(0x55AF50, step['configure'][1])
        threshold = s.invoke(0x55AF60, 0)
        history.append(dict(input=step, clock_reads=reads[mark:], threshold=threshold,
                            count=s.read32(0xABCD40), fps=s.read32(0xABCD44),
                            total=s.read32(0xABCD48), buckets=s.read32(0xABCD4C),
                            timer=[s.read32(0xABCD88), s.read32(0xABCD90)],
                            initialized=u.mem_read(0xABCD94, 1)[0],
                            reduced=u.mem_read(0xABCD50, 1)[0],
                            minimum=s.read32(0x829FF4), buffer=s.read32(0x829FF8)))
    return dict(input=case, history=history)


def laser_fps_cases():
    return [dict(name='first_due_skipped', steps=[
                dict(milliseconds=0), dict(milliseconds=15, logic_visits=1),
                dict(milliseconds=959, logic_visits=58), dict(milliseconds=960, logic_visits=1),
                dict(milliseconds=8000, logic_visits=17), dict(milliseconds=8000)]),
            dict(name='millisecond_wrap', initialized=True, timer=[0x0FFFFFF0, 0, 60], fps=44,
                 steps=[dict(milliseconds=0xFFFFFFFF, logic_visits=5), dict(milliseconds=0, logic_visits=6),
                        dict(milliseconds=960, logic_visits=7)]),
            dict(name='counter_overflow', total=0x7FFFFFF0, buckets=8,
                 steps=[dict(milliseconds=0, logic_visits=32)]),
            dict(name='minimum_hysteresis', steps=[dict(milliseconds=i * 960, logic_visits=fps)
                 for i, fps in enumerate([15, 14, 15, 19, 20, 19, 15, 14])]),
            dict(name='unsigned_thresholds', minimum=0xFFFFFFF0, buffer=32,
                 steps=[dict(milliseconds=i * 960, logic_visits=fps)
                        for i, fps in enumerate([0, 15, 16, 32])]),
            dict(name='counter_wrap', count=0xFFFFFFFF,
                 steps=[dict(milliseconds=0, logic_visits=1)]),
            dict(name='configure_preserves_latch', steps=[
                dict(milliseconds=0, logic_visits=14),
                dict(milliseconds=0, configure=[10, 5]),
                dict(milliseconds=0, configure=[15, 5])])]


def laser_detail_selection():
    rows = []
    for values in ([15, 20, 5], [-1, -2, -3]):
        s = LaserScene(dict(name='detail_selection'))
        u = s.u
        u.mem_write(RULES, dwords(*values))
        u.mem_write(0xABCD40, dwords(3, 44, 80, 2))
        u.mem_write(0xABCD50, b'\1')
        before = bytes(u.mem_read(0xABCD40, 17))
        history = []
        for name, begin, end in (
                ('fill_in_data', 0x6850F5, 0x685110),
                ('radar_movie_started', 0x657974, RET_MAGIC),
                ('radar_movie_finished', 0x657C69, 0x657C76),
                ('radar_initialize', 0x655C33, 0x655C40)):
            u.mem_write(SP, dwords(0, RET_MAGIC))  # movie tail POP ESI then RET
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, begin, end, count=60)
            assert before == bytes(u.mem_read(0xABCD40, 17))
            history.append(dict(caller=name, minimum=s.read32(0x829FF4), buffer=s.read32(0x829FF8)))
        rows.append(dict(rules_normal_movie_buffer=values, history=history, fps_and_latch_unchanged=True))
    return rows


def generate():
    births = [laser_birth(case) for case in laser_birth_cases()]
    draw_cases = laser_draw_cases()
    for kind in ('main', 'support'):
        birth = next(row['laser'] for row in births if row['input']['name'] == 'production_' + kind)
        for row in draw_cases:
            if row['name'].startswith('stock_' + kind + '_crop_'):
                for field in ('source', 'target', 'z_adjust'):
                    assert row[field] == birth[field], (row['name'], field)
    return {'source': 'unicorn/gamemd.exe',
            'recruit': [recruit(case) for case in recruit_cases()],
            'bonus': [bonus(case) for case in bonus_cases()],
            'damage': [damage(case) for case in damage_cases()],
            'beam': [beam(case) for case in beam_cases()],
            'cadence': [cadence(case) for case in cadence_cases()],
            'reader': [reader(case) for case in reader_cases()],
            'laser_birth': births,
            'laser_lifetime': [laser_lifetime(case) for case in laser_lifetime_cases()],
            'laser_draw': [LaserPixels(case).draw() for case in draw_cases],
            'laser_reset': laser_reset(),
            'laser_type_inputs': [laser_type_inputs(name) for name in ('ATESLA', 'GAPOWR')],
            'laser_flh': [laser_flh(case) for case in laser_flh_cases()],
            'laser_fps': [laser_fps(case) for case in laser_fps_cases()],
            'laser_detail_selection': laser_detail_selection()}


def main(argv=None):
    selected = list(sys.argv[1:] if argv is None else argv)
    if '--disk' in selected:
        selected.remove('--disk')
        finish_vectors(
            generate_disk, Path(__file__).with_name('disk_laser.json'),
            provenance=disk_metadata,
            source_paths={name: Path(module.__file__) for name, module in sorted(sys.modules.items())
                          if (name == '__main__' or name.startswith('tools.'))
                          and getattr(module, '__file__', None)
                          and str(module.__file__).endswith('.py')},
            argv=selected)
        return
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='Prism Tower support: BuildingClass::Mission_Attack 0x44ACF0\'s PrismType arm '
                  '(recruitment and master arm), ProcessDelayedFire 0x4503F0 modes 1 (bonus) and 2 (support '
                  'beam 0x44ABD0 with the LaserDrawClass constructor 0x54FE60), DetonateAtCoord\'s '
                  'multiplier block 0x469A56..0x469A83, their per-frame cadence through MissionClass::AI '
                  '0x5B3060 between BuildingClass::Update\'s ready checks, and RulesClass::ReadGeneral\'s '
                  '[General] gate and Prism keys 0x671130..0x6711FE. Laser extension: IsLaser FireAt '
                  'arm6FF4CC and SpawnLaser6FD210, original building GetFLH/target getters, static vector '
                  'initialization, Logic550150 lifetime, DrawAll550240->Draw550260->house-color5509F0, '
                  'software additive4BDF00/4BE9D0 and packed4BFD30 pixels, scene-clear caller534949. '
                  'Original BuildingType constructor and selected stock Rules/ART readers establish '
                  'ATESLA/GAPRIS and GAPOWR coordinates. Logic-count writer55AFB0, wall-clock throttle '
                  'tail55E33B, detail hysteresis55AF60 and normal/radar-movie threshold assignment slices.',
            entry_points={'mission_attack': MISSION_ATTACK, 'process_delayed_fire': PROCESS_DELAYED_FIRE,
                          'support_beam': SUPPORT_BEAM, 'laser_draw_ctor': LASER_CTOR,
                          'damage_block': DAMAGE_BLOCK[0], 'mission_ai': bc.MISSION_AI,
                          'mission_guard': MISSION_GUARD, 'building_get_fire_error': GET_FIRE_ERROR,
                          'techno_get_fire_error': TECHNO_GET_FIRE_ERROR,
                          'techno_rearm_test': TECHNO_REARM_TEST, 'building_set_target': BUILDING_SET_TARGET,
                          'begin_mode': bc.BEGIN_MODE,
                          'update_ready_commence_unless_building': bc.READY_COMMENCE_UNLESS_BUILDING[0],
                          'update_ready_commence': bc.READY_COMMENCE[0],
                          'rules_ctor_prism_defaults': CTOR_DEFAULTS[0], 'read_general_gate': GENERAL_GATE[0],
                          'read_general_prism': PRISM_BLOCK[0], 'find_or_allocate': 0x4653C0,
                          'ini_crc': 0x4A1DE0, 'crt_float_init': 0x7C8F5E,
                          'fireat_laser_arm': LASER_FIRE_ARM, 'spawn_laser': 0x6FD210,
                          'laser_vector_initializer': LASER_VECTOR_INIT, 'laser_update': LASER_UPDATE,
                          'laser_draw_all': LASER_DRAW_ALL, 'laser_draw': LASER_DRAW,
                          'laser_special': LASER_SPECIAL, 'surface_additive': LASER_ADDITIVE,
                          'surface_add_rgb565': 0x4BE9D0, 'surface_packed': LASER_PACKED,
                          'laser_destroy_all': LASER_DESTROY_ALL, 'scene_clear_call': 0x534949,
                          'building_get_flh': GET_FLH, 'techno_get_flh': 0x6F3AD0,
                          'building_target_coord': 0x4500A0, 'building_get_coords': 0x447AC0,
                          'logic_count_writer': 0x55AFB0, 'throttle_fps_tail': 0x55E33B,
                          'frame_clock': 0x6C8C40, 'detail_hysteresis': 0x55AF60,
                          'detail_normal_startup': 0x6850F5, 'radar_movie_minimum': 0x657974},
            assumptions=['the building_construction fixture building (slave_manager fixture, BuildingClass '
                         'vtable 0x7E3EBC) is the master; further towers are 0x720-byte copies of it with the '
                         'row fields written; the owner house building vector HOUSE+0x68 is supplied in row '
                         'order',
                         'the tower type is the slave_manager type with the Prism fields written (Strength 600, '
                         'DelayedFireDelay 28, SpecialAnim GAPRIS_A / GAPRIS_AD, Weapon[0]/Weapon[1] supplied '
                         'WeaponTypes with Range 2048 unless the row says otherwise); Rules PrismType/'
                         'PrismSupportModifier/Max/Delay/Duration/Height (retail 150/8/45/15/420) and '
                         'ConditionYellow 0.5 written directly; MissionControl rates as building_guard_attack',
                         'FPCW 0x0E7F for the x87 distance sum, Sqrt_Approx 0x4CAC40 and ftol 0x7C5F00',
                         'recruit rows fill the 4 KiB below the entry stack pointer with 0xCCCCCCCC, so stale '
                         'stack words copied into +0x70C/+0x710 show that value; in play they hold whatever '
                         'earlier calls left',
                         'cadence rows run only BuildingClass::Update\'s mission pieces and ProcessDelayedFire '
                         'per building in the row\'s Logic order; the passive target scan, TechnoClass::'
                         'AI_Update, the end-of-frame range drop and anim updates are not run: row events write '
                         'TarCom, power and the RANGE answer',
                         'damage rows run the block alone with ESI (bullet) and [EBP+8] (coordinate) set up as '
                         'DetonateAtCoord has them',
                         'reader rows: the building_body_rules INI fixture after the CRT float initializer; '
                         'each pass supplies a CCINIClass holding only [General] (or no section) with the pass '
                         'keys, indexed by the native CRC; BuildingTypes = GAPOWR, ATESLA',
                         'laser_birth and laser_flh execute original Building GetFLH; selected stock '
                         'type fields are independently executed by laser_type_inputs through whole '
                         'BuildingType constructor and focused original readers. Physical strings come '
                         'from existing lexical extractor; whole layered Scenario/physical file loading '
                         'and unrelated type readers are outside this native prerequisite boundary.',
                         'laser_birth executes only FireAt6FF4CC..6FF656 after the earlier FireAt '
                         'admission/weapon selection and projectile work. Source and target object '
                         'lifecycle/locations, selected/current weapon, HouseRGB and supplied timer frame are '
                         'boundary inputs. Production-named rows establish selected coordinates, not '
                         'whole object or scene parity. Building secondary controls execute the '
                         'original GetCurrentWeapon70E1A0/Building GetWeapon re-read while keeping '
                         'the IsLaser gate and FLH slot from the selected weapon. Ordinary support '
                         'uses actual4503F0/44ABD0.',
                         'laser_draw composes existing rally original projection/surface/A-buffer '
                         'fixture. Prepared RGB565 destination, Z/A planes, detail/FPS globals, '
                         'viewport/camera and original-constructor arguments are explicit input bounds. '
                         'Pixels, original fade arithmetic and surface calls execute; this is no full '
                         'scene/render-order parity claim. Direct3D and non-house-color generic drawing '
                         'are excluded; non-house birth and IsBigLaser are caller controls only.',
                         'DrawAll uses reverse vector order; Logic550150 runs before active-object '
                         'visits by original55B5C3/55B5FF caller reading. Lifetime corpus executes '
                         'same/repeated/skipped frame histories, not whole Logic. Scene-clear corpus '
                         'executes actual534949->550000; load67E739->6851F0->685297->534450 is static '
                         'caller evidence, not a whole save/load transport replay.',
                         'laser_fps executes Logic entry counter increments and original Throttle tail '
                         'with row-supplied calls/times, not Main_Tick scheduling. Counter globals and '
                         'latch start at loader-zero BSS unless case overrides. Min/buffer values are '
                         'supplied controls; native AudioVisual reader/default evidence is separately '
                         'owned by rules_oracle/weapon_laser. Radar-movie rows execute only threshold '
                         'assignment slices, not playback. Millisecond wrap is deliberately retained.',
                         'New laser rows assert unchanged original complete.text SHA256 and unchanged '
                         'RNG bytes/entries at applicable boundaries; no simulation RNG is substituted.'],
            substitutions=['SelectWeapon 0x6F3330 answered 0; GetFLH 0x453840 answered with per-building '
                           'sentinel coordinates in legacy recruit/bonus/beam/cadence only; new laser '
                           'Building rows execute GetFLH. PlayAnim 0x451890 observed and answered; DestroyNthAnim '
                           '0x451E40 answered by the refinery_dock observer (no-op on empty anim slots) and '
                           'observed at 0x44B532/0x44B5CD; Is_Operational 0x4555D0 answered from the row',
                           'recruit rows: BuildingClass::GetFireError 0x447F10 answered 0',
                           'bonus rows: BuildingClass::GetFireError 0x447F10 answered with the row code; Fire_At '
                           '0x6FDD50 answered with a bullet buffer (+0x150 = 0x100, as BulletClass::Construct '
                           '0x466546) or 0',
                           'beam and cadence rows: operator new 0x7C8E17 answered from a heap cursor (0 in the '
                           'allocation-failure row); the laser vector 0xABC878 supplied empty',
                           'cadence rows: TechnoClass::GetFireError 0x6FC0B0 redirected with a target from '
                           '0x6FC0BF to its native rearm test 0x6FC94F and from 0x6FC981 to its native RANGE '
                           '(0x6FCD0E, row-controlled) or OK (0x6FCD1D) tail, skipping the checks between; '
                           'Fire_At 0x6FDD50 answered with its rearm writes (+0x2F8 = ROF, +0x2EC = Frame, '
                           '+0x2F4 = ROF) and a bullet buffer; TechnoClass::SetTarget 0x6FCDB0 answered with its '
                           '+0x2B4 write; FacingClass::Set_Desired 0x4C9220, the direction 0x43ED40, '
                           'StartUncloaking 0x7036C0 and ClearBibArea 0x449540 answered; IsCloseEnough 0x6F7780 '
                           'true, IsHumanPlayer 0x50B730 and the EMP test 0x70EFD0 false',
                           'new laser rows: operator new/delete and atexit are observed transport '
                           'boundaries; original ctor/vector/lifetime/removal execute. Unit caller '
                           'controls use original Unit vtable but explicitly supply Techno GetFLH '
                           'output, excluding held locomotion/pose. BSurface backing/lock/stride/unlock '
                           'remain original; only draw vtable slots select original DSurface methods.',
                           'laser_fps timeGetTime import returns supplied u32 milliseconds; original '
                           '6C8C40 SHR4 executes; shared fast_scroll return transport is reused.']),
        source_paths={name: Path(module.__file__) for name, module in sorted(sys.modules.items())
                      if (name == '__main__' or name.startswith('tools.'))
                      and getattr(module, '__file__', None)
                      and str(module.__file__).endswith('.py')},
        argv=argv)


if __name__ == '__main__':
    main()
