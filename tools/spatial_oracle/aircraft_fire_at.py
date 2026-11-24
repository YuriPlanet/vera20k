"""Original AircraftClass::Fire_At (0x00415EE0), one call per row.

Each row runs the ORIGINAL function (vt+0x3CC, thiscall, Fire_At(target, weapon index), RET 8)
on the scratch Aircraft of aircraft_guard.py: a clone of the Aircraft vtable 0x007E22A4 with
only the slots below replaced by scratch INT3 stubs, entry hooks at native callees, the same
per-row assertions (native cleanup, EBX/EBP/ESI/EDI preserved, no read of unsupplied scratch
bytes) and, here, no write outside the stack but the bullet's velocity +0xE8..+0xFF. The log
uses the vocabulary of the FireAtHost methods of src/sim/aircraft/fire_at.rs.

Stubs: GetWeapon vt+0x3F8 (0x0070E140; asserts slot 0, answers a WeaponStruct whose type has
the row's Speed +0xA8) and UnInit vt+0xF8 (0x004DE5D0). Entry hooks: TechnoClass::FireAt
0x006FDD50 (asserts the target and weapon index Fire_At was given; answers the scratch bullet
or NULL), Drop_Payload 0x00415C60, FacingClass::Current 0x004C93D0 on SecondaryFacing +0x3A0
(answers the row's facing), the owner-house test 0x0050B6F0 (answers the row's `player`),
MapClass::IsShrouded 0x00586360 (records each coordinate; answers true at the row's
`shrouded_at` call) and MapClass::RevealArea 0x005678E0 (records the coordinate, radius and
final argument; asserts the owner house and the arguments 0, 0, 0, 1 before the final one).

Native: the trig 0x004CACB0/0x004CAD00, atan2 0x004CAE30, Sqrt_Approx 0x004CAC40 and ftol
0x007C5F00 under the game's control word 0x0E7F; the Vector3D helpers; GetCoords 0x005F65A0
(vt+0x48) of the aircraft and of the target, a Unit on its real vtable 0x007F5C70; and the
locomotor's Apparent_Speed: the owner's Locomotor +0x674 is a FlyLocomotionClass interface on
its real vtable 0x007E89F4, whose slot +0x84 (0x004CFE20) multiplies the type's Speed +0x678
(through vt+0x84 -> vt+0x88 0x0041C200) by the locomotor's CurrentSpeed +0x44. Its result is
logged after the call returns (0x00415F5C).
"""
from itertools import product
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.aircraft_guard import (HOUSE, LOCO, MAP, OBJECTS, OWNER, SCRATCH,
                                                 STACK_BASE, STACK_SIZE, TARGET, TYPE, UNIT_VT,
                                                 WEAPON_STRUCT, WEAPON_TYPE, ScratchAircraft,
                                                 s32, u32)

FIRE_AT, FIRE_AT_BODY = 0x415EE0, (0x415EE0, 0x4165BD)
FLY_LOCOMOTION_VT = 0x7E89F4
RULES_POINTER = 0x8871E0
# Scratch objects beyond aircraft_guard.py's layout.
BULLET, BULLET_TYPE, RULES = OBJECTS, OBJECTS + 0x1000, OBJECTS + 0x2000
VELOCITY = BULLET + 0xE8

# Cloned vtable slots: offset -> (stub name, native target asserted against the image, pops).
SLOTS = {0x3F8: ("get_weapon", 0x70E140, 4), 0xF8: ("uninit", 0x4DE5D0, 0)}
# Kept native and asserted: Fire_At itself, GetCoords, and the type read Apparent_Speed makes.
NATIVE_SLOTS = {0x3CC: FIRE_AT, 0x48: 0x5F65A0, 0x84: 0x6F3270, 0x88: 0x41C200}
HOOKS = {
    0x6FDD50: ("techno_fire_at", 8), 0x415C60: ("drop_payload", 0),
    0x4C93D0: ("facing", 4), 0x50B6F0: ("owner_is_player", 0),
    0x586360: ("is_shrouded", 4), 0x5678E0: ("reveal_area", 0x20),
}
# Fire_At's instruction after the locomotor's Apparent_Speed returns.
OBSERVERS = {0x415F5C: "apparent_speed"}


def bits(value):
    return f"{struct.unpack('<Q', struct.pack('<d', value))[0]:016x}"


def double(text):
    return struct.unpack("<d", struct.pack("<Q", int(text, 16)))[0]


