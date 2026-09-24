"""Original FireAt damage build and TechnoClass::ReceiveDamage defence divides.

Executes, case by case:
- TechnoClass::FireAt 0x006FE302..0x006FE460, the damage the bullet carries: Damage=
  (WeaponType +0xA4), zeroed for IsSonic (+0x130) or UseFireParticles (+0x129); for a
  positive value the firepower fold ftol(House+0x188 * Techno+0x160 * damage) and the
  FIREPOWER rank stage ftol(damage * Rules+0x670 VeteranCombat) (veteran: type +0x29E;
  elite: +0x29E or +0x2B0, ranks from VeterancyStruct 0x0074FF90 / 0x00750010 on the
  Techno+0x150 float); then, for any sign, the occupied stage ftol(damage * f32
  Rules+0xF40) (vt+0x400), the bunker stage ftol(damage * f32 Rules+0xF4C) (a
  Techno+0x2E4 link on a non-building, vt+0x2C != 6) and the open-topped stage
  ftol(damage * f32 Rules+0xF58) (Techno+0x82). ftol 0x007C5F00 runs.
- TechnoClass::ReceiveDamage 0x00701939..0x007019E3, the defence divides a
  non-negative, defended hit takes: ftol(damage / (GetArmorMultForType * Techno+0x158))
  with HouseClass::GetArmorMultForType 0x0050BD30 running on the owner's HouseType
  (+0x34) per-category floats (+0x100 infantry, +0x104 units, +0x108 aircraft,
  +0x10C buildings, +0x110 BuildCat 5 buildings, else 1.0f); then the STRONGER rank
  stage ftol(damage / Rules+0x688 VeteranArmor) (veteran: type +0x29D; elite: +0x29D or
  +0x2AF); then the minimum of one. The gate before it (ignoreDefenses, damage < 0)
  is control flow and is not executed here.

Supplied (a stub per slot returning the case's value with the native stack cleanup):
the Techno's vt+0x84 (its TechnoType), vt+0x400 (the occupied predicate; natively
BuildingClass 0x00458DD0 CanBeOccupied && CanOccupyFire && occupants > 0, 0 elsewhere)
and vt+0x2C (What_Am_I), and the TechnoType's vt+0x2C (its AbstractType). Everything
else is data in the case.

Rust consumers: src/sim/combat/damage/attacker.rs (fire_damage) and
src/sim/combat/damage/receive.rs (the defence divides) through
src/sim/combat/world_receiver.rs and src/sim/combat/mod.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDI, UC_X86_REG_EIP,
                               UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.native_oracle import (load_image, run_checked, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)

FIRE_BEGIN, FIRE_END = 0x6FE302, 0x6FE460
RECEIVE_BEGIN, RECEIVE_END = 0x701939, 0x7019E3
RULES_PTR = 0x8871E0
RULES, WEAPON = SCRATCH + 0x1000, SCRATCH + 0x3000
TECHNO, TECHNO_VT = SCRATCH + 0x4000, SCRATCH + 0x4800
TYPE, TYPE_VT = SCRATCH + 0x5000, SCRATCH + 0x6800
HOUSE, HOUSE_TYPE = SCRATCH + 0x7000, SCRATCH + 0x7800
DAMAGE = SCRATCH + 0x8000
STUB = SCRATCH + 0x9000
STUBS = {name: STUB + 0x10 * index for index, name in enumerate(
    ("techno_type", "techno_occupied", "techno_whatami", "type_whatami"))}
STUB_NAME = {address: name for name, address in STUBS.items()}
SP = STACK_BASE + STACK_SIZE - 0x4000
# Native stage instructions whose execution each row records.
FIRE_STAGES = {0x6FE328: "zeroed", 0x6FE337: "firepower_fold", 0x6FE3C8: "rank_firepower",
               0x6FE3F1: "occupied", 0x6FE421: "bunkered", 0x6FE445: "open_topped"}
RECEIVE_STAGES = {0x7019C4: "rank_armor", 0x7019DD: "minimum"}
# GetArmorMultForType's loads: which HouseType float (or the 1.0f default) it returned.
ARMOR_LOADS = {0x50BD5B: 0x100, 0x50BD69: 0x104, 0x50BD77: 0x108, 0x50BD8E: 0x110,
               0x50BD9C: 0x10C, 0x50BDA5: None}

F32_1_1 = 0x3F8CCCCD  # the %f single of "1.1"
F32_1_2 = 0x3F99999A
F32_1_3 = 0x3FA66666
F32_1_5 = 0x3FC00000
F32_0_8 = 0x3F4CCCCD
F32_1 = 0x3F800000


def u32(value):
    return struct.pack("<I", value & 0xFFFFFFFF)


def f64_from_f32(bits):
    """CCINIClass::ReadDouble's value: the parsed single widened to a double."""
    return struct.unpack("<d", struct.pack("<d", struct.unpack("<f", u32(bits))[0]))[0]


