"""Original AircraftClass::Mission_Guard (0x0041A5C0) and Mission_AreaGuard (0x0041A940), one
call per row.

Each row runs the ORIGINAL function on a supplied Aircraft whose vtable is a clone of 0x007E22A4
with only the slots below replaced by scratch INT3 stubs. A stub records its arguments and
returns the row's value with the native callee's stack cleanup; entry hooks at native callees do
the same without running their bodies. Every row asserts that the call returns to its caller
with the native cleanup (RET) and EBX/EBP/ESI/EDI preserved, writes nothing outside the stack
but the owner's Mission+0xBC, +0x6CC and +0x6D4 (each write logged in order), and reads no
scratch byte the row did not supply. The log names each call after the GuardHost method of
src/sim/aircraft/guard_mission.rs it stands for, with the native arguments that method fixes.

Both functions (thiscall, no arguments, RET) run on the owner's type with its real
AircraftTypeClass vtable 0x007E2868: GetFlightLevel vt+0xBC (0x00717800) reads the row's +0x618
natively. Stubs: GetHeight vt+0x1C8 (0x005F5F40), Queue_Mission vt+0x1E8 (0x0041BA90),
GetWeapon vt+0x3F8 (0x0070E140; slot 0's WeaponType is the row's), Assign_Destination vt+0x480
(0x0041AA80), Enter_Idle_Mode vt+0x484 (0x004176F0), Is_Armed vt+0x2AC (0x00701120), the dock
search vt+0x528 (0x0041BBD0; answers the row's dock or NULL), Assign_Target vt+0x3C8 (0x006FCDB0)
and vt+0x54 (0x0041B920). Entry hooks: HouseClass::IsControlledByHuman 0x0050B730,
RadioClass::In_Radio_Contact 0x0065AE30, MapClass::Get_CellClass_At_Coord 0x00565730 (answers a
scratch cell for the Location), HouseClass 0x00500300 (answers the row's bridge vehicle or
NULL), the MissionControl lookup 0x005B3A00 (answers a scratch MissionControlClass whose Rate
+0x10 is (rate + 0.5) / 900, so the native fmul by 900.0 and ftol 0x007C5F00 under the game's
control word 0x0E7F produce the row's frames, which the row asserts after each ftol), Scenario
RandomRanged 0x0065C7E0 (on Scenario+0x218 of the supplied Scenario pointer 0x00A8B230),
FootClass::Mission_Guard 0x004D5070 and FootClass::Mission_AreaGuard 0x004D6AA0.
RadioClass::Contact 0x0065AD30 and its contact's What_Am_I run natively over the row's radio
vector (+0xE4 items, +0xE8 count 1): a Building on the real vtable 0x007E3EBC whose type (+0x520)
has the row's UnitReload byte (+0x16AA), or a Unit on 0x007F5C70. A read of the Target +0x2B4
from the function is logged as `target`; Contact's first call of the UnitReload check (returning
to 0x0041A720) as `contact_reloads`.

The globals 0x00889ECC and 0x00889ECD that select Mission_Guard's two other rearm thresholds
stay 0, as the image maps them: the code sections hold no absolute reference to either address
but the two reads at 0x0041A6B7 and 0x0041A758.
"""
from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
                               UC_X86_REG_FPCW)

from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, OracleError, finish_vectors, load_image, provenance,
                                 run_checked)

GUARD, GUARD_BODY = 0x41A5C0, (0x41A5C0, 0x41A936)
AREA_GUARD, AREA_GUARD_BODY = 0x41A940, (0x41A940, 0x41A9DD)
AIRCRAFT_VT, AIRCRAFT_TYPE_VT = 0x7E22A4, 0x7E2868
BUILDING_VT, UNIT_VT, CELL_VT = 0x7E3EBC, 0x7F5C70, 0x7E4EEC
MAP, SCENARIO_POINTER = 0x87F7E8, 0xA8B230

