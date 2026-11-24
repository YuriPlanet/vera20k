"""Original TechnoClass::GetWeaponRange 0x007012C0 (vt+0x168) and TechnoClass::Threat_Range
0x00707E60 on a Unit transport, including the open-topped cargo minimum:

    GetWeaponRange(i)  w = GetWeapon(i) (vt+0x3F8, 0x0070E140, elite slot at veterancy >= 2.0 when
                       its WeaponType is set); no WeaponType -> 0; r = w.Range (+0xB4). Type+0x5E4
                       (OpenTopped) -> min(r, m), m = INT_MAX lowered by each passenger's
                       GetCurrentWeapon (vt+0x3F4, 0x0070E1A0: slot +0x138 when the type's
                       TurretCount +0x808 > 0, else slot 0) Range when its WeaponType is set; the
                       cargo walk starts at CargoClass head (+0x118) and follows NextObject (+0x30)
                       while the next object's AbstractFlags (+0x14) carry bit 2 (Foot).
    Threat_Range(mode) -1 -> -1; 0 -> GuardRange (Type+0x5B8) unless vt+0x330 (Unit 0x0041BF30:
                       false), else 0; otherwise GuardRange, or max(GWR(0), GWR(1)) when zero,
                       doubled; mode 2 clamps to [0x700, 0x1000], every other mode to [0, 0x1000].

Each row builds the transport with the ORIGINAL Unit vtable 0x007F5C70 and its passengers with
the ORIGINAL Infantry 0x007EB058 or Unit vtable (nothing replaced), supplied types and WeaponTypes
(only +0xB4 Range is set), and calls vt+0x168 with 0 and 1 and Threat_Range with -1, 0, 1 and 2
(thiscall, RET 4). Writes: none outside the stack (pure queries); a write hook fails the row.

Row schema (sparse over `DEFAULTS`): open_topped (Type+0x5E4); guard_range (Type+0x5B8, leptons);
veterancy (+0x150, float); weapons {slot0, slot1, elite_slot0, elite_slot1} (Range in leptons,
null = no WeaponType); passengers, head (newest) first, each {class infantry | unit,
turret_count, current_weapon (+0x138), veterancy, slot0, slot1, elite_slot0, elite_slot1}.
Ranges are multiples of 128 leptons so a Rust consumer can author them as `Range=` cells exactly.
Rust consumer: src/sim/combat/threat_range.rs (`original_threat_range_and_cargo_rows`).
"""

import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW, UC_X86_REG_FPTAG)

from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
                                 OracleError, finish_vectors, load_image, provenance, run_checked)

# ---- native identities ------------------------------------------------------------------------
GET_WEAPON_RANGE, THREAT_RANGE = 0x7012C0, 0x707E60
VTABLES = {"unit": 0x7F5C70, "infantry": 0x7EB058}
TYPE_AT = {"unit": 0x6C4, "infantry": 0x6C0}
MODES = (-1, 0, 1, 2)

# ---- scratch layout ---------------------------------------------------------------------------
REGION = 0x40000
SP = STACK_BASE + STACK_SIZE - 0x1000
FIRER, FIRER_TYPE = SCRATCH, SCRATCH + 0x2000
PASSENGERS, PASSENGER_TYPES = SCRATCH + 0x8000, SCRATCH + 0x10000
WEAPONS = SCRATCH + 0x30000
SLOTS = (("slot0", 0x898), ("slot1", 0x898 + 0x1C), ("elite_slot0", 0xA94),
         ("elite_slot1", 0xA94 + 0x1C))

NO_WEAPONS = {"slot0": None, "slot1": None, "elite_slot0": None, "elite_slot1": None}
DEFAULTS = {
    "open_topped": 1, "guard_range": 0, "veterancy": 0.0,
    "weapons": {**NO_WEAPONS, "slot0": 1408},
    "passengers": [],
}
PASSENGER_DEFAULTS = {"class": "infantry", "turret_count": 0, "current_weapon": 0,
                      "veterancy": 0.0, **NO_WEAPONS}


def u32(value):
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def i32(raw):
    return struct.unpack("<i", u32(raw))[0]


def resolve(row):
    extra = set(row) - set(DEFAULTS) - {"name"}
    if extra:
        raise ValueError(f"{row.get('name')}: unknown inputs {sorted(extra)}")
    full = {key: row.get(key, default) for key, default in DEFAULTS.items()}
    full["weapons"] = {**DEFAULTS["weapons"], **row.get("weapons", {})}
    full["passengers"] = []
    for passenger in row.get("passengers", []):
        unknown = set(passenger) - set(PASSENGER_DEFAULTS)
        if unknown:
            raise ValueError(f"{row.get('name')}: unknown passenger fields {sorted(unknown)}")
        full["passengers"].append({**PASSENGER_DEFAULTS, **passenger})
    for weapons in [full["weapons"], *full["passengers"]]:
        for key, _ in SLOTS:
            if weapons[key] is not None and weapons[key] % 128:
                raise ValueError(f"{row.get('name')}: {key} is not a multiple of 128 leptons")
    return full


