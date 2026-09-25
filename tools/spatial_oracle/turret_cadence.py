"""Original UnitClass::Facing_Update 0x00736990, frame after frame, for a turreted unit.

Each row is one retained native Unit whose Facing_Update runs once per frame, with the
global frame counter Frame [0x00A8ED84] advanced by one between calls. Every instruction
Facing_Update reaches runs natively on the fixture, including the calls it makes:
DirectionToTarget 0x005F3DB0 (both objects' vt+0x48 GetCoords 0x005F65A0, the
table-driven atan2 0x004CAE30 and ftol 0x007C5F00), the weapon getter vt+0x3F4 0x0070E1A0
(GetTechnoType vt+0x84 0x006F3270 -> vt+0x88 0x00741490, 0x00717880, GetWeapon vt+0x3F8
0x0070E140, 0x00750010, 0x007177C0), the WeaponStruct test 0x0070E240 and FacingClass
Set 0x004C9220, Current 0x004C93D0 and Is_Rotating 0x004C9480. Nothing is substituted.
The unit, its target and its NavCom are UnitClass objects with the original vtable
0x007F5C70; the unit's two FacingClass objects are built with the original constructor
0x004C91C0, Set_ROT 0x004C9680 (type ROT) and Snap 0x004C9300 (initial heading).

Native control flow (instruction read, then exercised by the rows below):
  Arm A 0x007369A5: Target +0x2B4 set and latch +0x6AF clear -> dir = DirectionToTarget.
    Turret (+0xCA1): weapon = vt+0x3F4; no weapon (0x0070E240) or OmniFire (+0x12B) -> no
    Set; TurretLocked (WeaponStruct+0x18) -> Secondary.Set(Primary.Current()) (0x00736A14);
    else Secondary(+0x3A0).Set(dir) (0x00736A89).
  Arm B 0x00736A8E: TurretSpins (+0xD21) -> Set(Current + 0x0800-rounded step) and skip the
    rest; else latch = 0 (0x00736AD5), and on a turret: Is_Rotating(+0x3A0) -> latch =
    Is_Rotating (0x00736B16); else, with no Target, idle = Frame - LastFire(+0x120) >=
    Rules[0x008871E0]+0xE04 + 5, cleared by Bunker +0x2E4, skipped by IsSimpleDeployer
    (+0xE13) with +0x6E0; idle -> NavCom +0x5A4 and not TurretLocked: skip if +0x6AD, else
    Set(DirectionToTarget(NavCom)) (0x00736BC3); otherwise Set(Primary.Current()).
  Tail 0x00736BE9: turret -> +0x4A0 = Is_Rotating(+0x3A0).

Case schema (`input`): start frame; frames (count); delay (Rules+0xE04); unit_coord [x,y,z]
leptons; turret_rot / hull_rot (type ROT fed to Set_ROT); turret_init / hull_init (16-bit
DirStruct Snap values); weapon {present, omni_fire, turret_locked}; turret_spins (+0xD21);
simple_deployer (+0xE13) / deployed (+0x6E0); bunkered (+0x2E4 non-null); magnetron
(+0x6AD); latch_init (+0x6AF); last_fire (+0x120, absolute frame); target_coord / navcom_coord
(null = no object); events: [{at: frame index, kind, value}] applied before that frame's call,
kind = target_coord [x,y,z] | clear_target | last_fire (absolute frame) | hull_set (16-bit,
native Set 0x004C9220 on +0x388 at that frame) | navcom_coord [x,y,z] | clear_navcom.

Per-frame schema (`frames[i]`, recorded after the call, Frame unchanged):
  frame      absolute Frame value for this call
  target_dir DirectionToTarget(Target) low 16 bits read natively before the call (null: none)
  sets       [[facing, dir16, arm]] each Set 0x004C9220 Facing_Update made; facing "turret" |
             "hull", arm "A" (return 0x00736A8E) | "B" (return 0x00736BE2)
  turret, hull  [current, desired, previous, start, duration, rate] of +0x3A0 / +0x388:
             current = Current 0x004C93D0 (u16); desired/previous = low 16 of +0x0/+0x4;
             start = +0x8 (i32 frame); duration = +0x10 (i32); rate = +0x14 (i16)
  latch      +0x6AF (0/1)      rotating  +0x4A0 (u32)
  turret_raw 24 bytes of +0x3A0 as hex, the unused +0xC word zeroed (Set/Snap store stack
             garbage there); the +0x2/+0x6 high words are native (DirectionToTarget leaves the
             target X's high word in its output DirStruct).
Summary per row: `first_idle_set` = frame of the first arm-B Set (null if none).

Rust consumers: src/sim/movement/turret.rs (`facing_update`) and
src/sim/world/unit_post.rs (`apply_unit_facing`).
"""
from __future__ import annotations

