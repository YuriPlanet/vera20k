"""Original discovery producers, actual Infantry Mark, and initial Jumpjet Process.

This executes original interior constructor/entry blocks and complete discovery,
Mark/Cell list/recalculation, MoveTo, and first Process bodies. It does not execute
whole object allocation/Unlimbo/ChangeOwner or later flight. See the sidecar for
the supplied footprint, visibility, map metadata, FNPC and selection-search seams.
"""
from pathlib import Path
import struct

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, provenance,
    run_checked,
)
from tools.spatial_oracle.jumpjet_coordinates import Jumpjet, initialize_jumpjet_levels
from tools.spatial_oracle.fly_landing_phase import initialize_air_tracker
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.walk_head_occupation import (
    CURRENT, DUMMY, LOCO, MAP, OUTPUT, OWNER, SCENARIO, TYPE, VTABLE,
)

HOUSE, OTHER = SCRATCH + 0x10000, SCRATCH + 0x18000
FOOTPRINT_GET, SELECTED_INDEX = SCRATCH + 0x20000, SCRATCH + 0x20100
FOOTPRINT, SELECTED_VTABLE, SELECTED_ARRAY = (
    SCRATCH + 0x20200, SCRATCH + 0x20300, SCRATCH + 0x20400,
)
AIR_ARRAYS, AIR_TRACKER = SCRATCH + 0x50000, 0x887888

OBSERVED = {
    0x4D3780: "mark", 0x5683C0: "map_put", 0x5687F0: "map_remove",
    0x47E8A0: "cell_put", 0x47EA90: "cell_remove", 0x47D2B0: "recalculate",
    0x6F4960: "discovery", 0x6F497E: "already_discovered",
    0x6F49D8: "discover_current", 0x6F4A31: "discover_noncurrent",
    0x6F49DE: "power_dirty", 0x6F49EA: "radar_dirty", 0x6F4A25: "house_1f4",
    0x6E53A0: "tag", 0x5217C0: "raw_put", 0x521850: "raw_remove",
    0x54D0F0: "motion", 0x54B980: "state0", 0x54BA25: "phase1",
    0x4134A0: "air_add", 0x4D3710: "speed", 0x5F6940: "coordinates",
    0x5F44A0: "deselect", 0x4AEB30: "selection_map_clear",
    0x54B8D0: "layer", 0x4A9720: "layer_changed",
}


def initialize_mark_projection(u, *, projection, secondary, count=6561,
                               extent=(40, 40)):
    """Bind the existing Mark/Recalc map buffers without replacing those bodies."""
    u.mem_write(MAP + 0xF4, dwords(*extent))
    u.mem_write(0x87F850, dwords(projection, count, secondary))


def initialize_owner_classification(block, *, owner):
    """Original InitManagers owner classification; preserve discovery history."""
    block(0x6F3F40, 0x6F3F65, {UC_X86_REG_ECX: owner})


def initialize_discovery_prefix(u, block, *, owner):
    """Existing native Techno constructor/InitManagers discovery prefixes."""
    block(0x6F2B4B, 0x6F2B4D, {})
    assert u.reg_read(UC_X86_REG_EBX) == 0
    block(0x6F2FB5, 0x6F2FC7, {UC_X86_REG_ESI: owner},
          [0x6F2FB5, 0x6F2FBB, 0x6F2FC1])
    initialize_owner_classification(block, owner=owner)


