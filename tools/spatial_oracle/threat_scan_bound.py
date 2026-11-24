"""Original Greatest_Threat weapon-derived scan bound, 0x6F9042..0x6F9148.

The preceding Threat_Range result is supplied in EDI. Original Unit GetType,
HasTurrets, GetWeaponRange, GetWeapon and cargo queries execute without callable
substitutions. The slice writes its signed cell bound at ESP+0x38 and stops
before Building adjustments, the air pre-pass, candidate traversal or scoring.

Inputs reuse threat_range_cargo's supplied runtime objects and weapon slots,
extended with slot 2, the selected slot and the type flags this slice reads.
They are not original constructors, INI readers or a legal whole-game scenario.
All outputs are native observations; Python does not calculate the bound.
"""

from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EFLAGS, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW, UC_X86_REG_FPTAG,
)

from tools.native_oracle import NATIVE_FPCW, OracleError, finish_vectors, provenance, run_checked
from tools.spatial_oracle import threat_range_cargo as cargo

ENTRY, STOP = 0x6F9042, 0x6F9148
HAS_TURRETS = 0x717880
EXTRA_SLOTS = (("slot2", 0x898 + 2 * 0x1C), ("elite_slot2", 0xA94 + 2 * 0x1C))
DEFAULTS = {
    **cargo.DEFAULTS,
    "open_topped": 0,
    "weapons": {**cargo.NO_WEAPONS, "slot0": 768, "slot1": 1536,
                "slot2": None, "elite_slot2": None},
    "threat_range": 0,
    "turret_count": 0,
    "is_gattling": False,
    "current_weapon": 0,
    "underwater": False,
    "organic": False,
    "self_healing": False,
    "air_range_bonus": 0,
}
PASSENGER_DEFAULTS = {**cargo.PASSENGER_DEFAULTS, "slot2": None, "elite_slot2": None}
# Observation points distinguish the three zero-range branches and both stores.
OBSERVED = (ENTRY, 0x6F9052, HAS_TURRETS, 0x6F9085, 0x6F90D6,
            0x6F90DE, 0x6F9116, 0x6F912E, 0x6F9144)


def resolve(sparse):
    unknown = set(sparse) - set(DEFAULTS) - {"name"}
    if unknown:
        raise OracleError(f"{sparse.get('name')}: unknown inputs {sorted(unknown)}")
    row = {**DEFAULTS, **sparse}
    row["weapons"] = {**DEFAULTS["weapons"], **sparse.get("weapons", {})}
    if set(row["weapons"]) != set(DEFAULTS["weapons"]):
        raise OracleError(f"{row['name']}: unknown weapon slot")
    row["passengers"] = []
    for passenger in sparse.get("passengers", []):
        if set(passenger) - set(PASSENGER_DEFAULTS):
            raise OracleError(f"{row['name']}: unknown passenger input")
        row["passengers"].append({**PASSENGER_DEFAULTS, **passenger})
    if len(row["passengers"]) > 4:
        raise OracleError(f"{row['name']}: passenger count exceeds fixture storage")
    for spec in [row, *row["passengers"]]:
        if not 0 <= spec["current_weapon"] <= 2:
            raise OracleError(f"{row['name']}: current slot outside supplied three-slot layout")
    return row


