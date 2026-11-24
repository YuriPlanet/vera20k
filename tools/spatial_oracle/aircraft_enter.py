"""Original AircraftClass::Mission_Enter (0x00419C80), one call per row.

Each row runs the ORIGINAL function (vt+0x240, thiscall, no arguments, RET) on the scratch
Aircraft of aircraft_guard.py: a clone of the Aircraft vtable 0x007E22A4 with only the slots
below replaced by scratch INT3 stubs, entry hooks at native callees, the same per-row assertions
(native cleanup, EBX/EBP/ESI/EDI preserved, no write but the stack and the owner's logged
Mission+0xBC, +0x6CC and +0x6D4, no read of unsupplied scratch bytes) and a log in the
vocabulary of the EnterHost methods of src/sim/aircraft/enter_mission.rs.

Stubs: GetHeight vt+0x1C8 (0x005F5F40), GetCell vt+0x1BC (0x005F6960; answers a scratch cell,
logged as `building_here`), Transmit_Message to contact 0 vt+0x274 (0x0065ACB0; DOCKING 0xE and
DOCK_NOW 0x15 answer the row's values, OVER_OUT 3 answers 0), Queue_Mission vt+0x1E8
(0x0041BA90), the layer vt+0x78 (0x0041ADC0), Enter_Idle_Mode vt+0x484 (0x004176F0),
Assign_Destination vt+0x480 (0x0041AA80), SetLocation vt+0x1B4 (0x004DB810), vt+0xD4 (0x004DB260),
the locomotor's Get_Status (ILocomotion +0x90 on a scratch interface at +0x674), and on the
NavCom (a Building, a Unit and a cell on clones of 0x007E3EBC, 0x007F5C70 and 0x007E4EEC) the
dock coordinate vt+0xA8 and the coordinate vt+0x48, which write the row's coordinate. The
NavCom's What_Am_I (vt+0x2C) and RadioClass::Contact 0x0065AD30 run natively. Entry hooks:
CellClass 0x0047C520 (the cell's first Building; answers the row's), TechnoClass 0x0070D8F0
(the pending entry; answers the row's value), RadioClass::In_Radio_Contact 0x0065AE30, and
PassengerClass 0x004733A0 (records its container and passenger). The Carryall hand-over's
callees (vt+0x124, vt+0x278, 0x00416AF0, 0x00473430, 0x0041A160) and _com_issue_error
0x007DC720 fail a row that reaches them; no row's type is a Carryall (+0xDFC) and every row has
a locomotor.

The log records `step_toward_nav_com` where state 7 takes its descending arm (0x00419F35); that
arm's coordinate query and SetLocation are the row's `step`.
"""
from itertools import product
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_ECX

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.aircraft_guard import (BUILDING_VT, CELL_VT, CONTACT, HERE_CELL, LOCO,
                                                 LOCO_VT, OBJECTS, OWNER, RADIO_ITEMS, TYPE,
                                                 UNIT_VT, ScratchAircraft, s32, u32)

MISSION_ENTER, ENTER_BODY = 0x419C80, (0x419C80, 0x41A13F)
# NavCom, dock and cell-building candidates.
BLD_A, BLD_B, CELL, UNIT = (OBJECTS + 0x1000 * n for n in range(4))
OBJECT_AT = {"bld_a": BLD_A, "bld_b": BLD_B, "cell": CELL, "unit": UNIT}
# Radio messages vt+0x274 sends: OVER_OUT, DOCKING, DOCK_NOW.
MESSAGES = {3: "over_out", 0xE: "docking", 0x15: "dock_now"}

# Cloned vtable slots: offset -> (stub name, native target asserted against the image, pops).
SLOTS = {
    0x1C8: ("height", 0x5F5F40, 0), 0x1BC: ("get_cell", 0x5F6960, 0),
    0x274: ("transmit", 0x65ACB0, 4), 0x1E8: ("queue_mission", 0x41BA90, 8),
    0x78: ("layer", 0x41ADC0, 0), 0x484: ("enter_idle_mode", 0x4176F0, 8),
    0x480: ("assign_destination", 0x41AA80, 8), 0x1B4: ("set_location", 0x4DB810, 4),
    0xD4: ("limbo", 0x4DB260, 0),
    # The Carryall hand-over's (0x0041A07B..0x0041A124).
    0x124: ("vt124", 0x4D3780, None), 0x278: ("transmit_message", 0x65AAA0, None),
}
NATIVE_SLOTS = {0x240: MISSION_ENTER}
STUBS = {"status": 4, "dock_coord": 8, "center_coord": 4}
HOOKS = {
    0x47C520: ("cell_building", 0), 0x70D8F0: ("pending_entry", 0),
    0x65AE30: ("in_radio_contact", 0), 0x4733A0: ("add_passenger", 4),
    0x7DC720: ("com_issue_error", None), 0x41A160: ("nearest_airfield", None),
    0x416AF0: ("hand_over_416af0", None), 0x473430: ("first_passenger", None),
}
# State 7's descending arm.
OBSERVERS = {0x419F35: "step"}