class Entry(Jumpjet):
    def __init__(self, row):
        self.fixture_ready = False
        super().__init__(dict(actions=[], level=0, slope=0, request=[2688, 2688, 0]))
        self.case = row
        self.trace = []
        self.visits = []
        self.stage = "setup"
        u = self.uc
        u.mem_map(SCRATCH + 0x10000, 0x50000)
        # Native Cell metadata recalculation uses these two map projection buffers.
        # The map fixture has blank TMP/overlay metadata and two allocated Cells.
        initialize_mark_projection(u, projection=SCRATCH + 0x30000,
                                   secondary=SCRATCH + 0x38000)
        initialize_air_tracker(u, STACK_BASE + STACK_SIZE - 0x1000,
                               buffers=AIR_ARRAYS, extent=(40, 40),
                               capacity=4, stride=16, construct=False)

        # Original Infantry vtable, except its supplied one-cell footprint.
        u.mem_write(VTABLE, bytes(u.mem_read(0x7EB058, 0x600)))
        u.mem_write(VTABLE + 0x108, dwords(FOOTPRINT_GET))
        u.mem_write(FOOTPRINT, packed(0, 0) + packed(0x7FFF, 0x7FFF) + bytes(128))
        u.mem_write(OWNER + 0x6C0, dwords(TYPE))
        owner = OTHER if row.get("foreign_constructor", False) else HOUSE
        u.mem_write(OWNER + 0x21C, dwords(owner))
        u.mem_write(0xA83D4C, dwords(HOUSE))
        u.mem_write(0xA8B238, dwords(row.get("mode", 5)))
        u.mem_write(HOUSE + 0x1EC, b"\1\1")
        u.mem_write(TYPE + 0x235, b"\1")
        u.mem_write(TYPE + 0x5E8, dwords(row.get("sight", 8)))
        u.mem_write(OWNER + 0x74, b"\0")
        u.mem_write(OWNER + 0x81, b"\0")
        # A non-null Tag is intentionally present on the current-owner rows.
        # Produced41A, not a null Tag substitute, suppresses its callback.
        u.mem_write(OWNER + 0x34, dwords(0 if owner == OTHER else SCRATCH + 0x20500))
        level = row.get("level", 0)
        bridge = row.get("bridge", False)
        u.mem_write(CURRENT + 0x11B, bytes((level, 0)))
        u.mem_write(CURRENT + 0x140, dwords(0x100 if bridge else 0))
        u.mem_write(OWNER + 0x9C, dwords(2496, 2624, level * 104 + (416 if bridge else 0)))
        u.mem_write(0xAC13BC, dwords(416))
        u.mem_write(0xAC13C8, dwords(104))
        u.mem_write(CURRENT + 0x124, dwords(0x1C))
        u.mem_write(CURRENT + 0x128, dwords(0x14))
        u.mem_write(CURRENT + 0x54, dwords(41, 42))
        for off, value in [(0xD70, row.get("rot", 4)), (0xD74, 30),
                           (0xD80, row.get("height", 500)), (0xD90, 15)]:
            u.mem_write(TYPE + off, dwords(value))
        for off, value in [(0xD78, 20.0), (0xD7C, 40.0), (0xD84, 2.0), (0xD88, 4.0)]:
            u.mem_write(TYPE + off, struct.pack("<f", value))
        self.fixture_ready = True

        # Original ctor zeroing and InitManagers discovery prefix. These are
        # interior blocks, not a replacement claim for whole constructors.
        self.stage = "constructor"
        initialize_discovery_prefix(u, self.block, owner=OWNER)
        self.after_constructor = self.history()
        # Actual startup table81373C..813754 order, under inherited startup0E7F.
        self.stage = "startup_link"
        initialize_jumpjet_levels(self.call)
        self.level_height = self.read32(0xABC5E8)
        self.call(0x54AD30, 0, [LOCO + 4, OWNER])
        u.mem_write(OUTPUT, dwords(row.get("facing", 0)))
        self.call(0x4C9300, OWNER + 0x388, [OUTPUT])

    def block(self, begin, end, registers, required=None):
        sp = STACK_BASE + STACK_SIZE - 0x1000
        self.uc.mem_write(sp, dwords(RET_MAGIC, *([0] * 16)))
        self.uc.reg_write(UC_X86_REG_ESP, sp)
        for register, value in registers.items():
            self.uc.reg_write(register, value)
        run_checked(self.uc, begin, end, count=30000,
                    required_addresses=required or [begin])

    def history(self):
        return dict(object=list(self.uc.mem_read(OWNER + 0x41A, 3)),
                    houses=[dict(power_dirty=bool(self.uc.mem_read(house + 0x5778, 1)[0]),
                                 radar_dirty=bool(self.uc.mem_read(house + 0x5779, 1)[0]),
                                 byte_1f4=bool(self.uc.mem_read(house + 0x1F4, 1)[0]))
                            for house in [HOUSE, OTHER]])

    def observe(self, u, address, size, data):
        if not self.fixture_ready:
            return super().observe(u, address, size, data)
        self.visits.append((self.stage, address))
        sp = u.reg_read(UC_X86_REG_ESP)
        if address in OBSERVED:
            event = dict(stage=self.stage, event=OBSERVED[address])
            if address == 0x4D3780:
                event["put"] = self.read32(sp + 4)
            elif address == 0x6F4960:
                event["current_house"] = self.read32(sp + 4) == HOUSE
            elif address == 0x4134A0:
                event["phase"] = self.read32(LOCO + 0x50)
            self.trace.append(event)
        if address == FOOTPRINT_GET:
            assert self.read32(sp + 4) == 0
            self.ret(4, FOOTPRINT)
        elif address == SELECTED_INDEX:
            assert self.read32(self.read32(sp + 4)) == OWNER
            assert self.read32(0xA8ECC8) == 1
            self.ret(4, 0)
        elif address in (0x586360, 0x5865E0):
            # Visibility answers only. Original Cell+48/ground and all callers run.
            self.ret(4, int(self.case.get("shroud", True)))
        elif address == 0x56DC20:
            args = [self.read32(sp + 4 + i * 4) for i in range(15)]
            self.trace.append(dict(stage=self.stage, event="fnpc_supplied",
                                   seed=list(struct.unpack("<hh", u.mem_read(args[1], 4))),
                                   scalar_args=args[2:12],
                                   target=list(struct.unpack("<hh", u.mem_read(args[12], 4)))))
            u.mem_write(args[0], packed(10, 10))
            self.ret(60, args[0])

    def state(self):
        history = self.history()
        self.call(0x54B8D0, 0, [LOCO + 4])
        layer = self.uc.reg_read(UC_X86_REG_EAX)
        self.call(0x4C93D0, OWNER + 0x388, [OUTPUT])
        return dict(history=history, layer=layer, phase=self.read32(LOCO + 0x50),
                    moving=bool(self.uc.mem_read(LOCO + 0x4C, 1)[0]),
                    position=list(struct.unpack("<iii", self.uc.mem_read(OWNER + 0x9C, 12))),
                    destination=list(struct.unpack("<iii", self.uc.mem_read(LOCO + 0x40, 12))),
                    owner_facing=self.read32(OUTPUT) & 65535,
                    desired_height=self.read32(LOCO + 0x80), linked_height=self.read32(LOCO + 0x2C),
                    rot=struct.unpack("<H", self.uc.mem_read(LOCO + 0x68, 2))[0],
                    speed=struct.unpack("<d", self.uc.mem_read(OWNER + 0x578, 8))[0],
                    air_cell=list(struct.unpack("<hh", self.uc.mem_read(OWNER + 0x560, 4))),
                    air_count=self.read32(AIR_TRACKER + 104 * 24 + 0x10),
                    air_bucket_first_is_owner=self.read32(AIR_ARRAYS + 104 * 16) == OWNER,
                    ground_head_is_owner=self.read32(CURRENT + 0xE4) == OWNER,
                    bridge_head_is_owner=self.read32(CURRENT + 0xE8) == OWNER,
                    marked=bool(self.uc.mem_read(OWNER + 0x74, 1)[0]),
                    raw=[self.read32(CURRENT + 0x124), self.read32(CURRENT + 0x128)],
                    raw_owners=[self.read32(CURRENT + 0x54), self.read32(CURRENT + 0x58)],
                    selected=bool(self.uc.mem_read(OWNER + 0x83, 1)[0]),
                    selected_count=self.read32(0xA8ECC8),
                    selected_map_is_owner=self.read32(MAP + 0x11A0) == OWNER)

    def execute(self):
        self.stage = "foot_entry"
        self.block(0x4D7221, 0x4D7235, {UC_X86_REG_ESI: OWNER}, [0x4D722F, 0x6F4960])
        after_entry = self.history()
        if self.case.get("repeat_entry", False):
            self.stage = "repeat_entry"
            self.block(0x4D7221, 0x4D7235, {UC_X86_REG_ESI: OWNER}, [0x6F497E])
        self.stage = "sight"
        self.block(0x51E0DF, 0x51E0F6, {UC_X86_REG_EDI: OWNER})
        after_sight = self.history()
        if self.case.get("change_owner", False):
            self.stage = "owner_change"
            self.block(0x701735, 0x701757, {UC_X86_REG_ESI: OWNER, UC_X86_REG_EBP: OTHER},
                       [0x701735, 0x701751])
        if self.case.get("conceal", False):
            self.stage = "conceal"
            self.call(0x6F4A40, OWNER, [])
        before_put = self.history()
        self.stage = "entry_put"
        self.call(0x4D3780, OWNER, [1])
        after_put = self.history()
        if self.case.get("registered", False):
            self.stage = "prior_registration"
            self.call(0x4134A0, AIR_TRACKER, [OWNER])
        if self.case.get("selected", False):
            self.uc.mem_write(OWNER + 0x83, b"\1")
            self.uc.mem_write(0xA8ECB8, dwords(SELECTED_VTABLE, SELECTED_ARRAY, 4, 0, 1))
            self.uc.mem_write(SELECTED_VTABLE + 0x10, dwords(SELECTED_INDEX))
            self.uc.mem_write(SELECTED_ARRAY, dwords(OWNER))
            self.uc.mem_write(MAP + 0x119C, b"\1")
            self.uc.mem_write(MAP + 0x11A0, dwords(OWNER))
        if self.case.get("move", True):
            self.stage = "move"
            self.call(0x54B1C0, 0, [LOCO + 4, 2688, 2688, 0])
        self.stage = "before"
        before = self.state()
        self.stage = "process"
        self.call(0x54AEC0, 0, [LOCO + 4])
        process_visits = {address for stage, address in self.visits if stage == "process"}
        required = {0x54AEC0, 0x54B8D0}
        if self.case.get("move", True):
            required |= {0x54D0F0, 0x4D3780, 0x5687F0, 0x47EA90, 0x47D2B0,
                         0x5F6940, 0x5683C0, 0x47E8A0, 0x6F4960, 0x6F497E,
                         0x54B980, 0x54BA25, 0x4D3710}
        assert required <= process_visits, [hex(x) for x in required - process_visits]
        process_events = [event for event in self.trace if event["stage"] == "process"]
        if self.case.get("move", True):
            assert [event["put"] for event in process_events if event["event"] == "mark"] == [0, 1]
            ordered = [event["event"] for event in process_events]
            corridor = ["motion", "cell_remove", "coordinates", "cell_put",
                        "already_discovered", "state0", "phase1"]
            assert [ordered.index(name) for name in corridor] == sorted(ordered.index(name) for name in corridor)
        else:
            assert [event["event"] for event in process_events] == ["layer", "layer"]
        assert (0x4134A0 in process_visits) == (self.case.get("move", True)
                                               and not self.case.get("registered", False))
        # Only the produced initial zero-target-altitude/nonnegative-height cases:
        # later ascent can call raw-clear54D42F. Mark itself does not inject it.
        assert not {0x5217C0, 0x521850, 0x6E53A0, 0x4A9720} & process_visits
        deselect = self.case.get("move", True) and self.case.get("change_owner", False) \
            and self.case.get("selected", False) and self.case.get("shroud", True)
        assert (0x5F44A0 in process_visits) == deselect
        self.stage = "after"
        after = self.state()
        assert before["position"] == after["position"]
        assert before["layer"] == after["layer"] == 2
        assert before["raw"] == after["raw"] == [0x1C, 0x14]
        assert before["raw_owners"] == after["raw_owners"] == [41, 42]
        assert after["air_count"] == int(self.case.get("move", True) or self.case.get("registered", False))
        assert after["air_bucket_first_is_owner"] == bool(after["air_count"])
        return dict(constructor=self.after_constructor, entry=after_entry,
                    sight=after_sight, before_put=before_put, put=after_put,
                    initialized_level_height=self.level_height, before=before, after=after,
                    trace=self.trace,
                    dummy=list(struct.unpack("<hh", self.uc.mem_read(DUMMY + 0x24, 4))),
                    random_indices=[self.read32(SCENARIO + 0x21C), self.read32(SCENARIO + 0x220)])


