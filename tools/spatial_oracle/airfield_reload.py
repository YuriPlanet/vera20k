"""Original BuildingClass::Mission_Repair (0x0044B780) on a UnitReload= airfield, one call per
row: the arm 0x0044C836..0x0044C96F that services the aircraft on its pads.

Each row runs the ORIGINAL function (thiscall, no arguments, RET) on the scratch objects of
aircraft_guard.py's ScratchAircraft, with the airfield as the owner: a clone of the Building
vtable 0x007E3EBC whose Transmit_Message vt+0x278 (0x0065AAA0) and Queue_Mission vt+0x1E8
(0x005B35E0) are scratch INT3 stubs, and a scratch BuildingTypeClass (+0x520) with UnitReload=
(+0x16AA) set and the bytes that select the arms before it clear: Bunker (+0x16AB),
ConstructionYard (+0x16B9), Hospital (+0x16C1), Armory (+0x16C2) and UnitRepair (+0x16A9). Its
radio vector (+0xE4 items, +0xE8 count) holds the row's slots; RadioClass::Contact_With_Whom
0x0065AD30 runs natively over it. The contacts are Aircraft on a clone of the Aircraft vtable
0x007E22A4 whose Get_Mission vt+0x184 (0x005B3040), Queue_Mission vt+0x1E8 (0x0041BA90),
Assign_Mission vt+0x1F0 (0x0041B9F0), vt+0x334 (0x004DE580) and Enter_Idle_Mode vt+0x484
(0x004176F0) are stubs. Their type getter vt+0x84 (0x006F3270, a jump to vt+0x88 0x0041C200,
which reads +0x6C4) runs natively, so the arm itself compares the row's Health (+0x6C) with the
Strength (+0xA0) of a scratch AircraftTypeClass. ReloadRate is the row's double at +0x1508 of a
scratch RulesClass named by the Rules pointer 0x008871E0; the arm's FMUL by the double 900.0 at
0x007E27F8 and ftol 0x007C5F00 run natively under the game's control word 0x0E7F.

A stub records its call and returns with the native callee's stack cleanup. Transmit_Message
answers the contact's code for that message (a message the row does not answer fails it);
Get_Mission answers the contact's missions in order, the last one repeating. A row's `recount`
makes vt+0x334 of that contact set the count +0xE8, so the arm's re-read of the count after each
slot shows. Every row asserts what ScratchAircraft.execute asserts (native cleanup,
EBX/EBP/ESI/EDI preserved, no write outside the stack, no read of unsupplied scratch bytes) and
that the arm was entered.

The log is in the vocabulary of the ReloadHost of src/sim/docking/airfield_reload.rs, with the
native arguments: `count` for each read of +0xE8 (0x0044C844, 0x0044C920), `contact N` for each
Contact_With_Whom(N) (0x0044C85C), `transmit 0xMM c` for Transmit_Message(message, contact),
`full_strength c` for the type getter's call (0x0044C882), `mission c` for Get_Mission,
`queue_mission c M A`, `enter_idle_mode c A B`, `assign_mission c M` and `vt334 c` on a contact,
and `queue_mission airfield M A` for the airfield's own Queue_Mission.
"""
from collections import defaultdict
from itertools import product
from pathlib import Path
import math
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP

from tools.native_oracle import SCRATCH, OracleError, finish_vectors, provenance
from tools.spatial_oracle.aircraft_guard import (BUILDING_VT, OBJECTS, OWNER, PAD_TYPE,
                                                 RADIO_ITEMS, TYPE, VTABLE, ScratchAircraft, s32,
                                                 u32)

MISSION_REPAIR, REPAIR_BODY = 0x44B780, (0x44B780, 0x44C97D)
RELOAD_ARM = 0x44C836
RULES_POINTER, RULES = 0x8871E0, SCRATCH + 0xD000   # RulesClass: ReloadRate +0x1508
AIRFIELD_TYPE = PAD_TYPE                            # BuildingTypeClass
CONTACT_AT = {name: OBJECTS + 0x1000 * n for n, name in enumerate("abcd")}
# The airfield type's arm selectors, in the order Mission_Repair tests them.
TYPE_FLAGS = {"bunker": 0x16AB, "construction_yard": 0x16B9, "hospital": 0x16C1,
              "armory": 0x16C2, "unit_repair": 0x16A9, "unit_reload": 0x16AA}