class Enter(ScratchAircraft):
    def __init__(self):
        super().__init__(SLOTS, NATIVE_SLOTS, STUBS, HOOKS, OBSERVERS)
        self.names = {OWNER: "aircraft", CONTACT: "contact"} | {
            address: name for name, address in OBJECT_AT.items()}

    def build(self, row):
        self.reset(row, ENTER_BODY)
        self.step = []
        nav, dock = (OBJECT_AT[row[key]] if row[key] else 0 for key in ("nav", "dock"))
        self.put32(OWNER + 0x5A4, nav)
        self.put32(OWNER + 0x6CC, dock)
        self.put32(OWNER + 0xB4, row["queued"])
        self.put32(OWNER + 0x674, LOCO)
        self.put32(LOCO, LOCO_VT)
        self.put32(LOCO_VT + 0x90, self.stub_at["status"])
        self.put32(OWNER + 0xE4, RADIO_ITEMS)
        self.put32(OWNER + 0xE8, 1)
        self.put32(RADIO_ITEMS, CONTACT if row["radio"] else 0)
        self.put8(TYPE + 0xE0D, row["airport_bound"])
        self.put8(TYPE + 0xDFC, 0)
        for name in ("bld_a", "bld_b"):
            self.object(OBJECT_AT[name], BUILDING_VT, {0xA8: "dock_coord", 0x48: "center_coord"})
        self.object(CELL, CELL_VT, {0x48: "center_coord"})
        self.object(UNIT, UNIT_VT, {0x48: "center_coord"})

    def stub(self, name):
        row, u = self.row, self.u
        if name == "status":
            if self.arg(0) != LOCO:
                self.fail(f"Get_Status on 0x{self.arg(0):08X}")
            self.calls.append(["status"])
            return row["status"]
        if name in ("dock_coord", "center_coord"):
            nav = OBJECT_AT.get(row["nav"])
            if not self.this(nav, name):
                return 0
            out = self.arg(0)
            self.step.append(["dock_coord", self.name(self.arg(1))] if name == "dock_coord"
                             else ["center_coord"])
            u.mem_write(out, b"".join(u32(v) for v in row["nav_coord"]))
            return out
        if not self.this(OWNER, name):
            return 0
        if name == "height":
            self.calls.append(["height"])
            return row["height"]
        if name == "get_cell":
            self.calls.append(["building_here"])
            return HERE_CELL
        if name == "layer":
            self.calls.append(["on_ground_layer"])
            return row["layer"]
        if name == "transmit":
            message = self.arg(0)
            if message not in MESSAGES:
                self.calls.append(["transmit", s32(message)])
                return 0
            self.calls.append([MESSAGES[message]])
            return {3: 0, 0xE: row["docking"], 0x15: row["dock_now"]}[message]
        if name == "queue_mission":
            self.calls.append(["queue", s32(self.arg(0)), s32(self.arg(1))])
        elif name == "enter_idle_mode":
            self.calls.append(["enter_idle_mode", s32(self.arg(0)), s32(self.arg(1))])
        elif name == "assign_destination":
            target, flag = self.arg(0), s32(self.arg(1))
            self.calls.append(["clear_destination", flag] if target == 0
                              else ["assign_destination", self.name(target), flag])
        elif name == "set_location":
            self.step.append(["set_location", self.coord(self.arg(0))])
        elif name == "limbo":
            self.calls.append(["limbo"])
            return 1
        return 0

    def hook(self, name):
        row, u = self.row, self.u
        if name == "cell_building":
            return OBJECT_AT[row["here"]] if self.this(HERE_CELL, name) and row["here"] else 0
        if name == "pending_entry":
            self.calls.append(["approach_pending_entry"])
            return self.this(OWNER, name) and row["pending"]
        if name == "in_radio_contact":
            self.calls.append(["in_radio_contact"])
            return self.this(OWNER, name) and row["radio"]
        # add_passenger: PassengerClass +0x114 of the contact, the aircraft its passenger.
        container = u.reg_read(UC_X86_REG_ECX) - 0x114
        self.calls.append(["add_passenger", self.name(container), self.name(self.arg(0))])
        return 0

    def observe(self, name):
        self.calls.append(["step_toward_nav_com"])

    def enter(self, row):
        row = {**DEFAULTS, **row}
        self.build(row)
        result = self.execute(MISSION_ENTER)
        result["dock"] = self.name(self.word(OWNER + 0x6CC))
        result["step"] = self.step
        return result


