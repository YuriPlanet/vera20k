"""Original AnimClass::Middle 0x00424F00 with the smudge placers it calls.

Executes Middle, and through it the real scorch/crater placers
`0x006B59A0` (Burn types, +0x2A1) and `0x006B5C90` (Crater types, +0x2A0) with
their DynamicVectorClass candidate lists, the Scenario `RandomRanged`
(0x0065C7E0, seeded through 0x0065C6D0) and the x87 coin flip, and records:
- the particle spawns (`0x0062E430`, ParticleType index and coordinate);
- the Reduce_Tiberium call (`0x00480A80`, amount);
- every SmudgeClass constructed (`0x006B4A50`, SmudgeType index and cell);
- the Scenario RNG before and after.

Middle reads the anim's coordinate through vt+0x48, its image through
vt+0x6C, its height through vt+0x1C8 (30 or more skips the marks), and its
AnimType (+0xC8): the middle frame (+0x298), that frame's cached width and
height (+0x29C/+0x2A0, or -1 to fetch them through `0x0069E7E0`), SpawnsParticle
(+0x2CC) and NumParticles (+0x2D0), Scorch (+0x36B), Crater (+0x36D) and
ForceBigCraters (+0x36E).

Supplied: the anim vt+0x48/+0x6C/+0x1C8 as stubs; MapClass::GetCell 0x005657A0
returning a fixed cell; the frame-size read 0x0069E7E0; SmudgeTypeClass
CanPlace 0x006B5F80 answering the case's admit mask (`smudge_can_place` executes
CanPlace itself over MapClass's cell table); the SmudgeType table
(0x00A8EC1C/0x00A8EC28: Burn +0x2A1, Crater +0x2A0, Width +0x298,
Height +0x29C); operator new 0x007C8E17 (a bump allocator) and delete
0x007C8B3D; the SmudgeClass constructor 0x006B4A50 and the particle
constructor 0x0062E430 as recorders.

Rust consumer: src/sim/anim_class.rs (the anim runtime's Middle) and
src/sim/combat/smudge_dispatch.rs (the placer).
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.native_oracle import (RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, NATIVE_FPCW,
                                 finish_vectors, load_image, provenance, run_checked)

MIDDLE, SEED = 0x424F00, 0x65C6D0
GET_CELL, FRAME_SIZE, REDUCE, PARTICLE = 0x5657A0, 0x69E7E0, 0x480A80, 0x62E430
CAN_PLACE, NEW, DELETE, SMUDGE_CTOR = 0x6B5F80, 0x7C8E17, 0x7C8B3D, 0x6B4A50
SMUDGE_ITEMS, SMUDGE_COUNT, SCENARIO_PTR = 0xA8EC1C, 0xA8EC28, 0xA8B230
ANIM, ANIM_VT, TYPE, CELL, SCENARIO, SMUDGES, IMAGE, PARTICLES = (
    SCRATCH + n * 0x1000 for n in range(8))
PARTICLE_ITEMS = 0xA83D9C
STUB = SCRATCH + 0x9000
STUBS = {name: STUB + 0x10 * index for index, name in enumerate(("coords", "image", "height"))}
HEAP = 0x40000000
SP = STACK_BASE + STACK_SIZE - 0x1000
# A small SmudgeType table: (burn, crater, width, height).
TABLE = [(1, 0, 1, 1), (1, 0, 2, 2), (0, 1, 1, 1), (0, 1, 2, 2), (1, 0, 1, 1), (0, 1, 1, 1),
         (0, 1, 3, 2), (1, 0, 2, 1)]


def dwords(*values):
    return struct.pack("<" + "I" * len(values), *[v & 0xFFFFFFFF for v in values])


def read32(uc, address):
    return struct.unpack("<I", uc.mem_read(address, 4))[0]


def execute(case):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, 0x10000)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.mem_map(HEAP, 0x100000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    events = []

    uc.mem_write(SCENARIO_PTR, dwords(SCENARIO))
    uc.mem_write(SP, dwords(RET_MAGIC, case["seed"]))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, SCENARIO + 0x218)
    run_checked(uc, SEED, RET_MAGIC, count=100_000)
    before = bytes(uc.mem_read(SCENARIO + 0x218, 0x3F4)).hex()

    uc.mem_write(ANIM, dwords(ANIM_VT))
    for slot, name in ((0x48, "coords"), (0x6C, "image"), (0x1C8, "height")):
        uc.mem_write(ANIM_VT + slot, dwords(STUBS[name]))
    uc.mem_write(ANIM + 0x9C, struct.pack("<iii", *case["location"]))
    uc.mem_write(ANIM + 0xC8, dwords(TYPE))
    uc.mem_write(TYPE + 0x298, dwords(case["middle_frame"]))
    uc.mem_write(TYPE + 0x29C, dwords(case["cached_size"][0]))
    uc.mem_write(TYPE + 0x2A0, dwords(case["cached_size"][1]))
    uc.mem_write(TYPE + 0x2CC, dwords(case["particle"]))
    uc.mem_write(TYPE + 0x2D0, dwords(case["particle_count"]))
    uc.mem_write(TYPE + 0x36B, bytes([case["scorch"]]))
    uc.mem_write(TYPE + 0x36D, bytes([case["crater"]]))
    uc.mem_write(TYPE + 0x36E, bytes([case["force_big"]]))
    # ParticleTypeClass::Array items: entry i reads back as 0x1000 + i.
    uc.mem_write(PARTICLE_ITEMS, dwords(PARTICLES))
    uc.mem_write(PARTICLES, dwords(*[0x1000 + i for i in range(8)]))
    table = case["table"]
    uc.mem_write(SMUDGE_ITEMS, dwords(SMUDGES))
    uc.mem_write(SMUDGE_COUNT, dwords(len(table)))
    for index, (burn, crater, width, height) in enumerate(table):
        smudge = SMUDGES + 0x100 + index * 0x400
        uc.mem_write(SMUDGES + index * 4, dwords(smudge))
        uc.mem_write(smudge + 0x298, dwords(width, height))
        uc.mem_write(smudge + 0x2A0, bytes([crater, burn]))
        uc.mem_write(smudge + 0x3F0, dwords(index))
    heap = [HEAP]

    def ret(value, pops):
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, read32(uc, sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def hook(_uc, address, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == STUBS["coords"]:
            out = read32(uc, sp + 4)
            uc.mem_write(out, struct.pack("<iii", *case["coord"]))
            ret(out, 4)
        elif address == STUBS["image"]:
            ret(IMAGE if case["image"] else 0, 0)
        elif address == STUBS["height"]:
            ret(case["height"], 0)
        elif address == GET_CELL:
            events.append(dict(call="get_cell",
                               cell=list(struct.unpack("<hh", uc.mem_read(read32(uc, sp + 4), 4)))))
            ret(CELL, 4)
        elif address == FRAME_SIZE:
            out, frame = read32(uc, sp + 4), read32(uc, sp + 8)
            events.append(dict(call="frame_size", frame=frame))
            uc.mem_write(out, struct.pack("<iiii", 0, 0, *case["frame_size"]))
            ret(out, 8)
        elif address == REDUCE:
            events.append(dict(call="reduce", amount=read32(uc, sp + 4)))
            ret(0, 4)
        elif address == PARTICLE:
            coord = list(struct.unpack("<iii", uc.mem_read(read32(uc, sp + 8), 12)))
            events.append(dict(call="particle", type=read32(uc, sp + 4) - 0x1000, coord=coord))
            ret(0, 8)
        elif address == CAN_PLACE:
            index = read32(uc, uc.reg_read(UC_X86_REG_ECX) + 0x3F0)
            events.append(dict(call="can_place", type=index, force=read32(uc, sp + 8) & 0xFF))
            ret(1 if (case["admit_mask"] >> index) & 1 else 0, 8)
        elif address == NEW:
            size = read32(uc, sp + 4)
            address_out = heap[0]
            heap[0] += (size + 15) & ~15
            ret(address_out, 0)  # cdecl: the caller pops
        elif address == DELETE:
            ret(0, 0)
        elif address == SMUDGE_CTOR:
            smudge = read32(uc, sp + 4)
            coord = list(struct.unpack("<iii", uc.mem_read(read32(uc, sp + 8), 12)))
            events.append(dict(call="smudge", type=read32(uc, smudge + 0x3F0), coord=coord,
                               house=struct.unpack("<i", uc.mem_read(sp + 12, 4))[0]))
            ret(uc.reg_read(UC_X86_REG_ECX), 12)

    uc.hook_add(UC_HOOK_CODE, hook)
    uc.mem_write(SP, dwords(RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, ANIM)
    run_checked(uc, MIDDLE, RET_MAGIC, count=2_000_000)
    assert uc.reg_read(UC_X86_REG_ESP) == SP + 4, "Middle takes no stack arguments"
    return dict(input=case, events=events, rng_before=before,
                rng_after=bytes(uc.mem_read(SCENARIO + 0x218, 0x3F4)).hex())


BASE = dict(location=[2688, 2688, 0], coord=[2688, 2688, 0], middle_frame=6,
            cached_size=[35, 34], frame_size=[35, 34], particle=-1, particle_count=0,
            scorch=0, crater=1, force_big=0, image=1, height=0, table=TABLE,
            admit_mask=0xFF, seed=1)


def cases():
    flags = [(0, 1, 0), (1, 0, 0), (1, 1, 0), (0, 1, 1), (1, 1, 1), (0, 0, 0)]
    sizes = [[35, 34], [129, 119], [61, 51], [60, 51], [61, 50], [22, 19]]
    for scorch, crater, force_big in flags:
        for size in sizes:
            for seed in (1, 31, 0x5CA1AB1E, 0xFFFFFFFF, 7, 0x12345678):
                yield dict(BASE, scorch=scorch, crater=crater, force_big=force_big,
                           cached_size=size, seed=seed)
    for height in (29, 30, -1, 31, 100):
        yield dict(BASE, scorch=1, crater=1, height=height)
    for image in (0, 1):
        for cached in ([-1, -1], [35, -1], [-1, 34]):
            yield dict(BASE, image=image, cached_size=cached, frame_size=[127, 124])
    for mask in (0x00, 0x01, 0x04, 0x08, 0x0C, 0x44, 0xF0):
        for scorch, crater in ((1, 0), (0, 1), (1, 1)):
            for seed in (1, 31, 7):
                yield dict(BASE, scorch=scorch, crater=crater, admit_mask=mask,
                           cached_size=[129, 119], seed=seed)
    for particle, count in ((3, 1), (3, 4), (5, 0)):
        yield dict(BASE, particle=particle, particle_count=count)
    for coord in ([0, 0, 0], [255, 255, 0], [256, 0, 0], [-1, 300, 0], [130, 260, 5]):
        yield dict(BASE, coord=coord, scorch=1, crater=0)


def generate():
    return [execute(case) for case in cases()]


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("AnimClass::Middle 0x00424F00 with the real scorch and crater placers "
               "0x006B59A0/0x006B5C90 over Scorch/Crater/ForceBigCraters, middle-frame sizes "
               "around the 60x50 large-mark test, the 30-lepton height gate, cached and "
               "fetched frame sizes, image presence, CanPlace admit masks, particle spawns, "
               "cell (0,0) and Scenario seeds."),
        assumptions=["x87 control word 0x0E7F (PC53, chop), the harness default.",
                     "The SmudgeType table and the anim's AnimType fields are supplied."],
        substitutions=[
            "anim vt+0x48 GetCoords, vt+0x6C Get_Image and vt+0x1C8 GetHeight are stubs",
            "MapClass::GetCell 0x005657A0 returns a fixed cell; 0x0069E7E0 returns the case's "
            "frame size",
            "SmudgeTypeClass CanPlace 0x006B5F80 answers the case's admit mask",
            "operator new 0x007C8E17 is a bump allocator; delete 0x007C8B3D is a no-op",
            "the SmudgeClass constructor 0x006B4A50, Reduce_Tiberium 0x00480A80 and the "
            "particle constructor 0x0062E430 record their arguments; the ParticleType array "
            "(0x00A83D9C) names entry i as 0x1000 + i",
        ],
        entry_points={"middle": MIDDLE, "scorch": 0x6B59A0, "crater": 0x6B5C90},
    ))