# The contacts' Aircraft vtable slots: offset -> (stub name, native target, pops).
SLOTS = {
    0x184: ("get_mission", 0x5B3040, 0), 0x1E8: ("queue_mission", 0x41BA90, 8),
    0x1F0: ("assign_mission", 0x41B9F0, 4), 0x334: ("vt334", 0x4DE580, 0),
    0x484: ("enter_idle_mode", 0x4176F0, 8),
}
# Run natively: the type getter vt+0x84 jumps to vt+0x88, which reads +0x6C4.
NATIVE_SLOTS = {0x84: 0x6F3270, 0x88: 0x41C200}
# The airfield's Building vtable slots: offset -> (stub name, native target, pops).
AIRFIELD_SLOTS = {0x278: ("transmit", 0x65AAA0, 8),
                  0x1E8: ("airfield_queue_mission", 0x5B35E0, 8)}
OBSERVERS = {RELOAD_ARM: "arm", 0x44C844: "count", 0x44C920: "count", 0x44C85C: "contact",
             0x44C882: "full_strength"}

ROGER, NEGATORY = 1, 0x0A
QUERY_RELOADED, NEED_TO_MOVE, RELOAD, REPAIR = 0x1D, 0x13, 0x1F, 0x1C
SLEEP, GUARD, ENTER, NO_MISSION = 0, 5, 7, -1
STRENGTH = 200


def f64_bits(value):
    return f"0x{struct.unpack('<Q', struct.pack('<d', value))[0]:016X}"


class Airfield(ScratchAircraft):
    def __init__(self):
        super().__init__(SLOTS, NATIVE_SLOTS,
                         {name: pops for name, _, pops in AIRFIELD_SLOTS.values()}, {}, OBSERVERS)
        for offset, (_, native, _) in AIRFIELD_SLOTS.items():
            if self.word(BUILDING_VT + offset) != native:
                raise OracleError(f"Building vt+0x{offset:X} is not 0x{native:08X}")
        self.u.mem_write(RULES_POINTER, u32(RULES))
        self.names = {OWNER: "airfield"} | {address: name for name, address in CONTACT_AT.items()}

    def build(self, row):
        # ScratchAircraft.reset lays out an Aircraft owner; the airfield replaces it whole.
        self.reset({**row, "location": [0, 0, 0], "state": 0, "ready": 0}, REPAIR_BODY)
        self.u.mem_write(OWNER, bytes(0x1000))
        self.declared = [(low, high) for low, high in self.declared
                         if not OWNER <= low < OWNER + 0x1000]
        self.entered, self.mission_reads = False, defaultdict(int)
        self.object(OWNER, BUILDING_VT,
                    {offset: name for offset, (name, _, _) in AIRFIELD_SLOTS.items()})
        self.put32(OWNER + 0x520, AIRFIELD_TYPE)
        for flag, offset in TYPE_FLAGS.items():
            self.put8(AIRFIELD_TYPE + offset, flag == "unit_reload")
        slots, recount = row["slots"], row.get("recount")
        if max(row["count"], recount["count"] if recount else 0) > len(slots):
            raise OracleError(f"{row['name']}: the count reaches past the supplied slots")
        self.put32(OWNER + 0xE4, RADIO_ITEMS, row["count"])
        if slots:
            self.put32(RADIO_ITEMS, *(CONTACT_AT[name] if name else 0 for name in slots))
        self.put32(TYPE + 0xA0, row["strength"])
        for name, contact in row["contacts"].items():
            self.put32(CONTACT_AT[name], VTABLE)
            self.put32(CONTACT_AT[name] + 0x6C, contact["health"])
            self.put32(CONTACT_AT[name] + 0x6C4, TYPE)
        self.put(RULES + 0x1508, struct.pack("<Q", int(row["reload_rate"], 16)))

    def contact(self, pointer, what):
        name = self.name(pointer)
        if name not in self.row["contacts"]:
            self.fail(f"{what} on {name}")
            return None
        return name

    def stub(self, name):
        row, u = self.row, self.u
        if name == "airfield_queue_mission":
            if self.this(OWNER, name):
                self.calls.append(f"queue_mission airfield {s32(self.arg(0))} {s32(self.arg(1))}")
            return 0
        if name == "transmit":
            contact = self.contact(self.arg(1), name) if self.this(OWNER, name) else None
            if contact is None:
                return 0
            message = self.arg(0)
            self.calls.append(f"transmit 0x{message:02X} {contact}")
            answer = row["contacts"][contact]["answers"].get(f"0x{message:02X}")
            if answer is None:
                self.fail(f"{contact} has no answer to 0x{message:02X}")
                return 0
            return answer
        contact = self.contact(u.reg_read(UC_X86_REG_ECX), name)
        if contact is None:
            return 0
        if name == "get_mission":
            self.calls.append(f"mission {contact}")
            missions, read = row["contacts"][contact]["missions"], self.mission_reads[contact]
            self.mission_reads[contact] += 1
            return missions[min(read, len(missions) - 1)]
        if name == "assign_mission":
            self.calls.append(f"assign_mission {contact} {s32(self.arg(0))}")
        elif name == "vt334":
            self.calls.append(f"vt334 {contact}")
            recount = row.get("recount")
            if recount and recount["contact"] == contact:
                u.mem_write(OWNER + 0xE8, u32(recount["count"]))
        else:                                           # queue_mission, enter_idle_mode
            self.calls.append(f"{name} {contact} {s32(self.arg(0))} {s32(self.arg(1))}")
        return 0

    def observe(self, name):
        u = self.u
        if name == "arm":
            self.entered = True
        elif name == "count":
            self.calls.append("count")
        elif name == "contact":
            self.calls.append(f"contact {s32(self.word(u.reg_read(UC_X86_REG_ESP)))}")
        else:
            self.calls.append(f"full_strength {self.name(u.reg_read(UC_X86_REG_ECX))}")

    def visit(self, row):
        row = {**DEFAULTS, **row}
        self.build(row)
        result = self.execute(MISSION_REPAIR)
        if not self.entered:
            raise OracleError(f"{row['name']}: the arm 0x{RELOAD_ARM:08X} was not reached")
        return {"input": row, "ret": result["ret"], "calls": result["calls"]}


