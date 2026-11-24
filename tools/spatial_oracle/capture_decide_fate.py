"""Original CaptureManagerClass::DecideUnitFate 0x004723B0 with supplied queries.

The original reason selection (Available_Money against AICaptureLowMoneyMark,
HouseClass::GetPowerRatio 0x004FCE30 on the controller's house, the float
Health/Strength ratio against AICaptureWoundedMark), the Scenario
RandomRanged(1, 100) draw, the choice-table walk through the original
DynamicVectorClass<int> vtable and the final Queue_Mission call execute. The
unit's type, QueueMission and the grinder/absorber seeks are scratch stubs; the
unit and controller are not Foot, so the Team arms do not run.

Rust consumer: src/sim/capture_manager_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

REGION = 0x50000
(MGR, CTRL, UNIT, UVT, TYPE, UHOUSE, CHOUSE, IHVT, RULES, SCENARIO, TABLES,
 CALLBACKS) = [SCRATCH + n * 0x6000 for n in range(12)]
SP = STACK_BASE + STACK_SIZE - 0x1000
DVC_INT_VTABLE = 0x7E4DD8
ENTRY = 0x4723B0
# Rules offsets of the four tables in reason order (LowMoney, LowPower,
# Wounded, Normal) and of the two marks.
TABLE_OFFSETS = (0xEA0, 0xE84, 0xE68, 0xE4C)
REASONS = ("low_money", "low_power", "wounded", "normal")
STOCK_TABLES = ([15, 75, 5, 5], [15, 5, 75, 5], [15, 40, 40, 5], [75, 5, 5, 15])


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, REGION)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.get_item = self.word(DVC_INT_VTABLE + 0x18)
        self.events = []
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == CALLBACKS + 0x100:
            self.events.append(dict(queue_mission=[self.word(sp + 4), self.word(sp + 8)]))
        elif pc == CALLBACKS + 0x200:
            self.events.append(dict(grinder=True))
        elif pc == CALLBACKS + 0x300:
            self.events.append(dict(absorber=True))
        elif pc == self.get_item and not self.events_have_table():
            this = u.reg_read(UC_X86_REG_ECX)
            for offset, reason in zip(TABLE_OFFSETS, REASONS):
                if this == RULES + offset:
                    self.events.append(dict(table=reason))
        elif pc == 0x65C7E0:
            self.events.append(dict(random=[self.word(sp + 4), self.word(sp + 8)]))

    def events_have_table(self):
        return any("table" in event for event in self.events)

    def stub(self, slot, body):
        address = CALLBACKS + slot * 0x100
        self.u.mem_write(address, body)
        return address

    def call(self, address, ecx, args=()):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(u, address, RET_MAGIC, count=200000)
        return struct.unpack("<i", dwords(u.reg_read(UC_X86_REG_EAX)))[0]

    def execute(self, row):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        # The manager's owner (the controller) and both houses.
        u.mem_write(MGR + 0x48, dwords(CTRL))
        u.mem_write(CTRL + 0x21C, dwords(CHOUSE))
        u.mem_write(UNIT, dwords(UVT))
        u.mem_write(UNIT + 0x21C, dwords(UHOUSE))
        u.mem_write(UNIT + 0x6C, dwords(row["health"]))
        u.mem_write(TYPE + 0xA0, dwords(row["strength"]))
        u.mem_write(UHOUSE + 0x1EC, bytes([row["unit_house_human"]]))
        u.mem_write(CHOUSE + 0x24, dwords(IHVT))
        u.mem_write(CHOUSE + 0x53A4, dwords(row["produced"]))
        u.mem_write(CHOUSE + 0x53A8, dwords(row["drained"]))
        # Unit virtuals: GetTechnoType (+84), Queue_Mission (+1E8, RET 8),
        # the grinder (+33C) and absorber (+340) seeks, which find nothing.
        u.mem_write(UVT + 0x84, dwords(self.stub(0, b"\xB8" + dwords(TYPE) + b"\xC3")))
        u.mem_write(UVT + 0x1E8, dwords(self.stub(1, b"\x31\xC0\xC2\x08\x00")))
        u.mem_write(UVT + 0x33C, dwords(self.stub(2, b"\x31\xC0\xC3")))
        u.mem_write(UVT + 0x340, dwords(self.stub(3, b"\x31\xC0\xC3")))
        # IHouse::Available_Money (COM this on the stack).
        u.mem_write(IHVT + 0x18, dwords(self.stub(4, b"\xB8" + dwords(row["money"]) + b"\xC2\x04\x00")))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(RULES + 0xEBC, dwords(row["money_mark"]))
        u.mem_write(RULES + 0xEC0, struct.pack("<f", row["wounded_mark"]))
        items = TABLES
        for offset, table in zip(TABLE_OFFSETS, row["tables"]):
            u.mem_write(RULES + offset, dwords(DVC_INT_VTABLE, items, len(table), 0, len(table), 10))
            u.mem_write(items, dwords(*table) if table else b"")
            items += 0x100
        u.mem_write(0xA8B230, dwords(SCENARIO))
        self.call(0x65C6D0, SCENARIO + 0x218, [row["seed"]])
        self.events = []
        self.call(ENTRY, MGR, [UNIT])
        events = self.events
        self.events = []
        next_random = self.call(0x65C780, SCENARIO + 0x218) & 0xFFFFFFFF
        return dict(input=row, events=events, next_random=next_random)


def inputs():
    base = dict(health=125, strength=125, unit_house_human=0, money=5000, money_mark=2000,
                wounded_mark=0.25, produced=100, drained=100, tables=list(STOCK_TABLES), seed=7)
    rows = [dict(base, unit_house_human=1)]
    # Each reason through the stock tables, with enough seeds to reach every choice.
    for seed in range(1, 25):
        rows.append(dict(base, seed=seed))
        rows.append(dict(base, seed=seed, money=1999))
        rows.append(dict(base, seed=seed, produced=50))
        rows.append(dict(base, seed=seed, health=31))
    # Reason boundaries.
    for money in (-1, 0, 1999, 2000, 2001):
        rows.append(dict(base, money=money))
    for produced, drained in ((0, 0), (0, 100), (100, 0), (99, 100), (100, 99), (-5, 10),
                              (-10, -5), (-5, -10), (0, -5), (7, 7)):
        rows.append(dict(base, produced=produced, drained=drained))
    for health, strength in ((31, 125), (32, 128), (33, 128), (0, 125), (1, 4), (1, 3),
                             (125, 125), (5, 21), (5, 0), (0, 0)):
        rows.append(dict(base, health=health, strength=strength))
    for mark in (0.3, 0.1, 0.5, 1.0):
        rows.append(dict(base, health=40, strength=125, wounded_mark=mark))
    # Walk edges: Do Nothing, an exhausted table, choices 4 and 6, a seventh
    # choice cut off, and a negative weight (signed sum).
    custom = [dict(tables=[[0, 0, 0, 0, 100]] * 4), dict(tables=[[10, 10]] * 4),
              dict(tables=[[1, 1, 1, 1, 1, 1, 94]] * 4), dict(tables=[[100]] * 4),
              dict(tables=[[0, 100]] * 4), dict(tables=[[0, 0, 100]] * 4), dict(tables=[[]] * 4),
              dict(tables=[[0, 0, 0, 100]] * 4), dict(tables=[[0, 0, 0, 0, 0, 100]] * 4),
              dict(tables=[[-50, 100]] * 4)]
    for seed in (3, 11, 29):
        for table in custom:
            rows.append(dict(base, seed=seed, **table))
    return rows


def generate():
    fixture = Fixture()
    return [fixture.execute(row) for row in inputs()]


def metadata():
    return provenance(
        scope="Original 0x004723B0 DecideUnitFate reason selection, Scenario RandomRanged(1,100), choice walk and Queue_Mission call",
        assumptions=[
            "Unit and controller are not Foot (flags+14 bit 2 clear), so the Team removal, Add To Team and TeamType override arms do not run",
            "Unit has no Temporal (+274) and its type is not OpenTopped (+5E4)",
            "Original GetPowerRatio 0x004FCE30 reads the controller house +53A4/+53A8; original DVC<int> vtable 0x007E4DD8 reads the tables",
            "Scenario RNG seeded through the original 0x0065C6D0; next_random from the original 0x0065C780",
        ],
        substitutions=[
            "Scratch stubs: unit GetTechnoType (+84), Queue_Mission (+1E8, recorded), grinder (+33C) and absorber (+340) seeks returning 0, IHouse Available_Money (+18) returning the supplied money",
        ],
        entry_points={"decide_unit_fate": ENTRY, "power_ratio": 0x4FCE30, "random_ranged": 0x65C7E0})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