def machine():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.mem_write(RULES_PTR, u32(RULES))
    uc.mem_write(TECHNO, u32(TECHNO_VT))
    uc.mem_write(TECHNO_VT + 0x84, u32(STUBS["techno_type"]))
    uc.mem_write(TECHNO_VT + 0x400, u32(STUBS["techno_occupied"]))
    uc.mem_write(TECHNO_VT + 0x2C, u32(STUBS["techno_whatami"]))
    uc.mem_write(TYPE, u32(TYPE_VT))
    uc.mem_write(TYPE_VT + 0x2C, u32(STUBS["type_whatami"]))
    uc.mem_write(TECHNO + 0x21C, u32(HOUSE))
    uc.mem_write(HOUSE + 0x34, u32(HOUSE_TYPE))
    return uc


def stubs(uc, case, calls, observed=()):
    def ret(value):
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, struct.unpack("<I", uc.mem_read(sp, 4))[0])
        uc.reg_write(UC_X86_REG_ESP, sp + 4)

    def hook(_uc, address, _size, _data):
        if address in observed:
            calls.append(observed[address])
            return
        name = STUB_NAME.get(address)
        if name is None:
            return
        calls.append(name)
        if name == "techno_type":
            ret(TYPE)
        elif name == "techno_occupied":
            ret(case["occupied"])
        elif name == "techno_whatami":
            ret(case["whatami"])
        elif name == "type_whatami":
            ret(case["type_whatami"])

    uc.hook_add(UC_HOOK_CODE, hook)


def techno(uc, case):
    uc.mem_write(TECHNO + 0x150, u32(case["veterancy"]))
    uc.mem_write(TECHNO + 0x158, struct.pack("<Q", case.get("armor_mult", 0x3FF0000000000000)))
    uc.mem_write(TECHNO + 0x160, struct.pack("<Q", case.get("unit_firepower",
                                                            0x3FF0000000000000)))
    uc.mem_write(HOUSE + 0x188, struct.pack("<Q", case.get("house_firepower",
                                                           0x3FF0000000000000)))


def fire_damage(case):
    uc = machine()
    techno(uc, case)
    uc.mem_write(WEAPON + 0xA4, u32(case["damage"]))
    uc.mem_write(WEAPON + 0x130, bytes([case["sonic"]]))
    uc.mem_write(WEAPON + 0x129, bytes([case["fire_particles"]]))
    uc.mem_write(TYPE + 0x29E, bytes([case["veteran_firepower"]]))
    uc.mem_write(TYPE + 0x2B0, bytes([case["elite_firepower"]]))
    uc.mem_write(TECHNO + 0x2E4, u32(0xDEAD0000 if case["bunker_link"] else 0))
    uc.mem_write(TECHNO + 0x82, bytes([case["open_topped"]]))
    uc.mem_write(RULES + 0x670, struct.pack("<d", f64_from_f32(case["veteran_combat"])))
    uc.mem_write(RULES + 0xF40, u32(case["occupy_mult"]))
    uc.mem_write(RULES + 0xF4C, u32(case["bunker_mult"]))
    uc.mem_write(RULES + 0xF58, u32(case["open_topped_mult"]))
    uc.mem_write(SP + 0x40, u32(WEAPON))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ESI, TECHNO)
    calls = []
    stubs(uc, case, calls, FIRE_STAGES)
    run_checked(uc, FIRE_BEGIN, FIRE_END, count=100_000)
    assert uc.reg_read(UC_X86_REG_ESP) == SP
    edi = struct.unpack("<i", u32(uc.reg_read(UC_X86_REG_EDI)))[0]
    stored = struct.unpack("<i", uc.mem_read(SP + 0x2C, 4))[0]
    assert edi == stored, "EDI and [ESP+0x2C] carry the same damage out of the block"
    return dict(input=case, calls=[c for c in calls if c not in FIRE_STAGES.values()],
                stages=[c for c in calls if c in FIRE_STAGES.values()], damage=stored)