import math
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, OracleError, finish_vectors, load_image, provenance,
                                 run_checked)

FACING_UPDATE, DIRECTION_TO = 0x736990, 0x5F3DB0
CTOR, SET_ROT, SET, SNAP, CURRENT, IS_ROTATING = (0x4C91C0, 0x4C9680, 0x4C9220, 0x4C9300,
                                                  0x4C93D0, 0x4C9480)
UNIT_VTABLE, FRAME, RULES_PTR, FTOL_CW = 0x7F5C70, 0xA8ED84, 0x8871E0, 0x822D80
ARM_RETURNS = {0x736A8E: "A", 0x736BE2: "B"}
UNIT, TYPE, TARGET, NAVCOM, RULES, WEAPON, OUT, BUNKER = (SCRATCH + n * 0x1000 for n in range(8))
PRIMARY, SECONDARY = UNIT + 0x388, UNIT + 0x3A0
SP = STACK_BASE + STACK_SIZE - 0x1000
# The only bytes Facing_Update may write outside the stack.
WRITABLE = [(PRIMARY, PRIMARY + 0x18), (SECONDARY, SECONDARY + 0x18), (UNIT + 0x4A0, UNIT + 0x4A4),
            (UNIT + 0x6AF, UNIT + 0x6B0)]


def dwords(*values):
    return struct.pack("<" + "I" * len(values), *(v & 0xFFFFFFFF for v in values))


class Machine:
    def __init__(self, case):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, SCRATCH_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        if struct.unpack("<H", u.mem_read(FTOL_CW, 2))[0] != NATIVE_FPCW:
            raise OracleError("ftol's control word 0x00822D80 is not 0x0E7F in the image")
        self.sets = None
        self.guard = False
        u.hook_add(UC_HOOK_CODE, self.on_set, begin=SET, end=SET)
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)

        u.mem_write(RULES_PTR, dwords(RULES))
        u.mem_write(RULES + 0xE04, dwords(case["delay"]))
        for obj in (UNIT, TARGET, NAVCOM):
            u.mem_write(obj, dwords(UNIT_VTABLE))
            u.mem_write(obj + 0x6C4, dwords(TYPE))
        u.mem_write(UNIT + 0x9C, struct.pack("<iii", *case["unit_coord"]))
        u.mem_write(UNIT + 0x150, struct.pack("<f", 0.0))            # Veterancy: rookie slots
        u.mem_write(UNIT + 0x120, dwords(case["last_fire"]))
        u.mem_write(UNIT + 0x2E4, dwords(BUNKER if case["bunkered"] else 0))
        u.mem_write(UNIT + 0x6AD, bytes([case["magnetron"]]))
        u.mem_write(UNIT + 0x6AF, bytes([case["latch_init"]]))
        u.mem_write(UNIT + 0x6E0, bytes([case["deployed"]]))
        u.mem_write(TYPE + 0xCA1, b"\x01")                             # Turret=yes
        u.mem_write(TYPE + 0xD21, bytes([case["turret_spins"]]))
        u.mem_write(TYPE + 0xE13, bytes([case["simple_deployer"]]))
        u.mem_write(TYPE + 0x808, dwords(0))                           # no multi-weapon index
        weapon = case["weapon"]
        u.mem_write(TYPE + 0x898, dwords(WEAPON if weapon["present"] else 0))
        u.mem_write(TYPE + 0x898 + 0x18, bytes([weapon["turret_locked"]]))
        u.mem_write(WEAPON + 0x12B, bytes([weapon["omni_fire"]]))
        self.frame(case["start"] - 1)
        for facing, rot, init in ((PRIMARY, case["hull_rot"], case["hull_init"]),
                                  (SECONDARY, case["turret_rot"], case["turret_init"])):
            self.call(CTOR, facing)
            self.call(SET_ROT, facing, rot)
            u.mem_write(OUT, dwords(init))
            self.call(SNAP, facing, OUT)
        self.set_target(case["target_coord"], TARGET, 0x2B4)
        self.set_target(case["navcom_coord"], NAVCOM, 0x5A4)

    def frame(self, value):
        self.u.mem_write(FRAME, dwords(value))

    def set_target(self, coord, obj, field):
        if coord is None:
            self.u.mem_write(UNIT + field, dwords(0))
        else:
            self.u.mem_write(obj + 0x9C, struct.pack("<iii", *coord))
            self.u.mem_write(UNIT + field, dwords(obj))

    def call(self, entry, this, *args):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, this)
        run_checked(u, entry, RET_MAGIC, count=200_000)
        if u.reg_read(UC_X86_REG_ESP) != SP + 4 + 4 * len(args):
            raise OracleError(f"0x{entry:08X}: unbalanced stack")
        return u.reg_read(UC_X86_REG_EAX)

    def on_set(self, u, _address, _size, _data):
        if self.sets is None:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        ret, arg = struct.unpack("<II", u.mem_read(sp, 8))
        if ret not in ARM_RETURNS:
            raise OracleError(f"FacingClass::Set from unexpected return 0x{ret:08X}")
        facing = {SECONDARY: "turret", PRIMARY: "hull"}[u.reg_read(UC_X86_REG_ECX)]
        value = struct.unpack("<H", u.mem_read(arg, 2))[0]
        self.sets.append([facing, value, ARM_RETURNS[ret]])

    def on_write(self, _u, _access, address, size, _value, _data):
        if not self.guard or STACK_BASE <= address < STACK_BASE + STACK_SIZE:
            return
        if not any(low <= address and address + size <= high for low, high in WRITABLE):
            raise OracleError(f"Facing_Update wrote outside the facing state: 0x{address:08X}")

    def facing(self, obj):
        self.call(CURRENT, obj, OUT)
        current = struct.unpack("<H", self.u.mem_read(OUT, 2))[0]
        desired, previous, start, _unused, duration, rate = struct.unpack(
            "<HxxHxxiiih", self.u.mem_read(obj, 22))
        return [current, desired, previous, start, duration, rate]

    def update(self):
        self.sets = []
        self.guard = True
        self.call(FACING_UPDATE, UNIT)
        self.guard = False
        sets, self.sets = self.sets, None
        return sets

    def direction(self):
        if struct.unpack("<I", self.u.mem_read(UNIT + 0x2B4, 4))[0] == 0:
            return None
        self.call(DIRECTION_TO, UNIT, OUT, TARGET)
        return struct.unpack("<H", self.u.mem_read(OUT, 2))[0]

    def byte(self, address):
        return self.u.mem_read(address, 1)[0]


