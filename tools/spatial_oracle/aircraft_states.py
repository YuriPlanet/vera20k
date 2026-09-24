"""Original AircraftClass::Mission_Attack 0x00417FE0, states 4..10, one visit per row.

Each row presets Mission+0xBC to the state under test and runs the ORIGINAL 0x00417FE0
(thiscall, no arguments, RET) on a supplied Aircraft, then records: the returned delay (EAX,
signed), the new Mission+0xBC, bytes +0x6C8 (pending ammo) and +0x6D2 (release latch), Ammo
+0x2FC, the byte ranges of the owner the call wrote, the landmark path through the body, the
ordered call log, and the Scenario RNG continuation `next_random` (the next raw Random()
0x0065C780 after the visit, read by an observation probe the visit returns into).

Stubbed (a scratch INT3 per slot in a clone of the Aircraft vtable 0x007E22A4, only these slots
replaced; the handler records the arguments and returns the row's supplied value with the
native callee's stack cleanup):
    select_weapon  vt+0x2E4 (target) -> weapon index                 RET 4   (0x006F3330)
    is_close       vt+0x3AC (target) -> bool                         RET 4   (0x006F7780)
    fire_error     vt+0x3C0 (target, weapon index, check range)      RET 0xC (0x0041A9E0)
    fire_at        vt+0x3CC (target, weapon index) -> bullet | null  RET 8   (0x00415EE0)
    uncloak        vt+0x45C (quiet)                                  RET 4   (0x007036C0)
    assign_dest    vt+0x480 (destination, 1)                         RET 8   (0x0041AA80)
    queue_mission  vt+0x1E8 (mission, 0)             state 10 only   RET 8   (0x0041BA90)
    enter_idle     vt+0x484 (0, 1)                   state 10 only   RET 8   (0x004176F0)
Entry-hooked (the ORIGINAL entry is reached; its arguments are recorded and it returns with its
native stack cleanup without running its body):
    scatter        CellClass 0x00481670 (ECX = cell, &source coords, a2, a3, a4), RET 0x10
    operator new 0x007C8E17 (cdecl size) -> a scratch bump allocation; operator delete
    0x007C8B3D (cdecl) -> no-op. Only the state-10 South edge scan allocates (its candidate
    vector 0x0042FCB0 / vt+8 0x0042F860); the CRT heap and its OS imports are not emulated.
Everything else runs natively, including: IFlyControl 0x007E2250 +0x18 strafe 0x0041B7F0 and
+0x1C fighter 0x0041B840 from the supplied weapon/projectile/type bytes; GetWeapon 0x0070E140;
DirTo 0x005F3DB0 and FacingClass::Set 0x004C9220; the target's GetCoords vt+0x48 0x005F65A0 and
MapClass::GetCell 0x00565730; the MissionControl entry 0x005B3A00, ftol 0x007C5F00 and Scenario
RandomRanged 0x0065C7E0 on [0x00A8B230]+0x218 (seeded by the original 0x0065C6D0); in state 10
House 0x0050B730 and 0x0050DA80, TechnoClass::SetTarget 0x006FCDB0 (through the clone's
untouched vt+0x3C8), MapClass PickCellOnEdge 0x004AA440 (0x00565520, 0x005654A0, 0x00578460,
0x004AAB30, 0x0042FCB0, 0x0042F860) and GetCell 0x005657A0 over a supplied flat map.

`calls` also names natively-run callees with their results: strafe, fighter, facing_set,
get_weapon and (state 10) house_control, set_target, house_edge, pick_edge, map_cell when the
body calls them directly, and every random_ranged draw wherever it happens (site = return
address, raw_draws = draws of the rejection loop).

Every row asserts: the visit returns to its caller with ESP = entry + 4 and EBX/EBP/ESI/EDI
preserved; no write outside the stack, the owner object, the Scenario RNG and the scratch heap
(plus the dummy-cell coordinate 0x00ABDC74 when a GetCell misses the table, reported); every
data read inside the scratch fixture or the zero-filled BSS falls on bytes the fixture
supplied (listed in the sidecar's assumptions) or a native initializer produced.

Pruning (state families): every combination of the axes is executed; combinations whose
outputs are identical (Ammo compared as after minus before) are kept once, and `covers` lists
the executed combinations each kept row stands for as a union of per-axis products, re-derived
and re-verified on every run. Evidence limits: the sidecar's assumptions and the aircraft
attack loop section of docs/plans/2026-09-21-combat-parity.md.
"""
from __future__ import annotations

import sys
from pathlib import Path

import bisect
import json
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW, UC_X86_REG_FPTAG)

from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
                                 OracleError, finish_vectors, load_image, provenance, run_checked)

# ---- native identities ------------------------------------------------------------------------
MISSION_ATTACK = 0x417FE0
BODY = (0x417FE0, 0x418D55)                 # function body, up to the jump tables at 0x418D58
AIRCRAFT_VT, UNIT_VT, FLY_VT = 0x7E22A4, 0x7F5C70, 0x7E2250
MAP, FRAME, RULES_PTR, SCENARIO_PTR = 0x87F7E8, 0xA8ED84, 0x8871E0, 0xA8B230
GAME_MODE = 0xA8B238                        # read by House 0x0050B730
MISSION_CONTROL = 0xA8E3A8                  # 0x005B3A00: MissionControl + Mission * 0x20
DUMMY_CELL, DUMMY_CELL_COORDS = 0xABDC50, 0xABDC74
EDGE_SENTINEL, EDGE_REFERENCE = 0x8A03F8, 0x889E68
STATIC_INITIALIZERS = (0x413C60, 0x4A85E0)  # write EDGE_REFERENCE and EDGE_SENTINEL (0, 0)
RNG_SEED, RANDOM = 0x65C6D0, 0x65C780
RANDOM_RANGED, RANDOM_RANGED_DRAW, RANDOM_RANGED_RET = 0x65C7E0, 0x65C837, 0x65C88A
SCATTER, OPERATOR_NEW, OPERATOR_DELETE = 0x481670, 0x7C8E17, 0x7C8B3D
BSS = (0x87E000, 0xB79BE4)                  # zero-filled .data tail: runtime globals

STATE_ENTRY = {4: 0x4182A3, 5: 0x41858C, 6: 0x41879D, 7: 0x4188AC, 8: 0x4189BB, 9: 0x418ACA,
               10: 0x418BEC}
