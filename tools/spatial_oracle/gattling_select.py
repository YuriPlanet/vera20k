"""Original weapon selection for gattling types: TechnoClass::What_Weapon_Should_I_Use 0x006F3330
(vt+0x2E4), reached through each class's own slot:

    Unit      vt+0x2E4 = 0x00746CD0: returns Type+0x6A8 when deployed (+0x6E0) with DeployFire
              (Type+0x6AC), otherwise tail-calls 0x006F3330
    Building  vt+0x2E4 = 0x006F3330 directly

Each row builds the firer with its ORIGINAL class vtable (nothing replaced), a supplied type and
weapons, and a target, calls the class's vt+0x2E4 (thiscall (target), RET 4) and records the returned
index. The only substitution is the target's vt+0x54 (label IsHighFlying), which returns the row's
verdict and is logged; the target's vtable is a clone of its class's original with that slot
replaced. The arms of 0x006F3330 read here (lane section 3):
    A 0x006F333B  TurretCount > 0 (0x00717880) and !IsGattling -> CurrentWeaponNumber +0x138 (-1 -> 0)
    B 0x006F337D  vt+0x400 (Unit 0x0041BFB0 false; Building 0x00458DD0 occupants firing) -> 0
    C/D           GetWeapon(1) / GetWeapon(0) WeaponType NULL -> 0
    E 0x006F33BF  slot-1 WeaponType +0x136 (NeverUse) -> 0
    F 0x006F33CD  no target -> 0
    G 0x006F33D9  +0x82 (in an open-topped transport) and Type+0xD50 != -1 -> Type+0xD50
    H 0x006F3428  IsGattling: s = +0x140; 2s+1 if slot-1 WeaponType's Projectile (+0xA0) has AA
                  (+0x2A4) and the target's AbstractFlags +0x14 bit 0 (Techno) is set and target
                  vt+0x54 is true; else 2s. No clamp of s.
GetWeapon (Unit 0x0070E140; Building 0x004526F0) resolves the elite slot when veterancy >= 2.0 and
the elite WeaponType is non-null, so elite rows read the elite slot-1 weapon's NeverUse and AA.

Row schema (sparse over `defaults`): class unit | building; stage +0x140; veterancy +0x150 (float);
current_weapon +0x138; open_topped +0x82; deployed +0x6E0 (Unit); type {is_gattling +0xCD5,
turret_count +0x808, open_transport_weapon +0xD50 (-1), deploy_fire +0x6AC (Unit),
deploy_fire_weapon +0x6A8 (Unit), can_be_occupied +0x157B, can_occupy_fire +0x157C (Building)};
occupants (Building +0x694, read by vt+0x408 0x004581F0); slot1 / elite_slot1 {present, aa,
never_use} and slot0 / elite_slot0 {present}; target {kind aircraft | unit | building (flags 7/7/3,
Techno bit set) | terrain (TerrainClass 0x007F522C, flags 2: Object only) | cell (CellClass
0x007E4EEC, flags 0) | none, high_flying (the vt+0x54 verdict)}.
Writes: none outside the stack (a pure query); a write hook fails the row otherwise.
Rust consumer: src/sim/combat/combat_weapon.rs (`original_weapon_selection_rows`).
"""

import itertools
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW, UC_X86_REG_FPTAG)

from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
                                 OracleError, finish_vectors, load_image, provenance, run_checked)

# ---- native identities ------------------------------------------------------------------------
SELECT = 0x6F3330
VTABLES = {"unit": 0x7F5C70, "building": 0x7E3EBC, "aircraft": 0x7E22A4, "terrain": 0x7F522C,
           "cell": 0x7E4EEC}
ENTRIES = {"unit": 0x746CD0, "building": 0x6F3330}
TYPE_AT = {"unit": 0x6C4, "building": 0x520, "aircraft": 0x6C4}
FLAGS = {"aircraft": 7, "unit": 7, "building": 3, "terrain": 2, "cell": 0}
ARMS = {0x6F3360: "A", 0x6F37AD: "0", 0x6F33F6: "G", 0x6F345C: "H odd", 0x6F346A: "H even",
        0x6F3477: "not gattling", 0x746CEA: "deploy fire"}

# ---- scratch layout ---------------------------------------------------------------------------
REGION = 0x40000
SP = STACK_BASE + STACK_SIZE - 0x1000
FIRER, TARGET = SCRATCH, SCRATCH + 0x1000
FIRER_TYPE = SCRATCH + 0x4000
WEAPONS, PROJECTILES = SCRATCH + 0x8000, SCRATCH + 0xA000
TARGET_VT, STUB = SCRATCH + 0x10000, SCRATCH + 0x11000