class Fixture:
    def __init__(self):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, REGION)
        u.mem_map(RET_MAGIC, 0x1000)
        if self.word(VTABLES["unit"] + 0x168) != GET_WEAPON_RANGE:
            raise OracleError("Unit vt+0x168 is not GetWeaponRange 0x007012C0")
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def on_write(self, u, _access, address, size, _value, _data):
        if not (STACK_BASE <= address and address + size <= STACK_BASE + STACK_SIZE):
            self.violations.append(f"write at 0x{address:08X} from 0x{u.reg_read(UC_X86_REG_EIP):08X}")
            u.emu_stop()

    def weapons(self, kind, spec):
        # Each WeaponType lives in its own 0x200 block; only Range (+0xB4) is set.
        for key, offset in SLOTS:
            if spec[key] is None:
                continue
            weapon = WEAPONS + 0x200 * self.next_weapon
            self.next_weapon += 1
            self.u.mem_write(kind + offset, u32(weapon))
            self.u.mem_write(weapon + 0xB4, u32(spec[key]))

    def build(self, row):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.mem_write(SP - 0x3000, bytes(0x3100))
        self.next_weapon = 0
        u.mem_write(FIRER, u32(VTABLES["unit"]))
        u.mem_write(FIRER + 0x14, bytes([7]))
        u.mem_write(FIRER + TYPE_AT["unit"], u32(FIRER_TYPE))
        u.mem_write(FIRER + 0x150, struct.pack("<f", row["veterancy"]))
        u.mem_write(FIRER_TYPE + 0x5E4, bytes([row["open_topped"]]))
        u.mem_write(FIRER_TYPE + 0x5B8, u32(row["guard_range"]))
        self.weapons(FIRER_TYPE, row["weapons"])
        passengers = row["passengers"]
        u.mem_write(FIRER + 0x114, u32(len(passengers)))
        u.mem_write(FIRER + 0x118, u32(PASSENGERS if passengers else 0))
        for n, spec in enumerate(passengers):
            passenger, kind = PASSENGERS + 0x1000 * n, PASSENGER_TYPES + 0x2000 * n
            u.mem_write(passenger, u32(VTABLES[spec["class"]]))
            u.mem_write(passenger + 0x14, bytes([7]))
            u.mem_write(passenger + TYPE_AT[spec["class"]], u32(kind))
            u.mem_write(passenger + 0x150, struct.pack("<f", spec["veterancy"]))
            u.mem_write(passenger + 0x138, u32(spec["current_weapon"]))
            u.mem_write(passenger + 0x30, u32(passenger + 0x1000 if n + 1 < len(passengers) else 0))
            u.mem_write(kind + 0x808, u32(spec["turret_count"]))
            self.weapons(kind, spec)

    def call(self, name, entry, arg):
        u = self.u
        u.mem_write(SP, u32(RET_MAGIC) + u32(arg))
        for register in (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDX, UC_X86_REG_ESI,
                         UC_X86_REG_EDI, UC_X86_REG_EBP):
            u.reg_write(register, 0)
        u.reg_write(UC_X86_REG_ECX, FIRER)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
        self.violations = []
        try:
            run_checked(u, entry, RET_MAGIC, count=20_000)
        except OracleError as error:
            raise OracleError(f"{name}: {self.violations or error}") from error
        if self.violations:
            raise OracleError(f"{name}: {self.violations}")
        if u.reg_read(UC_X86_REG_ESP) != SP + 8:
            raise OracleError(f"{name}: not RET 4")
        return i32(u.reg_read(UC_X86_REG_EAX))

    def execute(self, sparse):
        row = resolve(sparse)
        self.build(row)
        name = sparse["name"]
        return {"input": sparse,
                "weapon_range": [self.call(name, GET_WEAPON_RANGE, index) for index in (0, 1)],
                "threat_range": [self.call(name, THREAT_RANGE, mode) for mode in MODES]}


# ---- rows -------------------------------------------------------------------------------------
GI = {"slot0": 1024, "slot1": 1280, "elite_slot0": 1024, "elite_slot1": 1536}  # M60, Para, ParaE


