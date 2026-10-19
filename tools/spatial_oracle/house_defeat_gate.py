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


if __name__ == "__main__":
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--outcomes", action="store_true")
    selected, remaining = parser.parse_known_args()
    if selected.outcomes:
        finish_vectors(generate_outcomes, Path(__file__).with_name("house_outcomes.json"),
                       provenance=outcome_metadata, argv=remaining)
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata,
                       argv=remaining)