class Fixture(cargo.Fixture):
    def __init__(self):
        super().__init__()
        self.get_type = self.word(cargo.VTABLES["unit"] + 0x84)
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def weapons(self, kind, spec):
        super().weapons(kind, spec)
        for key, offset in EXTRA_SLOTS:
            if spec.get(key) is not None:
                weapon = cargo.WEAPONS + 0x200 * self.next_weapon
                self.next_weapon += 1
                self.u.mem_write(kind + offset, cargo.u32(weapon))
                self.u.mem_write(weapon + 0xB4, cargo.u32(spec[key]))

    def observe(self, u, address, _size, _data):
        if address in OBSERVED or address == self.get_type:
            self.visits.add(address)
        if address == cargo.GET_WEAPON_RANGE:
            if u.reg_read(UC_X86_REG_ECX) != cargo.FIRER:
                raise OracleError("GetWeaponRange receiver is not the supplied firer")
            self.weapon_slots.append(cargo.i32(self.word(u.reg_read(UC_X86_REG_ESP) + 4)))

    def execute(self, sparse):
        row = resolve(sparse)
        self.build(row)
        u = self.u
        u.mem_write(cargo.FIRER + 0x138, cargo.u32(row["current_weapon"]))
        u.mem_write(cargo.FIRER_TYPE + 0x808, cargo.u32(row["turret_count"]))
        for name, offset in (("is_gattling", 0xCD5), ("underwater", 0xD69),
                             ("organic", 0xD97), ("self_healing", 0xD14)):
            u.mem_write(cargo.FIRER_TYPE + offset, bytes([bool(row[name])]))
        u.mem_write(cargo.FIRER_TYPE + 0x68C, cargo.u32(row["air_range_bonus"]))
        u.mem_write(cargo.SP + 0x38, cargo.u32(0x5A5A5A5A))
        for register in (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
                         UC_X86_REG_EDX, UC_X86_REG_EBP):
            u.reg_write(register, 0)
        u.reg_write(UC_X86_REG_ESI, cargo.FIRER)
        u.reg_write(UC_X86_REG_EDI, row["threat_range"] & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_ESP, cargo.SP)
        u.reg_write(UC_X86_REG_EFLAGS, 2)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
        self.violations, self.weapon_slots, self.visits = [], [], set()
        before = bytes(u.mem_read(cargo.SCRATCH, cargo.REGION))
        required = [ENTRY, 0x6F9052]
        if row["threat_range"] == 0:
            required += [self.get_type, HAS_TURRETS, 0x6F9116, 0x6F912E, 0x6F9144]
            if row["turret_count"] > 0 and not row["is_gattling"]:
                required += [0x6F9085, cargo.GET_WEAPON_RANGE]
            elif row["underwater"] and row["organic"] and row["self_healing"]:
                required += [0x6F90D6]
            else:
                required += [0x6F90DE, cargo.GET_WEAPON_RANGE]
        try:
            run_checked(u, ENTRY, STOP, count=20_000, required_addresses=required,
                        context={"case": row["name"], "inputs": row})
        except OracleError as error:
            raise OracleError(f"{row['name']}: {self.violations or error}") from error
        if self.violations:
            raise OracleError(f"{row['name']}: {self.violations}")
        if u.reg_read(UC_X86_REG_ESP) != cargo.SP:
            raise OracleError(f"{row['name']}: scan-bound slice changed ESP")
        if bytes(u.mem_read(cargo.SCRATCH, cargo.REGION)) != before:
            raise OracleError(f"{row['name']}: scan-bound slice changed supplied objects")
        if row["threat_range"] != 0 and (self.weapon_slots or HAS_TURRETS in self.visits):
            raise OracleError(f"{row['name']}: hard bound did not bypass weapon selection")
        return {
            "input": row,
            "ring_bound_cells": struct.unpack("<i", u.mem_read(cargo.SP + 0x38, 4))[0],
            "weapon_range_slots": self.weapon_slots,
            "reached_addresses": [f"0x{address:08X}" for address in sorted(self.visits)],
        }


def rows():
    cases = []

    def add(name, **values):
        cases.append({"name": name, **values})

    add("fallback.secondary_longer")
    add("fallback.primary_longer", weapons={"slot0": 1792, "slot1": 512})
    add("fallback.equal_ranges", weapons={"slot0": 1280, "slot1": 1280})
    add("fallback.no_weapons", weapons={"slot0": None, "slot1": None})
    add("fallback.secondary_absent", weapons={"slot1": None})
    add("fallback.selected_slot_is_ignored", current_weapon=2, weapons={"slot2": 3072})
    add("turret.selected_primary_shorter", turret_count=1)
    add("turret.selected_secondary_longer", turret_count=1, current_weapon=1)
    add("turret.selected_secondary_shorter", turret_count=1, current_weapon=1,
        weapons={"slot0": 1792, "slot1": 512})
    add("turret.selected_slot2_longer", turret_count=1, current_weapon=2,
        weapons={"slot2": 2304})
    add("turret.selected_slot2_shorter", turret_count=3, current_weapon=2,
        weapons={"slot2": 512})
    add("turret.selected_slot2_absent", turret_count=1, current_weapon=2)
    add("turret.zero_count_uses_both", current_weapon=0)
    add("turret.negative_count_uses_both", turret_count=-1, current_weapon=0)
    add("turret.gattling_uses_both", turret_count=1, is_gattling=True, current_weapon=0)
    organic = dict(underwater=True, organic=True, self_healing=True, guard_range=2304)
    add("organic.guard_range", **organic)
    add("organic.guard_zero", **{**organic, "guard_range": 0})
    add("organic.guard_negative_fraction", **{**organic, "guard_range": -257})
    for flag in ("underwater", "organic", "self_healing"):
        add(f"organic.without_{flag}", **{**organic, flag: False})
    add("organic.turret_selection_precedes_guard", **organic, turret_count=1,
        current_weapon=2, weapons={"slot2": 512})
    add("organic.gattling_uses_guard", **organic, turret_count=1, is_gattling=True)
    add("elite.selected_slot2", turret_count=1, current_weapon=2, veterancy=2.0,
        weapons={"slot2": 512, "elite_slot2": 2048})
    add("elite.selected_slot2_fallback", turret_count=1, current_weapon=2,
        veterancy=2.0, weapons={"slot2": 1024})
    add("elite.selected_slot2_absent", turret_count=1, current_weapon=2, veterancy=2.0)
    add("elite.fallback_uses_elite_pair", veterancy=2.0,
        weapons={"elite_slot0": 2048, "elite_slot1": 512})
    add("elite.veteran_keeps_normal_slot", turret_count=1, current_weapon=2,
        veterancy=1.0, weapons={"slot2": 512, "elite_slot2": 2048})
    for value in (-257, -256, -255, -1, 0, 1, 255, 256, 257, 1408):
        add(f"selected.range_{value}", turret_count=1, current_weapon=2,
            weapons={"slot2": value})
    for value in (-257, -256, -255, -1, 0, 1, 255, 256, 257, 384):
        add(f"bonus.range_{value}", turret_count=1, air_range_bonus=value)
    for value in (-513, -257, -256, -255, -1, 1, 255, 256, 257, 513):
        add(f"hard.range_{value}_bypasses_weapon_and_bonus", threat_range=value,
            turret_count=1, current_weapon=2, weapons={"slot2": 3072},
            air_range_bonus=1792)
    add("cargo.selected_slot2_capped", open_topped=1, turret_count=1,
        current_weapon=2, weapons={"slot2": 2304}, passengers=[{"slot0": 896}])
    add("cargo.selected_slot2_shorter_than_passenger", open_topped=1, turret_count=1,
        current_weapon=2, weapons={"slot2": 512}, passengers=[{"slot0": 1536}])
    add("cargo.closed_ignores_passenger", turret_count=1, current_weapon=2,
        weapons={"slot2": 2304}, passengers=[{"slot0": 896}])
    add("cargo.fallback_pair_capped", open_topped=1, passengers=[{"slot0": 896}])
    add("cargo.selected_slot2_negative_passenger", open_topped=1, turret_count=1,
        current_weapon=2, weapons={"slot2": 2304}, passengers=[{"slot0": -257}])
    add("cargo.unarmed_passenger_skipped", open_topped=1, turret_count=1,
        current_weapon=2, weapons={"slot2": 2304}, passengers=[{}, {"slot0": 1152}])
    add("cargo.turreted_passenger_current_slot2", open_topped=1, turret_count=1,
        current_weapon=2, weapons={"slot2": 2304},
        passengers=[{"class": "unit", "turret_count": 1, "current_weapon": 2,
                     "slot0": 1536, "slot2": 640}])
    return cases