# Landmarks recorded in `path`: state entries, every jump-table target, the shared tails and
# the arms' internal decision blocks (read from the disassembly; execution settles which run).
LANDMARKS = {
    **{address: f"s{state}" for state, address in STATE_ENTRY.items()},
    0x418403: "s4.code0_release", 0x418478: "s4.release_scatter", 0x4184CC: "s4.suffix_strafe",
    0x418506: "s4.suffix_fighter", 0x418572: "s4.to_state5",
    0x418368: "s4.code2", 0x4183DB: "s4.code2_state1", 0x4183B1: "s4.code2_state4",
    0x4183BD: "s4.code2_curley", 0x4183F3: "s4.ret45",
    0x41834C: "s4.code9_uncloak", 0x418544: "s4.default", 0x418564: "s4.default_ammo",
    # Ammo == 0 exits inside the state-4 arms; the prefix test 0x4182B1 already left on Ammo 0,
    # so no row is expected to reach them (a row that did would change the payload).
    0x418372: "s4.code2_ammo0", 0x41854E: "s4.default_ammo0",
    0x4186B6: "s5.code0_fire", 0x418720: "s5.after_scatter", 0x418634: "s5.code2",
    0x41866B: "s5.code2_curley", 0x418688: "s5.state1", 0x4186A6: "s5.ret45",
    0x418623: "s5.code9_uncloak", 0x41874E: "s5.default", 0x418771: "s5.default_state1",
    0x41877C: "s5.default_curley",
    0x4187F2: "s6.code8_assign", 0x418805: "s6.fire", 0x418901: "s7.code8_assign",
    0x418914: "s7.fire", 0x418A10: "s8.code8_assign", 0x418A23: "s8.fire",
    0x418B1F: "s9.fire", 0x418B8A: "s9.delay",
    0x418BC5: "BC5", 0x418BCF: "BC5.ammo0", 0x418BDC: "ret1", 0x4189C5: "state10_ret1",
    0x418D17: "D17", 0x418D1D: "epilogue",
    0x418BFE: "s10.consume", 0x418C0E: "s10.decrement", 0x418C21: "s10.ammo0",
    0x418C38: "s10.clear_target", 0x418C43: "s10.edge", 0x418CAD: "s10.queue_mission",
    0x418CD1: "s10.ammo_left", 0x418CDD: "s10.to_state1", 0x418CF3: "s10.enter_idle",
}
# Natively-run callees named in the log when the body calls them directly: entry -> (name, pops).
NATIVE_LOGGED = {0x41B7F0: ("strafe", 4), 0x41B840: ("fighter", 4), 0x4C9220: ("facing_set", 4),
                 0x70E140: ("get_weapon", 4), 0x50B730: ("house_control", 0),
                 0x6FCDB0: ("set_target", 4), 0x50DA80: ("house_edge", 0),
                 0x4AA440: ("pick_edge", 0x1C), 0x5657A0: ("map_cell", 4)}

# ---- scratch layout ---------------------------------------------------------------------------
REGION = 0xA00000
SP = STACK_BASE + STACK_SIZE - 0x1000
OBJECTS = 0x10000                           # per-row region: [SCRATCH, SCRATCH + OBJECTS)
OWNER = SCRATCH + 0x1000                    # AircraftClass
TARGET = SCRATCH + 0x2000                   # UnitClass (original vtable 0x007F5C70)
TYPE = SCRATCH + 0x3000                     # AircraftTypeClass
WEAPON = (SCRATCH + 0x4000, SCRATCH + 0x4400)       # WeaponTypeClass slots 0 and 1
PROJECTILE = (SCRATCH + 0x4800, SCRATCH + 0x4C00)   # BulletTypeClass of each weapon
RULES = SCRATCH + 0x5000                    # RulesClass (+0x17E1 CurleyShuffle)
SCENARIO = SCRATCH + 0x7000                 # ScenarioClass; its RNG at +0x218, 0x3F4 bytes
RNG = SCENARIO + 0x218
VTABLE = SCRATCH + 0x8000                   # clone of 0x007E22A4 (0x600 bytes)
STUBS = SCRATCH + 0x8800                    # INT3 stubs, 0x10 apart
BULLET = SCRATCH + 0x8C00                   # opaque bullet returned by fire_at (never read)
AIRSTRIKE = SCRATCH + 0x8E00                # AirstrikeClass behind Aircraft+0x294 (state 10)
HOUSE = SCRATCH + 0x9000                    # HouseClass (+0x1EC, +0x1ED, +0x577C)
HEAP = (SCRATCH + 0xF000, SCRATCH + 0x10000)  # operator new bump allocations
TABLE = SCRATCH + 0x10000                   # MapClass cell pointer table, 0x40000 entries
CELLS = SCRATCH + 0x110000                  # 128 x 128 flat cells, 0x200 apart
CELL_SIZE, TABLE_ENTRIES, SIDE = 0x200, 0x40000, 128
# The supplied map: MapSize width +0xF4 and LocalSize (x, y, w, h) at +0xFC..+0x108.
MAP_WIDTH, LOCAL_SIZE = 64, (1, 4, 62, 50)
OWNER_CELL, TARGET_CELL = (60, 64), (64, 64)

# Observation probe outside native memory (like the harness's FSTP capture stub): the visit
# returns to RETURN_PROBE, where a hook captures ESP/EAX/callee-saved registers; the probe then
# calls the original Random 0x0065C780 on the Scenario RNG and stores the continuation.
RETURN_PROBE = RET_MAGIC + 0x200
PROBE_SLOT = RET_MAGIC + 0x300
PROBE_CODE = (bytes([0xB9]) + struct.pack("<I", RNG)                            # MOV ECX, rng
              + bytes([0xE8]) + struct.pack("<i", RANDOM - (RETURN_PROBE + 10))  # CALL Random
              + bytes([0xA3]) + struct.pack("<I", PROBE_SLOT))                  # MOV [slot], EAX
PROBE_END = RETURN_PROBE + len(PROBE_CODE)
SENTINELS = {UC_X86_REG_EBX: 0x0B0B0B0B, UC_X86_REG_EBP: 0x0E0E0E0E,
             UC_X86_REG_ESI: 0x05050505, UC_X86_REG_EDI: 0x0D0D0D0D}

STUB_POPS = {"select_weapon": 4, "is_close": 4, "fire_error": 0xC, "fire_at": 8, "uncloak": 4,
             "assign_dest": 8, "queue_mission": 8, "enter_idle": 8}
STUB_SLOTS = {0x2E4: "select_weapon", 0x3AC: "is_close", 0x3C0: "fire_error", 0x3CC: "fire_at",
              0x45C: "uncloak", 0x480: "assign_dest", 0x1E8: "queue_mission", 0x484: "enter_idle"}
NATIVE_SLOT_TARGETS = {0x2E4: 0x6F3330, 0x3AC: 0x6F7780, 0x3C0: 0x41A9E0, 0x3CC: 0x415EE0,
                       0x45C: 0x7036C0, 0x480: 0x41AA80, 0x1E8: 0x41BA90, 0x484: 0x4176F0}
STUB_AT = {name: STUBS + 0x10 * n for n, name in enumerate(STUB_POPS)}
STUB_NAME = {address: name for name, address in STUB_AT.items()}