WEAPON_DEFAULT = {"present": True, "aa": 1, "never_use": 0}
DEFAULTS = {
    "class": "unit", "stage": 0, "veterancy": 0.0, "current_weapon": 0, "open_topped": 0,
    "deployed": 0, "occupants": 0,
    "type": {"is_gattling": 1, "turret_count": 1, "open_transport_weapon": -1, "deploy_fire": 0,
             "deploy_fire_weapon": 7, "can_be_occupied": 0, "can_occupy_fire": 0},
    "slot0": {"present": True}, "elite_slot0": {"present": True},
    "slot1": WEAPON_DEFAULT, "elite_slot1": WEAPON_DEFAULT,
    "target": {"kind": "aircraft", "high_flying": True},
}


def u32(value):
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def i32(raw):
    return struct.unpack("<i", u32(raw))[0]


def resolve(row):
    extra = set(row) - set(DEFAULTS) - {"name"}
    if extra:
        raise ValueError(f"{row.get('name')}: unknown inputs {sorted(extra)}")
    full = {}
    for key, default in DEFAULTS.items():
        given = row.get(key)
        if isinstance(default, dict):
            unknown = set(given or {}) - set(default)
            if unknown:
                raise ValueError(f"{row.get('name')}: unknown {key} fields {sorted(unknown)}")
            full[key] = {**default, **(given or {})}
        else:
            full[key] = default if given is None else given
    if full["target"]["kind"] not in (*FLAGS, "none"):
        raise ValueError(f"{row.get('name')}: bad target kind")
    return full