def generate():
    cases = rows()
    if len({row["name"] for row in cases}) != len(cases):
        raise OracleError("duplicate scan-bound case names")
    fixture = Fixture()
    return {"defaults": DEFAULTS, "passenger_defaults": PASSENGER_DEFAULTS,
            "rows": [fixture.execute(row) for row in cases]}


def metadata():
    return provenance(
        scope="Original Greatest_Threat weapon-derived scan bound 0x6F9042..0x6F9148: "
              "nonzero Threat_Range bypass; current weapon for HasTurrets and non-gattling "
              "types; underwater organic self-healing GuardRange; fallback max slots 0/1; "
              "signed cell truncation and AirRangeBonus. Includes slot 2, elite fallback "
              "and open-topped cargo limits. Not full Greatest_Threat or target-acquisition parity.",
        assumptions=[
            "The preceding Threat_Range result is supplied in EDI; ESI is a supplied Unit "
            "with original vtable 0x007F5C70. Its GetType and original HasTurrets/GetWeaponRange/"
            "GetWeapon bodies execute; no caller, constructor, INI reader or weapon-selection "
            "writer is exercised.",
            "Reuse threat_range_cargo.Fixture for object/type/weapon storage, veterancy and "
            "cargo traversal. Slot 2 and elite slot 2 extend its original 0x1C-stride arrays. "
            "The current slot is supplied at Unit+0x138; TurretCount at type+0x808.",
            "Type IsGattling+0xCD5, Underwater+0xD69, Organic+0xD97, SelfHealing+0xD14, "
            "GuardRange+0x5B8 and AirRangeBonus+0x68C are supplied. WeaponType Range+0xB4 "
            "uses signed integer leptons, including fractional-cell boundaries.",
            "One emulator; scratch and stack are reset per case. General registers, EFLAGS, "
            "FPCW 0x0E7F and empty x87 stack are supplied before each run. Checked completion "
            "requires the selected branch and both applicable bound stores. Non-stack native "
            "writes are rejected; final ESP and all supplied scratch bytes must be unchanged.",
            "ring_bound_cells is read from ESP+0x38 at the original stop. weapon_range_slots "
            "and reached_addresses are passive instruction observations, not substituted results. "
            "The stop precedes Building-specific adjustments, air pre-pass, cell traversal, "
            "candidate eligibility/scoring, RNG and full scenario scheduling.",
        ],
        substitutions=[],
        entry_points={"scan_bound": ENTRY, "scan_bound_stop": STOP,
                      "has_turrets": HAS_TURRETS, "get_weapon_range": cargo.GET_WEAPON_RANGE,
                      "get_weapon": 0x70E140, "get_current_weapon": 0x70E1A0,
                      "is_elite": 0x750010},
    )


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata,
                   source_paths={"threat_scan_bound": Path(__file__),
                                 "threat_range_cargo": Path(cargo.__file__),
                                 "native_oracle": Path(__file__).parents[1] / "native_oracle.py"})