# What CCINIClass::ReadDouble (0x005283D0) stores for retail `ReloadRate=.3`: sscanf "%f"
# scans a float, widened to the double field, not 0.3's own double (src/rules/ini_value.rs
# parse_read_double).
RETAIL_RELOAD_RATE = struct.unpack("<f", struct.pack("<f", 0.3))[0]

DEFAULTS = {"strength": STRENGTH, "reload_rate": f64_bits(RETAIL_RELOAD_RATE)}


def contact(query_reloaded, need_to_move, reload, repair, health=STRENGTH, missions=(GUARD,)):
    answers = {QUERY_RELOADED: query_reloaded, NEED_TO_MOVE: need_to_move, RELOAD: reload,
               REPAIR: repair}
    return {"answers": {f"0x{message:02X}": answer for message, answer in answers.items()},
            "health": health, "missions": list(missions)}


# One contact each way through the arm.
PROFILES = {
    "full": contact(ROGER, NEGATORY, NEGATORY, NEGATORY),           # released at Strength
    "entering": contact(NEGATORY, ROGER, ROGER, ROGER, missions=(ENTER,)),
    "parked": contact(NEGATORY, NEGATORY, ROGER, ROGER),            # refuses NEED_TO_MOVE
    "landing": contact(NEGATORY, ROGER, ROGER, ROGER),              # serviced: queued Sleep
    "reloading": contact(NEGATORY, ROGER, ROGER, ROGER, missions=(SLEEP,)),
    "repairing": contact(NEGATORY, ROGER, NEGATORY, ROGER, missions=(SLEEP,)),
    "done": contact(NEGATORY, ROGER, NEGATORY, NEGATORY, missions=(SLEEP,)),  # then released
}


def pads(*profiles, count=None, **extra):
    """Slot n holds contact "abcd"[n] with that profile, or nothing."""
    names = [name if profile else None for name, profile in zip("abcd", profiles)]
    return dict(slots=names, count=len(profiles) if count is None else count,
                contacts={name: PROFILES[profile] for name, profile in zip("abcd", profiles)
                          if profile}, **extra)


def below(value):
    return math.nextafter(value, -math.inf)


def above(value):
    return math.nextafter(value, math.inf)


# ReloadRate values (minutes): the default, retail's and 0.3's own double, products with 900
# just below, at and above an integer, zero and negatives, past int32 and int64, infinity and
# a subnormal.
RATES = (
    ("default", 0.05), ("retail", RETAIL_RELOAD_RATE),
    ("retail_below", below(RETAIL_RELOAD_RATE)), ("retail_above", above(RETAIL_RELOAD_RATE)),
    ("0.3", 0.3), ("0.3_below", below(0.3)), ("0.3_above", above(0.3)),
    ("0.1", 0.1), ("0.1_below", below(0.1)),
    ("0.1_above", above(0.1)), ("0.15", 0.15), ("0.2", 0.2), ("0.6", 0.6), ("0.7", 0.7),
    ("0.125", 0.125), ("0.25", 0.25), ("0.5", 0.5), ("1", 1.0), ("1_below", below(1.0)),
    ("1_above", above(1.0)), ("2.5", 2.5), ("3", 3.0), ("1/900", 1 / 900),
    ("1/900_below", below(1 / 900)), ("1/900_above", above(1 / 900)), ("0", 0.0),
    ("-0", -0.0), ("-0.05", -0.05), ("-0.3", -0.3), ("-1_below", below(-1.0)),
    ("2386093", 2386093.0), ("1e16", 1e16), ("1.1e16", 1.1e16), ("1e300", 1e300),
    ("inf", math.inf), ("-inf", -math.inf), ("subnormal", 5e-324),
)
OTHER_ANSWERS = (0, 0x14, 0x17, 0x20, 0x21)