class FireAt(ScratchAircraft):
    def __init__(self):
        super().__init__(SLOTS, NATIVE_SLOTS, {}, HOOKS, OBSERVERS)
        if self.word(FLY_LOCOMOTION_VT + 0x84) != 0x4CFE20:
            raise ValueError("FlyLocomotionClass slot +0x84 is not Apparent_Speed 0x004CFE20")
        self.names = {OWNER: "aircraft", TARGET: "target", BULLET: "bullet", HOUSE: "house"}

    def build(self, row):
        self.reset(row, FIRE_AT_BODY)
        self.shroud_calls = 0
        self.put32(OWNER + 0x118, 0x1 if row["passenger"] else 0)
        self.put8(OWNER + 0x6CA, row["destroy"])
        self.put32(OWNER + 0x21C, HOUSE)
        self.put32(OWNER + 0x674, LOCO)
        self.put32(LOCO, FLY_LOCOMOTION_VT)
        self.put32(LOCO + 8, OWNER)
        self.put(LOCO + 0x44, struct.pack("<d", row["current_speed"]))
        self.put32(TYPE + 0x678, row["type_speed"])
        self.object(TARGET, UNIT_VT)
        self.put32(TARGET + 0x9C, *row["target_location"])
        self.put32(BULLET + 0xAC, BULLET_TYPE)
        self.put32(BULLET_TYPE + 0x2DC, row["rot"])
        self.put(VELOCITY, b"".join(struct.pack("<d", double(v)) for v in row["velocity"]))
        self.put32(WEAPON_STRUCT, WEAPON_TYPE)
        self.put32(WEAPON_TYPE + 0xA8, row["weapon0_speed"])
        self.put32(RULES + 0x18, row["sight_range"])
        self.u.mem_write(RULES_POINTER, u32(RULES))

    # Only the stack and the bullet's velocity may change.
    def on_write(self, u, _access, address, size, _value, _data):
        if not self.auditing:
            return
        if STACK_BASE <= address and address + size <= STACK_BASE + STACK_SIZE:
            return
        if VELOCITY <= address and address + size <= VELOCITY + 0x18:
            return
        self.fail(f"write outside the stack and the bullet's velocity at 0x{address:08X}+{size}"
                  f" from 0x{u.reg_read(UC_X86_REG_EIP):08X}")

    def stub(self, name):
        if not self.this(OWNER, name):
            return 0
        if name == "get_weapon":
            if self.arg(0) != 0:
                self.fail(f"GetWeapon({self.arg(0)})")
            self.calls.append(["weapon0_speed"])
            return WEAPON_STRUCT
        self.calls.append(["uninit"])
        return 0

    def hook(self, name):
        row = self.row
        if name == "techno_fire_at":
            if not self.this(OWNER, name):
                return 0
            if (self.arg(0), self.arg(1)) != (TARGET, row["weapon"]):
                self.fail(f"TechnoClass::FireAt({self.arg(0):08X}, {self.arg(1)})")
            self.calls.append(["techno_fire_at"])
            return BULLET if row["bullet"] else 0
        if name == "drop_payload":
            self.calls.append(["drop_payload"])
            return self.this(OWNER, name) and 0
        if name == "facing":
            if not self.this(OWNER + 0x3A0, name):
                return 0
            out = self.arg(0)
            self.u.mem_write(out, struct.pack("<H", row["facing"]))
            self.calls.append(["facing"])
            return out
        if name == "owner_is_player":
            self.calls.append(["owner_is_player"])
            return self.this(HOUSE, name) and row["player"]
        if not self.this(MAP, name):
            return 0
        if name == "is_shrouded":
            self.calls.append(["is_shrouded", self.coord(self.arg(0))])
            shrouded = self.shroud_calls == row["shrouded_at"]
            self.shroud_calls += 1
            return shrouded
        # reveal_area
        rest = [s32(self.arg(n)) for n in range(3, 7)]
        if self.arg(2) != HOUSE or rest != [0, 0, 0, 1]:
            self.fail(f"RevealArea house 0x{self.arg(2):08X}, arguments {rest}")
        self.calls.append(["reveal_area", self.coord(self.arg(0)), s32(self.arg(1)),
                           s32(self.arg(7))])
        return 0

    def observe(self, name):
        self.calls.append([name, s32(self.u.reg_read(UC_X86_REG_EAX))])

    def fire_at(self, row):
        row = {**DEFAULTS, **row}
        row["velocity"] = [v if isinstance(v, str) else bits(v) for v in row["velocity"]]
        self.build(row)
        result = self.execute(FIRE_AT, args=(TARGET, row["weapon"]))
        velocity = [bits(v) for v in struct.unpack("<3d", self.u.mem_read(VELOCITY, 0x18))]
        return {"input": row, "ret": self.name(result["ret"] & 0xFFFFFFFF),
                "calls": result["calls"], "velocity": velocity}


