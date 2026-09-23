"""Original HouseClass::Add_Tracking 0x004FF700 and Remove_Tracking 0x004FF550.

Each case builds one Techno of a fake type and calls the original function on a
house once. The routing into the house's plain counters executes: +0x2E8
(units, and unit-like buildings), +0x2EC (naval units with no Passengers=),
+0x2F0 (buildings, the short-game defeat count), +0x2F4 (infantry, latched by
the Techno's +0x438), +0x2F8 (aircraft), the Techno's tracked flag +0x3CC and
the infantry latch. The per-type counters (IndexClass Increment 0x0049FA00 /
Decrement 0x0049FA70 on house +0x5500 building, +0x5514 unit, +0x5528
infantry, +0x553C aircraft) and the owned-type set 0x00749020 are recorded at
entry and not run; so are the virtuals GetTechnoType (+0x84), WhatAmI (+0x2C),
the building pre-check (+0x80) and the type's ArrayIndex (+0x40).

Rust consumer: src/sim/house_tracking_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_FPCW

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

REGION = 0x10000
SP = STACK_BASE + STACK_SIZE - 0x1000
ADD_TRACKING, REMOVE_TRACKING = 0x4FF700, 0x4FF550
HOUSE = SCRATCH                     # counters up to +0x5600
TECHNO = SCRATCH + 0x6000
TECHNO_VT = SCRATCH + 0x7000
TYPE = SCRATCH + 0x8000
TYPE_VT = SCRATCH + 0x9800
UNDEPLOYS = SCRATCH + 0xA000
STUBS = SCRATCH + 0xC000
VIRTUALS = {0x84: "get_type", 0x2C: "what_am_i", 0x80: "precheck"}
TYPE_VIRTUALS = {0x40: "array_index"}
NATIVES = {0x49FA00: "increment", 0x49FA70: "decrement", 0x749020: "owned_type"}
COUNTERS = {0x5500: "buildings", 0x5514: "units", 0x5528: "infantry", 0x553C: "aircraft",
            0x1B40: "building_types", 0x1338: "unit_types", 0xB30: "infantry_types",
            0x328: "aircraft_types"}
PLAIN = {"units": 0x2E8, "naval": 0x2EC, "buildings": 0x2F0, "infantry": 0x2F4,
         "aircraft": 0x2F8}
TYPE_INDEX = 7


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, REGION)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def signed(self, address):
        return struct.unpack("<i", self.u.mem_read(address, 4))[0]

    def ret(self, value, cleaned):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        self.u.reg_write(UC_X86_REG_EIP, self.word(sp))
        self.u.reg_write(UC_X86_REG_ESP, sp + 4 + cleaned)

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if STUBS <= pc < STUBS + 0x100:
            name = VIRTUALS[pc - STUBS]
            if name == "get_type":
                self.ret(TYPE, 0)
            elif name == "what_am_i":
                self.ret(self.case["rtti"], 0)
            else:
                self.ret(self.case.get("precheck", 0), 0)
        elif STUBS + 0x100 <= pc < STUBS + 0x200:
            self.ret(TYPE_INDEX, 0)
        elif pc in NATIVES:
            index = self.word(sp + 4)
            counter = COUNTERS.get(this - HOUSE, hex(this - HOUSE))
            self.events.append([NATIVES[pc], counter, index])
            self.ret(0, 4)

    def execute(self, case):
        u = self.u
        self.case = case
        self.events = []
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        for offset in VIRTUALS:
            u.mem_write(STUBS + offset, b"\xCC")
            u.mem_write(TECHNO_VT + offset, dwords(STUBS + offset))
        u.mem_write(STUBS + 0x100 + 0x40, b"\xCC")
        u.mem_write(TYPE_VT + 0x40, dwords(STUBS + 0x100 + 0x40))
        u.mem_write(TECHNO, dwords(TECHNO_VT))
        u.mem_write(TYPE, dwords(TYPE_VT))
        u.mem_write(TYPE + 0x232, bytes([case.get("insignificant", 0)]))
        u.mem_write(TYPE + 0xC9F, bytes([case.get("dont_score", 0)]))
        u.mem_write(TYPE + 0xCCE, bytes([case.get("naval", 0)]))
        u.mem_write(TYPE + 0x5E0, struct.pack("<i", case.get("passengers", 0)))
        # A building's type pointer (+0x520) and its UndeploysInto (+0x408).
        u.mem_write(TECHNO + 0x520, dwords(TYPE))
        if "undeploys_into" in case:
            u.mem_write(TYPE + 0x408, dwords(UNDEPLOYS))
            u.mem_write(UNDEPLOYS + 0x5EC, bytes([case["undeploys_into"].get("resource_gatherer", 0)]))
        u.mem_write(TECHNO + 0x438, bytes([case.get("latched", 0)]))
        u.mem_write(TECHNO + 0x439, bytes([case.get("byte_439", 0)]))
        for name, offset in PLAIN.items():
            u.mem_write(HOUSE + offset, struct.pack("<i", case.get("start", 0)))
        u.mem_write(SP, dwords(RET_MAGIC) + dwords(TECHNO))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, HOUSE)
        entry = ADD_TRACKING if case["call"] == "add" else REMOVE_TRACKING
        run_checked(u, entry, RET_MAGIC, count=50_000)
        counts = {name: self.signed(HOUSE + offset) - case.get("start", 0)
                  for name, offset in PLAIN.items()}
        return dict(input=case, events=self.events, counts=counts,
                    tracked=u.mem_read(TECHNO + 0x3CC, 1)[0],
                    latched=u.mem_read(TECHNO + 0x438, 1)[0])


def inputs():
    cases = []
    kinds = [
        dict(name="unit", rtti=1),
        dict(name="unit_insignificant", rtti=1, insignificant=1),
        dict(name="unit_dont_score", rtti=1, dont_score=1),
        dict(name="naval_no_passengers", rtti=1, naval=1),
        dict(name="naval_transport", rtti=1, naval=1, passengers=3),
        dict(name="aircraft", rtti=2),
        dict(name="aircraft_dont_score", rtti=2, dont_score=1),
        dict(name="building", rtti=6),
        dict(name="building_insignificant", rtti=6, insignificant=1),
        dict(name="building_dont_score", rtti=6, dont_score=1),
        dict(name="building_1x1_undeployer", rtti=6, precheck=1),
        dict(name="building_undeploys_to_gatherer", rtti=6, undeploys_into=dict(resource_gatherer=1)),
        dict(name="building_undeploys_to_other", rtti=6, undeploys_into=dict(resource_gatherer=0)),
        dict(name="infantry", rtti=15),
        dict(name="infantry_latched", rtti=15, latched=1),
        dict(name="infantry_byte_439", rtti=15, byte_439=1),
        dict(name="infantry_dont_score", rtti=15, dont_score=1),
        dict(name="other_rtti", rtti=3),
    ]
    for call in ("add", "remove"):
        for kind in kinds:
            cases.append(dict(kind, name=f"{call}_{kind['name']}", call=call, start=5))
    return cases


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original 0x004FF700 HouseClass::Add_Tracking and 0x004FF550 HouseClass::Remove_Tracking",
        assumptions=[
            "House +2E8 units, +2EC naval without passengers, +2F0 buildings, +2F4 infantry, +2F8 aircraft; per-type counters +5500/+5514/+5528/+553C; owned-type sets +1B40/+1338/+B30/+328",
            "Techno +3CC tracked, +438 infantry latch, +439, +520 building type; TechnoType +232 Insignificant, +C9F DontScore, +CCE Naval, +5E0 Passengers, +408 UndeploysInto, +5EC ResourceGatherer",
        ],
        substitutions=[
            "Recorded at entry and returned without running: GetTechnoType (+84), WhatAmI (+2C), the building pre-check (+80, the case's value), the type's ArrayIndex (+40, 7), IndexClass Increment 0x0049FA00 / Decrement 0x0049FA70, the owned-type set 0x00749020",
        ],
        entry_points={"add_tracking": ADD_TRACKING, "remove_tracking": REMOVE_TRACKING})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
