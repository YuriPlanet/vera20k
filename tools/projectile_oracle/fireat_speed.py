"""Original FireAt launch speed, launch distance and aim point with the moving-target lead.

Executes, case by case:
- WeaponTypeClass::GetSpeed 0x00773070 (thiscall, the distance on the stack): for a
  projectile with ROT (+0x2DC) 0 it returns ftol(Sqrt_Approx(distance * gravity * 1.2))
  through 0x0048AB90, the gravity being Rules+0x16B8 (Gravity=) or, for a Floater
  (+0x295) projectile, 0x0048ACF0's Gravity * 0.5; otherwise the weapon's Speed= (+0xA8).
  Sqrt_Approx 0x004CAC40 and ftol 0x007C5F00 run.
- TechnoClass::FireAt's launch distance, 0x006FE4F6..0x006FE537: the signed32 squared
  X/Y deltas between the target coordinate [ESP+0x88] and the source [ESP+0x44], FILD,
  Sqrt_Approx and ftol. The value FireAt passes to GetSpeed at 0x006FE53A.
- The aim point 0x0070BCB0 (called by FireAt at 0x006FE62F): the current Target's
  vt+0x58 coordinate and, for a UnitClass target whose locomotor Is_Moving, the lead:
  ftol(distance / (GetSpeed(distance) * 0.9) * speed) along the target's body facing,
  with ObjectClass::Distance_AdjForFoundation 0x005F6360, GetSpeed, FacingClass::Current
  0x004C93D0 and Math sin/cos 0x004CACB0/0x004CAD00 all running.

Supplied (a stub per slot returning the case's value with the native stack cleanup):
the target's vt+0x58, vt+0x48 (both its Location), vt+0x2C (What_Am_I) and vt+0x538
(FootClass::GetCurrentSpeed's integer), its locomotor's Is_Moving (ILocomotion vt+0x10),
the firer's vt+0x48 (Location) and vt+0x3F4 (GetCurrentWeapon, a Weapon struct whose
first dword is the WeaponType). The target's FacingClass at +0x388 is supplied with
rate 0, so Current returns its stored facing. The null-coordinate globals 0x00B0EA90..
0x00B0EA98 the aim starts from are the zero-filled image BSS.

Rust consumers: src/sim/projectile/launch.rs (weapon_launch_speed,
fireat_launch_distance, lead_aim) through src/sim/combat/world_receiver.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP,
                               UC_X86_REG_FPCW)

from tools.native_oracle import (load_image, run_checked, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)

GET_SPEED, AIM = 0x773070, 0x70BCB0
DISTANCE_BEGIN, DISTANCE_END = 0x6FE4F6, 0x6FE537
RULES_PTR = 0x8871E0
RULES, WEAPON, PROJECTILE = SCRATCH + 0x1000, SCRATCH + 0x3000, SCRATCH + 0x3400
FIRER, FIRER_VT = SCRATCH + 0x4000, SCRATCH + 0x4800
TARGET, TARGET_VT = SCRATCH + 0x5000, SCRATCH + 0x5800
LOCO, LOCO_VT, WEAPON_STRUCT = SCRATCH + 0x6000, SCRATCH + 0x6100, SCRATCH + 0x6200
OUT = SCRATCH + 0x6400
STUB = SCRATCH + 0x9000
STUBS = {name: STUB + 0x10 * index for index, name in enumerate(
    ("target_58", "target_48", "target_whatami", "target_speed", "loco_moving",
     "firer_48", "firer_weapon"))}
STUB_NAME = {address: name for name, address in STUBS.items()}
SP = STACK_BASE + STACK_SIZE - 0x4000


def u32(value):
    return struct.pack("<I", value & 0xFFFFFFFF)


def read32(uc, address):
    return struct.unpack("<i", uc.mem_read(address, 4))[0]


def machine(gravity):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.mem_write(RULES_PTR, u32(RULES))
    uc.mem_write(RULES + 0x16B8, u32(gravity))
    return uc


def weapon(uc, projectile, rot, floater, speed):
    uc.mem_write(WEAPON + 0xA0, u32(PROJECTILE if projectile else 0))
    uc.mem_write(WEAPON + 0xA8, u32(speed))
    uc.mem_write(PROJECTILE + 0x2DC, u32(rot))
    uc.mem_write(PROJECTILE + 0x295, bytes([floater]))


def get_speed(case):
    uc = machine(case["gravity"])
    weapon(uc, case["projectile"], case["rot"], case["floater"], case["speed"])
    uc.mem_write(SP, u32(RET_MAGIC))
    uc.mem_write(SP + 4, u32(case["distance"]))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, WEAPON)
    run_checked(uc, GET_SPEED, RET_MAGIC, count=10_000)
    assert uc.reg_read(UC_X86_REG_ESP) == SP + 8, "GetSpeed pops its one argument"
    return dict(input=case, speed=struct.unpack("<i", u32(uc.reg_read(UC_X86_REG_EAX)))[0])


def distance(case):
    uc = machine(6)
    (sx, sy), (tx, ty) = case["source"], case["target"]
    for offset, value in ((0x44, sx), (0x48, sy), (0x88, tx), (0x8C, ty)):
        uc.mem_write(SP + offset, u32(value))
    uc.reg_write(UC_X86_REG_ESP, SP)
    run_checked(uc, DISTANCE_BEGIN, DISTANCE_END, count=10_000,
                required_addresses=[0x4CAC40, 0x7C5F00])
    assert uc.reg_read(UC_X86_REG_ESP) == SP
    return dict(input=case, distance=struct.unpack("<i", u32(uc.reg_read(UC_X86_REG_EAX)))[0])


def aim(case):
    uc = machine(case["gravity"])
    weapon(uc, True, case["rot"], 0, case["speed"])
    uc.mem_write(FIRER, u32(FIRER_VT))
    uc.mem_write(FIRER + 0x2B4, u32(TARGET if case["target"] else 0))
    for slot, name in ((0x48, "firer_48"), (0x3F4, "firer_weapon")):
        uc.mem_write(FIRER_VT + slot, u32(STUBS[name]))
    uc.mem_write(TARGET, u32(TARGET_VT))
    for slot, name in ((0x58, "target_58"), (0x48, "target_48"), (0x2C, "target_whatami"),
                       (0x538, "target_speed")):
        uc.mem_write(TARGET_VT + slot, u32(STUBS[name]))
    uc.mem_write(TARGET + 0x674, u32(LOCO))
    uc.mem_write(LOCO, u32(LOCO_VT))
    uc.mem_write(LOCO_VT + 0x10, u32(STUBS["loco_moving"]))
    # FacingClass at +0x388: the facing dword, rate (+0x14) 0 -> Current returns it.
    uc.mem_write(TARGET + 0x388, u32(case["facing"]))
    uc.mem_write(TARGET + 0x388 + 0x14, b"\0\0")
    uc.mem_write(WEAPON_STRUCT, u32(WEAPON if case["weapon"] else 0))
    uc.mem_write(SP, u32(RET_MAGIC))
    uc.mem_write(SP + 4, u32(OUT))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, FIRER)
    calls = []

    def ret(value, pops):
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, read32(uc, sp) & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def coords(values):
        out = read32(uc, uc.reg_read(UC_X86_REG_ESP) + 4) & 0xFFFFFFFF
        uc.mem_write(out, struct.pack("<iii", *values))
        ret(out, 4)

    def hook(_uc, address, _size, _data):
        name = STUB_NAME.get(address)
        if name is None:
            return
        calls.append(name)
        if name in ("target_58", "target_48"):
            coords(case["target_xyz"])
        elif name == "firer_48":
            coords(case["firer_xyz"])
        elif name == "target_whatami":
            ret(case["whatami"], 0)
        elif name == "target_speed":
            ret(case["target_speed"], 0)
        elif name == "loco_moving":
            ret(case["moving"], 4)
        elif name == "firer_weapon":
            ret(WEAPON_STRUCT, 0)

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, AIM, RET_MAGIC, count=100_000)
    assert uc.reg_read(UC_X86_REG_ESP) == SP + 8, "the aim pops its out pointer"
    return dict(input=case, calls=calls, aim=list(struct.unpack("<iii", uc.mem_read(OUT, 12))))


SPEED_DISTANCES = (0, 1, 2, 3, 100, 255, 256, 257, 384, 511, 512, 700, 1000, 1023, 1024,
                   1280, 1500, 1792, 2048, 2560, 3072, 4096, 5120, 8192, 20000, 1_000_000,
                   0x7FFFFFFF, -1, -256)


def speed_cases():
    for gravity in (6, 3, 0, 1, 10, -6):
        for floater in (0, 1):
            for distance_value in SPEED_DISTANCES:
                yield dict(projectile=True, rot=0, floater=floater, gravity=gravity, speed=40,
                           distance=distance_value)
    for rot in (1, 5, -1):
        for speed in (40, 100, 0):
            yield dict(projectile=True, rot=rot, floater=0, gravity=6, speed=speed,
                       distance=1024)
    for speed in (40, 0):
        yield dict(projectile=False, rot=0, floater=0, gravity=6, speed=speed, distance=1024)


def distance_cases():
    base = (40 * 256 + 128, 30 * 256 + 128)
    deltas = [(0, 0), (1, 0), (0, 1), (-1, 0), (128, 0), (255, 0), (256, 0), (257, 3),
              (1024, 0), (0, -1024), (724, 724), (-724, 724), (1000, 1000), (3, 4),
              (300, -400), (2048, 1536), (-5000, 1200), (46340, 0), (46341, 0),
              (50000, 50000), (65535, -65535), (123, 4567), (-8191, -8191)]
    for dx, dy in deltas:
        yield dict(source=list(base), target=[base[0] + dx, base[1] + dy])


FACINGS = (0x0000, 0x2000, 0x4000, 0x6000, 0x8000, 0xA000, 0xC000, 0xE000, 0x1234, 0xFFFF,
           0x3FFF, 0xC001)


def aim_cases():
    firer = (20 * 256 + 128, 20 * 256 + 128, 0)
    offsets = [(1024, 0, 0), (0, -1024, 0), (-700, 700, 0), (256, 0, 0), (2048, 1024, 208),
               (0, 0, 0), (3000, -2000, -104), (128, 64, 0)]
    for whatami in (1, 0xF, 2, 6):
        for moving in (0, 1):
            yield dict(target=True, whatami=whatami, moving=moving, target_speed=7,
                       facing=0x4000, gravity=6, rot=0, speed=40, weapon=True,
                       firer_xyz=list(firer),
                       target_xyz=[firer[0] + 1024, firer[1], firer[2]])
    for offset in offsets:
        for facing in FACINGS:
            for target_speed in (0, 1, 4, 7, 12, 30):
                yield dict(target=True, whatami=1, moving=1, target_speed=target_speed,
                           facing=facing, gravity=6, rot=0, speed=40, weapon=True,
                           firer_xyz=list(firer),
                           target_xyz=[firer[0] + offset[0], firer[1] + offset[1],
                                       firer[2] + offset[2]])
    for gravity, rot, speed in ((3, 0, 40), (10, 0, 40), (6, 10, 40), (6, 10, 100), (6, 10, 0)):
        for facing in (0x2000, 0x9000):
            yield dict(target=True, whatami=1, moving=1, target_speed=6, facing=facing,
                       gravity=gravity, rot=rot, speed=speed, weapon=True,
                       firer_xyz=list(firer), target_xyz=[firer[0] + 1500, firer[1] - 300, 0])
    yield dict(target=True, whatami=1, moving=1, target_speed=6, facing=0x2000, gravity=6,
               rot=0, speed=40, weapon=False, firer_xyz=list(firer),
               target_xyz=[firer[0] + 1500, firer[1], 0])
    yield dict(target=False, whatami=1, moving=1, target_speed=6, facing=0x2000, gravity=6,
               rot=0, speed=40, weapon=True, firer_xyz=list(firer),
               target_xyz=[firer[0] + 1500, firer[1], 0])


def generate():
    return dict(
        speed=[get_speed(case) for case in speed_cases()],
        distance=[distance(case) for case in distance_cases()],
        aim=[aim(case) for case in aim_cases()],
    )


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("WeaponTypeClass::GetSpeed over ROT/Floater/gravity/distance, FireAt's launch "
               "distance block over signed deltas incl. signed32 overflow, and the aim point "
               "0x0070BCB0 over target class, motion, speed, body facing, distance, gravity, "
               "projectile ROT/Speed, a missing weapon and a missing target."),
        assumptions=[
            "x87 control word 0x0E7F (PC53, chop), the harness default.",
            "Rules+0x16B8 Gravity, WeaponType +0xA0/+0xA8, BulletType +0x2DC/+0x295 supplied.",
            "The target FacingClass at +0x388 has rate 0 (Current returns the stored facing).",
            "0x00B0EA90..0x00B0EA98 (the aim's starting coordinate) are the zero-filled BSS.",
        ],
        substitutions=[
            "target vt+0x58 / vt+0x48 return the case's target coordinate",
            "firer vt+0x48 returns the case's firer coordinate",
            "target vt+0x2C returns the case's What_Am_I; vt+0x538 its current speed",
            "target locomotor ILocomotion vt+0x10 Is_Moving returns the case's flag",
            "firer vt+0x3F4 GetCurrentWeapon returns a Weapon struct naming the case's weapon",
        ],
        entry_points={"get_speed": GET_SPEED, "distance_begin": DISTANCE_BEGIN,
                      "distance_end": DISTANCE_END, "aim": AIM},
    ))