DEFAULTS = {
    "passenger": False, "destroy": False, "bullet": True, "rot": 100, "weapon": 0,
    "velocity": [1.0, 0.0, 0.0], "type_speed": 40, "current_speed": 1.0, "facing": 0x4000,
    "location": [15488, 16512, 1100], "target_location": [17000, 16000, 0],
    "weapon0_speed": 30, "player": False, "shrouded_at": None, "sight_range": 2,
    "state": 0, "ready": 0,
}

# Launch-like velocities: FireAt's ROT>0 unit vectors, ballistic vectors, the zero and
# vertical vectors the Vector3D guards seed, and inexact doubles.
LEVEL_VELOCITIES = [
    [40.0, 0.0, 0.0], [12.5, -7.25, -3.0], [-17.0, 3.5, 9.75], [0.0, 0.0, 0.0],
    [0.0, 0.0, -20.0], [0.1, 1 / 3, -2 / 3], [-25.0, -25.0, 0.0], [3.0, 4.0, 12.0],
]
HOMING_VELOCITIES = [
    [1.0, 0.0, 0.0], [0.6, 0.8, 0.0], [2 ** -0.5, -(2 ** -0.5), 0.0], [0.0, 0.0, -1.0],
    [0.36, 0.48, -0.8], [0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [-0.1, 0.3, 0.9486832980505138],
]
# Target offsets from the Location: below, ahead, behind, diagonal, far, level, above and
# the Location itself.
DELTAS = [
    [0, 0, -1100], [1000, 0, -1100], [0, 1000, -1100], [-700, -300, -1100],
    [3000, -4000, -900], [5, 3, 0], [12000, 9000, -1100], [100, 100, 200], [0, 0, 0],
    [-1, 0, -1100], [256, -255, -1000],
]


def name(**flags):
    return ".".join(f"{key}{int(value) if isinstance(value, bool) else value}"
                    for key, value in flags.items())


def fire_at_rows():
    rows = []
    # A first passenger: Drop_Payload and NULL, before FireAt and the +0x6CA test.
    for destroy, player in product((False, True), (False, True)):
        rows.append(dict(name=f"passenger.{name(d=destroy, p=player)}", passenger=True,
                         destroy=destroy, player=player, rot=1))
    # FireAt answers NULL: no course and no reveal; +0x6CA still uninits.
    for destroy, rot, player in product((False, True), (0, 1, 100), (False, True)):
        rows.append(dict(name=f"null.{name(d=destroy, rot=rot, p=player)}", bullet=False,
                         destroy=destroy, rot=rot, player=player, shrouded_at=0))
    # ROT other than 0 and 1 keeps FireAt's velocity.
    for rot in (100, 60, 2, -1, 0x7FFFFFFF):
        rows.append(dict(name=f"rot{rot}", rot=rot, velocity=[12.5, -7.25, -3.0]))
    # ROT 0: the locomotor's apparent speed, level, along SecondaryFacing.
    for n, (velocity, (type_speed, current), facing) in enumerate(product(
            LEVEL_VELOCITIES,
            ((40, 1.0), (75, 0.5), (100, 0.75), (1, 0.25), (0, 1.0), (61, 0.875)),
            (0, 0x2000, 0x4000, 0x6000, 0x8000, 0xA000, 0xC000, 0xE000, 0x1234, 0xFEDC,
             0x3FFF, 0x4001))):
        if n % 5:
            continue
        rows.append(dict(name=f"level.{n}", rot=0, velocity=velocity, type_speed=type_speed,
                         current_speed=current, facing=facing))
    for facing in (0, 0x4000, 0x8000, 0xC000):
        rows.append(dict(name=f"level.zero.{facing:04x}", rot=0, velocity=[0.0, 0.0, 0.0],
                         facing=facing))
        rows.append(dict(name=f"level.stopped.{facing:04x}", rot=0, current_speed=0.0,
                         velocity=[0.0, 0.0, -20.0], facing=facing))
    # ROT 1: at the target from the aircraft's GetCoords, at weapon 0's Speed.
    for n, (velocity, delta, speed) in enumerate(product(
            HOMING_VELOCITIES, DELTAS, (30, 70, 1, 0, 100))):
        if n % 3:
            continue
        location = DEFAULTS["location"]
        target = [location[i] + delta[i] for i in range(3)]
        rows.append(dict(name=f"homing.{n}", rot=1, velocity=velocity,
                         target_location=target, weapon0_speed=speed))
    # The reveal for the current player's aircraft: each probe in turn shrouded, or none;
    # with every course and the weapon index passed through.
    for shrouded_at, rot in product((None, 0, 1, 2, 3, 4, 5), (100, 0, 1)):
        rows.append(dict(name=f"reveal.{name(at=shrouded_at, rot=rot)}", player=True,
                         shrouded_at=shrouded_at, rot=rot))
    for sight_range in (5, 0, -1, 10):
        rows.append(dict(name=f"reveal.range{sight_range}", player=True, shrouded_at=5,
                         sight_range=sight_range, weapon=1))
    rows.append(dict(name="reveal.computer", player=False, shrouded_at=0))
    # +0x6CA after a shot: the reveal, then UnInit.
    for player in (False, True):
        rows.append(dict(name=f"destroy.{name(p=player)}", destroy=True, player=player,
                         shrouded_at=2, rot=1))
    return rows


def generate():
    fixture = FireAt()
    return {"fire_at": [fixture.fire_at(row) for row in fire_at_rows()]}


def metadata():
    return provenance(
        scope="AircraftClass::Fire_At 0x00415EE0 as vt+0x3CC, one call per row: a first "
              "passenger (+0x118) with +0x6CA set or not; TechnoClass::FireAt answering NULL "
              "or the bullet; a bullet type ROT +0x2DC of 0 (level along SecondaryFacing at "
              "the locomotor's Apparent_Speed), 1 (at the target's GetCoords at weapon 0's "
              "Speed) or another; the owner the current player or not, with each IsShrouded "
              "probe in turn answering true or none; AttackingAircraftSightRange (Rules+0x18) "
              "values; +0x6CA after a shot. Returned bullet, the ordered call log in FireAtHost "
              "vocabulary and the bullet's velocity bits afterwards. Not TechnoClass::FireAt's, "
              "Drop_Payload's, IsShrouded's, RevealArea's or UnInit's own work. The Locomotor's "
              "CurrentSpeed takes exact binary fractions only (0, .25, .5, .75, .875, 1): the "
              "native 0.1 ramp's rounded doubles (0.7999999999999999 on its eighth frame), "
              "which VERA's SimFixed ramp does not carry, are not covered.",
        assumptions=[
            "One emulator; per row the scratch region (0x10000 bytes) and the stack frame are "
            "rewritten, as aircraft_guard.py's ScratchAircraft does. The owner holds a clone of "
            "the Aircraft vtable 0x007E22A4 with only the stubbed slots replaced, the first "
            "passenger +0x118, +0x6CA, the house +0x21C, the Location +0x9C, the type +0x6C4 "
            "(Speed +0x678) and the Locomotor +0x674. The target is a Unit on its real vtable "
            "with its Location. The Rules pointer 0x008871E0 names a scratch RulesClass with "
            "+0x18.",
            "Per row: RET 8 to the caller with EBX/EBP/ESI/EDI preserved; no write outside the "
            "stack but the bullet's velocity +0xE8..+0xFF; every read of scratch bytes falls on "
            "bytes the row supplied.",
        ],
        substitutions=[
            "GetWeapon vt+0x3F8 answers a WeaponStruct whose type has the row's Speed; UnInit "
            "vt+0xF8 records its call.",
            "Entry hooks: TechnoClass::FireAt 0x006FDD50 answers the scratch bullet or NULL; "
            "Drop_Payload 0x00415C60 records; FacingClass::Current 0x004C93D0 answers the "
            "row's facing; 0x0050B6F0 answers the row's `player`; MapClass::IsShrouded "
            "0x00586360 answers true at the row's call; MapClass::RevealArea 0x005678E0 "
            "records.",
        ],
        entry_points={"fire_at": FIRE_AT, "apparent_speed": 0x4CFE20, "ftol": 0x7C5F00})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