def rows():
    rows = []
    # One contact over every combination of the answers to QUERY_RELOADED, NEED_TO_MOVE,
    # RELOAD and REPAIR, its Health against the type's Strength and its Get_Mission.
    for query, health, mission, move, reload, repair in product(
            (ROGER, NEGATORY), (STRENGTH, STRENGTH - 1, STRENGTH + 1),
            (ENTER, SLEEP, GUARD, NO_MISSION), (ROGER, NEGATORY), (ROGER, NEGATORY),
            (ROGER, NEGATORY)):
        rows.append(dict(name=f"one.q{query}.h{health}.m{mission}.n{move}.r{reload}.p{repair}",
                         slots=["a"], count=1,
                         contacts={"a": contact(query, move, reload, repair, health,
                                                (mission,))}))
    # Other answers than ROGER and NEGATORY to each message where it is asked.
    for answer in OTHER_ANSWERS:
        for message, spec in (
                (QUERY_RELOADED, contact(answer, ROGER, NEGATORY, NEGATORY, missions=(SLEEP,))),
                (NEED_TO_MOVE, contact(NEGATORY, answer, ROGER, ROGER)),
                (RELOAD, contact(NEGATORY, ROGER, answer, ROGER, missions=(SLEEP,))),
                (REPAIR, contact(NEGATORY, ROGER, NEGATORY, answer, missions=(SLEEP,)))):
            rows.append(dict(name=f"answer.0x{message:02X}.{answer}", slots=["a"], count=1,
                             contacts={"a": spec}))
    # Get_Mission answering differently on its second call.
    for missions in ((GUARD, SLEEP), (SLEEP, GUARD), (NO_MISSION, SLEEP), (SLEEP, NO_MISSION),
                     (SLEEP, ENTER), (ENTER, SLEEP)):
        rows.append(dict(name=f"missions.{missions[0]}.{missions[1]}", slots=["a"], count=1,
                         contacts={"a": contact(NEGATORY, ROGER, NEGATORY, NEGATORY,
                                                missions=missions)}))
    # Which slots hold a contact, and which way through the arm each takes.
    for profile in PROFILES:
        rows.append(dict(name=f"pads.{profile}", **pads(profile)))
    for first, second in product((None, "full", "parked", "landing", "done"), repeat=2):
        rows.append(dict(name=f"pads.{first}.{second}", **pads(first, second)))
    for profiles in ((None, None, None, None), ("full",) * 4, ("done",) * 4,
                     ("entering", "parked", "reloading", "repairing"),
                     ("landing", "full", "done", "parked"), (None, "done", None, "full"),
                     ("full", None, "landing"), ("parked",) * 3):
        rows.append(dict(name="pads." + ".".join(map(str, profiles)), **pads(*profiles)))
    # The count against the slots: none, fewer than the vector holds, and changed by a
    # release (vt+0x334) during the visit.
    rows.append(dict(name="count0.empty", slots=[], count=0, contacts={}))
    rows.append(dict(name="count0.done", **pads("done", count=0)))
    rows.append(dict(name="count1.none.done", **pads(None, "done", count=1)))
    rows.append(dict(name="count2.full.landing.done", **pads("full", "landing", "done", count=2)))
    rows.append(dict(name="recount.3to1", **pads("full", "done", "done",
                                                 recount={"contact": "a", "count": 1})))
    rows.append(dict(name="recount.1to3", **pads("full", "done", "done", count=1,
                                                 recount={"contact": "a", "count": 3})))
    rows.append(dict(name="recount.2to0", **pads("done", "full",
                                                 recount={"contact": "a", "count": 0})))
    # One contact in two slots.
    rows.append(dict(name="twice.done", slots=["a", "a"], count=2,
                     contacts={"a": PROFILES["done"]}))
    rows.append(dict(name="twice.landing.full", slots=["a", "b", "a"], count=3,
                     contacts={"a": PROFILES["landing"], "b": PROFILES["full"]}))
    # ReloadRate: the frames a serviced visit returns, and a visit that services nothing.
    for label, rate in RATES:
        rows.append(dict(name=f"rate.{label}", reload_rate=f64_bits(rate), **pads("landing")))
    for label, rate in (("inf", math.inf), ("1e300", 1e300), ("default", 0.05)):
        rows.append(dict(name=f"rate.{label}.unserviced", reload_rate=f64_bits(rate),
                         **pads("full", "parked")))
    return rows