def receive_divide(case):
    uc = machine()
    techno(uc, case)
    for offset, bits in zip((0x100, 0x104, 0x108, 0x10C, 0x110), case["house_type_mults"]):
        uc.mem_write(HOUSE_TYPE + offset, u32(bits))
    uc.mem_write(TYPE + 0xE08, u32(case["build_cat"]))
    uc.mem_write(TYPE + 0x29D, bytes([case["veteran_stronger"]]))
    uc.mem_write(TYPE + 0x2AF, bytes([case["elite_stronger"]]))
    uc.mem_write(RULES + 0x688, struct.pack("<d", f64_from_f32(case["veteran_armor"])))
    uc.mem_write(SP + 0x14, u32(case["damage"]))
    uc.mem_write(DAMAGE, u32(case["damage"]))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ESI, TECHNO)
    uc.reg_write(UC_X86_REG_EBX, DAMAGE)
    calls = []
    observed = dict(RECEIVE_STAGES)
    observed.update({address: ("armor_load", offset) for address, offset in ARMOR_LOADS.items()})
    stubs(uc, case, calls, observed)
    run_checked(uc, RECEIVE_BEGIN, RECEIVE_END, count=100_000,
                required_addresses=[0x50BD30, 0x7C5F00])
    assert uc.reg_read(UC_X86_REG_ESP) == SP
    loads = [c[1] for c in calls if isinstance(c, tuple)]
    assert len(loads) == 1, "GetArmorMultForType loads exactly one float"
    armor = (struct.unpack("<I", uc.mem_read(HOUSE_TYPE + loads[0], 4))[0]
             if loads[0] is not None else struct.unpack("<I", uc.mem_read(0x7E2AC8, 4))[0])
    return dict(input=case, calls=[c for c in calls if c in ("techno_type", "type_whatami")],
                stages=[c for c in calls if c in RECEIVE_STAGES.values()],
                armor_mult_for_type=armor,
                damage=struct.unpack("<i", uc.mem_read(DAMAGE, 4))[0])


VETERANCY = {"rookie": 0x00000000, "almost": 0x3F7FFFFF, "veteran": 0x3F800000,
             "veteran_half": 0x3FC00000, "almost_elite": 0x3FFFFFFF, "elite": 0x40000000}
FIRE_DAMAGES = (0, 1, 3, 4, 5, 8, 9, 10, 15, 20, 25, 30, 40, 50, 65, 90, 100, 150, 200, 1000,
                2147483647, -1, -3, -50, -100)


def fire_cases():
    stock = dict(sonic=0, fire_particles=0, veteran_firepower=0, elite_firepower=0,
                 bunker_link=0, whatami=1, occupied=0, open_topped=0, type_whatami=0x28,
                 veteran_combat=F32_1_1, occupy_mult=F32_1_2, bunker_mult=F32_1_3,
                 open_topped_mult=F32_1_2, veterancy=VETERANCY["rookie"])
    for damage in FIRE_DAMAGES:
        yield dict(stock, damage=damage)
        yield dict(stock, damage=damage, sonic=1)
        yield dict(stock, damage=damage, fire_particles=1)
        for rank in ("veteran", "elite", "almost", "almost_elite"):
            for veteran_firepower, elite_firepower in ((1, 0), (0, 1), (1, 1)):
                yield dict(stock, damage=damage, veterancy=VETERANCY[rank],
                           veteran_firepower=veteran_firepower, elite_firepower=elite_firepower)
        yield dict(stock, damage=damage, occupied=1)
        yield dict(stock, damage=damage, occupied=1, veterancy=VETERANCY["elite"],
                   veteran_firepower=1)
        yield dict(stock, damage=damage, bunker_link=1, whatami=1)
        yield dict(stock, damage=damage, bunker_link=1, whatami=6)
        yield dict(stock, damage=damage, open_topped=1)
        yield dict(stock, damage=damage, open_topped=1, veterancy=VETERANCY["veteran"],
                   veteran_firepower=1)
        yield dict(stock, damage=damage, occupied=1, bunker_link=1, open_topped=1)
        yield dict(stock, damage=damage, sonic=1, occupied=1, open_topped=1)
    for house, unit in ((0x3FF8000000000000, 0x3FF0000000000000),
                        (0x3FF0000000000000, 0x4000000000000000),
                        (0x3FF199999999999A, 0x3FF3333333333333)):
        for damage in (10, 65, -50):
            yield dict(stock, damage=damage, house_firepower=house, unit_firepower=unit)
            yield dict(stock, damage=damage, house_firepower=house, unit_firepower=unit,
                       veterancy=VETERANCY["veteran"], veteran_firepower=1)
    for combat in (F32_1, F32_1_2, F32_1_5):
        for occupy in (F32_1, F32_1_5, F32_0_8):
            for damage in (10, 25, 36, 65):
                yield dict(stock, damage=damage, veteran_combat=combat, occupy_mult=occupy,
                           occupied=1, veterancy=VETERANCY["veteran"], veteran_firepower=1)