CLASSES = {  # weapon-0 projectile ROT / Inviso and Type+0xE0E (Fighter) for each class
    "strafe": dict(rot=1, inviso=0, fighter=0),          # HORNET/ASW: NormalBomb/DepthCharge ROT 1
    "fighter": dict(rot=100, inviso=0, fighter=1),       # ORCA/BEAG/BPLN: AirToGroundMissile ROT 100
    "neither": dict(rot=100, inviso=0, fighter=0),
    "strafe_fighter": dict(rot=1, inviso=0, fighter=1),  # no retail type; precedence only
}

DEFAULTS = {
    "state": 4, "code": 0, "class": "neither", "ammo": 1, "target": True, "curley": 1,
    "is_close": 1, "select": 0, "bullet": False, "pending": 0, "latch": 0,
    "burst": [1, 1], "rof": [20, 37], "range": [1280, 768], "speed": 30,
    "rate": 0.016, "seed": 31, "mission": 1, "frame": 1000, "target_xyz": None,
    # state 10
    "flag_3d4": 0, "game_mode": 1, "house_1ec": 0, "house_1ed": 0, "airstrike": False, "edge": 0,
}


def u32(value):
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def i32(raw: bytes) -> int:
    return struct.unpack("<i", raw)[0]


def s32(value: int) -> int:
    return struct.unpack("<i", u32(value))[0]


def lep(cell, z=0):
    """Center of a cell in leptons."""
    return [cell[0] * 256 + 128, cell[1] * 256 + 128, z]


def rate_double(rate: float) -> bytes:
    """ReadINI stores the [Mission] Rate float widened to the table's double."""
    return struct.pack("<d", struct.unpack("<f", struct.pack("<f", rate))[0])


def cell_address(x, y):
    return CELLS + (y * SIDE + x) * CELL_SIZE


