"""Original HouseClass::Update multiplayer defeat gate 0x004F8E86..0x004F8F87.

Each case runs the gate block once for one house, entered with ESI = house
and EBP = 0 as HouseClass::Update holds them there. The block's reads execute:
GameMode [0xA8B238], the Defeated byte +0x1F5, the frame [0xA8ED84],
HouseType MultiplayPassive (+0x34 -> +0x1A6), ShortGame [0xA8B262], the
building count +0x2F0, the unit type counter +0x5514 at BaseUnit[1], [2], [0]
(Rules +0xB24, type +0xDF8) through CounterClass::GetItemCount 0x0049FAE0, and
in a normal game the active unit, infantry and aircraft totals (+0x5564,
+0x5578, +0x558C through IndexClass::GetTotal 0x0049FB60) plus the active
building counter +0x5550 at BuildRefinery[2] (Rules +0x8E8). Blowup_All
0x004FC6D0 and MPlayer_Defeated 0x004FC0B0 are recorded at entry and not run.

Rust consumer: src/sim/world/house_defeat_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP,
                               UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_FPCW)

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

REGION = 0x10000
SP = STACK_BASE + STACK_SIZE - 0x1000
GATE_BEGIN, GATE_END = 0x4F8E86, 0x4F8F87
BLOWUP_ALL, MPLAYER_DEFEATED = 0x4FC6D0, 0x4FC0B0
GAME_MODE, FRAME, SHORT_GAME, RULES_PTR = 0xA8B238, 0xA8ED84, 0xA8B262, 0x8871E0
HOUSE = SCRATCH
HOUSE_TYPE = SCRATCH + 0x6000
RULES = SCRATCH + 0x7000
BASE_UNITS = SCRATCH + 0x8000       # three UnitType pointers
BUILD_REFINERY = SCRATCH + 0x8100   # three BuildingType pointers
TYPES = SCRATCH + 0x9000            # type n at TYPES + n * 0x1000 (only +0xDF8 used)
COUNTER_ITEMS = SCRATCH + 0xE000    # items for the two per-type counters
BASE_INDICES = (3, 4, 5)            # ArrayIndex of AMCV, SMCV, PCV
YAREFN_INDEX = 9


def type_at(n):
    return TYPES + n * 0x400


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

    def observe(self, u, pc, _size, _data):
        if pc in (BLOWUP_ALL, MPLAYER_DEFEATED):
            self.events.append("blowup_all" if pc == BLOWUP_ALL else "mplayer_defeated")
            sp = u.reg_read(UC_X86_REG_ESP)
            u.reg_write(UC_X86_REG_EIP, self.word(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 4)

    def counter(self, offset, items, values, total=0):
        """A CounterClass at house + offset: vtable, items, capacity 16, total at +0x10."""
        u = self.u
        u.mem_write(HOUSE + offset + 4, dwords(items))
        u.mem_write(HOUSE + offset + 8, dwords(16))
        u.mem_write(HOUSE + offset + 0x10, struct.pack("<i", total))
        for index, value in values.items():
            u.mem_write(items + 4 * index, struct.pack("<i", value))

    def execute(self, case):
        u = self.u
        self.events = []
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.mem_write(GAME_MODE, dwords(case.get("game_mode", 1)))
        u.mem_write(FRAME, struct.pack("<i", case.get("frame", 100)))
        u.mem_write(SHORT_GAME, bytes([case.get("short_game", 1)]))
        u.mem_write(RULES_PTR, dwords(RULES))
        u.mem_write(RULES + 0xB24, dwords(BASE_UNITS))
        u.mem_write(RULES + 0x8E8, dwords(BUILD_REFINERY))
        for slot, index in enumerate(BASE_INDICES):
            u.mem_write(BASE_UNITS + 4 * slot, dwords(type_at(slot)))
            u.mem_write(type_at(slot) + 0xDF8, dwords(index))
        refinery = case.get("build_refinery_2", True)
        u.mem_write(BUILD_REFINERY + 8, dwords(type_at(3) if refinery else 0))
        u.mem_write(type_at(3) + 0xDF8, dwords(YAREFN_INDEX))
        u.mem_write(HOUSE + 0x34, dwords(HOUSE_TYPE))
        u.mem_write(HOUSE_TYPE + 0x1A6, bytes([case.get("passive", 0)]))
        u.mem_write(HOUSE + 0x1F5, bytes([case.get("defeated", 0)]))
        u.mem_write(HOUSE + 0x2F0, struct.pack("<i", case.get("buildings", 0)))
        base = case.get("base_units", {})
        self.counter(0x5514, COUNTER_ITEMS,
                     {BASE_INDICES[int(slot)]: n for slot, n in base.items()})
        self.counter(0x5550, COUNTER_ITEMS + 0x100,
                     {YAREFN_INDEX: case.get("active_yarefn", 0)})
        self.counter(0x5564, COUNTER_ITEMS + 0x200, {}, case.get("active_units", 0))
        self.counter(0x5578, COUNTER_ITEMS + 0x300, {}, case.get("active_infantry", 0))
        self.counter(0x558C, COUNTER_ITEMS + 0x400, {}, case.get("active_aircraft", 0))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, HOUSE)
        u.reg_write(UC_X86_REG_EBP, 0)
        u.reg_write(UC_X86_REG_ECX, 0)
        run_checked(u, GATE_BEGIN, GATE_END, count=10_000)
        return dict(input=case, events=self.events)


def inputs():
    return [
        dict(name="short_no_base"),
        dict(name="short_building", buildings=1),
        dict(name="short_negative_buildings", buildings=-1),
        dict(name="short_mcv_slot0", base_units={"0": 1}),
        dict(name="short_mcv_slot1", base_units={"1": 1}),
        dict(name="short_mcv_slot2", base_units={"2": 2}),
        dict(name="short_mcv_cancelling", base_units={"0": 1, "1": -1}),
        dict(name="short_ignores_units", active_units=5, active_infantry=3),
        dict(name="campaign", game_mode=0),
        dict(name="already_defeated", defeated=1),
        dict(name="frame_zero", frame=0),
        dict(name="passive_house", passive=1),
        dict(name="normal_empty", short_game=0),
        dict(name="normal_building", short_game=0, buildings=1),
        dict(name="normal_units", short_game=0, active_units=1),
        dict(name="normal_infantry", short_game=0, active_infantry=1),
        dict(name="normal_aircraft", short_game=0, active_aircraft=1),
        dict(name="normal_yarefn", short_game=0, active_yarefn=1),
        dict(name="normal_no_build_refinery_2", short_game=0, build_refinery_2=False, active_yarefn=1),
        dict(name="normal_cancelling_totals", short_game=0, active_units=-1, active_infantry=1),
        dict(name="normal_ignores_base_units", short_game=0, base_units={"0": 1}),
    ]


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original HouseClass::Update defeat gate 0x004F8E86..0x004F8F87 with CounterClass::GetItemCount 0x0049FAE0 and IndexClass::GetTotal 0x0049FB60",
        assumptions=[
            "GameMode [0xA8B238], frame [0xA8ED84], ShortGame [0xA8B262], Rules [0x8871E0] +B24 BaseUnit items, +8E8 BuildRefinery items, type +DF8 ArrayIndex",
            "House +34 HouseType (+1A6 MultiplayPassive), +1F5 Defeated, +2F0 buildings, CounterClass +5514 unit types, +5550 active building types, +5564/+5578/+558C active unit/infantry/aircraft (items +4, capacity +8, total +10)",
        ],
        substitutions=[
            "Recorded at entry and returned without running: Blowup_All 0x004FC6D0, MPlayer_Defeated 0x004FC0B0",
        ],
        entry_points={"gate": GATE_BEGIN, "join": GATE_END})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
