"""Original launch scatter, shrapnel launch velocity and cluster direction math.

Executes, case by case:
- TechnoClass::FireAt's launch scatter, 0x006FE663..0x006FE8EE, both arms
  (flak: FlakScatter and not Inviso; plain). Sqrt_Approx 0x004CAC40,
  Math::sin 0x004CACB0, Math::cos 0x004CAD00 and ftol 0x007C5F00 run.
- BulletClass::SpawnShrapnel's child velocity, from the zeroed velocity to the
  child's launch call: the hostile-object branch 0x0046A5B2..0x0046A875 and
  the random-cell branch 0x0046AA66..0x0046AD29. Math::atan2 0x004CAE30,
  the vector lengths 0x0041C430/0x0041C350/0x0041C3C0, Sqrt_Approx,
  Math::sin/cos and ftol run.
- The random-direction coordinate helper 0x0049F420 (no cell snap), as
  BulletClass::ResolveImpactCoordAndDetonate calls it for each cluster at
  0x00469067, at the cluster distances 0x100..0x200.

Supplied at entry and returned without running: Random__RandomRanged
0x0065C7E0 (the case's draw results, arguments recorded), Random 0x0065C780
(the case's raw word), TechnoClass::GetWeaponRange (vt+0x168; the case's
range, argument recorded), the target's and the cell's GetCoords (vt+0x48;
the case's coordinate). Execution stops on the child bullet's launch call
(vt+0x1F0), whose velocity argument is the observed result.

Rust consumers: src/sim/projectile/launch.rs (fireat_launch_scatter,
shrapnel_launch_velocity) and src/sim/combat/inviso_scatter.rs
(random_direction_coord_for_byte).
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.native_oracle import (load_image, run_checked, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, NATIVE_FPCW, finish_vectors, provenance)

RULES_PTR, SCENARIO_PTR = 0x8871E0, 0xA8B230
RANDOM_RANGED, RANDOM = 0x65C7E0, 0x65C780
RULES, SCENARIO = SCRATCH + 0x1000, SCRATCH + 0x3000
TECHNO, TECHNO_VT = SCRATCH + 0x4000, SCRATCH + 0x4800
BULLET_TYPE, FRAME = SCRATCH + 0x5000, SCRATCH + 0x5800
PARENT, TARGET, TARGET_VT = SCRATCH + 0x6000, SCRATCH + 0x6400, SCRATCH + 0x6800
CHILD, CHILD_VT, WEAPON = SCRATCH + 0x7000, SCRATCH + 0x7400, SCRATCH + 0x7800
STUB = SCRATCH + 0x9000                     # one address per supplied call
RANGE_STUB, COORDS_STUB = STUB, STUB + 0x10
SP = STACK_BASE + STACK_SIZE - 0x4000


def u32(value):
    return struct.pack("<I", value & 0xFFFFFFFF)


def read32(uc, address):
    return struct.unpack("<i", uc.mem_read(address, 4))[0]


def machine():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.mem_write(RULES_PTR, u32(RULES))
    uc.mem_write(SCENARIO_PTR, u32(SCENARIO))
    return uc


def returns(uc, value, stack_bytes):
    """Return from the intercepted call as a callee-cleanup function would."""
    sp = uc.reg_read(UC_X86_REG_ESP)
    uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EIP, read32(uc, sp) & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, sp + 4 + stack_bytes)


def scatter(case):
    """FireAt 0x006FE663..0x006FE8EE with the delta at [ESP+0x30]."""
    uc = machine()
    x, y, z = case["delta"]
    uc.mem_write(RULES + 0x1734, u32(case["scatter"]))
    uc.mem_write(TECHNO, u32(TECHNO_VT))
    uc.mem_write(TECHNO_VT + 0x168, u32(RANGE_STUB))
    uc.mem_write(FRAME + 0xC, u32(case["weapon"]))
    for offset, flag in ((0x2A2, 1), (0x29B, 1), (0x2A3, case["flak_scatter"]),
                         (0x29E, case["inviso"])):
        uc.mem_write(BULLET_TYPE + offset, bytes([flag]))
    for offset, value in ((0x30, x), (0x34, y), (0x38, z), (0x94, x), (0x98, y), (0x9C, z)):
        uc.mem_write(SP + offset, u32(value))
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ECX, BULLET_TYPE),
                            (UC_X86_REG_EAX, y), (UC_X86_REG_EDI, z),
                            (UC_X86_REG_ESI, TECHNO), (UC_X86_REG_EBP, FRAME)):
        uc.reg_write(register, value & 0xFFFFFFFF)
    draws = list(case["draws"])
    calls = []

    def hook(_uc, address, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == RANDOM_RANGED:
            calls.append(["random_ranged", read32(uc, sp + 4), read32(uc, sp + 8)])
            returns(uc, draws.pop(0), 8)
        elif address == RANGE_STUB:
            calls.append(["weapon_range", read32(uc, sp + 4)])
            returns(uc, case["range"], 4)

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x6FE663, 0x6FE8EE, count=200_000,
                required_addresses=[0x4CACB0, 0x4CAD00])
    assert not draws, "every supplied draw is consumed"
    assert uc.reg_read(UC_X86_REG_ESP) == SP
    return dict(input=case, calls=calls,
                result=[read32(uc, SP + 0x94), read32(uc, SP + 0x98), read32(uc, SP + 0x9C)])


def shrapnel(case):
    """SpawnShrapnel's child velocity, up to the child's launch call."""
    uc = machine()
    uc.mem_write(PARENT + 0x9C, struct.pack("<iii", *case["origin"]))
    uc.mem_write(WEAPON + 0xA8, u32(case["speed"]))
    uc.mem_write(TARGET, u32(TARGET_VT))
    uc.mem_write(TARGET_VT + 0x48, u32(COORDS_STUB))
    uc.mem_write(CHILD, u32(CHILD_VT))
    random_cell = case["random_cell"]
    uc.mem_write(SP + (0x44 if random_cell else 0x4C), u32(CHILD))
    registers = [(UC_X86_REG_ESP, SP), (UC_X86_REG_EDI, PARENT), (UC_X86_REG_ESI, WEAPON)]
    # The object branch reads its target from EBP; the cell branch from the
    # MapClass cell lookup's result in EAX.
    registers.append((UC_X86_REG_EAX, TARGET) if random_cell else (UC_X86_REG_EBP, TARGET))
    for register, value in registers:
        uc.reg_write(register, value)

    def hook(_uc, address, _size, _data):
        if address == COORDS_STUB:
            out = read32(uc, uc.reg_read(UC_X86_REG_ESP) + 4) & 0xFFFFFFFF
            uc.mem_write(out, struct.pack("<iii", *case["target"]))
            returns(uc, out, 4)

    uc.hook_add(UC_HOOK_CODE, hook)
    # Stop on the child's launch call (vt+0x1F0), its two arguments pushed.
    begin, launch = (0x46AA66, 0x46AD29) if random_cell else (0x46A5B2, 0x46A875)
    run_checked(uc, begin, launch, count=200_000, required_addresses=[0x4CAE30, 0x4CAC40])
    sp = uc.reg_read(UC_X86_REG_ESP)
    coords, velocity = read32(uc, sp) & 0xFFFFFFFF, read32(uc, sp + 4) & 0xFFFFFFFF
    return dict(input=case, coords=list(struct.unpack("<iii", uc.mem_read(coords, 12))),
                bits=[f"{bits:016x}" for bits in struct.unpack("<QQQ", uc.mem_read(velocity, 24))])


def direction(case):
    """0x0049F420(out, in, distance, snap=0) with one supplied raw draw."""
    uc = machine()
    source, result = SCRATCH + 0xA000, SCRATCH + 0xA100
    uc.mem_write(source, struct.pack("<iii", *case["base"]))
    ret = SCRATCH + 0xB000
    uc.mem_write(SP, u32(ret) + u32(case["distance"]) + u32(0))
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ECX, result),
                            (UC_X86_REG_EDX, source)):
        uc.reg_write(register, value)

    def hook(_uc, address, _size, _data):
        if address == RANDOM:
            returns(uc, case["raw"], 0)

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x49F420, ret, count=100_000, required_addresses=[0x4CACB0, 0x4CAD00])
    return dict(input=case, result=list(struct.unpack("<iii", uc.mem_read(result, 12))))


def scatter_cases():
    rows = []
    deltas = [(1280, 0, 0), (1024, 0, 77), (-900, 600, -300), (37, -2211, 512),
              (0, 0, 0), (3000, 3000, 0), (-1, 1, 1), (255, -256, 104)]
    raws = [0, 1, 0x3FFFFFFF, 0x7FFFFFFE, 0x12345678, 0x6D0A2C11, 0x00010000]
    for index, delta in enumerate(deltas):
        for raw in raws[index % 3::3] + [raws[(index * 5) % len(raws)]]:
            for flak, inviso in ((1, 0), (0, 0), (1, 1)):
                scatter_value = [256, 0, 384][index % 3]
                # The flak arm's draws are RandomRanged(0, scatter) then the raw
                # angle; the plain arm's RandomRanged(scatter/2, scatter).
                low = 0 if flak and not inviso else scatter_value // 2
                roll = [low, scatter_value, (low + scatter_value) // 2][raw % 3]
                rows.append(dict(delta=list(delta), scatter=scatter_value, flak_scatter=flak,
                                 inviso=inviso, weapon=index % 2, range=[1280, 1536, 2560][index % 3],
                                 draws=[roll, raw]))
    # Boundaries of the draw-to-word chain the Rust constants must reproduce.
    for raw in (0x55555554, 0x55555555, 0x2AAAAAAA, 0x7FFFFFFD, 0x40000000, 0x3FFFFFFE):
        rows.append(dict(delta=[512, -512, 0], scatter=256, flak_scatter=0, inviso=0,
                         weapon=0, range=1280, draws=[256, raw]))
    return rows


def shrapnel_cases():
    rows = []
    origin = [20 * 256 + 128, 30 * 256 + 128, 0]
    offsets = [(512, 0, 0), (0, 512, 0), (-384, 256, 0), (300, -700, 208), (1, 1, 0),
               (0, 0, 0), (-1536, -1536, -104), (2048, 5, 0), (-3, 1200, 416)]
    for random_cell in (False, True):
        for speed in (10, 40, 0):
            for dx, dy, dz in offsets:
                rows.append(dict(random_cell=random_cell, speed=speed, origin=origin,
                                 target=[origin[0] + dx, origin[1] + dy, origin[2] + dz]))
    return rows


def direction_cases():
    rows = []
    for distance in (0x100, 0x155, 0x200):
        for byte in range(256):
            rows.append(dict(base=[40 * 256 + 64, 50 * 256 + 200, 104], distance=distance,
                             raw=byte | (0x5A5A00 if byte & 1 else 0x7F00)))
    # Near the map's edges: a coordinate leaving the 512-cell square falls back.
    for base in ([100, 100, 0], [511 * 256 + 200, 300, 0], [300, 511 * 256 + 250, 0]):
        for byte in (0, 32, 64, 96, 128, 160, 192, 224):
            rows.append(dict(base=base, distance=0x200, raw=byte))
    return rows


def generate():
    return dict(scatter=[scatter(row) for row in scatter_cases()],
                shrapnel=[shrapnel(row) for row in shrapnel_cases()],
                direction=[direction(row) for row in direction_cases()])


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="FireAt launch scatter 0x006FE663..0x006FE8EE (both arms), SpawnShrapnel child "
              "velocity (object branch 0x0046A5B2..0x0046A875, cell branch 0x0046AA66..0x0046AD29) "
              "and the random-direction helper 0x0049F420 at cluster distances",
        assumptions=["x87 control word 0x0E7F (53-bit, chop) as WinMain sets it",
                     "the retail sine, atan and Sqrt_Approx tables are the image's initialised data"],
        substitutions=["Random__RandomRanged 0x0065C7E0 and Random 0x0065C780 return the case's "
                       "values; their arguments are recorded",
                       "GetWeaponRange (vt+0x168) returns the case's range",
                       "the target's/cell's GetCoords (vt+0x48) returns the case's coordinate",
                       "the child's launch call (vt+0x1F0) is observed, not run"],
        entry_points=dict(scatter=0x6FE663, shrapnel_object=0x46A5B2,
                          shrapnel_cell=0x46AA66, direction=0x49F420)))
