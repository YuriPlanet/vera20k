"""Original AircraftClass::Mission_Hunt (0x00414A80), one call per row.

Each row runs the ORIGINAL function (vt+0x228, thiscall, no arguments, RET) on the scratch
Aircraft of aircraft_guard.py: a clone of the Aircraft vtable 0x007E22A4 with only the slots
below replaced by scratch INT3 stubs, entry hooks at native callees, the same per-row assertions
(native cleanup, EBX/EBP/ESI/EDI preserved, no write but the stack and the owner's logged
Mission+0xBC, +0x6CC and +0x6D4, no read of unsupplied scratch bytes) and a log in the
vocabulary of the HuntHost methods of src/sim/aircraft/hunt_mission.rs.

Stubs: Greatest_Threat vt+0x3C4 (0x004D9920; records its mask and third argument, asserts the
coordinate it is handed is the Location, and answers the row's result for that call),
Assign_Target vt+0x3C8 (0x006FCDB0; writes its argument to the Target +0x2B4, as the original
does), Queue_Mission vt+0x1E8 (0x0041BA90) and Enter_Idle_Mode vt+0x484 (0x004176F0; in the
rows that say so it commences another mission, writing it to the current mission +0xAC). Entry
hooks: TeamClass::Remove_Member 0x006EA870 (records the team, member and arguments) and Scenario
RandomRanged 0x0065C7E0 (on Scenario+0x218 of the Scenario pointer 0x00A8B230). The original
MissionControl lookup 0x005B3A00 runs: it indexes the MissionControlClass array 0x00A8E3A8 by
the current mission. Each row writes there a Rate +0x10 of (frames + 0.5) / 900 per mission, so
the native fmul by 900.0 and ftol 0x007C5F00 under the game's control word 0x0E7F produce that
mission's frames, which the row asserts after the ftol. The game mode the function reads at
0x00A8B238 is the row's. A read of the Target +0x2B4 from the function is logged as `target`.
"""
from itertools import product
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.aircraft_guard import (HOUSE, OWNER, SCENARIO, TARGET, TEAM,
                                                 ScratchAircraft, s32, u32)

MISSION_HUNT, HUNT_BODY = 0x414A80, (0x414A80, 0x414BAE)
GAME_MODE = 0xA8B238
# MissionControlClass array (0x20 per mission) that 0x005B3A00 indexes; Hunt's id.
MISSION_CONTROLS, HUNT = 0xA8E3A8, 15
# Each mission's frames: Hunt, and what the idle mode may commence (Enter, Guard, Move, Retreat).
RATES = {HUNT: 30, 7: 14, 5: 27, 2: 0, 4: 450}
# What each Greatest_Threat call may answer.
THREATS = {"harvester": TARGET, "any": TARGET + 0x80}

# Cloned vtable slots: offset -> (stub name, native target asserted against the image, pops).
SLOTS = {
    0x3C4: ("greatest_threat", 0x4D9920, 0xC), 0x3C8: ("assign_target", 0x6FCDB0, 4),
    0x1E8: ("queue_mission", 0x41BA90, 8), 0x484: ("enter_idle_mode", 0x4176F0, 8),
}
NATIVE_SLOTS = {0x228: MISSION_HUNT}
HOOKS = {0x6EA870: ("leave_team", 0xC), 0x65C7E0: ("random_ranged", 8)}
# The instruction after the ftol of the Rate.
OBSERVERS = {0x414AC7: "rate_frames"}


class Hunt(ScratchAircraft):
    def __init__(self):
        super().__init__(SLOTS, NATIVE_SLOTS, {}, HOOKS, OBSERVERS)
        self.read_observers = {OWNER + 0x2B4: "target"}
        self.names = {OWNER: "aircraft", TEAM: "team"} | {
            address: name for name, address in THREATS.items()}

    def build(self, row):
        self.reset(row, HUNT_BODY)
        self.scans = list(row["scans"])
        self.put32(OWNER + 0x21C, HOUSE)
        self.put32(OWNER + 0x2FC, row["ammo"])
        self.put32(OWNER + 0x5D4, TEAM if row["team"] else 0)
        self.put32(OWNER + 0x2B4, THREATS["any"] if row["target"] else 0)
        self.put32(OWNER + 0xAC, HUNT)
        for mission, frames in row["rates"].items():
            self.u.mem_write(MISSION_CONTROLS + 0x20 * int(mission) + 0x10,
                             struct.pack("<d", (frames + 0.5) / 900))
        self.u.mem_write(GAME_MODE, u32(row["game_mode"]))

    def stub(self, name):
        if not self.this(OWNER, name):
            return 0
        if name == "greatest_threat":
            mask, coord, arg3 = self.arg(0), self.arg(1), self.arg(2)
            if self.coord(coord) != self.row["location"]:
                self.fail(f"Greatest_Threat at {self.coord(coord)}, not the Location")
                return 0
            self.calls.append(["greatest_threat", s32(mask), s32(arg3)])
            if not self.scans:
                self.fail("Greatest_Threat called more often than the row answers")
                return 0
            answer = self.scans.pop(0)
            return THREATS[answer] if answer else 0
        if name == "assign_target":
            target = self.arg(0)
            self.calls.append(["assign_target", self.name(target)])
            self.put32(OWNER + 0x2B4, target)
        elif name == "queue_mission":
            self.calls.append(["queue", s32(self.arg(0)), s32(self.arg(1))])
        elif name == "enter_idle_mode":
            self.calls.append(["enter_idle_mode", s32(self.arg(0)), s32(self.arg(1))])
            if self.row["idle_commences"] is not None:
                self.put32(OWNER + 0xAC, self.row["idle_commences"])
        return 0

    def hook(self, name):
        row = self.row
        if name == "leave_team":
            if not self.this(TEAM, name):
                return 0
            self.calls.append(["leave_team", self.name(self.arg(0)), s32(self.arg(1)),
                               s32(self.arg(2))])
            return 0
        self.calls.append(["jitter", s32(self.arg(0)), s32(self.arg(1))])
        return row["jitter"] if self.this(SCENARIO + 0x218, name) else 0

    def observe(self, name):
        frames = s32(self.u.reg_read(UC_X86_REG_EAX))
        current = self.word(OWNER + 0xAC)
        if frames != self.row["rates"][current]:
            self.fail(f"ftol answered {frames} for mission {current}")

    def hunt(self, row):
        row = {**DEFAULTS, **row}
        self.build(row)
        result = self.execute(MISSION_HUNT)
        if self.scans:
            raise ValueError(f"{row['name']}: unused scan answers {self.scans}")
        result["target"] = self.name(self.word(OWNER + 0x2B4))
        return result