def generate():
    cases = [dict(move=False), {}, dict(sight=0), dict(level=2),
             dict(bridge=True, registered=True), dict(facing=0x9300),
             dict(registered=True), dict(height=100), dict(rot=300), dict(rot=-1),
             dict(change_owner=True), dict(change_owner=True, selected=True),
             dict(change_owner=True, selected=True, shroud=False),
             dict(selected=True), dict(conceal=True),
             dict(foreign_constructor=True, repeat_entry=True)]
    return [dict(input=row, output=Entry(row).execute()) for row in cases]


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="Original per-Techno discovery producers plus actual Infantry Mark/Cell PUT/REMOVE/recalculation and initial Jumpjet Process; bounded same-position phase0-to1 corridor, not full constructor/Unlimbo/ChangeOwner, later flight or Tag delivery.",
        entry_points={"constructor_zero": 0x6F2FB5, "initial_owner": 0x6F3F40,
                      "foot_entry_block": 0x4D7221, "sight_gate": 0x51E0DF,
                      "owner_change_block": 0x701735, "conceal": 0x6F4A40,
                      "discovery": 0x6F4960, "mark": 0x4D3780,
                      "put": 0x47E8A0, "remove": 0x47EA90, "recalculate": 0x47D2B0,
                      "jumpjet_ctor": 0x54AC40, "link": 0x54AD30,
                      "move_to": 0x54B1C0, "process": 0x54AEC0,
                      "layer": 0x54B8D0, "air_add": 0x4134A0,
                      "deselect": 0x5F44A0, "startup_level": 0x54AB00},
        assumptions=[
            "Pinned original Infantry vtable7EB058 except footprint; original RTTI15, type/height/high-flying/coordinate/layer/Mark/discovery/selection receivers execute. No direct41A/B/C seeding: original ctor XOR6F2B4B and stores6F2FB5/BB/C1, InitManagers prefix6F3F40..65, Foot entry4D7221..35 and Sight51E0DF..F6 execute as declared interior blocks, not whole allocation/Unlimbo.",
            "Two supplied House allocations: currentA83D4C human/control true, other flags false; GameMode5, ScenarioInit0, mission0, active owner alive/nonlimbo, no swap/abort. Nonnull Tag on initial-current-owner rows is skipped by produced41A or41B; foreign-constructor row has null Tag and observes House1F4. No Tag runtime substitution or delivery claim.",
            "Optional original owner-change assignment/classification block701735..757 preserves prior41B/C; it is not full ChangeOwner. Conceal row executes complete6F4A40 on current human owner. Noncurrent repeat entry demonstrates aggregate41C, not per-house storage.",
            "Sparse real Cells9,10 and10,10, blank TMP/overlay metadata, projection buffers with stride81/capacity6561, map span40x40. Current XY2496,2624 and nonnegative ground-relative Z: flat0, level2, or structural deck416 with OnBridge false. Cell47D2B0 executes; arbitrary sparse maps and all projection metadata excluded.",
            "Current raw bitmap prestate ground0x1c/deck0x14 and owner indices41/42 are supplied. This does not execute the later InfantryUnlimbo raw-entry gate51E0F6..114 or establish its startup height bound; it establishes that these prestates survive the declared initial Process unchanged.",
            "Original Jumpjet constructor54AC40 and Link54AD30 copy declared ROT/speed/height/climb/acceleration/crash/no-wobble defaults; motion starts with constructor zero target altitude and velocity. Original CRT54AA30..54AB00 in table81373C..813754 order computes ABC5E8 under captured startup FPCW0E7F; ground/bridge globals104/416 otherwise supplied. Later ascent54D42F may clear raw Infantry bits and is excluded; only these initial Process rows assert no raw receiver.",
            "AirTracker has400 supplied capacity4 vectors. Foot560 constructor-null value starts absent; declared prior-registration rows execute original4134A0 before Process. List layer and registration are independent. No allocator or whole FootUnlimbo registration claim.",
            "Selection rows supply one existing selected owner and matching Map selection pointer; original Process/5F44A0 compaction/count/flag/map-pointer clear execute. No selection acquisition/UI claim. Events are selected instruction-entry observations; state dumps capture actual writes.",
        ],
        substitutions=[
            "Only Infantry+108 footprint returns declared one-cell (0,0), terminating(7fff,7fff); original48DEE0 footprint copy and full Mark/map/Cell list/recalculation/discovery run. Mark is never replaced and raw occupation callbacks are never injected.",
            "586360/5865E0 shroud answers are declared booleans; original callers/coordinate producers run. GameMode5 forces CellPUT discovery even in the false-shroud selection contrast. No arbitrary visibility computation claim.",
            "MoveTo FNPC56DC20 returns declared10,10; original caller54B1C0/54D6D0/4ACA10/481180 and Scenario RNG execute. Whole Infantry51AA40 setter/search admission remains outside this corpus.",
            "Selected vector virtual index lookup returns0 after checking supplied one-member list and requested owner; actual5F44A0 and4AEB10/4AEB30 execute. No dynamic allocator or generic vector search proof.",
        ],
    ))