LOCATION = [15488, 16512, 200]
DEFAULTS = {
    "state": 1, "height": 200, "airport_bound": False, "nav": "bld_a", "dock": "bld_a",
    "here": "bld_a", "layer": 3, "docking": 1, "pending": False, "radio": True, "status": 0,
    "queued": -1, "dock_now": 1, "location": LOCATION, "nav_coord": [15488, 16512, 0],
    "ready": 2,
}
NAVS = ("cell", "unit", "bld_a")
BUILDINGS = (None, "bld_a", "bld_b")
# NavCom coordinate offsets from the Location for the landing step.
STEPS = ((0, 0), (3, -4), (5, -5), (6, -6), (-6, 6), (-300, 1000))


def bits(**flags):
    return ".".join(f"{key}{int(value) if isinstance(value, bool) else value}"
                    for key, value in flags.items())


def enter_rows():
    rows = []
    # Before the state switch, from state 1 (which only sets +0x6D4): GetHeight 0, 200 or -1 x
    # AirportBound, without a NavCom over the layer, the DOCKING answer and a pending entry, and
    # with a cell, Unit or Building NavCom over the cell's Building and the dock.
    for height, airport_bound in product((0, 200, -1), (False, True)):
        for layer, docking, pending in product((2, 3), (1, 0), (False, True)):
            rows.append(dict(name=f"head.{bits(z=height, ab=airport_bound)}.none"
                                  f".{bits(l=layer, dk=docking, p=pending)}",
                             height=height, airport_bound=airport_bound, nav=None, dock=None,
                             here=None, layer=layer, docking=docking, pending=pending))
        for nav, here, dock in product(NAVS, BUILDINGS, BUILDINGS):
            rows.append(dict(name=f"head.{bits(z=height, ab=airport_bound)}.{nav}"
                                  f".here_{here}.dock_{dock}",
                             height=height, airport_bound=airport_bound, nav=nav, here=here,
                             dock=dock))
    # Every state with every locomotor status, and once with every other read flipped.
    for state in (0, 1, 2, 3, 4, 5, 8, 9, -1):
        for status in range(4):
            rows.append(dict(name=f"state{state}.status{status}", state=state, status=status))
        rows.append(dict(name=f"state{state}.flipped", state=state, status=1, height=0,
                         airport_bound=True, radio=False, pending=True, queued=2, docking=0,
                         dock_now=10))
    # State 6 over every read.
    for status, radio, pending, queued, airport_bound, height, docking in product(
            range(4), (False, True), (False, True), (-1, 2), (False, True), (0, 200), (1, 0)):
        rows.append(dict(name=f"state6.{bits(s=status, r=radio, p=pending, q=queued)}"
                              f".{bits(ab=airport_bound, z=height, dk=docking)}",
                         state=6, status=status, radio=radio, pending=pending, queued=queued,
                         airport_bound=airport_bound, height=height, docking=docking))
    # State 7 over the status, the DOCK_NOW answer, AirportBound and GetHeight.
    for status, dock_now, airport_bound, height in product(range(4), (1, 5, 0, 10),
                                                          (False, True), (0, 200)):
        rows.append(dict(name=f"state7.{bits(s=status, dn=dock_now, ab=airport_bound, z=height)}",
                         state=7, status=status, dock_now=dock_now, airport_bound=airport_bound,
                         height=height))
    # The landing step toward each NavCom, and without one.
    for nav, (dx, dy) in product(NAVS, STEPS):
        rows.append(dict(name=f"step.{nav}.dx{dx}.dy{dy}", state=7, status=1, nav=nav,
                         nav_coord=[LOCATION[0] + dx, LOCATION[1] + dy, 0]))
    rows.append(dict(name="step.none", state=7, status=1, nav=None, dock=None, layer=2))
    # Without a NavCom, the checks before the switch interleave with states 0, 6 and 7.
    for state, (layer, docking, pending), status in product(
            (0, 6, 7), ((2, 0, False), (3, 1, False), (3, 0, True)), (0, 1)):
        rows.append(dict(name=f"none.state{state}.{bits(l=layer, dk=docking, p=pending)}"
                              f".status{status}",
                         state=state, nav=None, dock=None, layer=layer, docking=docking,
                         pending=pending, status=status))
    return rows