# Scratch layout (SCRATCH .. SCRATCH + 0x10000).
OWNER = SCRATCH + 0x1000            # AircraftClass
TYPE = SCRATCH + 0x2000             # AircraftTypeClass on its real vtable 0x007E2868
VTABLE = SCRATCH + 0x3000           # clone of 0x007E22A4 (0x600 bytes)
STUBS, STUBS_SIZE = SCRATCH + 0x3800, 0x800   # INT3 stubs, 0x10 apart
OBJECTS = SCRATCH + 0x4000          # 0x1000 per object; a cloned vtable at +0x800
PAD_TYPE = SCRATCH + 0x9000         # BuildingTypeClass (+0x16AA UnitReload)
# Small or opaque scratch objects, 0x100 apart.
(HOUSE, MISSION_CONTROL, WEAPON_STRUCT, WEAPON_TYPE, RADIO_ITEMS, OWN_CELL, DOCK, BRIDGE_UNIT,
 TEAM, NAV, TARGET, LOCO, LOCO_VT, CONTACT, HERE_CELL) = (SCRATCH + 0xB000 + 0x100 * n
                                                         for n in range(15))
SCENARIO = SCRATCH + 0xC000         # ScenarioClass (+0x218, the RNG RandomRanged runs on)
# Guard's radio contacts: a pad Building and a Unit.
PAD, CONTACT_UNIT = OBJECTS, OBJECTS + 0x1000
SP = STACK_BASE + STACK_SIZE - 0x1000
SENTINELS = {UC_X86_REG_EBX: 0x0B0B0B0B, UC_X86_REG_EBP: 0x0E0E0E0E,
             UC_X86_REG_ESI: 0x05050505, UC_X86_REG_EDI: 0x0D0D0D0D}
# Owner fields whose writes the log records: (offset, size) -> name.
LOGGED_WRITES = {(0xBC, 4): "set_state", (0x6CC, 4): "set_dock", (0x6D4, 1): "set_transition_ready"}


def u32(value):
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def s32(value):
    return struct.unpack("<i", u32(value))[0]


