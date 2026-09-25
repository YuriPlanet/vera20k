"""Original BulletClass impact resolution 0x00468D80 up to its first DetonateAtCoord.

Executes the ladder BulletClass::AI runs on every detonating bullet (called at
0x00466771 with 0 and at 0x00467FA2 with the AI's impact flag) from its entry to
the first `DetonateAtCoord 0x004690B0` call, and records the coordinate passed
there (or that no call happens, for Cluster <= 0):
- start from the bullet's Location (+0x9C); remember the Target (+0x10C) when its
  vt+0x54 (IsInAir) answers true;
- unless the BulletType is Inaccurate (+0x2A2): a Target whose vt+0x48 lies
  within 0x20 (ftol Sqrt_Approx 3-D) and a type that is not Airburst (+0x294)
  selects vt+0x48;
- unless the warhead has EMEffect (+0x154) or the type is Airburst: with the
  argument clear, a type that is neither Arcing (+0x29B) nor ROT > 0 (+0x2DC)
  selects the ProximityDetector reference (+0xD0) when it is not Empty;
- an in-air Target whose vt+0x78 layer is not 2 selects vt+0xA4 within 0x80,
  else any Target within 0x2A (ObjectClass::DistanceTo 0x005F6360, less the
  foundation for a Building) selects vt+0x58, or vt+0xA4 for a Building whose
  type has a TargetCoordOffset (+0xEBC).

Runs: 0x00468D80, ObjectClass::GetCoords 0x005F65A0 on the bullet, 0x0041C230,
Sqrt_Approx 0x004CAC40, ftol 0x007C5F00 and DistanceTo 0x005F6360.
Supplied: the BulletType, warhead and bullet fields; the Target's vt+0x48 /
+0x54 / +0x58 / +0x78 / +0xA4 / +0x2C as stubs returning the case's values (the
three coordinates distinct, so each row shows which getter won); the Building
type's Width/Height (0x0045ECA0 / 0x0045EC90) as stubs.

Rust consumer: src/sim/projectile.rs (resolve_impact_coord).
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP,
                               UC_X86_REG_FPCW)

from tools.native_oracle import (load_image, run_checked, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)

LADDER, DETONATE_AT = 0x468D80, 0x4690B0
BULLET_VTABLE = 0x7E46E4
WIDTH, HEIGHT = 0x45ECA0, 0x45EC90
BULLET, TYPE, WARHEAD = SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x3000
TARGET, TARGET_VT, BUILDING_TYPE = SCRATCH + 0x4000, SCRATCH + 0x4800, SCRATCH + 0x5000
STUB = SCRATCH + 0x9000
STUBS = {name: STUB + 0x10 * index for index, name in enumerate(
    ("coords_48", "in_air", "coords_58", "layer", "coords_a4", "what_am_i"))}
STUB_NAME = {address: name for name, address in STUBS.items()}
SP = STACK_BASE + STACK_SIZE - 0x4000
TARGET_XYZ = (5000, 6000, 416)
# Distinct answers per getter: equal in retail for a Unit, apart for a Building.
OFFSET_58, OFFSET_A4 = (3, 5, 7), (11, 13, 17)


def u32(value):
    return struct.pack("<I", value & 0xFFFFFFFF)


def read32(uc, address):
    return struct.unpack("<I", uc.mem_read(address, 4))[0]


def shifted(base, delta):
    return [base[axis] + delta[axis] for axis in range(3)]


def execute(case):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)

    target = case["target"]
    uc.mem_write(BULLET, u32(BULLET_VTABLE))
    uc.mem_write(BULLET + 0x9C, struct.pack("<iii", *case["location"]))
    uc.mem_write(BULLET + 0xAC, u32(TYPE))
    uc.mem_write(BULLET + 0x128, u32(WARHEAD))
    uc.mem_write(BULLET + 0x10C, u32(TARGET if target else 0))
    uc.mem_write(BULLET + 0xD0, struct.pack("<iii", *case["reference"]))
    uc.mem_write(TYPE + 0x294, bytes([case["airburst"]]))
    uc.mem_write(TYPE + 0x29B, bytes([case["arcing"]]))
    uc.mem_write(TYPE + 0x2A2, bytes([case["inaccurate"]]))
    uc.mem_write(TYPE + 0x2AC, u32(case["cluster"]))
    uc.mem_write(TYPE + 0x2DC, u32(case["rot"]))
    uc.mem_write(WARHEAD + 0x154, bytes([case["em_effect"]]))
    uc.mem_write(TARGET, u32(TARGET_VT))
    for slot, name in ((0x48, "coords_48"), (0x54, "in_air"), (0x58, "coords_58"),
                       (0x78, "layer"), (0xA4, "coords_a4"), (0x2C, "what_am_i")):
        uc.mem_write(TARGET_VT + slot, u32(STUBS[name]))
    uc.mem_write(TARGET + 0x520, u32(BUILDING_TYPE))
    if target:
        uc.mem_write(BUILDING_TYPE + 0xEBC, struct.pack("<iii", *target["target_coord_offset"]))

    calls = []

    def ret(value, pops):
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, read32(uc, sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def coords(values):
        out = read32(uc, uc.reg_read(UC_X86_REG_ESP) + 4)
        uc.mem_write(out, struct.pack("<iii", *values))
        ret(out, 4)

    def hook(_uc, address, _size, _data):
        if address == WIDTH:
            calls.append("width")
            ret(target["foundation"][0], 4)
            return
        if address == HEIGHT:
            calls.append("height")
            ret(target["foundation"][1], 0)
            return
        name = STUB_NAME.get(address)
        if name is None:
            return
        calls.append(name)
        if name == "coords_48":
            coords(TARGET_XYZ)
        elif name == "coords_58":
            coords(shifted(TARGET_XYZ, OFFSET_58))
        elif name == "coords_a4":
            coords(shifted(TARGET_XYZ, OFFSET_A4))
        elif name == "in_air":
            ret(target["in_air"], 0)
        elif name == "layer":
            ret(target["layer"], 0)
        elif name == "what_am_i":
            ret(target["what_am_i"], 0)

    uc.hook_add(UC_HOOK_CODE, hook)
    uc.mem_write(SP, u32(RET_MAGIC))
    uc.mem_write(SP + 4, u32(case["impact_flag"]))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, BULLET)
    stop = run_checked(uc, LADDER, (DETONATE_AT, RET_MAGIC), count=200_000)
    detonation = None
    if stop == DETONATE_AT:
        pointer = read32(uc, uc.reg_read(UC_X86_REG_ESP) + 4)
        detonation = list(struct.unpack("<iii", uc.mem_read(pointer, 12)))
    return dict(input=case, calls=calls, detonation=detonation)


GROUND = dict(in_air=0, layer=2, what_am_i=1, foundation=[0, 0], target_coord_offset=[0, 0, 0])
TARGETS = {
    "ground_unit": GROUND,
    "cell": dict(GROUND, what_am_i=0xB),
    "air_unit": dict(GROUND, in_air=1, layer=3, what_am_i=3),
    "in_air_ground_layer": dict(GROUND, in_air=1, layer=2),
    "building_2x2": dict(GROUND, what_am_i=6, foundation=[2, 2]),
    "building_4x3_offset": dict(GROUND, what_am_i=6, foundation=[4, 3],
                                target_coord_offset=[0, 64, 0]),
    "building_3x3_offset_z": dict(GROUND, what_am_i=6, foundation=[3, 3],
                                  target_coord_offset=[0, 0, 5]),
}
DELTAS = [(0, 0, 0), (31, 0, 0), (32, 0, 0), (0, -31, 0), (0, 0, 32), (41, 0, 0), (42, 0, 0),
          (0, 41, 0), (-42, 0, 0), (17, 17, 17), (18, 18, 18), (20, 20, 20), (24, 24, 21),
          (25, 25, 20), (30, 30, 0), (29, 29, 5), (127, 0, 0), (128, 0, 0), (90, 90, 0),
          (91, 91, 0), (300, 0, 0), (290, 0, 0), (250, 0, 0), (440, 0, 0), (480, 0, 0),
          (500, 0, 0), (1000, -700, 30)]
BASE_FLAGS = dict(inaccurate=0, airburst=0, arcing=1, rot=0, em_effect=0, impact_flag=1,
                  cluster=1, reference=[4000, 4000, 0])


def cases():
    for name, target in TARGETS.items():
        for delta in DELTAS:
            yield dict(BASE_FLAGS, target_kind=name, target=target,
                       location=shifted(TARGET_XYZ, delta))
    variations = [dict(inaccurate=1), dict(airburst=1), dict(em_effect=1), dict(cluster=0),
                  dict(cluster=-1), dict(cluster=3),
                  dict(arcing=0, impact_flag=0), dict(arcing=0, impact_flag=1),
                  dict(arcing=0, impact_flag=0, rot=5), dict(arcing=0, impact_flag=0, rot=-1),
                  dict(arcing=0, impact_flag=0, reference=[0, 0, 0]),
                  dict(arcing=0, impact_flag=0, reference=[0, 0, 1]),
                  dict(arcing=0, impact_flag=0, em_effect=1),
                  dict(arcing=0, impact_flag=0, airburst=1)]
    for variation in variations:
        for name in ("ground_unit", "air_unit", "building_4x3_offset"):
            for delta in ((0, 0, 0), (31, 0, 0), (41, 0, 0), (127, 0, 0), (500, 0, 0)):
                yield dict(BASE_FLAGS, **variation, target_kind=name, target=TARGETS[name],
                           location=shifted(TARGET_XYZ, delta))
    for variation in (dict(), dict(arcing=0, impact_flag=0),
                      dict(arcing=0, impact_flag=0, reference=[0, 0, 0])):
        for delta in ((0, 0, 0), (41, 0, 0), (500, 0, 0)):
            yield dict(BASE_FLAGS, **variation, target_kind="none", target=None,
                       location=shifted(TARGET_XYZ, delta))


def generate():
    return [execute(case) for case in cases()]


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("BulletClass impact resolution 0x00468D80 to its first DetonateAtCoord over "
               "target kind (none, ground unit, cell, air unit, in-air on the ground layer, "
               "buildings with and without TargetCoordOffset), 3-D distances around the "
               "0x20 / 0x2A / 0x80 thresholds and the foundation adjustment, the Inaccurate, "
               "Airburst, Arcing, ROT, EMEffect, Cluster and impact-flag gates, and the "
               "ProximityDetector reference (+0xD0) arm."),
        assumptions=[
            "x87 control word 0x0E7F (PC53, chop), the harness default.",
            "BulletType +0x294/+0x29B/+0x2A2/+0x2AC/+0x2DC, warhead +0x154, bullet +0x9C, "
            "+0xD0, +0x10C and BuildingType +0xEBC supplied.",
            "CoordStruct Empty (0x0089DE30) is the image's zero-filled value.",
        ],
        substitutions=[
            "target vt+0x48 / vt+0x58 / vt+0xA4 return the case's coordinate plus distinct "
            "offsets (0 / (3,5,7) / (11,13,17)) so each row names the getter used",
            "target vt+0x54 IsInAir, vt+0x78 layer and vt+0x2C What_Am_I return the case's values",
            "BuildingTypeClass Width 0x0045ECA0 / Height 0x0045EC90 return the case's foundation",
            "DetonateAtCoord 0x004690B0 is the stop boundary; its coordinate argument is recorded",
        ],
        entry_points={"ladder": LADDER, "detonate_at": DETONATE_AT},
    ))