def execute(case):
    m = Machine(case)
    events = {}
    for event in case["events"]:
        events.setdefault(event["at"], []).append(event)
    frames = []
    for index in range(case["frames"]):
        frame = case["start"] + index
        m.frame(frame)
        for event in events.get(index, []):
            kind, value = event["kind"], event.get("value")
            if kind == "target_coord":
                m.set_target(value, TARGET, 0x2B4)
            elif kind == "clear_target":
                m.set_target(None, TARGET, 0x2B4)
            elif kind == "navcom_coord":
                m.set_target(value, NAVCOM, 0x5A4)
            elif kind == "clear_navcom":
                m.set_target(None, NAVCOM, 0x5A4)
            elif kind == "last_fire":
                m.u.mem_write(UNIT + 0x120, dwords(value))
            elif kind == "hull_set":
                m.u.mem_write(OUT, dwords(value))
                m.call(SET, PRIMARY, OUT)
            else:
                raise ValueError(kind)
        target_dir = m.direction()
        sets = m.update()
        raw = bytearray(m.u.mem_read(SECONDARY, 24))
        raw[0xC:0x10] = b"\0\0\0\0"
        frames.append(dict(frame=frame, target_dir=target_dir, sets=sets,
                           turret=m.facing(SECONDARY), hull=m.facing(PRIMARY),
                           latch=m.byte(UNIT + 0x6AF),
                           rotating=struct.unpack("<I", m.u.mem_read(UNIT + 0x4A0, 4))[0],
                           turret_raw=bytes(raw).hex()))
    first_idle = next((f["frame"] for f in frames for s in f["sets"] if s[2] == "B"), None)
    return dict(input=case, frames=frames, first_idle_set=first_idle)


# ---- cases -------------------------------------------------------------------------------------
START = 1000
CENTER = [10 * 256 + 128, 10 * 256 + 128, 0]
RADIUS = 2560