class ScratchAircraft:
    """One emulator for every row: a scratch Aircraft on a clone of the Aircraft vtable.

    `slots` maps an Aircraft vtable offset to (stub name, native target asserted against the
    image, pops); `stubs` adds stub names (-> pops) for other scratch vtables; `hooks` maps a
    native entry to (name, pops) and replaces its body; `observers` maps a native instruction
    to a name and only records. Subclasses answer them in `stub`, `hook` and `observe` and name
    pointers in `names`. A pops of None fails the row when reached.
    """

    def __init__(self, slots, native_slots, stubs, hooks, observers):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, SCRATCH_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        asserted = [(offset, native) for offset, (_, native, _) in slots.items()]
        for offset, native in asserted + list(native_slots.items()):
            if self.word(AIRCRAFT_VT + offset) != native:
                raise OracleError(f"Aircraft vt+0x{offset:X} is not 0x{native:08X}")
        self.slots, self.hooks, self.observers = slots, hooks, observers
        names = [name for name, _, _ in slots.values()] + list(stubs)
        self.stub_at = {name: STUBS + 0x10 * n for n, name in enumerate(names)}
        self.stub_name = {address: name for name, address in self.stub_at.items()}
        self.stub_pops = {name: pops for name, _, pops in slots.values()} | stubs
        assert 0x10 * len(names) <= STUBS_SIZE
        # The Scenario pointer RandomRanged's `this` comes from (0x0041A90D).
        u.mem_write(SCENARIO_POINTER, u32(SCENARIO))
        self.auditing = False
        self.read_observers = {}
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUBS, end=STUBS + STUBS_SIZE - 1)
        for address in hooks:
            u.hook_add(UC_HOOK_CODE, self.on_hook, begin=address, end=address)
        for address in observers:
            u.hook_add(UC_HOOK_CODE, self.on_observe, begin=address, end=address)
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)
        u.hook_add(UC_HOOK_MEM_READ, self.on_read, begin=SCRATCH, end=SCRATCH + SCRATCH_SIZE - 1)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def put(self, address, blob):
        self.u.mem_write(address, blob)
        self.declared.append((address, address + len(blob)))

    def put32(self, address, *values):
        self.put(address, b"".join(u32(v) for v in values))

    def put8(self, address, value):
        self.put(address, bytes([int(value) & 0xFF]))

    def name(self, pointer):
        if pointer == 0:
            return None
        if pointer in self.names:
            return self.names[pointer]
        raise OracleError(f"unexpected pointer 0x{pointer:08X}")

    def coord(self, address):
        return [s32(v) for v in struct.unpack("<3I", self.u.mem_read(address, 12))]

    # -- fixture
    def reset(self, row, body):
        u = self.u
        self.row, self.body, self.calls, self.violations = row, body, [], []
        self.declared, self.undeclared_reads = [], set()
        u.mem_write(SCRATCH, bytes(SCRATCH_SIZE))
        u.mem_write(SP - 0x1000, bytes(0x1100))
        u.mem_write(STUBS, b"\xCC" * STUBS_SIZE)
        vt = bytearray(u.mem_read(AIRCRAFT_VT, 0x600))
        for offset, (name, _, _) in self.slots.items():
            vt[offset:offset + 4] = u32(self.stub_at[name])
        self.put(VTABLE, bytes(vt))
        self.put32(OWNER, VTABLE)
        self.put32(OWNER + 0x6C4, TYPE)
        self.put32(TYPE, AIRCRAFT_TYPE_VT)
        self.put32(OWNER + 0x9C, *row["location"])
        self.put32(OWNER + 0xBC, row["state"])
        self.put8(OWNER + 0x6D4, row["ready"])

    def object(self, address, vtable, replaced=None):
        """An object at `address` on `vtable`, or on a clone of it at address + 0x800 whose
        `replaced` slots (offset -> stub name) are stubs."""
        if replaced:
            vt = bytearray(self.u.mem_read(vtable, 0x600))
            for offset, name in replaced.items():
                vt[offset:offset + 4] = u32(self.stub_at[name])
            self.put(address + 0x800, bytes(vt))
            vtable = address + 0x800
        self.put32(address, vtable)

    # -- hooks
    def arg(self, n):
        return self.word(self.u.reg_read(UC_X86_REG_ESP) + 4 * (n + 1))

    def this(self, expected, what):
        if self.u.reg_read(UC_X86_REG_ECX) != expected:
            self.fail(f"{what} called on 0x{self.u.reg_read(UC_X86_REG_ECX):08X}")
            return False
        return True

    def returns(self, value, pops):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, int(value) & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.word(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def fail(self, message):
        self.violations.append(message)
        self.u.emu_stop()

    def from_body(self, name):
        caller = self.word(self.u.reg_read(UC_X86_REG_ESP))
        if not self.body[0] <= caller < self.body[1]:
            self.fail(f"{name} called from 0x{caller:08X}, outside the function under test")
            return False
        return True

    def on_stub(self, _u, address, _size, _data):
        name = self.stub_name.get(address)
        if name is None:
            return self.fail(f"unknown stub 0x{address:08X}")
        if not self.from_body(name):
            return None
        pops = self.stub_pops[name]
        if pops is None:
            return self.fail(f"{name} reached")
        value = self.stub(name)
        if not self.violations:
            self.returns(value, pops)
        return None

    def on_hook(self, _u, address, _size, _data):
        name, pops = self.hooks[address]
        if not self.from_body(name):
            return None
        if pops is None:
            return self.fail(f"{name} reached")
        value = self.hook(name)
        if not self.violations:
            self.returns(value, pops)
        return None

    def on_observe(self, _u, address, _size, _data):
        if self.auditing:
            self.observe(self.observers[address])

    def on_write(self, u, _access, address, size, value, _data):
        if not self.auditing:
            return
        if STACK_BASE <= address and address + size <= STACK_BASE + STACK_SIZE:
            return
        name = LOGGED_WRITES.get((address - OWNER, size))
        if name == "set_dock":
            self.calls.append([name, self.name(value & 0xFFFFFFFF)])
        elif name is not None:
            self.calls.append([name, s32(value) if size == 4 else value & 0xFF])
        else:
            self.fail(f"write outside the stack and the logged owner fields at "
                      f"0x{address:08X}+{size} from 0x{u.reg_read(UC_X86_REG_EIP):08X}")

    def on_read(self, u, _access, address, size, _value, _data):
        if not self.auditing:
            return
        name = self.read_observers.get(address)
        if name is not None and self.body[0] <= u.reg_read(UC_X86_REG_EIP) < self.body[1]:
            self.calls.append([name])
        if not any(low <= address and address + size <= high for low, high in self.declared):
            self.undeclared_reads.add((address, size, u.reg_read(UC_X86_REG_EIP)))

    # -- one call
    def execute(self, entry, args=()):
        """`args` are the stack arguments the callee pops (RET 4 * len(args))."""
        u, row = self.u, self.row
        u.mem_write(SP, u32(RET_MAGIC) + b"".join(u32(arg) for arg in args))
        for register, value in SENTINELS.items():
            u.reg_write(register, value)
        u.reg_write(UC_X86_REG_ECX, OWNER)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.auditing = True
        try:
            run_checked(u, entry, RET_MAGIC, count=100_000, context={"row": row["name"]})
        except OracleError as error:
            raise OracleError(f"{row['name']}: {self.violations or error}") from error
        finally:
            self.auditing = False
        if self.violations:
            raise OracleError(f"{row['name']}: {self.violations}")
        if u.reg_read(UC_X86_REG_ESP) != SP + 4 + 4 * len(args):
            raise OracleError(f"{row['name']}: unbalanced stack at return")
        if any(u.reg_read(register) != value for register, value in SENTINELS.items()):
            raise OracleError(f"{row['name']}: callee-saved register not restored")
        if self.undeclared_reads:
            reads = ", ".join(f"0x{a:08X}+{s}@0x{pc:08X}"
                              for a, s, pc in sorted(self.undeclared_reads))
            raise OracleError(f"{row['name']}: read of unsupplied scratch bytes: {reads}")
        return {"input": row, "ret": s32(u.reg_read(UC_X86_REG_EAX)), "calls": self.calls,
                "state": s32(self.word(OWNER + 0xBC)), "ready": u.mem_read(OWNER + 0x6D4, 1)[0]}


# Cloned vtable slots: offset -> (stub name, native target asserted against the image, pops).
SLOTS = {
    0x1C8: ("height", 0x5F5F40, 0), 0x1E8: ("queue_mission", 0x41BA90, 8),
    0x3F8: ("get_weapon", 0x70E140, 4), 0x480: ("assign_destination", 0x41AA80, 8),
    0x484: ("enter_idle_mode", 0x4176F0, 8), 0x2AC: ("is_armed", 0x701120, 0),
    0x528: ("dock_search", 0x41BBD0, 0xC), 0x3C8: ("assign_target", 0x6FCDB0, 4),
    0x54: ("vt54", 0x41B920, 0),
}
NATIVE_SLOTS = {0x21C: GUARD, 0x220: AREA_GUARD}
# Entry hooks: address -> (name, pops).
HOOKS = {
    0x50B730: ("house_human", 0), 0x65AE30: ("in_radio_contact", 0),
    0x565730: ("cell_at_coord", 4), 0x500300: ("bridge_unit", 4),
    0x5B3A00: ("mission_control", 0), 0x65C7E0: ("random_ranged", 8),
    0x4D5070: ("foot_guard", 0), 0x4D6AA0: ("foot_area_guard", 0),
}
# RadioClass::Contact's entry, and the instructions after Mission_Guard's two ftol calls.
OBSERVERS = {0x65AD30: "contact", 0x41A627: "rate_frames", 0x41A90B: "rate_frames"}
# The UnitReload check's two Contact(0) calls return here (0x0041A71B, 0x0041A734).
CONTACT_CHECK, CONTACT_TYPE_READ = 0x41A720, 0x41A739


class Guard(ScratchAircraft):
    def __init__(self):
        super().__init__(SLOTS, NATIVE_SLOTS, {}, HOOKS, OBSERVERS)
        self.read_observers = {OWNER + 0x2B4: "target"}
        self.names = {OWN_CELL: "own_cell", DOCK: "dock", BRIDGE_UNIT: "bridge_unit",
                      TYPE + 0x3E8: "type_dock", OWNER: "aircraft"}

    def build(self, row, body):
        self.reset(row, body)
        self.put32(OWNER + 0x21C, HOUSE)
        self.put32(OWNER + 0x5D4, TEAM if row["team"] else 0)
        self.put32(OWNER + 0x5A4, NAV if row["nav_com"] else 0)
        self.put32(OWNER + 0x2FC, row["ammo"])
        self.put32(OWNER + 0x2B4, TARGET if row["target"] else 0)
        contact = {None: 0, "reload": PAD, "building": PAD, "unit": CONTACT_UNIT}[row["contact"]]
        self.put32(OWNER + 0xE4, RADIO_ITEMS)
        self.put32(OWNER + 0xE8, 1)
        self.put32(RADIO_ITEMS, contact)
        self.object(PAD, BUILDING_VT)
        self.put32(PAD + 0x520, PAD_TYPE)
        self.put8(PAD_TYPE + 0x16AA, row["contact"] == "reload")
        self.object(CONTACT_UNIT, UNIT_VT)
        self.put32(TYPE + 0x618, row["flight_level"])
        self.put32(TYPE + 0x684, row["type_ammo"])
        self.put32(WEAPON_STRUCT, WEAPON_TYPE if row["weapon"] else 0)
        self.put(MISSION_CONTROL + 0x10, struct.pack("<d", (row["rate"] + 0.5) / 900))

    def stub(self, name):
        row = self.row
        if not self.this(OWNER, name):
            return 0
        if name == "height":
            return row["height"]
        if name == "is_armed":
            return row["armed"]
        if name == "get_weapon":
            if self.arg(0) != 0:
                self.fail(f"GetWeapon({self.arg(0)})")
            return WEAPON_STRUCT
        if name == "vt54":
            self.calls.append(["in_air"])
            return row["in_air"]
        if name == "queue_mission":
            self.calls.append(["queue", s32(self.arg(0)), s32(self.arg(1))])
        elif name == "enter_idle_mode":
            self.calls.append(["enter_idle_mode", s32(self.arg(0)), s32(self.arg(1))])
        elif name == "assign_destination":
            target, flag = self.arg(0), s32(self.arg(1))
            self.calls.append({OWN_CELL: ["assign_own_cell", flag],
                               DOCK: ["assign_dock", "dock", flag]}.get(
                target, ["assign_destination", self.name(target), flag]))
        elif name == "assign_target":
            target = self.arg(0)
            self.calls.append(["clear_target"] if target == 0
                              else ["assign_target", self.name(target)])
        elif name == "dock_search":
            self.calls.append(["find_dock", self.name(self.arg(0)), s32(self.arg(1)),
                               s32(self.arg(2))])
            return DOCK if row["dock"] else 0
        return 0

    def hook(self, name):
        row = self.row
        if name == "house_human":
            return self.this(HOUSE, name) and row["human"]
        if name == "in_radio_contact":
            self.calls.append(["in_radio_contact"])
            return self.this(OWNER, name) and row["contact"] is not None
        if name in ("cell_at_coord", "bridge_unit"):
            if not self.this(MAP if name == "cell_at_coord" else HOUSE, name):
                return 0
            if self.coord(self.arg(0)) != row["location"]:
                self.fail(f"{name} at {self.coord(self.arg(0))}, not the Location")
                return 0
            if name == "cell_at_coord":
                return OWN_CELL
            self.calls.append(["attack_bridge_unit"])
            return BRIDGE_UNIT if row["bridge_unit"] else 0
        if name == "mission_control":
            self.calls.append(["rate"])
            return MISSION_CONTROL if self.this(OWNER, name) else 0
        if name == "random_ranged":
            self.calls.append(["jitter", s32(self.arg(0)), s32(self.arg(1))])
            return row["jitter"] if self.this(SCENARIO + 0x218, name) else 0
        self.calls.append([name])                       # foot_guard, foot_area_guard
        return row["foot"] if self.this(OWNER, name) else 0

    def observe(self, name):
        u = self.u
        if name == "rate_frames":
            if s32(u.reg_read(UC_X86_REG_EAX)) != self.row["rate"]:
                self.fail(f"ftol answered {s32(u.reg_read(UC_X86_REG_EAX))}")
            return
        caller = self.word(u.reg_read(UC_X86_REG_ESP))
        if caller == CONTACT_CHECK:
            self.calls.append(["contact_reloads"])
        elif caller != CONTACT_TYPE_READ:
            self.fail(f"RadioClass::Contact called from 0x{caller:08X}")

    def guard(self, row):
        row = {**GUARD_DEFAULTS, **row}
        self.build(row, GUARD_BODY)
        return self.execute(GUARD)

    def area_guard(self, row):
        row = {**GUARD_DEFAULTS, **row}
        self.build(row, AREA_GUARD_BODY)
        return self.execute(AREA_GUARD)


GUARD_DEFAULTS = {
    "height": 300, "flight_level": 600, "team": False, "nav_com": False, "weapon": True,
    "armed": True, "ammo": 4, "type_ammo": 4, "human": False, "contact": None, "dock": False,
    "target": False, "bridge_unit": False, "in_air": False, "rate": 27, "jitter": 1, "foot": 77,
    "location": [15488, 16512, 600], "state": 3, "ready": 2,
}
# Ammo against the type's Ammo=: -1, 0, both sides of the stock and half thresholds, odd and
# negative types (half rounds toward zero).
AMMO_PAIRS = ((-1, -1), (-1, 0), (-1, 4), (0, -1), (0, 0), (0, 1), (0, 3), (1, 3), (2, 3),
              (0, 4), (1, 4), (2, 4), (3, 4), (4, 4), (5, 4), (-2, -3), (-2, -2))
CONTACTS = (None, "reload", "building", "unit")
BUSY = {"armed": True, "human": True, "contact": "reload", "target": True, "ammo": 0,
        "dock": True, "in_air": True, "bridge_unit": True}
QUIET = {"armed": False}


def bits(**flags):
    return ".".join(f"{key}{int(value) if isinstance(value, bool) else value}"
                    for key, value in flags.items())


def guard_rows():
    rows = []
    # At the type's flight level: team, NavCom and weapon, with the rest quiet or busy.
    for team, nav_com, weapon in product((False, True), repeat=3):
        for label, extra in (("quiet", QUIET), ("busy", BUSY)):
            rows.append(dict(name=f"level.{bits(t=team, n=nav_com, w=weapon)}.{label}",
                             height=600, team=team, nav_com=nav_com, weapon=weapon, **extra))
    # Below it, the rearm checks over Ammo, Is_Armed, the radio contact and the dock search;
    # a Target ends a row that passes them.
    for (ammo, type_ammo), armed, contact, dock in product(AMMO_PAIRS, (False, True), CONTACTS,
                                                           (False, True)):
        rows.append(dict(name=f"rearm.ammo{ammo}_{type_ammo}.{bits(a=armed)}.c{contact}"
                              f".{bits(d=dock)}",
                         ammo=ammo, type_ammo=type_ammo, armed=armed, contact=contact,
                         dock=dock, target=True))
    # Without a Target: the idle frames, the bridge search, the high-flight check, the Rate
    # epilogue and the Foot body, short of Ammo (no dock found) or not.
    for (ammo, type_ammo), armed, height, contact, human, bridge_unit, in_air in product(
            ((4, 4), (0, 4)), (False, True), (300, 0), (None, "unit"), (False, True),
            (False, True), (False, True)):
        rows.append(dict(name=f"tail.ammo{ammo}_{type_ammo}.{bits(a=armed, z=height)}"
                              f".c{contact}.{bits(h=human, b=bridge_unit, air=in_air)}",
                         ammo=ammo, type_ammo=type_ammo, armed=armed, height=height,
                         contact=contact, human=human, bridge_unit=bridge_unit, in_air=in_air))
    # Heights above the flight level, below 0 and just off the ground; Rate and RandomRanged
    # answers; flight levels other than 600.
    for height in (900, -50, 1):
        for human in (False, True):
            rows.append(dict(name=f"height{height}.{bits(h=human)}", height=height, human=human))
    for rate, jitter in ((0, 0), (1800, 2), (27, 0), (27, 2)):
        rows.append(dict(name=f"epilogue.rate{rate}.jitter{jitter}", human=True, rate=rate,
                         jitter=jitter))
        rows.append(dict(name=f"level_rate.rate{rate}", height=600, team=True, rate=rate))
    for flight_level, height in ((0, 0), (1500, 1500), (1500, 600)):
        rows.append(dict(name=f"flight_level{flight_level}.z{height}", flight_level=flight_level,
                         height=height, team=True, nav_com=True))
    return rows


def area_guard_rows():
    rows = []
    for team in (False, True):
        for label, extra in (("quiet", QUIET), ("busy", BUSY)):
            rows.append(dict(name=f"level.{bits(t=team)}.{label}", height=600, team=team,
                             **extra))
    for ammo, armed, contact, target, height in product((-1, 0, 1), (False, True),
                                                        (None, "unit"), (False, True), (300, 0)):
        rows.append(dict(name=f"below.ammo{ammo}.{bits(a=armed)}.c{contact}"
                              f".{bits(tg=target, z=height)}",
                         ammo=ammo, armed=armed, contact=contact, target=target, height=height))
    rows.append(dict(name="below.team.human", ammo=0, team=True, human=True, target=True))
    return rows


def generate():
    fixture = Guard()
    return {"mission_guard": [fixture.guard(row) for row in guard_rows()],
            "mission_area_guard": [fixture.area_guard(row) for row in area_guard_rows()]}


def metadata():
    return provenance(
        scope="AircraftClass::Mission_Guard 0x0041A5C0 and Mission_AreaGuard 0x0041A940 as "
              "vt+0x21C and vt+0x220, one call per row. Mission_Guard at the type's flight "
              "level over team (+0x5D4) x NavCom (+0x5A4) x a slot-0 weapon, the rest quiet or "
              "busy; below it, Ammo (+0x2FC) against the type's Ammo= (+0x684) for -1, 0, both "
              "sides of the stock and half thresholds, odd and negative types, over Is_Armed x "
              "radio contact (none, a UnitReload pad, a pad without UnitReload, a Unit) x dock "
              "found, with a Target; without a Target, over Is_Armed x GetHeight 300/0 x radio "
              "contact x IsControlledByHuman x a bridge vehicle found x vt+0x54, short of Ammo "
              "or not; heights 900, -50 and 1, flight levels 0 and 1500, Rate and RandomRanged "
              "answers. Mission_AreaGuard at the flight level over team, quiet or busy; below "
              "it over Ammo -1/0/1 x Is_Armed x radio contact x Target x GetHeight 300/0. "
              "Returned value, ordered call log in GuardHost vocabulary with arguments, the "
              "writes to Mission+0xBC, +0x6CC and +0x6D4 in order, and both bytes afterwards. "
              "Not the dock search's, the house search's, the Foot bodies' or "
              "Enter_Idle_Mode's own work, and not the rearm thresholds of the globals "
              "0x00889ECC/0x00889ECD set.",
        assumptions=[
            "One emulator; per row the scratch region (0x10000 bytes) and the stack frame are "
            "rewritten. The owner holds a clone of the Aircraft vtable 0x007E22A4 with only the "
            "stubbed slots replaced, House +0x21C, the Location +0x9C, the radio vector "
            "+0xE4/+0xE8 and its type +0x6C4 on the real AircraftTypeClass vtable 0x007E2868 "
            "with the row's FlightLevel (+0x618) and Ammo= (+0x684).",
            "Mission+0xBC is preset 3 and +0x6D4 is preset 2 so a write shows; neither "
            "function reads either.",
            "The x87 control word is the game's 0x0E7F (the value ftol 0x007C5F00 compares "
            "with 0x00822D80).",
            "The globals 0x00889ECC and 0x00889ECD stay 0 as mapped: the code sections hold no "
            "absolute reference to either address but the reads at 0x0041A6B7 and 0x0041A758.",
            "RadioClass::Contact 0x0065AD30 and the contact's What_Am_I (Building 0x00459EC0, "
            "Unit 0x00746E20) run natively; the pad's type is a scratch BuildingTypeClass with "
            "the row's UnitReload (+0x16AA).",
            "Per row: RET to the caller with ESP + 4 and EBX/EBP/ESI/EDI preserved; no write "
            "outside the stack but the logged owner fields; every read of scratch bytes falls "
            "on bytes the row supplied.",
        ],
        substitutions=[
            "GetHeight vt+0x1C8, Is_Armed vt+0x2AC and vt+0x54 answer the row's values; "
            "GetWeapon vt+0x3F8 (slot 0 asserted) answers a WeaponStruct holding the row's "
            "WeaponType or NULL; Queue_Mission vt+0x1E8, Assign_Destination vt+0x480, "
            "Enter_Idle_Mode vt+0x484 and Assign_Target vt+0x3C8 record their arguments; the "
            "dock search vt+0x528 records its arguments and answers the row's dock or NULL.",
            "Entry hooks: IsControlledByHuman 0x0050B730 (on the owner's house) and "
            "In_Radio_Contact 0x0065AE30 answer the row's values; Get_CellClass_At_Coord "
            "0x00565730 (asserted on the Location) answers a scratch cell; HouseClass "
            "0x00500300 (asserted on the owner's house and Location) answers the row's bridge "
            "vehicle or NULL; 0x005B3A00 answers a scratch MissionControlClass whose Rate makes "
            "the native ftol produce the row's frames; RandomRanged 0x0065C7E0 (on "
            "Scenario+0x218) answers the row's value; FootClass::Mission_Guard 0x004D5070 and "
            "Mission_AreaGuard 0x004D6AA0 answer the row's value.",
        ],
        entry_points={"mission_guard": GUARD, "mission_area_guard": AREA_GUARD,
                      "get_flight_level": 0x717800, "radio_contact": 0x65AD30,
                      "building_what_am_i": 0x459EC0, "unit_what_am_i": 0x746E20,
                      "ftol": 0x7C5F00})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