class Fixture:
    def __init__(self):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, REGION)
        u.mem_map(RET_MAGIC, 0x1000)
        for cls, entry in ENTRIES.items():
            if self.word(VTABLES[cls] + 0x2E4) != entry:
                raise OracleError(f"{cls} vt+0x2E4 is not 0x{entry:08X}")
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUB, end=STUB)
        for address in ARMS:
            u.hook_add(UC_HOOK_CODE, self.on_arm, begin=address, end=address)
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def fail(self, message):
        self.violations.append(message)
        self.u.emu_stop()

    def on_stub(self, u, _address, _size, _data):
        if u.reg_read(UC_X86_REG_ECX) != TARGET:
            return self.fail("high_flying on another object")
        self.calls.append("high_flying")
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, int(self.row["target"]["high_flying"]))
        u.reg_write(UC_X86_REG_EIP, self.word(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4)

    def on_arm(self, _u, address, _size, _data):
        self.arms.append(ARMS[address])

    def on_write(self, u, _access, address, size, _value, _data):
        if not (STACK_BASE <= address and address + size <= STACK_BASE + STACK_SIZE):
            self.fail(f"write at 0x{address:08X} from 0x{u.reg_read(UC_X86_REG_EIP):08X}")

    def build(self, row):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.mem_write(SP - 0x3000, bytes(0x3100))
        u.mem_write(STUB, b"\xCC")
        cls, kind = row["class"], row["type"]
        u.mem_write(FIRER, u32(VTABLES[cls]))
        u.mem_write(FIRER + 0x14, bytes([FLAGS[cls]]))
        u.mem_write(FIRER + TYPE_AT[cls], u32(FIRER_TYPE))
        u.mem_write(FIRER + 0x140, u32(row["stage"]))
        u.mem_write(FIRER + 0x138, u32(row["current_weapon"]))
        u.mem_write(FIRER + 0x150, struct.pack("<f", row["veterancy"]))
        u.mem_write(FIRER + 0x82, bytes([row["open_topped"]]))
        if cls == "unit":
            u.mem_write(FIRER + 0x6E0, bytes([row["deployed"]]))
            u.mem_write(FIRER_TYPE + 0x6AC, bytes([kind["deploy_fire"]]))
            u.mem_write(FIRER_TYPE + 0x6A8, u32(kind["deploy_fire_weapon"]))
        else:
            u.mem_write(FIRER + 0x694, u32(row["occupants"]))
            u.mem_write(FIRER_TYPE + 0x157B, bytes([kind["can_be_occupied"]]))
            u.mem_write(FIRER_TYPE + 0x157C, bytes([kind["can_occupy_fire"]]))
        u.mem_write(FIRER_TYPE + 0xCD5, bytes([kind["is_gattling"]]))
        u.mem_write(FIRER_TYPE + 0x808, u32(kind["turret_count"]))
        u.mem_write(FIRER_TYPE + 0xD50, u32(kind["open_transport_weapon"]))
        # Four weapon slots: base 0/1 at +0x898, elite 0/1 at +0xA94, each with its own projectile.
        for n, (key, base) in enumerate((("slot0", 0x898), ("slot1", 0x898 + 0x1C),
                                         ("elite_slot0", 0xA94), ("elite_slot1", 0xA94 + 0x1C))):
            spec = row[key]
            if not spec["present"]:
                continue
            weapon, projectile = WEAPONS + 0x200 * n, PROJECTILES + 0x400 * n
            u.mem_write(FIRER_TYPE + base, u32(weapon))
            u.mem_write(weapon + 0xA0, u32(projectile))
            u.mem_write(projectile + 0x2A4, bytes([spec.get("aa", 1)]))
            u.mem_write(weapon + 0x136, bytes([spec.get("never_use", 0)]))
        target = row["target"]
        if target["kind"] != "none":
            u.mem_write(TARGET_VT, bytes(u.mem_read(VTABLES[target["kind"]], 0x600)))
            u.mem_write(TARGET_VT + 0x54, u32(STUB))
            u.mem_write(TARGET, u32(TARGET_VT))
            u.mem_write(TARGET + 0x14, bytes([FLAGS[target["kind"]]]))

    def execute(self, sparse):
        row = self.row = resolve(sparse)
        self.build(row)
        u = self.u
        self.calls, self.arms, self.violations = [], [], []
        target = 0 if row["target"]["kind"] == "none" else TARGET
        u.mem_write(SP, u32(RET_MAGIC) + u32(target))
        for register in (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDX, UC_X86_REG_ESI,
                         UC_X86_REG_EDI, UC_X86_REG_EBP):
            u.reg_write(register, 0)
        u.reg_write(UC_X86_REG_ECX, FIRER)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
        try:
            run_checked(u, ENTRIES[row["class"]], RET_MAGIC, count=20_000)
        except OracleError as error:
            raise OracleError(f"{sparse['name']}: {self.violations or error}") from error
        if self.violations:
            raise OracleError(f"{sparse['name']}: {self.violations}")
        if u.reg_read(UC_X86_REG_ESP) != SP + 8:
            raise OracleError(f"{sparse['name']}: not RET 4")
        if "not gattling" in self.arms:
            raise OracleError(f"{sparse['name']}: left the gattling arms (non-gattling path)")
        return {"input": sparse, "index": i32(u.reg_read(UC_X86_REG_EAX)),
                "arm": self.arms[-1] if self.arms else None, "calls": self.calls}


# ---- rows -------------------------------------------------------------------------------------
def grid_rows():
    """Stage 0..2 x slot-1 AA x target high-flying x elite x NeverUse, both classes, plus null."""
    out = []
    for cls, stage, aa, high, elite, never in itertools.product(
            ("unit", "building"), (0, 1, 2), (0, 1), (False, True), (False, True), (0, 1)):
        weapon = {"aa": aa, "never_use": never}
        out.append({"name": f"{cls}.stage_{stage}.aa_{aa}.high_{int(high)}.elite_{int(elite)}"
                            f".never_{never}",
                    "class": cls, "stage": stage, "veterancy": 2.0 if elite else 0.0,
                    "slot1": weapon, "elite_slot1": weapon,
                    "target": {"high_flying": high}})
    for cls, stage in itertools.product(("unit", "building"), (0, 1, 2)):
        out.append({"name": f"{cls}.stage_{stage}.null_target", "class": cls, "stage": stage,
                    "target": {"kind": "none"}})
    return out


def detail_rows():
    out = []

    def add(name, **kw):
        out.append({"name": name, **kw})

    # Which slot-1 weapon the elite test reads.
    add("elite.reads_elite_slot1_aa_off", veterancy=2.0, stage=1,
        slot1={"aa": 1}, elite_slot1={"aa": 0})
    add("elite.reads_elite_slot1_aa_on", veterancy=2.0, stage=1,
        slot1={"aa": 0}, elite_slot1={"aa": 1})
    add("elite.reads_elite_slot1_never_use", veterancy=2.0, stage=1,
        slot1={"never_use": 0}, elite_slot1={"never_use": 1})
    add("elite.base_never_use_ignored", veterancy=2.0, stage=1,
        slot1={"never_use": 1}, elite_slot1={"never_use": 0})
    add("elite.missing_elite_slot1_falls_back", veterancy=2.0, stage=1,
        slot1={"aa": 1}, elite_slot1={"present": False})
    add("veteran_is_not_elite", veterancy=1.0, stage=1, slot1={"aa": 0}, elite_slot1={"aa": 1})
    # Target kinds: the Techno bit (+0x14 bit 0), not the Object bit, gates vt+0x54.
    for kind in ("unit", "building", "terrain", "cell"):
        add(f"target.{kind}.high_flying", stage=1, target={"kind": kind, "high_flying": True})
    # Out-of-range stages: no clamp.
    for stage in (3, 5, -1, 0x40000000):
        add(f"stage_unclamped.{stage}.air", stage=stage)
        add(f"stage_unclamped.{stage}.ground", stage=stage, target={"high_flying": False})
    # Arms ahead of H.
    add("arm_a.turret_count_not_gattling", type={"is_gattling": 0, "turret_count": 1},
        current_weapon=3)
    add("arm_a.turret_count_not_gattling_current_minus_1",
        type={"is_gattling": 0, "turret_count": 1}, current_weapon=-1)
    add("arm_a.gattling_turret_count_0", type={"turret_count": 0}, stage=2)
    add("arm_a.gattling_turret_count_2", type={"turret_count": 2}, stage=2, current_weapon=5)
    add("arm_b.building_occupants_firing", **{"class": "building"}, stage=2, occupants=1,
        type={"can_be_occupied": 1, "can_occupy_fire": 1})
    add("arm_b.building_occupiable_empty", **{"class": "building"}, stage=2, occupants=0,
        type={"can_be_occupied": 1, "can_occupy_fire": 1})
    add("arm_c.slot1_missing", stage=2, slot1={"present": False}, elite_slot1={"present": False})
    add("arm_d.slot0_missing", stage=2, slot0={"present": False}, elite_slot0={"present": False})
    add("arm_g.open_topped_transport_weapon", stage=2, open_topped=1,
        type={"open_transport_weapon": 1})
    add("arm_g.open_topped_no_transport_weapon", stage=2, open_topped=1)
    add("arm_g.transport_weapon_not_open_topped", stage=2, type={"open_transport_weapon": 1})
    add("unit_override.deployed_deploy_fire", stage=2, deployed=1, type={"deploy_fire": 1})
    add("unit_override.deployed_no_deploy_fire", stage=2, deployed=1)
    add("unit_override.deploy_fire_not_deployed", stage=2, type={"deploy_fire": 1})
    return out


def generate():
    rows = grid_rows() + detail_rows()
    names = [r["name"] for r in rows]
    if len(set(names)) != len(names):
        raise OracleError("duplicate row names")
    fixture = Fixture()
    return {"defaults": DEFAULTS, "rows": [fixture.execute(r) for r in rows]}


def metadata():
    return provenance(
        scope="TechnoClass::What_Weapon_Should_I_Use 0x006F3330 through Unit vt+0x2E4 0x00746CD0 "
              "and Building vt+0x2E4 for IsGattling types: the returned weapon index over stage "
              "0..2 x slot-1 AA x target high-flying x elite x NeverUse (unit and building), null "
              "target, elite vs base slot-1 reads, target AbstractFlags kinds, unclamped stages, and "
              "the arms ahead of H (A, B, C/D, G, the Unit deploy-fire override). Not the "
              "non-gattling selection path (0x006F3477 onwards; rows reaching it fail).",
        assumptions=[
            "One emulator; per row the scratch region and stack are rewritten, general registers "
            "zeroed, FPCW 0x0E7F, empty x87 stack.",
            "Firer: original Unit 0x007F5C70 or Building 0x007E3EBC vtable (vt+0x2E4 asserted "
            "0x00746CD0 / 0x006F3330); flags +0x14 = 7 / 3; type +0x6C4 / +0x520; Building "
            "+0x702 = 0 (no upgrades).",
            "Type: weapon slots 0/1 and elite 0/1 each with their own WeaponType and Projectile; "
            "only +0x136 (NeverUse), +0xA0 (Projectile) and Projectile +0x2A4 (AA) are set.",
            "Target: a clone of its class's original vtable with vt+0x54 replaced; AbstractFlags "
            "+0x14 = 7 aircraft/unit, 3 building, 2 TerrainClass, 0 CellClass.",
            "No write outside the stack is allowed.",
        ],
        substitutions=["Target vt+0x54 (label IsHighFlying) returns the row's verdict, logged."],
        entry_points={"what_weapon_should_i_use": SELECT, "unit_vt_2e4": ENTRIES["unit"],
                      "get_weapon": 0x70E140, "building_get_weapon": 0x4526F0,
                      "has_turrets": 0x717880})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