def generate():
    fixture = Enter()
    return {"mission_enter": [fixture.enter(row) for row in enter_rows()]}


def metadata():
    return provenance(
        scope="AircraftClass::Mission_Enter 0x00419C80 as vt+0x240, one call per row. Before "
              "the state switch (from state 1): GetHeight 0/200/-1 x AirportBound (+0xE0D), "
              "without a NavCom over the layer (2, 3) x the DOCKING answer (ROGER or not) x a "
              "pending entry, and with a cell, Unit or Building NavCom over the cell's first "
              "Building (none, the NavCom, another) x the dock +0x6CC (none, the NavCom, "
              "another). Mission+0xBC 0..5, 8, 9 and -1 with each Get_Status 0..3, and once "
              "with every other read flipped. State 6 over Get_Status 0..3 x In_Radio_Contact x "
              "a pending entry x the queued mission (none or Move) x AirportBound x GetHeight "
              "0/200 x the DOCKING answer. State 7 over Get_Status 0..3 x the DOCK_NOW answer "
              "(1, 5, 0, 10) x AirportBound x GetHeight 0/200. The descending step toward a "
              "cell, Unit or Building NavCom at offsets within and beyond 5 leptons, and "
              "without a NavCom. States 0, 6 and 7 without a NavCom. Returned value, ordered "
              "call log in EnterHost vocabulary, the writes to Mission+0xBC, +0x6CC and +0x6D4 "
              "in order, Mission+0xBC, +0x6D4 and the dock afterwards, and the step's "
              "coordinate query and SetLocation argument. Not a Carryall type's hand-over, a "
              "missing locomotor, the radio exchanges' or the pending entry's own work.",
        assumptions=[
            "One emulator; per row the scratch region (0x10000 bytes) and the stack frame are "
            "rewritten, as aircraft_guard.py's ScratchAircraft does. The owner holds a clone of "
            "the Aircraft vtable 0x007E22A4 with only the stubbed slots replaced, the NavCom "
            "+0x5A4, the dock +0x6CC, the queued mission +0xB4, a scratch ILocomotion +0x674, "
            "the Location +0x9C, the radio vector +0xE4/+0xE8 (the contact present when the "
            "row's In_Radio_Contact is) and its type +0x6C4 with the row's AirportBound "
            "(+0xE0D) and Carryall (+0xDFC) 0.",
            "Mission+0xBC holds the row's state and +0x6D4 is preset 2 so a write shows.",
            "Per row: RET to the caller with ESP + 4 and EBX/EBP/ESI/EDI preserved; no write "
            "outside the stack but the logged owner fields; every read of scratch bytes falls "
            "on bytes the row supplied.",
        ],
        substitutions=[
            "GetHeight vt+0x1C8, the layer vt+0x78 and Get_Status (ILocomotion +0x90) answer "
            "the row's values; GetCell vt+0x1BC answers a scratch cell whose first Building "
            "(0x0047C520 hook) is the row's; Transmit_Message vt+0x274 answers the row's "
            "DOCKING and DOCK_NOW answers and 0 to OVER_OUT; Queue_Mission vt+0x1E8, "
            "Enter_Idle_Mode vt+0x484, Assign_Destination vt+0x480, SetLocation vt+0x1B4 and "
            "vt+0xD4 record their arguments; the NavCom's vt+0xA8 and vt+0x48 record theirs "
            "and write the row's coordinate.",
            "Entry hooks: TechnoClass 0x0070D8F0 and In_Radio_Contact 0x0065AE30 answer the "
            "row's values; PassengerClass 0x004733A0 records its container and passenger.",
        ],
        entry_points={"mission_enter": MISSION_ENTER, "radio_contact": 0x65AD30,
                      "building_what_am_i": 0x459EC0, "unit_what_am_i": 0x746E20,
                      "cell_what_am_i": 0x487E60})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