def at_bearing(dir16, radius=RADIUS, origin=CENTER):
    """Coordinate at a 16-bit bearing (0 = north/-Y, 0x4000 = east/+X), leptons."""
    angle = dir16 / 65536 * 2 * math.pi
    return [origin[0] + round(radius * math.sin(angle)), origin[1] - round(radius * math.cos(angle)),
            origin[2]]


def deg(value):
    return round(value / 360 * 65536) & 0xFFFF


BASE = dict(start=START, frames=40, delay=36, unit_coord=CENTER, turret_rot=5, hull_rot=5,
            turret_init=0, hull_init=0, weapon=dict(present=1, omni_fire=0, turret_locked=0),
            turret_spins=0, simple_deployer=0, deployed=0, bunkered=0, magnetron=0, latch_init=0,
            last_fire=START, target_coord=None, navcom_coord=None, events=[])


def case(name, **changes):
    return dict(BASE, name=name, **changes)


def cases():
    # 1. Stationary target at bearings relative to the initial turret heading.
    for init in (0x0000, 0x6000):
        for rel in (0, 45, 90, 179, 181, 270, 13, 200.5):
            yield case(f"aim_init{init:04x}_rel{rel}", turret_init=init, hull_init=init,
                       target_coord=at_bearing((init + deg(rel)) & 0xFFFF))
    yield case("aim_latch_preset", latch_init=1, target_coord=at_bearing(deg(90)))
    yield case("aim_hull_differs", hull_init=deg(135), target_coord=at_bearing(deg(90)))
    yield case("aim_rot10", turret_rot=10, target_coord=at_bearing(deg(179)))
    yield case("aim_rot200_clamped", turret_rot=200, target_coord=at_bearing(deg(181)))
    yield case("aim_omni_fire", weapon=dict(present=1, omni_fire=1, turret_locked=0),
               target_coord=at_bearing(deg(90)))
    yield case("aim_turret_locked", hull_init=deg(135),
               weapon=dict(present=1, omni_fire=0, turret_locked=1),
               target_coord=at_bearing(deg(90)))
    yield case("aim_no_weapon", weapon=dict(present=0, omni_fire=0, turret_locked=0),
               target_coord=at_bearing(deg(90)))
    yield case("aim_turret_spins", turret_spins=1, target_coord=at_bearing(deg(90)), frames=20)
    # A target that appears after the turret has settled; and a mid-arc retarget.
    yield case("aim_target_appears", events=[dict(at=5, kind="target_coord",
                                                  value=at_bearing(deg(120)))])
    yield case("aim_retarget_mid_arc", target_coord=at_bearing(deg(150)),
               events=[dict(at=6, kind="target_coord", value=at_bearing(deg(300)))])

    # 2. Moving targets: the target coordinate changes before every call.
    def moving(name, path, frames=60, **changes):
        return case(name, frames=frames, target_coord=path(0),
                    events=[dict(at=i, kind="target_coord", value=path(i))
                            for i in range(1, frames)], **changes)
    yield moving("move_orbit_cw_slow", lambda i: at_bearing((deg(60) + 0x180 * i) & 0xFFFF))
    yield moving("move_orbit_ccw_slow", lambda i: at_bearing((deg(60) - 0x180 * i) & 0xFFFF))
    yield moving("move_orbit_cw_fast", lambda i: at_bearing((deg(60) + 0x700 * i) & 0xFFFF))
    yield moving("move_linear_pass", lambda i: [CENTER[0] - 3000 + 110 * i, CENTER[1] - 900, 0])
    yield moving("move_orbit_cw_rot10", lambda i: at_bearing((deg(60) + 0x300 * i) & 0xFFFF),
                 turret_rot=10)

    # 3. Target cleared at F (index 30) after the turret settled on it; hull faces elsewhere.
    F = 30
    target, hull = at_bearing(deg(100)), deg(250)

    def idle(name, offset, delay=36, frames=80, events=(), **changes):
        return case(name, frames=frames, delay=delay, hull_init=hull, target_coord=target,
                    events=[dict(at=F, kind="clear_target"),
                            dict(at=F, kind="last_fire", value=START + F - offset), *events],
                    **changes)
    for offset in (0, 10, 40, 41, 50):
        yield idle(f"idle_d36_lf{offset}", offset)
        yield idle(f"idle_d36_lf{offset}_navcom", offset, navcom_coord=at_bearing(deg(330)))
    for offset in (0, 4, 5, 6):
        yield idle(f"idle_d0_lf{offset}", offset, delay=0)
    for offset in (100, 104, 105, 106):
        yield idle(f"idle_d100_lf{offset}", offset, delay=100)
    yield idle("idle_bunkered", 50, bunkered=1)
    yield idle("idle_bunkered_navcom", 50, bunkered=1, navcom_coord=at_bearing(deg(330)))
    yield idle("idle_navcom_magnetron", 50, magnetron=1, navcom_coord=at_bearing(deg(330)))
    yield idle("idle_navcom_turret_locked", 50, navcom_coord=at_bearing(deg(330)),
               weapon=dict(present=1, omni_fire=0, turret_locked=1))
    yield idle("idle_simple_deployer_deployed", 50, simple_deployer=1, deployed=1)
    yield idle("idle_simple_deployer_undeployed", 50, simple_deployer=1, deployed=0)
    yield idle("idle_no_weapon_navcom", 50, navcom_coord=at_bearing(deg(330)),
               weapon=dict(present=0, omni_fire=0, turret_locked=0))
    # The target clears while the turret is still turning toward it.
    yield case("idle_clear_mid_arc", frames=60, hull_init=hull, target_coord=at_bearing(deg(180)),
               events=[dict(at=5, kind="clear_target"),
                       dict(at=5, kind="last_fire", value=START - 100)])
    # A target reappearing one and two frames after the idle-return Set (latch 0, then 1).
    for delta in (1, 2):
        yield idle(f"idle_target_returns_f{delta}", 50,
                   events=[dict(at=F + delta, kind="target_coord", value=at_bearing(deg(20)))])
    yield idle("idle_navcom_moves", 50, navcom_coord=at_bearing(deg(330)),
               events=[dict(at=F + 6, kind="navcom_coord", value=at_bearing(deg(30)))])

    # 4. The hull turns during the idle return.
    for name, at, value, hull_rot in (("idle_hull_turns_away", F + 2, deg(160), 5),
                                      ("idle_hull_turns_toward", F + 2, deg(110), 5),
                                      ("idle_hull_turns_slow", F + 2, deg(10), 2),
                                      ("idle_hull_turns_fast", F + 2, deg(10), 10),
                                      ("idle_hull_turns_before", F - 8, deg(20), 5)):
        yield idle(name, 50, hull_rot=hull_rot, events=[dict(at=at, kind="hull_set", value=value)])
    yield idle("idle_hull_turns_navcom", 50, navcom_coord=at_bearing(deg(330)),
               events=[dict(at=F + 2, kind="hull_set", value=deg(10))])