class Fixture:
    def __init__(self):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, REGION)
        u.mem_map(RET_MAGIC, 0x1000)
        for offset, native in NATIVE_SLOT_TARGETS.items():   # the slots the stubs replace
            if self.word(AIRCRAFT_VT + offset) != native:
                raise OracleError(f"Aircraft vt+0x{offset:X} is not 0x{native:08X}")
        for vtable, offset, native in ((AIRCRAFT_VT, 0x48, 0x5F65A0), (UNIT_VT, 0x48, 0x5F65A0),
                                       (AIRCRAFT_VT, 0x3F8, 0x70E140), (AIRCRAFT_VT, 0x3C8, 0x6FCDB0),
                                       (FLY_VT, 0x18, 0x41B7F0), (FLY_VT, 0x1C, 0x41B840)):
            if self.word(vtable + offset) != native:
                raise OracleError(f"0x{vtable:08X}+0x{offset:X} is not 0x{native:08X}")
        self.original_dummy = bytes(u.mem_read(DUMMY_CELL, 0x200))
        self.seeded, self.auditing, self.row = {}, False, None
        # Static state written once; the write audit proves no row modifies it.
        self.static = []
        self.declared = self.static
        u.mem_write(SP, u32(RET_MAGIC))
        for initializer in STATIC_INITIALIZERS:
            u.mem_write(SP, u32(RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, initializer, RET_MAGIC, count=100)
        self.static += [(EDGE_REFERENCE, EDGE_REFERENCE + 4), (EDGE_SENTINEL, EDGE_SENTINEL + 4)]
        table = bytearray(TABLE_ENTRIES * 4)
        cells = bytearray(SIDE * SIDE * CELL_SIZE)
        for y in range(SIDE):
            for x in range(SIDE):
                struct.pack_into("<I", table, (y * 512 + x) * 4, cell_address(x, y))
                struct.pack_into("<hh", cells, (y * SIDE + x) * CELL_SIZE + 0x24, x, y)
        self.put(TABLE, bytes(table))
        self.put(CELLS, bytes(cells))                     # flat, empty, unflagged cells
        self.put32(MAP + 0x13C, TABLE, TABLE_ENTRIES)
        self.put32(MAP + 0xF4, MAP_WIDTH)
        self.put32(MAP + 0xFC, *LOCAL_SIZE)
        u.mem_write(RETURN_PROBE, PROBE_CODE)
        u.hook_add(UC_HOOK_CODE, self.on_return, begin=RETURN_PROBE, end=RETURN_PROBE)
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUBS, end=STUBS + 0x3FF)
        u.hook_add(UC_HOOK_CODE, self.on_scatter, begin=SCATTER, end=SCATTER)
        u.hook_add(UC_HOOK_CODE, self.on_allocator, begin=OPERATOR_NEW, end=OPERATOR_NEW)
        u.hook_add(UC_HOOK_CODE, self.on_allocator, begin=OPERATOR_DELETE, end=OPERATOR_DELETE)
        u.hook_add(UC_HOOK_CODE, self.on_body, begin=BODY[0], end=BODY[1] - 1)
        for address in NATIVE_LOGGED:
            u.hook_add(UC_HOOK_CODE, self.on_native, begin=address, end=address)
        u.hook_add(UC_HOOK_CODE, self.on_random_ranged, begin=RANDOM_RANGED, end=RANDOM_RANGED)
        u.hook_add(UC_HOOK_CODE, self.on_raw_draw, begin=RANDOM_RANGED_DRAW, end=RANDOM_RANGED_DRAW)
        u.hook_add(UC_HOOK_CODE, self.on_random_ranged_ret, begin=RANDOM_RANGED_RET,
                   end=RANDOM_RANGED_RET)
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)
        u.hook_add(UC_HOOK_MEM_READ, self.on_read, begin=SCRATCH, end=SCRATCH + REGION - 1)
        u.hook_add(UC_HOOK_MEM_READ, self.on_read, begin=BSS[0], end=BSS[1] - 1)

    # -- memory helpers
    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def put(self, address, blob: bytes):
        """Write fixture data and declare it (the read audit accepts reads of declared bytes)."""
        self.u.mem_write(address, blob)
        self.declared.append((address, address + len(blob)))

    def put32(self, address, *values):
        self.put(address, b"".join(u32(v) for v in values))

    def put8(self, address, value):
        self.put(address, bytes([int(value) & 0xFF]))

    def seal(self):
        merged = []
        for low, high in sorted(self.declared):
            if merged and low <= merged[-1][1]:
                merged[-1][1] = max(merged[-1][1], high)
            else:
                merged.append([low, high])
        self.sealed = merged
        self.sealed_starts = [low for low, _ in merged]

    def is_declared(self, address, size):
        n = bisect.bisect_right(self.sealed_starts, address) - 1
        return n >= 0 and address + size <= self.sealed[n][1]

    # -- fixture
    def build(self, row):
        u = self.u
        self.declared = list(self.static)
        u.mem_write(SCRATCH, bytes(OBJECTS))
        u.mem_write(SP - 0x4000, bytes(0x4100))
        u.mem_write(STUBS, b"\xCC" * 0x400)
        u.mem_write(DUMMY_CELL, self.original_dummy)
        self.heap_next = HEAP[0]
        # Globals the body and its native callees read.
        self.put32(FRAME, row["frame"])
        self.put32(RULES_PTR, RULES)
        self.put8(RULES + 0x17E1, row["curley"])
        self.put32(SCENARIO_PTR, SCENARIO)
        self.put32(GAME_MODE, row["game_mode"])
        self.put(MISSION_CONTROL, bytes(0x20 * 32))        # 32 MissionControl entries
        self.put(MISSION_CONTROL + 1 * 0x20 + 0x10, rate_double(0.016))   # retail [Attack] Rate
        self.put(MISSION_CONTROL + row["mission"] * 0x20 + 0x10, rate_double(row["rate"]))
        # The owner: cloned vtable with only the stub slots replaced.
        vt = bytearray(u.mem_read(AIRCRAFT_VT, 0x600))
        for offset, name in STUB_SLOTS.items():
            vt[offset:offset + 4] = u32(STUB_AT[name])
        self.put(VTABLE, bytes(vt))
        self.put32(OWNER, VTABLE)
        self.put32(OWNER + 0x9C, *lep(OWNER_CELL, 500))
        self.put32(OWNER + 0xAC, row["mission"])
        self.put32(OWNER + 0xBC, row["state"])
        self.put(OWNER + 0x150, struct.pack("<f", 0.0))     # veterancy: not elite
        self.put32(OWNER + 0x21C, HOUSE)
        self.put32(OWNER + 0x294, AIRSTRIKE if row["airstrike"] else 0)
        self.put32(OWNER + 0x2B4, TARGET if row["target"] else 0)
        self.put32(OWNER + 0x2D0, 0)                        # no SpawnManager of its own
        self.put32(OWNER + 0x2FC, row["ammo"])
        self.put32(OWNER + 0x304, 0)                        # no fire particle system
        self.put8(OWNER + 0x3D4, row["flag_3d4"])
        for facing in (0x388, 0x3A0):                       # desired 0, ROT 0: Set is immediate
            self.put(OWNER + facing, bytes(0x18))
            self.put32(OWNER + facing + 8, 0xFFFFFFFF)
        self.put32(OWNER + 0x6C0, FLY_VT)
        self.put32(OWNER + 0x6C4, TYPE)
        self.put8(OWNER + 0x6C8, row["pending"])
        self.put8(OWNER + 0x6D2, row["latch"])
        self.put32(AIRSTRIKE + 0x4C, 0)                     # not this aircraft: 0x41DB40 unreached
        self.put8(HOUSE + 0x1EC, row["house_1ec"])
        self.put8(HOUSE + 0x1ED, row["house_1ed"])
        self.put32(HOUSE + 0x577C, row["edge"])
        # Type, weapons, projectiles.
        cls = row["class"] if isinstance(row["class"], dict) else CLASSES[row["class"]]
        self.put32(TYPE + 0x678, row["speed"])
        self.put8(TYPE + 0xE0E, cls["fighter"])
        for n in (0, 1):
            weapon, projectile = WEAPON[n], PROJECTILE[n]
            self.put32(TYPE + 0x898 + 0x1C * n, weapon)
            self.put32(TYPE + 0xA94 + 0x1C * n, 0)          # no elite weapon
            self.put32(weapon + 0x9C, row["burst"][n])
            self.put32(weapon + 0xA0, projectile)
            self.put32(weapon + 0xB0, row["rof"][n])
            self.put32(weapon + 0xB4, row["range"][n])
            self.put32(projectile + 0x2DC, cls["rot"] if n == 0 else 100)
            self.put8(projectile + 0x29E, cls["inviso"] if n == 0 else 0)
        # The target: a Unit whose GetCoords (vt+0x48) returns Location +0x9C.
        self.put32(TARGET, UNIT_VT)
        self.put32(TARGET + 0x9C, *(row["target_xyz"] or lep(TARGET_CELL)))
        # Scenario RNG: the original seeder's output for this seed (run once per seed, cached).
        seeded = self.seeded.get(row["seed"])
        if seeded is None:
            u.mem_write(RNG, bytes(0x3F4))
            u.mem_write(SP, u32(RET_MAGIC) + u32(row["seed"]))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, RNG)
            run_checked(u, RNG_SEED, RET_MAGIC, count=200_000)
            seeded = self.seeded[row["seed"]] = bytes(u.mem_read(RNG, 0x3F4))
        self.put(RNG, seeded)
        self.put(HEAP[0], bytes(HEAP[1] - HEAP[0]))
        self.seal()

    # -- naming
    def name(self, value):
        if value == 0:
            return None
        named = {TARGET: "target", OWNER: "owner", BULLET: "bullet", DUMMY_CELL: "dummy_cell",
                 EDGE_REFERENCE: "edge_reference", EDGE_SENTINEL: "edge_sentinel"}
        if value in named:
            return named[value]
        if CELLS <= value < CELLS + SIDE * SIDE * CELL_SIZE and (value - CELLS) % CELL_SIZE == 0:
            x, y = struct.unpack("<hh", self.u.mem_read(value + 0x24, 4))
            return f"cell({x},{y})"
        for n in (0, 1):
            if value == TYPE + 0x898 + 0x1C * n:
                return f"weapon{n}"
            if value == TYPE + 0xA94 + 0x1C * n:
                return f"elite{n}"
        return f"0x{value:08X}"

    # -- hooks
    def arg(self, n):
        return self.word(self.u.reg_read(UC_X86_REG_ESP) + 4 * (n + 1))

    def returns(self, value, pops):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, int(value) & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.word(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def fail(self, message):
        self.violations.append(message)
        self.u.emu_stop()

    def on_stub(self, u, address, _size, _data):
        name = STUB_NAME.get(address)
        if name is None:
            return self.fail(f"unknown stub 0x{address:08X}")
        if u.reg_read(UC_X86_REG_ECX) != OWNER:
            return self.fail(f"{name} called on 0x{u.reg_read(UC_X86_REG_ECX):08X}")
        caller = self.word(u.reg_read(UC_X86_REG_ESP))
        if not BODY[0] <= caller < BODY[1]:
            return self.fail(f"{name} called from 0x{caller:08X}, outside Mission_Attack")
        row, pops = self.row, STUB_POPS[name]
        args = [self.arg(n) for n in range(pops // 4)]
        entry = {"call": name}
        value = 0
        if name in ("select_weapon", "is_close"):
            entry["args"] = [self.name(args[0])]
            value = row["select"] if name == "select_weapon" else row["is_close"]
            entry["ret"] = s32(value)
        elif name == "fire_error":
            entry["args"] = [self.name(args[0]), s32(args[1]), s32(args[2])]
            value = row["code"]
            entry["ret"] = s32(value)
        elif name == "fire_at":
            entry["args"] = [self.name(args[0]), s32(args[1])]
            value = BULLET if row["bullet"] else 0
            entry["ret"] = self.name(value)
        elif name == "assign_dest":
            entry["args"] = [self.name(args[0]), s32(args[1])]
        else:                                               # uncloak, queue_mission, enter_idle
            entry["args"] = [s32(a) for a in args]
        self.calls.append(entry)
        return self.returns(value, pops)

    def on_scatter(self, u, _address, _size, _data):
        caller = self.word(u.reg_read(UC_X86_REG_ESP))
        if not BODY[0] <= caller < BODY[1]:
            return self.fail(f"scatter called from 0x{caller:08X}")
        source = self.arg(0)
        self.calls.append({"call": "scatter", "cell": self.name(u.reg_read(UC_X86_REG_ECX)),
                           "source": list(struct.unpack("<iii", u.mem_read(source, 12))),
                           "a2": s32(self.arg(1)), "a3": s32(self.arg(2)), "a4": s32(self.arg(3))})
        return self.returns(0, 0x10)

    def on_allocator(self, u, address, _size, _data):
        if address == OPERATOR_NEW:
            size = (self.arg(0) + 7) & ~7
            if self.heap_next + size > HEAP[1]:
                return self.fail("scratch heap exhausted")
            pointer, self.heap_next = self.heap_next, self.heap_next + size
            self.allocations += 1
            return self.returns(pointer, 0)                 # cdecl: the caller pops
        self.frees += 1
        return self.returns(0, 0)

    def on_native(self, u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        caller = self.word(sp)
        if not BODY[0] <= caller < BODY[1]:
            return                                          # nested: runs silently
        name, pops = NATIVE_LOGGED[address]
        entry = {"native": name}
        if name == "facing_set":
            which = u.reg_read(UC_X86_REG_ECX) - OWNER
            entry["facing"] = {0x388: "primary", 0x3A0: "secondary"}.get(which, hex(which))
            entry["dir"] = struct.unpack("<H", u.mem_read(self.arg(0), 2))[0]
        elif name == "get_weapon":
            entry["args"] = [s32(self.arg(0))]
        elif name == "set_target":
            entry["args"] = [self.name(self.arg(0))]
        elif name == "pick_edge":
            entry["args"] = [s32(self.arg(1)), self.name(self.arg(2)), self.name(self.arg(3)),
                             s32(self.arg(4)), s32(self.arg(5)), s32(self.arg(6))]
            entry["out"] = self.arg(0)
        elif name == "map_cell":
            entry["args"] = [list(struct.unpack("<hh", u.mem_read(self.arg(0), 4)))]
        self.calls.append(entry)
        self.pending_returns.append((caller, sp + 4 + pops, entry))

    def on_random_ranged(self, u, _address, _size, _data):
        if u.reg_read(UC_X86_REG_ECX) != RNG:
            return self.fail("RandomRanged on an RNG other than the Scenario's")
        entry = {"native": "random_ranged", "args": [s32(self.arg(0)), s32(self.arg(1))],
                 "site": f"0x{self.word(u.reg_read(UC_X86_REG_ESP)):08X}", "raw_draws": 0}
        self.calls.append(entry)
        self.draws.append(entry)

    def on_raw_draw(self, _u, _address, _size, _data):
        if not self.draws or "ret" in self.draws[-1]:
            return self.fail("raw RandomRanged draw outside a logged call")
        self.draws[-1]["raw_draws"] += 1

    def on_random_ranged_ret(self, u, _address, _size, _data):
        if not self.draws or "ret" in self.draws[-1]:
            return self.fail("RandomRanged return without a logged entry")
        self.draws[-1]["ret"] = s32(u.reg_read(UC_X86_REG_EAX))

    def on_body(self, u, address, _size, _data):
        if self.pending_returns:
            caller, esp, entry = self.pending_returns[-1]
            if address == caller and u.reg_read(UC_X86_REG_ESP) == esp:
                self.pending_returns.pop()
                eax = u.reg_read(UC_X86_REG_EAX)
                name = entry["native"]
                if name in ("strafe", "fighter", "house_edge"):
                    entry["ret"] = s32(eax)
                elif name in ("facing_set", "house_control"):
                    entry["ret"] = eax & 0xFF
                elif name in ("get_weapon", "map_cell"):
                    entry["ret"] = self.name(eax)
                elif name == "pick_edge":
                    entry["ret"] = list(struct.unpack("<hh", u.mem_read(entry.pop("out"), 4)))
        label = LANDMARKS.get(address)
        if label is not None and (not self.path or self.path[-1] != label):
            self.path.append(label)

    def on_write(self, u, _access, address, size, _value, _data):
        if not self.auditing:
            return
        end = address + size
        if STACK_BASE <= address and end <= STACK_BASE + STACK_SIZE:
            return
        if OWNER <= address and end <= OWNER + 0x1000:
            self.owner_writes.update(range(address - OWNER, end - OWNER))
            return
        if RNG <= address and end <= RNG + 0x3F4:
            return
        if HEAP[0] <= address and end <= HEAP[1]:
            return
        if address == PROBE_SLOT and size == 4 and self.returned is not None:
            return
        if DUMMY_CELL_COORDS <= address and end <= DUMMY_CELL_COORDS + 4:
            self.dummy_write = True
            return
        self.fail(f"write outside the stack/owner/RNG/heap at 0x{address:08X}+{size} "
                  f"from 0x{u.reg_read(UC_X86_REG_EIP):08X}")

    def on_read(self, u, _access, address, size, _value, _data):
        if self.auditing and not self.is_declared(address, size):
            self.undeclared_reads.add((address, size, u.reg_read(UC_X86_REG_EIP)))

    def on_return(self, u, _address, _size, _data):
        """At the caller-side return address: capture the visit's result before the probe."""
        self.returned = (u.reg_read(UC_X86_REG_ESP),
                         {register: u.reg_read(register) for register in SENTINELS},
                         u.reg_read(UC_X86_REG_EAX))

    # -- one visit
    def execute(self, row):
        row = {**DEFAULTS, **row}
        self.row = row
        self.calls, self.path, self.pending_returns, self.violations = [], [], [], []
        self.draws, self.allocations, self.frees = [], 0, 0
        self.owner_writes, self.undeclared_reads, self.dummy_write = set(), set(), False
        self.returned = None
        self.build(row)
        u = self.u
        u.mem_write(SP, u32(RETURN_PROBE))
        for register, value in SENTINELS.items():
            u.reg_write(register, value)
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_EDX, 0)
        u.reg_write(UC_X86_REG_ECX, OWNER)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
        self.auditing = True
        try:
            run_checked(u, MISSION_ATTACK, PROBE_END, count=500_000,
                        required_addresses=[RETURN_PROBE])
        except OracleError as error:
            raise OracleError(f"{row.get('name')}: {self.violations or error}") from error
        finally:
            self.auditing = False
        name = row.get("name")
        if self.violations:
            raise OracleError(f"{name}: {self.violations}")
        esp, registers, eax = self.returned
        if esp != SP + 4:
            raise OracleError(f"{name}: unbalanced stack at return")
        if registers != SENTINELS:
            raise OracleError(f"{name}: callee-saved register not restored")
        if self.pending_returns or any("ret" not in d for d in self.draws):
            raise OracleError(f"{name}: a logged native call never returned")
        if self.allocations != self.frees:
            raise OracleError(f"{name}: {self.allocations} allocations, {self.frees} frees")
        if self.undeclared_reads:
            reads = ", ".join(f"0x{a:08X}+{s}@0x{pc:08X}" for a, s, pc in sorted(self.undeclared_reads))
            raise OracleError(f"{name}: read of unsupplied fixture/BSS bytes: {reads}")
        out = {
            "delay": s32(eax),
            "state": i32(u.mem_read(OWNER + 0xBC, 4)),
            "pending": u.mem_read(OWNER + 0x6C8, 1)[0],
            "latch": u.mem_read(OWNER + 0x6D2, 1)[0],
            "ammo": i32(u.mem_read(OWNER + 0x2FC, 4)),
            "owner_writes": ranges(self.owner_writes),
            "path": list(self.path),
            "calls": list(self.calls),
            "next_random": self.word(PROBE_SLOT),
        }
        if row["state"] == 10:
            out["target_after"] = self.name(self.word(OWNER + 0x2B4))
            out["byte_6d5"] = u.mem_read(OWNER + 0x6D5, 1)[0]
            out["allocations"] = self.allocations
        if self.dummy_write:
            out["dummy_cell_coords"] = list(struct.unpack("<hh", u.mem_read(DUMMY_CELL_COORDS, 4)))
        return out


def ranges(offsets):
    """Merge written byte offsets into [hex offset, length] runs."""
    out = []
    for offset in sorted(offsets):
        if out and out[-1][0] + out[-1][1] == offset:
            out[-1][1] += 1
        else:
            out.append([offset, 1])
    return [[f"0x{o:03X}", n] for o, n in out]


# ---- pruning ----------------------------------------------------------------------------------
def product_form(combos, axes):
    """{axis: values} when the combos are exactly a product of per-axis value sets, else None."""
    form = {axis: [v for v in domain if any(c[axis] == v for c in combos)]
            for axis, domain in axes.items()}
    size = 1
    for values in form.values():
        size *= len(values)
    return form if size == len(combos) else None


def merge_forms(forms, axes):
    """Union product forms that differ in exactly one axis."""
    forms = [dict(form) for form in forms]
    changed = True
    while changed:
        changed = False
        for i in range(len(forms)):
            for j in range(i + 1, len(forms)):
                differing = [axis for axis in axes if forms[i][axis] != forms[j][axis]]
                if len(differing) == 1:
                    axis = differing[0]
                    values = set(forms[i][axis]) | set(forms[j][axis])
                    forms[i][axis] = [v for v in axes[axis] if v in values]
                    del forms[j]
                    changed = True
                    break
            if changed:
                break
    return forms


def decompose(combos, axes, depth=0):
    """The covered combos as a short union of product forms."""
    form = product_form(combos, axes)
    if form is not None:
        return [form]
    if depth >= 3:
        return merge_forms([{axis: [c[axis]] for axis in axes} for c in combos], axes)
    best = None
    for axis, domain in axes.items():
        parts = [[c for c in combos if c[axis] == value] for value in domain]
        forms = merge_forms([f for part in parts if part
                             for f in decompose(part, axes, depth + 1)], axes)
        if best is None or len(forms) < len(best):
            best = forms
    return best


def expand(forms, axes):
    out = []
    for form in forms:
        combos = [{}]
        for axis in axes:
            combos = [{**c, axis: v} for c in combos for v in form[axis]]
        out.extend(combos)
    return out


def canonical(combo):
    return json.dumps(combo, sort_keys=True)


def product_rows(fixture, axes, base, name):
    """Execute the whole product; keep one row per distinct normalized result with `covers`."""
    combos = [{}]
    for axis, domain in axes.items():
        combos = [{**c, axis: v} for c in combos for v in domain]
    groups = {}
    for combo in combos:
        row = {"name": name(combo), **base, **combo}
        out = fixture.execute(row)
        key = dict(out, ammo=out["ammo"] - row["ammo"])  # Ammo compared as after - before
        key = json.dumps(key, sort_keys=True)
        group = groups.setdefault(key, {"input": row, "out": out, "covered": []})
        group["covered"].append(combo)
    rows = []
    for group in groups.values():
        covers = decompose(group["covered"], axes)
        expanded = [canonical(c) for c in expand(covers, axes)]
        if sorted(expanded) != sorted(canonical(c) for c in group["covered"]):
            raise OracleError(f"{group['input']['name']}: covers do not reproduce its class")
        rows.append({"input": group["input"], "covers": covers, **group["out"]})
    return rows, len(combos)


# ---- families ---------------------------------------------------------------------------------
CODES = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, -1]
AXES = {  # per-state cross product; the first value of each axis is the representative default
    "code": CODES,
    "class": ["strafe", "fighter", "neither"],
    "ammo": [1, 0, -1],
    "target": [True, False],
    "curley": [1, 0],
    "is_close": [1, 0],
}
STATE4_CLASSES = AXES["class"] + ["strafe_fighter"]
PRESETS = {4: dict(pending=0, latch=0), 5: dict(pending=0, latch=0), 6: dict(pending=1, latch=1),
           7: dict(pending=1, latch=1), 8: dict(pending=1, latch=1), 9: dict(pending=1, latch=1),
           10: dict(latch=1)}
STATE10_AXES = {
    "pending": [1, 0],
    "ammo": [1, 0, -1, 2],
    "target": [True, False],
    "flag_3d4": [0, 1],
    "game_mode": [1, 0],
    "house_1ec": [0, 1],
    "house_1ed": [0, 1],
    "airstrike": [False, True],
    "edge": [0, 1, 2, 3, -1, 4],
}


def state_rows(fixture, state):
    axes = dict(AXES, **({"class": STATE4_CLASSES} if state == 4 else {}))

    def name(c):
        return (f"s{state}.c{c['code']}.{c['class']}.a{c['ammo']}.t{int(c['target'])}"
                f".cs{c['curley']}.ic{c['is_close']}")

    return product_rows(fixture, axes, {"state": state, **PRESETS[state]}, name)


def state10_rows(fixture):
    def name(c):
        return (f"s10.p{c['pending']}.a{c['ammo']}.t{int(c['target'])}.f{c['flag_3d4']}"
                f".g{c['game_mode']}.h{c['house_1ec']}{c['house_1ed']}.as{int(c['airstrike'])}"
                f".e{c['edge']}")

    return product_rows(fixture, STATE10_AXES, {"state": 10, **PRESETS[10]}, name)


def delay_rows(fixture):
    """State 9's (GetWeapon(0)->Range + 0x400) / AircraftType+0x678, signed CDQ/IDIV."""
    weapon_ranges = [-512, 0, 767, 768, 1280, 2304, 0x7FFFFBFF, 0x7FFFFFFF]
    speeds = [1, 30, 35, 255, -1]
    rows, faults = [], []
    for weapon_range in weapon_ranges:
        for speed in speeds:
            row = {"name": f"s9.delay.r{weapon_range}.v{speed}", "state": 9, "class": "strafe",
                   **PRESETS[9], "range": [weapon_range, 768], "speed": speed}
            rows.append({"input": row, **fixture.execute(row)})
    for weapon_range, speed in [(r, 0) for r in weapon_ranges] + [(0x7FFFFC00, -1)]:
        row = {"name": f"s9.delay_fault.r{weapon_range}.v{speed}", "state": 9, "class": "strafe",
               **PRESETS[9], "range": [weapon_range, 768], "speed": speed}
        try:
            fixture.execute(row)
        except OracleError as error:
            pc = fixture.u.reg_read(UC_X86_REG_EIP)
            if pc != 0x418BB4:
                raise OracleError(f"{row['name']}: faulted at 0x{pc:08X}, not the IDIV") from error
            faults.append({"input": row, "fault_at": f"0x{pc:08X}", "error": "divide error (#DE)"})
            continue
        raise OracleError(f"{row['name']}: expected the IDIV at 0x00418BB4 to fault")
    return rows, faults


def extra_rows(fixture):
    """Targeted rows outside the products: selected weapon 1, a returned bullet, burst counts and
    the target's cell (sub-cell and off-map coordinates)."""
    rows = []

    def add(name, **row):
        full = {"name": name, **PRESETS[row["state"]], **row}
        rows.append({"input": full, **fixture.execute(full)})

    weapon1 = dict(select=1, burst=[1, 2], rof=[20, 37], range=[1280, 768])
    for cls in ("strafe", "fighter", "neither"):
        add(f"select1.s4.{cls}", state=4, code=0, **{"class": cls}, **weapon1)
    for state in (5, 6, 7, 8, 9):
        add(f"select1.s{state}", state=state, code=0, **{"class": "strafe"}, **weapon1)
    add("select1.s6.code8", state=6, code=8, **{"class": "strafe"}, **weapon1)
    for state in (4, 5, 6, 7, 8, 9):
        add(f"bullet.s{state}", state=state, code=0, bullet=True, burst=[2, 1],
            **{"class": "strafe" if state != 5 else "fighter"})
    add("burst0.s4.strafe", state=4, code=0, burst=[0, 1], **{"class": "strafe"})
    add("burst_negative.s4.neither", state=4, code=0, burst=[-1, 1], **{"class": "neither"})
    add("burst3.s4.fighter", state=4, code=0, burst=[3, 1], **{"class": "fighter"})
    add("target_subcell.s6", state=6, code=0, target_xyz=[16700, 16300, 200], **{"class": "strafe"})
    add("target_offmap.s6", state=6, code=0, target_xyz=[-300, -40, 0], **{"class": "strafe"})
    add("target_offmap.s4", state=4, code=0, target_xyz=[-300, -40, 0], **{"class": "fighter"})
    return rows


def epilogue_rows(fixture):
    """State 5 code 3 is the bare epilogue: ftol(MissionControl[Mission].Rate * 900) + RandomRanged(0, 2)."""
    rows = []
    for seed in range(1, 13):
        row = {"name": f"epilogue.seed{seed}", "state": 5, "code": 3, **PRESETS[5], "seed": seed}
        rows.append({"input": row, **fixture.execute(row)})
    for rate in (0.0, 0.001, 0.05, 0.1, 0.0333, 1.0):
        row = {"name": f"epilogue.rate{rate}", "state": 5, "code": 3, **PRESETS[5], "rate": rate}
        rows.append({"input": row, **fixture.execute(row)})
    row = {"name": "epilogue.mission5", "state": 5, "code": 3, **PRESETS[5], "mission": 5,
           "rate": 0.05}
    rows.append({"input": row, **fixture.execute(row)})
    return rows


def state10_seed_rows(fixture):
    """The edge scan's draws over several seeds, every edge (Ammo 0, human house)."""
    rows = []
    for edge in (0, 1, 2, 3):
        for seed in (1, 2, 3, 4, 5):
            row = {"name": f"s10.edge{edge}.seed{seed}", "state": 10, **PRESETS[10], "ammo": 0,
                   "pending": 0, "house_1ec": 1, "edge": edge, "seed": seed}
            rows.append({"input": row, **fixture.execute(row)})
    return rows


RELEASE_JSON = Path(__file__).with_name("aircraft_attack_release.json")


def release_crosscheck(fixture):
    """Every non-elite row of aircraft_attack_release.json, re-run as a whole state-4 visit
    (GetFireError stubbed to 0): the same state, pending, latch, delay and loop counts."""
    reference = json.loads(RELEASE_JSON.read_text(encoding="utf-8"))["releases"]
    checked = 0
    for ref in reference:
        case = ref["input"]
        if "veterancy" in case:
            continue                                        # elite rows: not supplied here
        row = {"name": f"release.{checked}", "state": 4, "code": 0, **PRESETS[4],
               "ammo": case["ammo"], "burst": [case["burst"], 1], "rof": [20, 37],
               "class": dict(rot=case["rot"], inviso=int(case["inviso"]), fighter=int(case["fighter"]))}
        out = fixture.execute(row)
        mine = dict(state=out["state"], pending=bool(out["pending"]), latch_6d2=bool(out["latch"]),
                    delay=out["delay"],
                    fires=sum(c.get("call") == "fire_at" for c in out["calls"]),
                    selects=sum(c.get("call") == "select_weapon" for c in out["calls"]) - 1)
        theirs = dict(state=ref["state"], pending=ref["pending"], latch_6d2=ref["latch_6d2"],
                      delay=ref["delay"], fires=sum(e["call"] == "fire" for e in ref["events"]),
                      selects=sum(e["call"] == "select" for e in ref["events"]))
        if mine != theirs:
            raise OracleError(f"release cross-check {case}: {mine} != {theirs}")
        checked += 1
    return checked


def generate():
    fixture = Fixture()
    payload = {"defaults": DEFAULTS, "axes": {**AXES, "state4_class": STATE4_CLASSES},
               "state10_axes": STATE10_AXES, "presets": {str(k): v for k, v in PRESETS.items()},
               "map": {"width": MAP_WIDTH, "local_size": list(LOCAL_SIZE), "cells": [SIDE, SIDE],
                       "owner_cell": list(OWNER_CELL), "target_cell": list(TARGET_CELL)},
               "summary": {}}
    rows = []
    for state in (4, 5, 6, 7, 8, 9):
        kept, executed = state_rows(fixture, state)
        rows.extend(kept)
        payload["summary"][f"state{state}"] = {"executed": executed, "rows": len(kept)}
    payload["states"] = rows
    payload["state10"], executed = state10_rows(fixture)
    payload["summary"]["state10"] = {"executed": executed, "rows": len(payload["state10"])}
    payload["state10_seeds"] = state10_seed_rows(fixture)
    payload["state9_delay"], payload["state9_delay_faults"] = delay_rows(fixture)
    payload["extras"] = extra_rows(fixture)
    payload["epilogue"] = epilogue_rows(fixture)
    for family in ("state10_seeds", "state9_delay", "state9_delay_faults", "extras", "epilogue"):
        payload["summary"][family] = {"rows": len(payload[family])}
    payload["release_crosscheck_rows"] = release_crosscheck(fixture)
    return payload


def metadata():
    return provenance(
        scope="AircraftClass::Mission_Attack 0x00417FE0 visits in states 4..10 on a supplied "
              "Aircraft: per state 4..9 the full product code {0..11, -1} x class {strafe, "
              "fighter, neither (+ strafe_fighter in state 4)} x Ammo {1, 0, -1} x Target {set, "
              "null} x CurleyShuffle {1, 0} x IsClose {1, 0}, and for state 10 pending x Ammo "
              "{1, 0, -1, 2} x Target x +0x3D4 x [0x00A8B238] x House+0x1EC/+0x1ED x +0x294 x "
              "House+0x577C {0..3, -1, 4}, executed in full and kept once per distinct result "
              "(`covers`); plus state-9 delay divisions, faulting divisors, the bare epilogue over "
              "seeds/Rates/mission, state-10 edge draws over seeds, selected-weapon, bullet, burst "
              "and target-cell rows, and a re-run of the 300 non-elite aircraft_attack_release "
              "rows. Returned delay, Mission+0xBC, +0x6C8, +0x6D2, Ammo, owner write set, path, "
              "ordered call log and Scenario RNG continuation. Not FireAt, GetFireError, IsClose, "
              "Scatter recipients, AssignDestination, Uncloak, Enter_Idle_Mode or Queue_Mission "
              "behaviour, and not frame-to-frame sortie timing.",
        assumptions=[
            "One emulator; per row the object region (0x10000 bytes of scratch), the stack frame "
            "and the declared globals are rewritten; the flat map (cell table and 128 x 128 "
            "cells) is written once and the write hook proves no row modifies it.",
            "Owner Aircraft at cell (60, 64) z 500 (+0x9C), Mission +0xAC = 1, veterancy 0.0, "
            "House +0x21C, no SpawnManager (+0x2D0 = 0) or fire particles (+0x304 = 0); both "
            "FacingClass fields desired 0, timer -1, ROT 0 (Set is immediate); IFlyControl +0x6C0 "
            "= original 0x007E2250; type +0x6C4 with Type+0x678 (default 30) and Fighter +0xE0E.",
            "Target: a Unit on the original vtable 0x007F5C70 at cell (64, 64) z 0, only +0x9C "
            "supplied (read by GetCoords 0x005F65A0).",
            "Two weapon slots (Type+0x898/+0x8B4), no elite slots: Burst +0x9C, Projectile +0xA0, "
            "ROF +0xB0 (20 / 37), Range +0xB4 (1280 / 768); projectile ROT +0x2DC and Inviso "
            "+0x29E per class (strafe ROT 1, others ROT 100; slot 1 always ROT 100).",
            "Rules [0x008871E0]+0x17E1 CurleyShuffle per row; Frame [0x00A8ED84] 1000; "
            "MissionControl [0x00A8E3A8]: entry 1 Rate = f32(0.016) widened to double (retail "
            "[Attack] Rate=.016), entry Mission = the row's Rate; all other entries 0.",
            "Scenario [0x00A8B230] with its RNG at +0x218 filled by the original seeder "
            "0x0065C6D0 (seed 31 unless stated); FPCW 0x0E7F equals ftol's [0x00822D80].",
            "Map 0x0087F7E8: cell table +0x13C (0x40000 entries, count +0x140) over 128 x 128 "
            "flat cells (all bytes 0 except MapCoords +0x24), MapSize width +0xF4 = 64, "
            "LocalSize +0xFC..+0x108 = (1, 4, 62, 50); [0x00889E68] and [0x008A03F8] set by "
            "their original static initializers 0x00413C60 and 0x004A85E0 (both (0, 0)).",
            "State 10: House +0x1EC, +0x1ED, +0x577C and [0x00A8B238] per row; AirstrikeClass "
            "behind +0x294 has +0x4C != owner, so SetTarget's 0x0041DB40 call is not reached.",
            "Excluded because they fault natively (verified in state9_delay_faults): "
            "AircraftType+0x678 = 0 and (Range 0x7FFFFC00, Type+0x678 = -1), divide error at "
            "the IDIV 0x00418BB4.",
            "Per row: RET to the probe with ESP = entry + 4 and EBX/EBP/ESI/EDI preserved; no "
            "write outside stack/owner/RNG/heap (dummy-cell coordinates reported); every read "
            "of scratch or BSS bytes falls on supplied bytes.",
        ],
        substitutions=[
            "Cloned Aircraft vtable slots return the row's value, logged with arguments: "
            "vt+0x2E4 select_weapon, vt+0x3AC is_close, vt+0x3C0 fire_error, vt+0x3CC fire_at "
            "(null or an opaque bullet), vt+0x45C uncloak, vt+0x480 assign_dest, vt+0x1E8 "
            "queue_mission, vt+0x484 enter_idle (the last three return 0).",
            "CellClass 0x00481670 entry hook: logs (cell, source coords, a2, a3, a4) and returns "
            "RET 0x10 without running the body (recipients' Scatter and their RNG excluded).",
            "operator new 0x007C8E17 / operator delete 0x007C8B3D entry hooks: scratch bump "
            "allocation / no-op (state-10 South edge candidate vector only; allocations must "
            "equal frees).",
            "Observation probe at RET_MAGIC+0x200 (outside native memory): the visit returns "
            "into MOV ECX,rng / CALL 0x0065C780 / MOV [slot],EAX to read the RNG continuation.",
        ],
        entry_points={"mission_attack": MISSION_ATTACK, "strafe": 0x41B7F0, "fighter": 0x41B840,
                      "get_weapon": 0x70E140, "dir_to": 0x5F3DB0, "facing_set": 0x4C9220,
                      "get_cell": 0x565730, "mission_control": 0x5B3A00, "ftol": 0x7C5F00,
                      "random_ranged": RANDOM_RANGED, "random": RANDOM, "rng_seed": RNG_SEED,
                      "house_control": 0x50B730, "house_edge": 0x50DA80, "set_target": 0x6FCDB0,
                      "pick_cell_on_edge": 0x4AA440, "map_cell": 0x5657A0,
                      "edge_reference_initializer": 0x413C60, "edge_sentinel_initializer": 0x4A85E0})


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--probe":
        fixture = Fixture()
        for probe in json.loads(sys.argv[2]):
            print(json.dumps({"input": probe, **fixture.execute(probe)}))
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
