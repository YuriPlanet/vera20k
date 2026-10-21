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

Additive --outcomes selects the full offline result bodies and shared timer
heads, writing house_outcomes.json with --write. It reuses this Fixture and the
existing ReadDouble owner. VERA20K_OUTCOME_RULES selects physical RULESMD.INI
(default ini/rulesmd.ini); file/archive IO and full scenario timing are excluded.

Additive --ares-survival executes KeepAlive input/tracking, both survival arms
and bounded persistence controls. See ares_house_survival.md.
"""
import argparse
import hashlib
import os
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


class OutcomeFixture(Fixture):
    """Reuse the gate VM transport for full offline Flag bodies and timer heads.

    UI/audio sinks record entry arguments and return; original Flag branches,
    double multiply/ftol, timer writes and EndScenario remain instructions.
    HouseUpdate heads stop BEFORE the wall-clock/Vox wait, never fake its end.
    """
    FLAGS = {"win": 0x4FC9E0, "lose": 0x4FCBD0, "end": 0x4FCDC0}
    SINK_CLEANUP = {0x684240: 0, 0x734E60: 8, 0x6D4DB0: 8,
                    0x752700: 4, 0x4F42F0: 4, 0x4068E0: 0}

    def observe(self, u, pc, _size, _data):
        if pc in getattr(self, "gate_stops", {}):
            self.gate_result = self.gate_stops[pc]
            u.emu_stop()
        elif pc in self.SINK_CLEANUP:
            sp = u.reg_read(UC_X86_REG_ESP)
            event = dict(address=hex(pc), ecx=u.reg_read(UC_X86_REG_ECX),
                         stack=[self.word(sp + n) for n in (4, 8)])
            if pc in (0x734E60, 0x752700):
                event["name"] = bytes(u.mem_read(event["ecx"], 80)).split(b"\0", 1)[0].decode("ascii")
            self.events.append(event)
            if pc == 0x734E60:
                # Supplied CSF text storage; native string table IO excluded.
                u.reg_write(UC_X86_REG_EAX, SCRATCH + 0xFE00)
            u.reg_write(UC_X86_REG_EIP, self.word(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 4 + self.SINK_CLEANUP[pc])
        else:
            super().observe(u, pc, _size, _data)

    def reset_outcome(self, case, savour_bits):
        u = self.u
        self.events = []
        self.gate_stops = {}
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.mem_write(GAME_MODE, dwords(case.get("game_mode", 0)))
        u.mem_write(FRAME, struct.pack("<i", case.get("frame", 100)))
        u.mem_write(RULES_PTR, dwords(RULES))
        u.mem_write(RULES + 0x14C8, struct.pack("<Q", savour_bits))
        u.mem_write(0xA83D4C, dwords(HOUSE))
        u.mem_write(0xAC1198, dwords(HOUSE if case.get("observer", False) else 0))
        u.mem_write(0xA8B550, struct.pack("<i", case.get("max_ahead", 0)))
        u.mem_write(0x887324, dwords(SCRATCH + 0xF000))
        u.mem_write(HOUSE + 0x1EC, bytes([case.get("human", 1), 0]))
        u.mem_write(HOUSE + 0x1F6, bytes([0, case.get("won", 0), case.get("lost", 0)]))
        u.mem_write(HOUSE + 0x27C, struct.pack("<i", case.get("allow_win", 0)))
        u.mem_write(HOUSE + 0x298, struct.pack("<iii", *case.get("timer", [-1, 0, 0])))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, HOUSE)

    def constructor_savour(self):
        self.u.reg_write(UC_X86_REG_ESI, RULES)
        run_checked(self.u, 0x66731D, 0x667331, count=2,
                    required_addresses=(0x66731D, 0x667327))
        return struct.unpack("<Q", self.u.mem_read(RULES + 0x14C8, 8))[0]

    def flag(self, case, savour_bits):
        self.reset_outcome(case, savour_bits)
        fn = self.FLAGS[case["operation"]]
        self.u.mem_write(SP, dwords(RET_MAGIC, int(case.get("skip_timer", False))))
        run_checked(self.u, fn, RET_MAGIC, count=20_000, required_addresses=(fn,))
        return dict(input=case, savour_bits=f"{savour_bits:016x}",
                    won=self.u.mem_read(HOUSE + 0x1F7, 1)[0],
                    lost=self.u.mem_read(HOUSE + 0x1F8, 1)[0],
                    timer=list(struct.unpack("<iii", self.u.mem_read(HOUSE + 0x298, 12))),
                    al=self.u.reg_read(UC_X86_REG_EAX) & 255, calls=self.events)

    def result_gate(self, case):
        self.reset_outcome(case, 0)
        u = self.u
        u.reg_write(UC_X86_REG_ESI, HOUSE)
        u.reg_write(UC_X86_REG_EBP, 0)
        win = case["operation"] == "win_gate"
        begin = 0x4F85BE if win else 0x4F86FE
        join = 0x4F86FE if win else 0x4F87C2
        self.gate_result = "continue"
        self.gate_stops = {0x4F860B if win else 0x4F8732: "result_wait"}
        run_checked(u, begin, (join, *self.gate_stops), count=100,
                    required_addresses=(begin,))
        return dict(input=case, gate=self.gate_result)

    def banner_center(self, view, canvas):
        self.gate_stops = {}
        self.u.mem_write(0x886FA8, struct.pack("<ii", *view))
        self.u.mem_write(SCRATCH + 0xF002, struct.pack("<hh", *canvas))
        self.u.reg_write(UC_X86_REG_ESI, SCRATCH + 0xF000)
        run_checked(self.u, 0x6D4E95, 0x6D4EB9, count=30,
                    required_addresses=(0x6D4E95, 0x6D4EB7))
        signed = lambda reg: struct.unpack("<i", dwords(self.u.reg_read(reg)))[0]
        return dict(view=view, canvas=canvas,
                    origin=[signed(UC_X86_REG_EAX), signed(UC_X86_REG_ECX)])


def outcome_inputs():
    return [
        dict(name="win_ctor", operation="win", delay="constructor"),
        dict(name="win_retail", operation="win", delay="retail"),
        dict(name="lose_retail", operation="lose", delay="retail"),
        dict(name="win_read_point03", operation="win", delay=".03"),
        dict(name="win_signed_delay", operation="win", delay="-.03"),
        dict(name="win_already_won", operation="win", won=1, timer=[17, 0, 41]),
        dict(name="win_already_lost", operation="win", lost=1, timer=[17, 0, 41]),
        dict(name="lose_replaces_win", operation="lose", won=1),
        dict(name="lose_already_lost_clears_win", operation="lose", won=1, lost=1,
             timer=[17, 0, 41]),
        dict(name="announce67_preserves_running", operation="win", skip_timer=True,
             timer=[17, 0, 41]),
        dict(name="announce68_preserves_paused", operation="lose", skip_timer=True,
             timer=[-1, 0, 41]),
        dict(name="announce68_ctor_zero", operation="lose", skip_timer=True),
        dict(name="end69_paused_win", operation="end", won=1, timer=[-1, 0, 41]),
        dict(name="end69_running_lose", operation="end", lost=1, timer=[17, 0, 41]),
        dict(name="end69_no_result", operation="end", delay="retail"),
        dict(name="win_offline_mode5", operation="win", game_mode=5, delay="retail"),
        dict(name="lan_win_alignment", operation="win", game_mode=3, frame=103, max_ahead=10),
        dict(name="wol_loss_max_ahead", operation="lose", game_mode=4, frame=103, max_ahead=73),
        dict(name="lan_observer_loss", operation="lose", game_mode=3, frame=103, max_ahead=10, observer=True),
        dict(name="lan_negative_savour", operation="lose", game_mode=3, frame=103, max_ahead=2, delay="-.03"),
        dict(name="lan_wrapping_expiry", operation="win", game_mode=3, frame=2147483640, max_ahead=10),
        dict(name="lan_paused_anchor", operation="lose", game_mode=3, frame=-1, max_ahead=10, delay="-.03"),
        dict(name="wol_skip_delay", operation="lose", game_mode=4, frame=103, max_ahead=73, skip_timer=True, timer=[17,0,41]),
    ]


def _retail_savour_token(path):
    # Input extraction only, not an INI/scalar implementation: require this
    # unique physical AudioVisual key, then execute the existing ReadDouble.
    raw = path.read_bytes()
    selected = False
    found = []
    for line in raw.splitlines():
        line = line.split(b";", 1)[0].strip()
        if line.startswith(b"[") and line.endswith(b"]"):
            selected = line == b"[AudioVisual]"
        elif selected and b"=" in line:
            key, value = (part.strip() for part in line.split(b"=", 1))
            if key == b"SavourDelay":
                found.append(value.decode("ascii"))
    assert len(found) == 1, (path, "unique physical AudioVisual SavourDelay required", found)
    return raw, found[0]


def generate_outcomes():
    from tools.rules_oracle.read_double_percent import Reader
    from tools.spatial_oracle.building_body_rules import TYPE
    path = Path(os.environ.get("VERA20K_OUTCOME_RULES", "ini/rulesmd.ini"))
    raw, token = _retail_savour_token(path)
    reader = Reader()
    reader.u.mem_write(TYPE + 0x1F8, b"AudioVisual\0")
    fixture = OutcomeFixture()
    constructor = fixture.constructor_savour()
    parsed = {value: reader.read(value, key_name="SavourDelay")
              for value in dict.fromkeys((token, ".03", "-.03"))}
    bits = dict(constructor=constructor, retail=parsed[token], **parsed)
    flags = [fixture.flag(case, bits[case.get("delay", "constructor")])
             for case in outcome_inputs()]
    gates = []
    for win in (True, False):
        for mode, count, timer, frame in [(0, 0, [100, 0, 90], 189),
                                          (0, 0, [100, 0, 90], 190),
                                          (0, 1, [100, 0, 90], 190),
                                          (5, 1, [100, 0, 90], 190),
                                          (0, 0, [-1, 0, 1], 190),
                                          (0, 0, [-1, 0, 0], 190),
                                          (0, 0, [-1, 0, -1], 190)]:
            case = dict(name=f"{'win' if win else 'lose'}_mode{mode}_tags{count}_timer{timer[0]}_{timer[2]}_frame{frame}",
                        operation="win_gate" if win else "lose_gate", won=int(win),
                        lost=int(not win), game_mode=mode, allow_win=count, timer=timer, frame=frame)
            gates.append(fixture.result_gate(case))
    return dict(constructor_bits=f"{constructor:016x}",
                retail=dict(file=path.name, sha256=hashlib.sha256(raw).hexdigest(),
                            token=token, reader_bits=f"{parsed[token]:016x}"),
                flag_controls=flags, result_gates=gates,
                banner_centers=[fixture.banner_center([640, 480], canvas)
                                for canvas in ([400, 80], [641, 483])])


def outcome_metadata():
    return provenance(
        scope="Full HouseFlagWin4FC9E0/FlagLose4FCBD0/EndScenario4FCDC0 and exact-zero/AllowWin result heads4F85BE/4F86FE",
        assumptions=["FlagToDie+1F6 supplied zero; campaign0, LAN3, WOL4 and skirmish5; current PlayerPtr, observer pointer and human bytes supplied",
                     "Constructor66731D..667331 original stores establish Rules+14C8 bits; physical AudioVisual SavourDelay token is passed through existing original5283D0 Reader; FPCW0E7F",
                     "House+298/+29C/+2A0 raw timer and +27C AllowWin count are independent supplied inputs; original branches and arithmetic execute"],
        substitutions=["Record and return UI/input/audio/dirty sinks684240,734E60,6D4DB0,752700,4F42F0,4068E0; no media device, localized text IO, rendering or actual sound",
                       "Result heads stop before6C8C40 WallClock/Vox wait at4F860B/4F8732; no completion, teardown or postgame/campaign-next-scene claim",
                       "No Windows startup, full House constructor/world, negotiated transport timing producer, FlagToDie producer or RNG path; MaxAhead is an explicit supplied input"],
        entry_points={**OutcomeFixture.FLAGS, "win_gate": 0x4F85BE, "lose_gate": 0x4F86FE,
                      "constructor_savour": 0x66731D, "read_double": 0x5283D0,
                      "banner_center": 0x6D4E95})


ARES_HOOKS = {0x4F8EBD: 0x10024990, 0x4FF71B: 0x10024580,
              0x4FF563: 0x100245D0}
ARES_EXTRA = SCRATCH + 0x18000
ARES_ACTOR, ARES_TYPE, ARES_TYPE_EXT = (SCRATCH + x for x in (0x20000, 0x22000, 0x24000))
ARES_SECOND_HOUSE, ARES_SECOND_EXT = SCRATCH + 0x40000, SCRATCH + 0x58000
ARES_FRAME, ARES_BRIDGE_SP = SCRATCH + 0x68000, SCRATCH + 0x6A000


class AresFixture(Fixture):
    """The existing gate VM, with real exports at three saved-register seams."""

    def __init__(self, ares):
        from tools import native_oracle
        super().__init__()
        # Retain the historical stack's top128KiB and SP; only its unused
        # lower portion overlaps the DLL's preferred image.
        self.u.mem_unmap(STACK_BASE, 0xE0000)
        native_oracle.map_pe32_image(self.u, ares, image_base=0x10000000,
                                    image_size=0xE0000, header_size=0x400)
        self.u.mem_map(SCRATCH + REGION, 0x70000)
        self.code = [(native_oracle.IMAGE_BASE + rva,
                      bytes(self.u.mem_read(native_oracle.IMAGE_BASE + rva, size)))
                     for rva, _raw, size, _virtual, flags in native_oracle._sections(
                         native_oracle.image_bytes()) if flags & 0x20000000]
        self.code.append((0x10001000, bytes(self.u.mem_read(0x10001000, 0x92000))))
        self.events, self.hooks = [], []
        self.base_visits, self.class_visits, self.object_visits = [], [], []

    def signed(self, address):
        return struct.unpack('<i', self.u.mem_read(address, 4))[0]

    def put(self, address, value):
        self.u.mem_write(address, dwords(value & 0xFFFFFFFF))

    def observe(self, u, pc, size, data):
        if pc in (0x65C780, 0x65C7E0, 0x70D150, 0x7258D0, 0x5F5280):
            raise AssertionError(('Unexpected RNG/timer/detach entry', hex(pc)))
        if pc == 0x749020:
            # Existing house_tracking.Fixture's owned-type-set boundary.
            # CounterClass increment/decrement themselves execute here.
            sp = u.reg_read(UC_X86_REG_ESP)
            u.reg_write(UC_X86_REG_EIP, self.word(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 8)
            u.reg_write(UC_X86_REG_EAX, 0)
            return
        if pc == 0x100249D5:
            self.base_visits.append((u.reg_read(UC_X86_REG_ECX) - BASE_UNITS) // 4)
        class_entries = {0x10024A06: 'unit', 0x10024A54: 'infantry', 0x10024A92: 'aircraft'}
        object_entries = {0x10024A20: 'unit', 0x10024A66: 'infantry', 0x10024AA4: 'aircraft'}
        if pc in class_entries:
            self.class_visits.append(class_entries[pc])
        if pc in object_entries:
            address = self.word(u.reg_read(UC_X86_REG_ECX))
            self.object_visits.append([object_entries[pc], self.object_names[address]])
        super().observe(u, pc, size, data)

    def checked_code(self):
        assert all(bytes(self.u.mem_read(at, len(raw))) == raw for at, raw in self.code)

    def run_hooks(self, begin, end, **kwargs):
        from unicorn.x86_const import UC_X86_REG_EDI
        ends = (end,) if isinstance(end, int) else tuple(end)
        active, pc = set(ARES_HOOKS), begin
        for _part in range(12):
            stopped = run_checked(self.u, pc, ends + tuple(active), **kwargs)
            if stopped in ends:
                self.checked_code()
                return stopped
            context = self.u.context_save()
            self.u.mem_write(ARES_FRAME, bytes(0x40))
            self.put(ARES_FRAME + 8, self.u.reg_read(UC_X86_REG_EDI))
            self.put(ARES_FRAME + 0xC, self.u.reg_read(UC_X86_REG_ESI))
            self.put(ARES_FRAME + 0x24, self.u.reg_read(UC_X86_REG_EAX))
            self.u.mem_write(ARES_BRIDGE_SP, dwords(RET_MAGIC + 0x100, ARES_FRAME))
            self.u.reg_write(UC_X86_REG_ESP, ARES_BRIDGE_SP)
            run_checked(self.u, ARES_HOOKS[stopped], RET_MAGIC + 0x100,
                        count=100_000, required_addresses=(ARES_HOOKS[stopped],))
            continuation = self.u.reg_read(UC_X86_REG_EAX)
            saved_eax = self.word(ARES_FRAME + 0x24)
            self.hooks.append(dict(site=f'{stopped:08X}', returned=f'{continuation:08X}'))
            self.u.context_restore(context)
            if stopped in (0x4FF71B, 0x4FF563):
                self.u.reg_write(UC_X86_REG_EAX, saved_eax)
            assert continuation != 0
            pc = continuation
            active.remove(stopped)
            if pc in ends:
                self.checked_code()
                return pc
        raise AssertionError('Too many survival hook continuations')

    def invoke(self, entry, this=0, args=(), end=RET_MAGIC):
        self.u.mem_write(SP, dwords(RET_MAGIC, *args))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ECX, this)
        self.run_hooks(entry, end, count=100_000)

    def init_house(self, house, extension, items):
        from unicorn.x86_const import UC_X86_REG_EDI
        self.u.mem_write(house, bytes(0x16088))
        self.u.mem_write(extension, bytes(0x1000))
        self.put(house + 0x16084, extension)
        self.u.reg_write(UC_X86_REG_ESI, extension)
        self.u.reg_write(UC_X86_REG_EDI, house)
        # Actual HouseExt constructor's zero-store prefix, not a supplied result.
        run_checked(self.u, 0x10021027, 0x10021059)
        for slot, offset in enumerate((0x5500, 0x5514, 0x5528, 0x553C)):
            self.put(house + offset + 4, items + slot * 0x100)
            self.put(house + offset + 8, 16)
            self.u.mem_write(items + slot * 0x100, bytes(0x100))

    def counts(self, house=HOUSE):
        extension = self.word(house + 0x16084)
        return dict(keep_alive=self.signed(extension + 0x18),
                    building_keep_alive=self.signed(extension + 0x1C),
                    buildings=self.signed(house + 0x2F0), vehicles=self.signed(house + 0x2E8),
                    building_type=self.signed(self.word(house + 0x5504) + 12),
                    unit_type=self.signed(self.word(house + 0x5518) + 12))

    def prepare_actor(self, case, pair):
        self.u.mem_write(ARES_ACTOR, bytes(0x2000))
        self.u.mem_write(ARES_TYPE, bytes(0x2000))
        self.u.mem_write(ARES_TYPE_EXT, bytes(0x1000))
        building = case['class'] == 'building'
        self.put(ARES_ACTOR, 0x7E3EBC if building else 0x7F5C70)
        self.put(ARES_ACTOR + (0x520 if building else 0x6C4), ARES_TYPE)
        self.put(ARES_ACTOR + 0x21C, HOUSE)
        # Original UnitType ArrayIndex virtual reads the shared +DF8 field;
        # no type-category virtual is used by the tracked class arms.
        self.put(ARES_TYPE, 0x7F6218)
        self.put(ARES_TYPE + 0xDF8, 3)
        self.put(ARES_TYPE + 0x2FC, ARES_TYPE_EXT)
        self.u.mem_write(ARES_TYPE + 0x232, bytes([case.get('insignificant', False)]))
        self.u.mem_write(ARES_TYPE + 0xC9F, bytes([case.get('dont_score', False)]))
        self.u.mem_write(ARES_TYPE_EXT + 0x646, bytes([pair['value'], pair['is_set']]))
        if case.get('unit_like'):
            self.put(ARES_TYPE + 0x408, ARES_TYPE + 0x1000)
            self.put(ARES_TYPE + 0xEF0, 0)  # original foundation tables:1x1

    def tracking(self, case, pair):
        self.hooks = []
        self.init_house(HOUSE, ARES_EXTRA, COUNTER_ITEMS)
        self.prepare_actor(case, pair)
        self.put(ARES_EXTRA + 0x18, case.get('start', 0))
        if case['class'] == 'building':
            self.put(ARES_EXTRA + 0x1C, case.get('start', 0))
        steps = []
        for operation in case['operations']:
            mark = len(self.hooks)
            self.invoke(0x4FF700 if operation == 'gain' else 0x4FF550, HOUSE, (ARES_ACTOR,))
            steps.append(dict(operation=operation, after=self.counts(), hooks=self.hooks[mark:]))
        return dict(input=case, steps=steps)

    def capture(self, pair):
        self.hooks = []
        self.init_house(HOUSE, ARES_EXTRA, COUNTER_ITEMS)
        self.init_house(ARES_SECOND_HOUSE, ARES_SECOND_EXT, SCRATCH + 0x60000)
        self.prepare_actor(dict(**{'class': 'unit'}, insignificant=True), pair)
        self.invoke(0x4FF700, HOUSE, (ARES_ACTOR,))
        before = dict(old=self.counts(), new=self.counts(ARES_SECOND_HOUSE))
        self.u.mem_write(SP, dwords(ARES_ACTOR))  # preceding7015D7 PUSH ESI
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ESI, ARES_ACTOR)
        self.u.reg_write(UC_X86_REG_EBP, ARES_SECOND_HOUSE)
        mark = len(self.hooks)
        self.run_hooks(0x7015D8, 0x7015EB, count=100_000)
        return dict(input=dict(**{'class': 'unit'}, insignificant=True, keep_alive=True),
                    before=before, after=dict(old=self.counts(), new=self.counts(ARES_SECOND_HOUSE)),
                    owner_pointer_still_old=self.word(ARES_ACTOR + 0x21C) == HOUSE,
                    hooks=self.hooks[mark:])

    def survival(self, case):
        # Reuse every historical gate setup statement; the dedicated mode
        # supplies its additional vector/extension/array priors at run entry.
        global run_checked
        original_runner = run_checked
        self.hooks, self.base_visits, self.class_visits, self.object_visits = [], [], [], []
        def wrapped(u, begin, end, **kwargs):
            global run_checked
            self.put(HOUSE + 0x16084, ARES_EXTRA)
            self.put(ARES_EXTRA + 0x18, case.get('keep_alive_count', 0))
            indices = case.get('base_indices', [3, 4, 5, 6])
            self.put(RULES + 0xB30, len(indices))
            self.put(HOUSE + 0x551C, case.get('capacity', 16))
            unique_indices = list(dict.fromkeys(indices))
            for slot, index in enumerate(indices):
                typ = SCRATCH + 0x28000 + unique_indices.index(index) * 0x1000
                self.put(BASE_UNITS + slot * 4, typ)
                self.put(typ + 0xDF8, index)
            for index, count in case.get('counts', {}).items():
                self.put(COUNTER_ITEMS + int(index) * 4, count)
            self.object_names = {}
            for cls, address, count_address in [('unit', 0x8B410C, 0x8B4118),
                                                ('infantry', 0xA83DEC, 0xA83DF8),
                                                ('aircraft', 0xA8E394, 0xA8E3A0)]:
                nodes = case.get('objects', {}).get(cls, [])
                array = SCRATCH + 0x35000 + len(self.object_names) * 0x100
                self.put(address, array); self.put(count_address, len(nodes))
                for index, node in enumerate(nodes):
                    at = SCRATCH + 0x30000 + len(self.object_names) * 0x1000
                    self.u.mem_write(at, bytes(0x1000))
                    self.object_names[at] = node['name']
                    self.put(array + index * 4, at)
                    assert node['category'] == cls
                    self.put(at + 0x21C, HOUSE if node['owner'] == 'ours' else ARES_SECOND_HOUSE)
                    self.u.mem_write(at + 0x81, bytes([node['limbo']]))
                    self.put(at + 0x69C, ARES_TYPE_EXT if node['parasite'] else 0)
            run_checked = original_runner
            try:
                return self.run_hooks(begin, end, **kwargs)
            finally:
                run_checked = wrapped
        run_checked = wrapped
        try:
            row = Fixture.execute(self, case)
        finally:
            run_checked = original_runner
        row.update(base_slots_visited=self.base_visits, classes_visited=self.class_visits,
                   objects_visited=self.object_visits, hooks=self.hooks)
        return row

    def stream(self, name, pair, count):
        from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_EDI
        stream, reference, buffer = SCRATCH + 0x65000, SCRATCH + 0x65100, SCRATCH + 0x66000
        self.u.mem_write(stream, dwords(buffer, buffer, buffer + 0x100, 0))
        self.put(reference, stream)
        self.u.mem_write(ARES_TYPE_EXT + 0x646, bytes([pair['value'], pair['is_set']]))
        self.put(ARES_EXTRA + 0x18, count)
        self.invoke(0x1000F0C0, ARES_TYPE_EXT + 0x646, (reference,))
        self.u.reg_write(UC_X86_REG_EBX, ARES_EXTRA)
        self.u.reg_write(UC_X86_REG_EDI, reference)
        self.run_hooks(0x1001F4D7, 0x1001F4E4, count=100_000)
        size = self.word(stream + 4) - buffer
        stored = bytes(self.u.mem_read(buffer, size)).hex()
        self.u.mem_write(ARES_TYPE_EXT + 0x646, b'\xCC\xCC')
        self.put(ARES_EXTRA + 0x18, 0x55667788)
        self.put(stream + 0xC, 0)
        self.invoke(0x1000D7E0, ARES_TYPE_EXT + 0x646, (reference,))
        self.u.reg_write(UC_X86_REG_EBX, reference)
        self.u.reg_write(UC_X86_REG_EDI, ARES_EXTRA)
        self.run_hooks(0x1001F366, 0x1001F373, count=100_000)
        return dict(name=name, input=dict(nullable=pair, keep_alive_count=count),
                    stored_bytes=stored, restored=dict(nullable=dict(
                        value=self.u.mem_read(ARES_TYPE_EXT + 0x646, 1)[0],
                        is_set=self.u.mem_read(ARES_TYPE_EXT + 0x647, 1)[0]),
                        keep_alive_count=self.signed(ARES_EXTRA + 0x18)),
                    consumed_bytes=self.word(stream + 0xC))


def ares_reader_history(ares):
    """Use the existing physical Rules VM and unchanged Ares nullable reader."""
    from tools.rules_oracle.weapon_speed_order import retail_history
    from tools import native_oracle
    from unicorn.x86_const import UC_X86_REG_EBX
    layers = [
        ('UNSET.INI', '[CONTROL]\nName=present\n'),
        ('YES.INI', '[CONTROL]\nKeepAlive=yes\n'),
        ('MISSING.INI', '[CONTROL]\nName=present\n'),
        ('EMPTY.INI', '[CONTROL]\nKeepAlive=\n'),
        ('INVALID.INI', '[CONTROL]\nKeepAlive=on\n'),
        ('BRACKET_NONE.INI', '[CONTROL]\nKeepAlive=<none>\n'),
        ('NONE.INI', '[CONTROL]\nKeepAlive=none\n'),
        ('NO.INI', '[CONTROL]\nKeepAlive=no\n'),
        ('PREFIX.INI', '[CONTROL]\nKeepAlive=yeSplease\n'),
        ('TRIM.INI', '[CONTROL]\nKeepAlive= \tno \t\n'),
        ('KEYCASE.INI', '[CONTROL]\nKEEPALIVE=yes\n'),
        ('OTHER.INI', '[OTHER]\nKeepAlive=yes\n')]
    class Inputs:
        def lookup(self, name):
            return dict(layers)[name.upper()].encode('ascii')
    m = retail_history(Inputs())
    try:
        loaded = [(name, body, *m.load_physical_ini(name)) for name, body in layers]
        ext, typ = m.alloc(0x1000), m.alloc(0x2000)
        m.u.mem_write(typ, dwords(0x7F6218))
        m.u.mem_write(typ + 0x24, b'CONTROL\0')
        m.u.mem_unmap(native_oracle.STACK_BASE, native_oracle.STACK_SIZE)
        native_oracle.map_pe32_image(m.u, ares, image_base=0x10000000,
                                    image_size=0xE0000, header_size=0x400)
        m.u.mem_map(0x33000000, 0x20000)
        sp = 0x33010000
        code = bytes(m.u.mem_read(0x10001000, 0x92000))
        m.u.mem_write(sp, dwords(RET_MAGIC, typ)); m.u.reg_write(UC_X86_REG_ESP, sp)
        m.u.reg_write(UC_X86_REG_ECX, ext)
        run_checked(m.u, 0x1003C710, RET_MAGIC)
        def pair():
            value, present = bytes(m.u.mem_read(ext + 0x646, 2))
            return dict(value=value, is_set=present)
        default, diagnostics, reads = pair(), [], []
        def observe(u, pc, *_):
            esp = u.reg_read(UC_X86_REG_ESP)
            if pc == 0x528A10:
                reads.append(dict(section=m.string(m.read32(esp + 4)),
                    key=m.string(m.read32(esp + 8)), default=m.string(m.read32(esp + 12)),
                    capacity=m.read32(esp + 20)))
            elif pc == 0x10061960:
                diagnostics.append(m.string(m.read32(esp + 12)))
                m.ret(0, 16)
            elif pc in (0x65C780, 0x65C7E0, 0x70D150, 0x7258D0, 0x5F5280):
                raise AssertionError(('Unexpected reader RNG/timer/detach', hex(pc)))
        handle = m.u.hook_add(UC_HOOK_CODE, observe)
        rows = []
        for name, body, ini, _receipt in loaded:
            before, mark, read_mark = pair(), len(diagnostics), len(reads)
            m.u.mem_write(sp, dwords(RET_MAGIC, ini)); m.u.reg_write(UC_X86_REG_ESP, sp)
            m.u.reg_write(UC_X86_REG_ECX, ext)
            stop = run_checked(m.u, 0x1003DFB0, (0x1003DFD9, RET_MAGIC),
                               required_addresses=(0x100091C0,))
            if stop == 0x1003DFD9:
                m.u.mem_write(sp, bytes(0x200)); m.u.mem_write(sp + 0x10, dwords(ini))
                m.u.reg_write(UC_X86_REG_ESP, sp); m.u.reg_write(UC_X86_REG_EBX, ext)
                m.u.reg_write(UC_X86_REG_ESI, typ + 0x24)
                run_checked(m.u, 0x1003F47C, 0x1003F492,
                    required_addresses=(0x1000E9B0, 0x1000FC50, 0x10007860, 0x528A10))
            rows.append(dict(file=name, physical_ini=body, before=before, after=pair(),
                             section_admitted=stop == 0x1003DFD9,
                             diagnostics=diagnostics[mark:], reads=reads[read_mark:]))
        m.u.hook_del(handle); m.check_code()
        assert bytes(m.u.mem_read(0x10001000, 0x92000)) == code
        return dict(name='physical_retention', constructor_default=default, steps=rows)
    finally:
        m.close()


def ares_tracking_inputs():
    cases = [
        dict(name='default_building', **{'class': 'building'}, keep_alive=None),
        dict(name='default_unit', **{'class': 'unit'}, keep_alive=None),
        dict(name='default_unit_like_building', **{'class': 'building'}, keep_alive=None, unit_like=True),
        dict(name='excluded_default', **{'class': 'building'}, keep_alive=None, dont_score=True),
        dict(name='excluded_override_yes', **{'class': 'unit'}, keep_alive=True, insignificant=True),
        dict(name='building_override_no', **{'class': 'building'}, keep_alive=False),
        dict(name='building_wrap', **{'class': 'building'}, keep_alive=None, start=2147483647),
        dict(name='unit_underflow', **{'class': 'unit'}, keep_alive=True,
             insignificant=True, start=-2147483648, operations=['loss', 'gain'])]
    return [dict(case, operations=case.get('operations', ['gain', 'loss'])) for case in cases]


def ares_survival_inputs():
    cases = [
        dict(name='short_fourth', counts={'6': 1}),
        dict(name='short_empty', base_indices=[]),
        dict(name='short_all_zero'),
        dict(name='short_duplicates', base_indices=[3, 3, 4, 6], counts={'3': 1}),
        dict(name='short_negative', counts={'6': -1}),
        dict(name='short_cancelling', counts={'3': 1, '4': -1}),
        dict(name='short_capacity', base_indices=[15, 16], capacity=16, counts={'16': 7}),
        dict(name='short_negative_keep_alive', base_indices=[], keep_alive_count=-1),
        dict(name='short_ignores_original_buildings', base_indices=[], buildings=1),
        dict(name='passive_skips_hook', passive=True, counts={'6': 1}),
        dict(name='normal_unit', short_game=False, active_units=1,
             objects={'unit': [dict(name='ours')]}),
        dict(name='normal_zero_total', short_game=False,
             objects={'unit': [dict(name='ours')]}),
        dict(name='normal_other_owner', short_game=False, active_units=1,
             objects={'unit': [dict(name='other', ours=False)]}),
        dict(name='normal_limbo', short_game=False, active_units=1,
             objects={'unit': [dict(name='limbo', limbo=True)]}),
        dict(name='normal_limbo_parasite', short_game=False, active_units=1,
             objects={'unit': [dict(name='parasite', limbo=True, parasite=True)]}),
        dict(name='normal_class_order', short_game=False, active_units=-1,
             active_infantry=1, active_aircraft=1, objects={
                 'unit': [dict(name='other', ours=False)],
                 'infantry': [dict(name='limbo', limbo=True)],
                 'aircraft': [dict(name='air')]}),
        dict(name='normal_keep_alive_first', short_game=False, keep_alive_count=1,
             active_units=1, objects={'unit': [dict(name='ignored')]}),
        dict(name='normal_totals_without_objects', short_game=False,
             active_units=1, active_infantry=-1, active_aircraft=1),
        dict(name='normal_ignores_refinery', short_game=False, active_yarefn=1)]
    for case in cases:
        for key, value in dict(short_game=True, base_indices=[3, 4, 5, 6], capacity=16,
                               counts={}, keep_alive_count=0, active_units=0,
                               active_infantry=0, active_aircraft=0).items():
            case.setdefault(key, value)
        objects = case.get('objects', {})
        case['objects'] = {category: [dict(name=node['name'], category=category,
            owner='ours' if node.get('ours', True) else 'other',
            limbo=node.get('limbo', False), parasite=node.get('parasite', False))
            for node in objects.get(category, [])] for category in ('unit', 'infantry', 'aircraft')}
    return cases


def generate_ares_survival():
    from tools.rules_oracle.ares_prerequisites import ARES_SHA256, identified_bytes
    ares = identified_bytes(Path(os.environ['VERA20K_ARES_DLL']), ARES_SHA256)
    readers = ares_reader_history(ares)
    pairs = {None: readers['constructor_default'],
             True: next(r['after'] for r in readers['steps'] if r['file'] == 'YES.INI'),
             False: next(r['after'] for r in readers['steps'] if r['file'] == 'NO.INI')}
    fixture = AresFixture(ares)
    tracking = [fixture.tracking(case, pairs[case['keep_alive']]) for case in ares_tracking_inputs()]
    capture = fixture.capture(pairs[True])
    streams = [fixture.stream('unset', pairs[None], tracking[0]['steps'][1]['after']['keep_alive']),
               fixture.stream('true_wrapped', pairs[True], tracking[6]['steps'][0]['after']['keep_alive']),
               fixture.stream('false', pairs[False], tracking[0]['steps'][0]['after']['keep_alive'])]
    survival = [fixture.survival(case) for case in ares_survival_inputs()]
    fixture.checked_code()
    return dict(schema_version=1, reader_histories=[readers], tracking_histories=tracking,
                capture=capture, survival_cases=survival, stream_cases=streams)


def ares_survival_metadata():
    return provenance(
        scope='Ares KeepAlive retained physical reader, tracking hooks/continuations, fullvector short-game and normal Foot eligibility, bounded native stream slices.',
        assumptions=[
            'Ares3.0 exports10024990/10024580/100245D0 enter from installed4F8EBD/4FF71B/4FF563 hooks. Shared register frames carry original saved EDI/ESI/EAX; original CPU context and returned saved EAX are restored before each actual continuation. Neither executable text is patched.',
            'Existing physical Rules retail_history owns synthetic INI file/load/section/ReadString. Full1003C710 establishes nullable KeepAlive+646/+647 unset. Exact field1003F47C..1003F492 runs through original nullable/2048-byte/first-character bool helpers. File sequence is explicit and excludes full Rules Process/type reset, unrelated extension keys and real archive discovery.',
            'Existing gate Fixture supplies House/Rules/type-index/counter priors. All BaseUnit entries are valid nonnull synthetic UnitType pointers; signed index>=capacity skips, corrupt negative indices/pointers are excluded. Native count and scan results are observed, including fourth entry, duplicates, negative/cancelling counts and capacity boundary.',
            'Native HouseExt constructor zero-store prefix executes; Add/Remove_Tracking and real Unit/Building object virtuals, shared Type+DF8 ArrayIndex getter and original CounterClass increments/decrements execute. Supplied per-object type facts and retained nullable pairs come from executed reader controls. Captured transfer executes7015D8..7015EB with old owner pointer retained and the new House explicit in EBP; wider ChangeOwner/deletion bodies are excluded.',
            'Normal scans require nonzero per-class active totals, then original class/vector order and current owner, InLimbo/ParasiteImUsing fields; no health/map-membership equivalence is assumed. The supplied parasite pointer represents allocated ParasiteClass, not eater or attached status.',
            'Native nullable stream helpers and HouseExt+18 save/load field slices execute on a preallocated native byte-vector stream with original copy leaves. They demonstrate retained bytes, not complete SAV files, swizzling or all extension fields. Zero Main/RandomRanged, action-timer-start and detach entries are asserted within these bounded bodies.'],
        substitutions=[
            'Existing Fixture records Blowup_All4FC6D0/MPlayer_Defeated4FC0B0 and returns without executing. Existing house_tracking owned-type-set749020 boundary returns zero at entry; this set is not a survival input. No complete class constructor/lifecycle scheduler, death RNG, full teardown or original scenario comparison.',
            'Retain the existing single gate VM; map Ares over unused lower stack space, retain its historical SP and expand scratch only. Reuse existing physical Rules VM/PE mapper; no new INI/parser/counter/RNG implementation. DebugINIParseFailed is a recorded diagnostic sink.'],
        entry_points=dict(gate=GATE_BEGIN, survival_export=0x10024990,
                          add_tracking=0x4FF700, remove_tracking=0x4FF550,
                          tracking_helper=0x10021F70, type_constructor=0x1003C710,
                          keep_alive_reader=0x1003F47C, nullable_reader=0x1000E9B0,
                          capture_tracking=0x7015D8, nullable_save=0x1000F0C0,
                          nullable_load=0x1000D7E0, house_count_save=0x1001F4D7,
                          house_count_load=0x1001F366))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--outcomes", action="store_true")
    parser.add_argument("--ares-survival", action="store_true")
    selected, remaining = parser.parse_known_args()
    if selected.ares_survival:
        if selected.outcomes:
            parser.error('--ares-survival and --outcomes are separate corpora')
        finish_vectors(generate_ares_survival, Path(__file__).with_name('ares_house_survival.json'),
                       provenance=ares_survival_metadata, argv=remaining)
    elif selected.outcomes:
        finish_vectors(generate_outcomes, Path(__file__).with_name("house_outcomes.json"),
                       provenance=outcome_metadata, argv=remaining)
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata,
                       argv=remaining)