DEFAULTS = {
    "ammo": 1, "team": False, "target": False, "game_mode": 1, "scans": [], "rates": RATES,
    "idle_commences": None, "jitter": 2, "location": [15488, 16512, 600], "state": 0,
    "ready": 2,
}
# What each scan answers, in call order: the harvester pass, then the plain one.
SCAN_ANSWERS = {
    1: (("harvester",), (None, "any"), (None, None)),
    0: (("any",), (None,)),
}


def bits(**flags):
    return ".".join(f"{key}{int(value) if isinstance(value, bool) else value}"
                    for key, value in flags.items())


def hunt_rows():
    rows = []
    # Without Ammo, in a team or not, whatever else holds: the jitter varies, and the idle
    # mode keeps Hunt or commences another mission, whose Rate the return takes.
    for ammo, team, target, game_mode in product((0,), (False, True), (False, True), (0, 1)):
        for idle, jitter in ((None, 0), (None, 2), (7, 1), (5, 2), (2, 1), (4, 0)):
            rows.append(dict(name=f"empty.{bits(t=team, tg=target, gm=game_mode)}"
                                  f".idle{idle if idle is not None else 'hunt'}.j{jitter}",
                             ammo=ammo, team=team, target=target, game_mode=game_mode,
                             idle_commences=idle, jitter=jitter))
    # With Ammo (1, unlimited -1, 3): a held Target, or the scans in each game mode.
    for ammo, team in product((1, -1, 3), (False, True)):
        rows.append(dict(name=f"held.ammo{ammo}.{bits(t=team)}", ammo=ammo, team=team,
                         target=True))
        for game_mode in (0, 1, 2):
            for scans in SCAN_ANSWERS[min(game_mode, 1)]:
                label = "_".join(answer or "none" for answer in scans)
                rows.append(dict(name=f"scan.ammo{ammo}.{bits(t=team, gm=game_mode)}.{label}",
                                 ammo=ammo, team=team, game_mode=game_mode, scans=scans))
    return rows


def generate():
    fixture = Hunt()
    return {"mission_hunt": [fixture.hunt(row) for row in hunt_rows()]}


def metadata():
    return provenance(
        scope="AircraftClass::Mission_Hunt 0x00414A80 as vt+0x228, one call per row. Without "
              "Ammo (+0x2FC 0): a team (+0x5D4) or not x a Target or not x the game mode "
              "(0x00A8B238) 0/1, over six idle-mode/jitter pairs: the idle mode keeps Hunt or "
              "commences Enter, Guard, Move or Retreat, each with its own Rate. With Ammo 1, -1 "
              "or 3, in a team or not: a held Target, and without one in game mode 0, 1 and 2 "
              "over what each Greatest_Threat call answers. Returned value, the ordered call "
              "log in HuntHost vocabulary (Target reads included) and the Target afterwards. "
              "Not Greatest_Threat's, Assign_Target's, Queue_Mission's, Enter_Idle_Mode's or "
              "Remove_Member's own work.",
        assumptions=[
            "One emulator; per row the scratch region (0x10000 bytes) and the stack frame are "
            "rewritten, as aircraft_guard.py's ScratchAircraft does. The owner holds a clone of "
            "the Aircraft vtable 0x007E22A4 with only the stubbed slots replaced, Ammo +0x2FC, "
            "the team +0x5D4, the Target +0x2B4, the house +0x21C, the Location +0x9C and the "
            "current mission +0xAC (Hunt). The MissionControlClass array 0x00A8E3A8 holds the "
            "row's Rate for each mission it names.",
            "Per row: RET to the caller with ESP + 4 and EBX/EBP/ESI/EDI preserved; no write "
            "outside the stack but the logged owner fields; every read of scratch bytes falls "
            "on bytes the row supplied.",
        ],
        substitutions=[
            "Greatest_Threat vt+0x3C4 answers the row's result per call and Assign_Target "
            "vt+0x3C8 stores its argument as the Target; Queue_Mission vt+0x1E8 records its "
            "arguments, and Enter_Idle_Mode vt+0x484 records its arguments and writes the "
            "row's commenced mission to +0xAC.",
            "Entry hooks: TeamClass::Remove_Member 0x006EA870 records its arguments; "
            "RandomRanged 0x0065C7E0 answers the row's jitter.",
        ],
        entry_points={"mission_hunt": MISSION_HUNT, "ftol": 0x7C5F00})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