def generate():
    fixture = Airfield()
    return {"mission_repair": [fixture.visit(row) for row in rows()]}


def metadata():
    return provenance(
        scope="BuildingClass::Mission_Repair 0x0044B780 on an airfield whose type has only "
              "UnitReload= of the arm selectors, one call per row: its arm 0x0044C836..0x0044C96F. "
              "One contact over every combination of the answers (ROGER or NEGATORY) to "
              "QUERY_RELOADED 0x1D, NEED_TO_MOVE 0x13, RELOAD 0x1F and REPAIR 0x1C, its Health "
              "against the type's Strength (equal, one below, one above) and its Get_Mission "
              "(Enter 7, Sleep 0, Guard 5, none -1); the answers 0, 0x14, 0x17, 0x20 and 0x21 to "
              "each message where it is asked; Get_Mission answering differently on its second "
              "call. One to four slots holding nothing or a contact taking each way through the "
              "arm, a count of 0, a count below the slots the vector holds, a count changed by a "
              "release during the visit, and one contact in two slots. ReloadRate 0.05 (the "
              "default), the value ReadDouble stores for retail `.3` (0.3 as a float, widened; "
              "every combination row runs with it), 0.3's own double and values whose product "
              "with 900 sits just below, at or above an integer, zero, negatives, products past int32 and int64, infinities and "
              "a subnormal, on a serviced visit, and three on a visit that services nothing. "
              "Returned value and the ordered log of the count reads, Contact_With_Whom calls, "
              "Transmit_Message calls with message and contact, type getter calls, Get_Mission "
              "calls, the contacts' Queue_Mission, Enter_Idle_Mode, Assign_Mission (vt+0x1F0) "
              "and vt+0x334 with their arguments, and the airfield's Queue_Mission. Not the arms "
              "for other building types, the radio receivers' or the stubbed callees' own work.",
        assumptions=[
            "One emulator; per row the scratch region (0x10000 bytes) and the stack frame are "
            "rewritten, as aircraft_guard.py's ScratchAircraft does. The airfield is its owner "
            "on a clone of the Building vtable 0x007E3EBC with only the stubbed slots replaced, "
            "the type +0x520, the radio vector +0xE4/+0xE8 (the row's slots, the count its own) "
            "and nothing else; its type is a scratch BuildingTypeClass with UnitReload "
            "(+0x16AA) 1 and Bunker (+0x16AB), ConstructionYard (+0x16B9), Hospital (+0x16C1), "
            "Armory (+0x16C2) and UnitRepair (+0x16A9) 0.",
            "Each contact is an Aircraft on a clone of the Aircraft vtable 0x007E22A4 with only "
            "the stubbed slots replaced, the row's Health (+0x6C) and a type (+0x6C4) on the "
            "real AircraftTypeClass vtable 0x007E2868 whose Strength (+0xA0) is the row's.",
            "The Rules pointer 0x008871E0 names a scratch RulesClass whose ReloadRate (+0x1508) "
            "is the row's double.",
            "The x87 control word is the game's 0x0E7F: tube_startup_capture.json records it "
            "from WinMain entry on, and ftol 0x007C5F00 compares with 0x00822D80, which holds "
            "0x0E7F.",
            "Per row: RET to the caller with ESP + 4 and EBX/EBP/ESI/EDI preserved; no write "
            "outside the stack; every read of scratch bytes falls on bytes the row supplied; "
            "the arm's first instruction 0x0044C836 is reached.",
        ],
        substitutions=[
            "The airfield's Transmit_Message vt+0x278 records message and contact and answers "
            "the contact's code for that message; its Queue_Mission vt+0x1E8 records its "
            "arguments.",
            "The contacts' Get_Mission vt+0x184 answers the contact's missions in order, the "
            "last repeating; Queue_Mission vt+0x1E8, Enter_Idle_Mode vt+0x484, Assign_Mission "
            "vt+0x1F0 and vt+0x334 record their arguments, and vt+0x334 of a row's recount "
            "contact sets the airfield's count +0xE8.",
        ],
        entry_points={"mission_repair": MISSION_REPAIR, "unit_reload_arm": RELOAD_ARM,
                      "contact_with_whom": 0x65AD30, "techno_type_getter": 0x6F3270,
                      "aircraft_type_getter": 0x41C200, "ftol": 0x7C5F00})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