RECEIVE_DAMAGES = (0, 1, 2, 3, 5, 10, 13, 25, 26, 30, 32, 36, 49, 65, 90, 100, 150, 200, 1000,
                   123457, 2147483647)
CATEGORIES = ((0x10, 0, "infantry"), (0x28, 0, "unit"), (0x03, 0, "aircraft"),
              (0x07, 0, "building"), (0x07, 5, "defense"), (0x07, 3, "power"),
              (0x0F, 0, "other"))


def receive_cases():
    stock_mults = [F32_1] * 5
    distinct = [F32_0_8, F32_1_2, F32_1_5, 0x3F000000, 0x40000000]  # 0.8, 1.2, 1.5, 0.5, 2.0
    stock = dict(whatami=1, occupied=0, veteran_stronger=0, elite_stronger=0,
                 veteran_armor=F32_1_5, veterancy=VETERANCY["rookie"], build_cat=0)
    for type_whatami, build_cat, _name in CATEGORIES:
        for mults in (stock_mults, distinct):
            for damage in RECEIVE_DAMAGES:
                yield dict(stock, damage=damage, type_whatami=type_whatami, build_cat=build_cat,
                           house_type_mults=list(mults))
    for damage in RECEIVE_DAMAGES:
        for rank in ("veteran", "elite", "almost", "almost_elite"):
            for veteran_stronger, elite_stronger in ((1, 0), (0, 1), (1, 1), (0, 0)):
                yield dict(stock, damage=damage, type_whatami=0x28, house_type_mults=stock_mults,
                           veterancy=VETERANCY[rank], veteran_stronger=veteran_stronger,
                           elite_stronger=elite_stronger)
        for armor_mult in (0x3FF8000000000000, 0x4000000000000000, 0x3FE0000000000000,
                           0x3FF199999999999A, 0x0000000000000000):
            yield dict(stock, damage=damage, type_whatami=0x28, house_type_mults=distinct,
                       armor_mult=armor_mult)
            yield dict(stock, damage=damage, type_whatami=0x10, house_type_mults=stock_mults,
                       armor_mult=armor_mult, veterancy=VETERANCY["elite"], veteran_stronger=1)
        for zero in range(5):
            mults = list(stock_mults)
            mults[zero] = 0
            for type_whatami, build_cat, _name in CATEGORIES:
                yield dict(stock, damage=damage, type_whatami=type_whatami, build_cat=build_cat,
                           house_type_mults=mults)
        for veteran_armor in (F32_1, F32_1_2, 0x40000000, 0):
            yield dict(stock, damage=damage, type_whatami=0x28, house_type_mults=stock_mults,
                       veterancy=VETERANCY["veteran"], veteran_stronger=1,
                       veteran_armor=veteran_armor)


def generate():
    return dict(
        fire=[fire_damage(case) for case in fire_cases()],
        receive=[receive_divide(case) for case in receive_cases()],
    )


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("FireAt's damage build over Damage= sign/magnitude (incl. int32 overflow), "
               "IsSonic/UseFireParticles, the firepower fold, rank x FIREPOWER flags, "
               "VeteranCombat, and the occupied/bunker/open-topped stages; ReceiveDamage's "
               "defence divides over damage, target category and BuildCat, HouseType "
               "per-category floats incl. zero, the per-object armour multiplier incl. zero, "
               "rank x STRONGER flags and VeteranArmor incl. zero, and the minimum of one."),
        assumptions=[
            "x87 control word 0x0E7F (PC53, chop), the harness default.",
            "Rules VeteranCombat/VeteranArmor hold ReadDouble's widened single.",
            "The FireAt block's [ESP+0x40] is the WeaponType; the ReceiveDamage block's "
            "[ESP+0x14] and [EBX] hold the incoming damage (the prologue's copies).",
            "The ignoreDefenses / damage < 0 gate before the ReceiveDamage block is not run.",
        ],
        substitutions=[
            "Rows record which stage instructions executed (stages) and which float "
            "GetArmorMultForType loaded (armor_mult_for_type), by address observation.",
            "Techno vt+0x84 returns the case's TechnoType",
            "Techno vt+0x400 returns the case's occupied flag",
            "Techno vt+0x2C returns the case's What_Am_I",
            "TechnoType vt+0x2C returns the case's AbstractType",
        ],
        entry_points={"fire_begin": FIRE_BEGIN, "fire_end": FIRE_END,
                      "receive_begin": RECEIVE_BEGIN, "receive_end": RECEIVE_END},
    ))