def generate():
    return [execute(c) for c in cases()]


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("UnitClass::Facing_Update 0x00736990 per frame on a Turret=yes Unit: stationary "
               "targets at bearings around both initial headings, preset latch, ROT 5/10/200, "
               "OmniFire, TurretLocked, no weapon, TurretSpins, appearing/retargeted/moving "
               "targets, target clears against LastFire around the Rules+0xE04 + 5 threshold "
               "(delay 0/36/100), NavCom, Bunker, +0x6AD, IsSimpleDeployer, and hull turns "
               "during the idle return. Non-turret arm (type +0x67C / locomotor) not covered."),
        assumptions=[
            "x87 control word 0x0E7F; the image's 0x00822D80 already holds 0x0E7F for ftol.",
            "Unit, target and NavCom are UnitClass objects on the original vtable 0x007F5C70 "
            "with Location +0x9C; type +0x6C4 with Turret +0xCA1 = 1, +0x808 = 0 and weapon "
            "slot 0 at +0x898; Veterancy +0x150 = 0.0.",
            "FacingClass objects built by 0x004C91C0 + Set_ROT 0x004C9680 + Snap 0x004C9300 at "
            "start-1; Rules [0x008871E0]+0xE04 supplied per row.",
            "Frame [0x00A8ED84] advanced by one before each call; events apply before the call.",
            "Facing_Update writes only +0x388/+0x3A0 facings, +0x4A0 and +0x6AF (write-guarded).",
        ],
        substitutions=[],
        entry_points={"facing_update": FACING_UPDATE, "direction_to_target": DIRECTION_TO,
                      "set": SET, "current": CURRENT, "is_rotating": IS_ROTATING,
                      "snap": SNAP, "set_rot": SET_ROT, "constructor": CTOR},
    ))