def rows():
    out = []

    def add(name, **kw):
        out.append({"name": name, **kw})

    add("closed.empty", open_topped=0)
    add("closed.gi_ignored", open_topped=0, passengers=[GI])
    add("open.empty", passengers=[])
    add("open.gi", passengers=[GI])
    add("open.five_riders_take_the_shortest",
        passengers=[{"slot0": r} for r in (1024, 1536, 1280, 896, 2304)])
    add("open.rider_longer_than_own", passengers=[{"slot0": 2304}])
    add("open.unarmed_head_is_skipped", passengers=[{}, GI])
    add("open.only_unarmed_riders", passengers=[{}, {}])
    add("open.elite_rider_reads_elite_slot0", passengers=[{**GI, "veterancy": 2.0,
                                                           "elite_slot0": 1280}])
    add("open.elite_rider_without_elite_slot0",
        passengers=[{"veterancy": 2.0, "slot0": 896}])
    add("open.veteran_rider_is_not_elite", passengers=[{"veterancy": 1.0, "slot0": 896,
                                                        "elite_slot0": 1280}])
    add("open.rider_secondary_is_not_read", passengers=[{"slot0": 1280, "slot1": 384}])
    add("open.turreted_rider_reads_its_current_slot",
        passengers=[{"class": "unit", "turret_count": 1, "current_weapon": 1,
                     "slot0": 2304, "slot1": 896}])
    add("open.turretless_rider_ignores_current_slot",
        passengers=[{"class": "unit", "current_weapon": 1, "slot0": 2304, "slot1": 896}])
    # Retail MakeupKit (the Spy's only weapon) is Range=-2.
    add("open.spy_rider", passengers=[{"slot0": -512}, GI])
    add("open.spy_rider_both_slots", weapons={"slot1": 896}, passengers=[{"slot0": -512}])
    add("open.both_slots_capped", weapons={"slot1": 2304}, passengers=[GI])
    add("open.secondary_only", weapons={"slot0": None, "slot1": 1408}, passengers=[GI])
    add("open.unarmed_transport", weapons={"slot0": None}, passengers=[GI])
    add("open.elite_transport", veterancy=2.0, weapons={"elite_slot0": 1792},
        passengers=[{"slot0": 2304}])
    add("open.elite_transport_capped", veterancy=2.0, weapons={"elite_slot0": 1792},
        passengers=[GI])
    add("guard_range.overrides_weapons", guard_range=2304, passengers=[GI])
    add("guard_range.small", guard_range=512)
    add("clamp.short_weapon", weapons={"slot0": 512}, open_topped=0)
    add("clamp.long_weapon", weapons={"slot0": 2304}, open_topped=0)
    add("clamp.exact_ceiling", weapons={"slot0": 2048}, open_topped=0)
    return out


def generate():
    table = rows()
    names = [r["name"] for r in table]
    if len(set(names)) != len(names):
        raise OracleError("duplicate row names")
    fixture = Fixture()
    return {"defaults": DEFAULTS, "passenger_defaults": PASSENGER_DEFAULTS, "modes": list(MODES),
            "rows": [fixture.execute(r) for r in table]}


def metadata():
    return provenance(
        scope="TechnoClass::GetWeaponRange 0x007012C0 (vt+0x168, slots 0 and 1) and "
              "TechnoClass::Threat_Range 0x00707E60 (modes -1, 0, 1, 2) on a Unit transport: "
              "closed and open-topped, empty and loaded, the cargo minimum over infantry and unit "
              "riders (unarmed, elite, veteran, turreted, Spy Range=-2), transport slot and elite "
              "variants, GuardRange and the mode clamps. Not the Greatest_Threat walk bound or "
              "the candidate acceptance that consume these values.",
        assumptions=[
            "One emulator; per row the scratch region and stack are rewritten, general registers "
            "zeroed, FPCW 0x0E7F, empty x87 stack.",
            "Transport: original Unit vtable 0x007F5C70 (vt+0x168 asserted 0x007012C0), flags "
            "+0x14 = 7, type at +0x6C4, CargoClass at +0x114 (count, head).",
            "Riders: original Infantry 0x007EB058 or Unit vtable, flags +0x14 = 7 (Foot), type at "
            "+0x6C0 / +0x6C4, NextObject +0x30, veterancy +0x150, CurrentWeaponNumber +0x138.",
            "Types: weapon slots 0/1 at +0x898 and elite 0/1 at +0xA94 (stride 0x1C), each "
            "WeaponType with only Range +0xB4 set; OpenTopped +0x5E4, GuardRange +0x5B8, "
            "TurretCount +0x808.",
            "No write outside the stack is allowed.",
        ],
        substitutions=[],
        entry_points={"get_weapon_range": GET_WEAPON_RANGE, "threat_range": THREAT_RANGE,
                      "get_weapon": 0x70E140, "get_current_weapon": 0x70E1A0,
                      "has_turrets": 0x717880, "is_elite": 0x750010})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
