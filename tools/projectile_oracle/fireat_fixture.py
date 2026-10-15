"""Shared synthetic FireAt-tail/BulletFire fixture, not a complete game launch.

The five corpus producers own their case matrices. This module owns their common
memory layout, native execution, supplied leaves, and observed row schemas.
Importing it does not locate an executable, run native code, or write references.
"""

import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_FPCW, OracleError, load_image, provenance, run_checked,
)

BASE = 0x20000000
SOURCE, STYPE, WEAPON, PTYPE, BULLET, TARGET, RULES, VT, REF, HOOK = (
    BASE + index * 0x2000 for index in range(10)
)
SP = BASE + 0x40000
ENTRY = 0x6FE8EE
LAUNCHED = 0x6FF01A
FAILED = 0x6FF93C
VARIANTS = frozenset(("ordinary", "directed", "radial", "building", "voxel", "second-probe"))


def w32(machine, address, value):
    machine.mem_write(address, struct.pack("<I", value & 0xFFFFFFFF))


def r32(machine, address):
    return struct.unpack("<I", machine.mem_read(address, 4))[0]


def run(dx, dy, dz, speed=100, arcing=True, lobber=False, floater=False, *,
        variant="ordinary", dropping=False, rot=-1, turret=False, hull=0,
        barrel=0, source_z=0, building_height=2, voxel=True, vertical=False,
        seed=0, stored_weapon_speed=None, radial_segments=0, radial_index=0):
    """Execute one fresh case and retain that producer's existing JSON schema."""
    if variant not in VARIANTS:
        raise ValueError(f"Unknown FireAt fixture variant: {variant}")
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(BASE, 0x50000)
    for register, value in (
        (UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, SP + 0x1000),
        (UC_X86_REG_EBX, WEAPON), (UC_X86_REG_ECX, PTYPE),
        (UC_X86_REG_ESI, SOURCE), (UC_X86_REG_EDI, dz & 0xFFFFFFFF),
        (UC_X86_REG_EAX, dy & 0xFFFFFFFF), (UC_X86_REG_FPCW, NATIVE_FPCW),
    ):
        u.reg_write(register, value)
    for address, value in (
        (SOURCE, VT), (SOURCE + 0x2B4, TARGET), (VT + 0x84, HOOK),
        (VT + 0x3F8, HOOK + 16), (VT + 0x48, 0x5F65A0),
        (TARGET, VT), (REF, WEAPON), (WEAPON + 0xA0, PTYPE),
        (WEAPON + 0xA8, speed if stored_weapon_speed is None else stored_weapon_speed),
        (BULLET + 0xAC, PTYPE),
        (0x8871E0, RULES), (RULES + 0x16B8, 6), (SP + 0x28, speed),
        (SP + 0x3C, BULLET), (SP + 0x40, WEAPON), (SP + 0x68, PTYPE),
        (SP + 0x94, dx), (SP + 0x98, dy), (SP + 0x9C, dz),
        (SP + 0x1000 + 12, 0), (VT + 0x2C, HOOK + 32),
        (VT + 0x58, 0x5F65A0), (BULLET, 0x7E46E4),
        (BULLET + 0x10C, TARGET),
    ):
        w32(u, address, value)

    z = source_z if variant == "building" else 0
    source = [1280, 1280, z]
    launch_origin = [1390, 1320, 80] if variant == "directed" else source
    u.mem_write(SOURCE + 0x9C, struct.pack("<iii", *source))
    u.mem_write(TARGET + 0x9C, struct.pack("<iii", 1280 + dx, 1280 + dy, z + dz))
    u.mem_write(SP + 0x44, struct.pack("<iii", *launch_origin))
    u.mem_write(PTYPE + 0x29B, bytes([arcing]))
    u.mem_write(PTYPE + 0x295, bytes([floater]))
    u.mem_write(WEAPON + 0x12E, bytes([lobber]))

    # These are supplied virtual results, not arithmetic replacements.
    virtual_returns = {HOOK: (STYPE, 4), HOOK + 16: (REF, 8), HOOK + 32: (1, 4)}
    if variant in ("directed", "radial"):
        for address, value in (
            (VT + 0x308, 0x740F80), (VT + 0x2A8, 0x746E30),
            (SOURCE + 0x6C4, STYPE), (SOURCE + 0x388, hull),
            (SOURCE + 0x3A0, barrel), (PTYPE + 0x2DC, rot),
        ):
            w32(u, address, value)
        u.mem_write(PTYPE + 0x29C, bytes([dropping]))
        u.mem_write(STYPE + 0xCA1, bytes([turret]))
        w32(u, STYPE + 0x6A4, radial_segments)
        w32(u, SOURCE + 0x43C, radial_index)
        u.mem_write(PTYPE + 0x2C0, bytes([vertical]))
    elif variant == "building":
        for address, value in (
            (VT + 0xAC, 0x41BE00), (VT + 0x300, 0x6F3D60),
            (VT + 0x2A8, HOOK + 48), (TARGET + 0x520, STYPE),
            (STYPE + 0xEF4, building_height),
        ):
            w32(u, address, value)
        virtual_returns[HOOK + 32] = (6, 4)
        virtual_returns[HOOK + 48] = (SOURCE + 0x388, 8)
    elif variant == "voxel":
        w32(u, SP + 0x1000 + 8, TARGET)
        u.mem_write(PTYPE + 0x236, bytes([voxel]))
        u.mem_write(PTYPE + 0x2C0, bytes([vertical]))
    elif variant == "second-probe":
        w32(u, SP - 32, seed)
        w32(u, SP - 28, seed)

    seen = []
    second = []
    headings = []
    scalars = []
    rng_addresses = (0x886B88, BASE + 0x30000)
    if variant == "radial":
        from tools.rmg_oracle.gen_rng_vectors import seeded_struct
        w32(u, 0xA8B230, rng_addresses[1] - 0x218)
        for address in rng_addresses:
            u.mem_write(address, seeded_struct(31))
        rng_before = [hashlib.sha256(bytes(u.mem_read(a, 0x3F4))).hexdigest()
                      for a in rng_addresses]

    def return_from_leaf(value, stack_bytes):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, r32(u, sp))
        u.reg_write(UC_X86_REG_ESP, sp + stack_bytes)

    def observe(_machine, address, _size, _data):
        if address in virtual_returns:
            return_from_leaf(*virtual_returns[address])
        elif address in (0x5F4EC0, 0x4A9770, 0x4A9720):
            if address == 0x5F4EC0:
                sp = u.reg_read(UC_X86_REG_ESP)
                u.mem_write(BULLET + 0x9C, bytes(u.mem_read(r32(u, sp + 4), 12)))
                u.mem_write(BULLET + 0x90, b"\x01")
            return_from_leaf(1, 12 if address == 0x5F4EC0 else 8)
        elif address in (0x70D590, 0x48A8D0, 0x48A9D0, 0x4CB3D0):
            seen.append(hex(address))
        elif variant == "radial" and address == 0x6FEB1B:
            headings.append(u.reg_read(UC_X86_REG_EAX) & 0xFFFF)
        elif variant == "radial" and address == 0x6FEA52:
            scalars.append(struct.unpack("<i", u.mem_read(SP + 0x28, 4))[0])
        elif variant == "radial" and address in (0x65C780, 0x65C7E0):
            raise OracleError("Radial launch unexpectedly consumed RNG")
        elif variant == "second-probe" and address == 0x48A954:
            sp = u.reg_read(UC_X86_REG_ESP)
            second.append(dict(
                al=u.reg_read(UC_X86_REG_EAX) & 255,
                output=bytes(u.mem_read(sp + 0x10, 8)).hex(),
                first=bytes(u.mem_read(sp + 0x24, 8)).hex(),
            ))

    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        endpoint = run_checked(u, ENTRY, (LAUNCHED, FAILED), count=200000)
    finally:
        u.hook_del(hook)
    raw = bytes(u.mem_read(SP + 0x50, 24))
    if variant != "radial" and endpoint == LAUNCHED and raw != bytes(u.mem_read(BULLET + 0xE8, 24)):
        raise OracleError(f"{variant} FireAt/BulletFire persistent velocity bits differ")

    row = dict(delta=[dx, dy, dz], speed=speed)
    if variant in ("ordinary", "directed", "radial", "building"):
        row.update(arcing=arcing, lobber=lobber, floater=floater)
    if variant in ("directed", "radial"):
        origin = list(struct.unpack("<iii", u.mem_read(BULLET + 0x9C, 12)))
        row.update(dropping=dropping, rot=rot, turret=turret, hull=hull,
                   barrel=barrel, origin=origin)
        if variant == "radial":
            row.update(radial_segments=radial_segments, radial_index=radial_index,
                       radial_next=struct.unpack("<i", u.mem_read(SOURCE + 0x43C, 4))[0],
                       vertical=vertical, headings=headings,
                       launch_scalar=scalars[0],
                       fired_bits=[f"{v:016x}" for v in struct.unpack("<QQQ", u.mem_read(BULLET + 0xE8, 24))],
                       rng_before=rng_before,
                       rng_after=[hashlib.sha256(bytes(u.mem_read(a, 0x3F4))).hexdigest()
                                  for a in rng_addresses])
    elif variant == "building":
        row.update(source_z=source_z, building_height=building_height)
    elif variant == "voxel":
        row.update(voxel=voxel, vertical=vertical)
    elif variant == "second-probe":
        row.update(seed=seed)
    row.update(
        velocity=list(struct.unpack("<ddd", raw)),
        bits=[f"{value:016x}" for value in struct.unpack("<QQQ", raw)],
        pitch=r32(u, SP + 0x80) & 65535,
    )
    if variant == "voxel":
        row.update(max_speed=r32(u, BULLET + 0x110))
    row.update(success=u.mem_read(SP + 0x27, 1)[0])
    if variant == "second-probe":
        row.update(second=second)
    else:
        row.update(calls=seen)
    if variant == "directed" and row["success"]:
        expected_origin = source if dropping else launch_origin
        if row["origin"] != expected_origin:
            raise OracleError(f"Directed launch origin {row['origin']} differs from {expected_origin}")
    return row


def launch_provenance(*, scope, assumptions, substitutions=(), entry_points=None):
    """Common evidence boundary plus each producer's variant-specific facts."""
    return provenance(
        scope=scope,
        assumptions=[
            "Each case starts a fresh emulator with the pinned retail executable. Synthetic source, target, type, weapon, Bullet, Rules, virtual table and stack state supply the FireAt tail at 0x006FE8EE; constructors, full FireAt admission, aim selection and retail INI loading are not executed.",
            "Source X/Y are 1280/1280; Rules gravity is 6. The producer supplies delta, speed and selected type flags. Unwritten mapped fixture memory and native BSS remain zero; this is not a Windows process initialization claim.",
            "Ambient x87 control word is 0x0E7F: 53-bit precision, truncation, masked exceptions. Native velocity bytes are observed as binary64 including signed zero; this corpus does not establish physical x87 hardware equivalence.",
            "Execution must reach 0x006FF01A or the failed-launch boundary 0x006FF93C within 200000 instructions and the checked runner's time limit. At the normal boundary the FireAt stack vector must equal persistent Bullet velocity bytes. Failed-launch vector/pitch bytes are observations, not valid launched velocities.",
            *assumptions,
        ],
        substitutions=[
            "Virtual GetType/GetWeapon and the selected class gate return supplied type/weapon references and a fixture class value. Native source location getter 0x005F65A0 remains executable.",
            "World SetLocation 0x005F4EC0 copies the requested coordinates to Bullet+0x9C, sets Bullet+0x90 to 1 and returns success. Display Remove 0x004A9770 and Submit 0x004A9720 return success. World admission, registration, map state and those leaf bodies are excluded.",
            *substitutions,
        ],
        entry_points={
            "fireat_tail_begin": ENTRY, "fireat_launched_boundary": LAUNCHED,
            "fireat_failed_boundary": FAILED, "bullet_fire": 0x468670,
            **(entry_points or {}),
        },
    )
