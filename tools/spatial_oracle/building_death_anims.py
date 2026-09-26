"""Original building death anims: the ReceiveDamage debris block, then
BuildingClass::DestructionEffects steps 7 and 8, on one Scenario stream.

Executes, on a BuildingClass with the original vtable 0x007E3EBC (vt+0x48
0x00447AC0: Location + (Width * 128 - 0x80, Height * 128 - 0x80, 0); vt+0x84
0x006F3270 -> vt+0x88 0x00459EE0; vt+0xAC 0x00459EF0: Location - (0x80, 0x80, 0)),
its Location (+0x9C) and a BuildingType (+0x520) holding the fields the path
reads: the Foundation index (+0xEF0, read by Width 0x0045EC90 and Height
0x0045ECA0 through the original tables 0x008192B8/0x00819310), MaxDebris
(+0x5BC), MinDebris (+0x5C0), DebrisTypes (+0x314, empty), DebrisAnims (+0x5C4)
and Explosion (+0x72C):

1. TechnoClass::ReceiveDamage 0x00702281..0x00702572: the MaxDebris gate, the
   piece count RandomRanged(MinDebris, MaxDebris - 1) at 0x007022C8, the voxel
   arm (skipped: no DebrisTypes) and the DebrisAnims arm: per piece new(0x1C8),
   vt+0x48 + (0, 0, 0x14), RandomRanged(0, n - 1) at 0x00702473, then the whole
   AnimClass constructor 0x00421EA0 (Bouncer arm, BounceClass::Init) and, for
   its zero delay, AnimClass::Start 0x00424CE0.
2. DestructionEffects 0x0044177E..0x00441A2B (the building still on the map):
   step 7, Width/Height, a RandomRanged(0, dimension - 2) per dimension over 2,
   the RandomRanged(0, 99) roll at 0x00441819 and the real placer 0x006B59A0
   (roll < 50, Burn types) or 0x006B5C90 (Crater types) with force 1 and size
   0x64 at the Location cell's centre; step 8 over the caller's fourth argument,
   the foundation cell list `vt+0x108(0)` returns (BuildingTypeClass vt+0x90
   0x0045EC20 reads +0xDFC, which 0x00461541 sets to 0x0089C900 + index * 0x78,
   the lists the static initializer 0x0045B1C0 builds; executed here): per cell
   vt+0xAC, 0x0049F420 with radius 0x40, new(0x1C8), RandomRanged(0, 3) at
   0x004419DC, Next at 0x004419FB and the unsigned modulo pick, then the
   constructor (and Start for a zero delay).

AnimClass::Start runs natively. It calls AnimClass::Middle 0x00424F00 only for
MiddleFrameIndex 0 (the AnimType constructor's 0 at 0x0042754A; loading an
image sets frames / 2 at 0x00427C6B), i.e. the art-less `gtpowexp`; Middle's
draw arms are gated by Scorch (+0x36B), Crater (+0x36D) and SpawnsParticle
(+0x2CC), unset for it. The Report sound 0x007509E0 returns at its audio gate
(0x008464AC clear); its variation picks use the NonCritical stream 0x00886B88.

Supplied: GAPOWR's retail values (rulesmd.ini [GAPOWR], artmd.ini Foundation=2x2
and the AnimTypes' Bouncer/RandomRate/Elasticity/MaxXYVel/MinZVel/Report/
Scorch/Crater), the [SmudgeTypes] table (Burn +0x2A1, Crater +0x2A0, Width
+0x298, Height +0x29C), the anim-constructor environment of
`anim_bouncer_launch.Machine`, SmudgeTypeClass CanPlace 0x006B5F80 answering the
case's admit mask, MapClass::GetCell 0x005657A0 and the floor height 0x00578080
(a fixed cell; height 0), operator delete 0x007C8B3D (no-op) and the SmudgeClass
constructor 0x006B4A50 (recorded).

Schema: `smudge_types` is the supplied [SmudgeTypes] table (index order);
`rows[]` hold `input`, `events` (ordered `ranged`/`next` draws with call
site and result, `can_place`, `smudge`, `anim_ctor` {type, coord, delay, loop,
flags}, `start`, `middle`, `unlimbo`, ...), `debris` (per debris piece: its constructor
event and `location`/`bounce` state), `raw_draw_count`, and the Scenario RNG
states `rng_before`, `rng_after_debris` and `rng_after` (0x3F4-byte hex).

Rust consumer: `retail_dustbowl_death_anims_use_the_types_lists` in
src/sim/combat/destruction_effects_tests.rs (the production receiver on the
retail Dustbowl map, reseeded at the kill).
"""
import struct
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle import anim_bouncer_launch as launch
from tools.spatial_oracle.anim_bouncer_launch import dwords, read32

DEBRIS_BEGIN, DEBRIS_END = 0x702281, 0x702572
EFFECTS_BEGIN, EFFECTS_END = 0x44177E, 0x441A2B
FOUNDATION_INIT, FOUNDATION_LISTS, FOUNDATION_STRIDE = 0x45B1C0, 0x89C900, 0x78
BUILDING_VT = 0x7E3EBC
CAN_PLACE, SMUDGE_CTOR, DELETE, CELL_AT, FLOOR = 0x6B5F80, 0x6B4A50, 0x7C8B3D, 0x5657A0, 0x578080
MIDDLE = 0x424F00
SMUDGE_ITEMS, SMUDGE_COUNT = 0xA8EC1C, 0xA8EC28

BUILDING = launch.MEM + 0x100000
BUILDING_TYPE = launch.MEM + 0x101000
VECTORS = launch.MEM + 0x104000
SMUDGES = launch.MEM + 0x110000

# rulesmd.ini [GAPOWR]; Foundation=2x2 from artmd.ini is table index 3.
GAPOWR = dict(foundation=3, max_debris=6, min_debris=4,
              debris_anims=["DBRIS1LG", "DBRIS1SM", "DBRIS4LG", "DBRIS4SM", "DBRIS5LG", "DBRIS5SM"],
              explosion=["TWLT070", "S_BANG48", "S_BRNL58", "S_CLSN58", "S_TUMU60", "gtpowexp"])
# artmd.ini: (has an art section and image, Report= set, Scorch=, Crater=). `gtpowexp`
# has no section: no image, no report, no marks.
EXPLOSION_ART = {
    "TWLT070": (True, True, 1, 1),
    "S_BANG48": (True, True, 1, 1),
    "S_BRNL58": (True, True, 1, 1),
    "S_CLSN58": (True, True, 0, 1),
    "S_TUMU60": (True, True, 1, 1),
    "gtpowexp": (False, False, 0, 0),
}
# rulesmd.ini [SmudgeTypes] in list order: (name, Burn, Crater, Width, Height).
SMUDGE_TYPES = ([(f"CR{i}", 0, 0, 1, 1) for i in range(1, 7)]
                + [(f"BURN{i:02}", 0, 0, 1, 1) for i in range(1, 17)]
                + [(f"BURNT{i:02}", 1, 0, 1, 1) for i in range(1, 7)]
                + [("BURNT07", 1, 0, 2, 1), ("BURNT08", 1, 0, 2, 1),
                   ("BURNT09", 1, 0, 1, 2), ("BURNT10", 1, 0, 1, 2),
                   ("BURNT11", 1, 0, 2, 2), ("BURNT12", 1, 0, 2, 2)]
                + [(f"CRATER{i:02}", 0, 1, 1, 1) for i in range(1, 11)]
                + [("CRATER11", 0, 1, 2, 2), ("CRATER12", 0, 1, 2, 2)])
# The image-bound AnimTypes' frame count in `Machine.anim_type` is 16.
MIDDLE_FRAME = 8


class Machine(launch.Machine):
    def __init__(self, seed, admit_mask):
        super().__init__(seed)
        self.admit_mask = admit_mask
        self.constructed = []

    def hook(self, uc, address, size, data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address in (launch.START, MIDDLE):
            # Recorded, then executed.
            self.events.append(dict(call="start" if address == launch.START else "middle",
                                    anim=uc.reg_read(UC_X86_REG_ECX) - launch.HEAP))
        elif address == CAN_PLACE:
            index = read32(uc, uc.reg_read(UC_X86_REG_ECX) + 0x3F0)
            self.events.append(dict(call="can_place", type=SMUDGE_TYPES[index][0],
                                    force=read32(uc, sp + 8) & 0xFF))
            self.ret(1 if (self.admit_mask >> index) & 1 else 0, 8)
        elif address == SMUDGE_CTOR:
            smudge = read32(uc, sp + 4)
            coord = list(struct.unpack("<iii", uc.mem_read(read32(uc, sp + 8), 12)))
            self.events.append(dict(call="smudge", type=SMUDGE_TYPES[read32(uc, smudge + 0x3F0)][0],
                                    coord=coord))
            self.ret(uc.reg_read(UC_X86_REG_ECX), 12)
        elif address == DELETE:
            self.ret(0, 0)
        elif address == CELL_AT:
            self.ret(launch.CELL, 4)
        elif address == FLOOR:
            self.ret(0, 4)
        else:
            if address == launch.CTOR:
                self.constructed.append(uc.reg_read(UC_X86_REG_ECX))
            super().hook(uc, address, size, data)

    def vector(self, base, pointers, at):
        self.uc.mem_write(at, dwords(*pointers))
        self.uc.mem_write(base + 4, dwords(at, len(pointers)))
        self.uc.mem_write(base + 0x10, dwords(len(pointers)))


def execute(case):
    machine = Machine(case["seed"], case["admit_mask"])
    uc = machine.uc
    uc.mem_write(launch.SP - 0x100, dwords(launch.STOP))
    uc.reg_write(UC_X86_REG_ESP, launch.SP - 0x100)
    run_checked(uc, FOUNDATION_INIT, launch.STOP, count=200_000)

    debris_types = []
    for name in case["debris_anims"]:
        pointer, _ = machine.anim_type(name, *launch.type_values(name))
        uc.mem_write(pointer + 0x298, dwords(MIDDLE_FRAME))
        uc.mem_write(pointer + 0x2CC, dwords(0xFFFFFFFF))
        debris_types.append(pointer)
    explosion_types = []
    for name in case["explosion"]:
        image, report, scorch, crater = EXPLOSION_ART[name]
        pointer, _ = machine.anim_type(name, 0.0, 0.0, 0.0, None)
        uc.mem_write(pointer + 0x35A, b"\x00")  # Bouncer=no
        uc.mem_write(pointer + 0x2CC, dwords(0xFFFFFFFF))
        uc.mem_write(pointer + 0x298, dwords(MIDDLE_FRAME if image else 0))
        if not image:
            uc.mem_write(pointer + 0x2BC, dwords(0, 0))
        if report:
            uc.mem_write(pointer + 0x2F8, dwords(0))
        uc.mem_write(pointer + 0x36B, bytes([scorch]))
        uc.mem_write(pointer + 0x36D, bytes([crater]))
        explosion_types.append(pointer)

    uc.mem_write(BUILDING_TYPE, bytes(0x2000))
    uc.mem_write(BUILDING_TYPE + 0xEF0, dwords(case["foundation"]))
    uc.mem_write(BUILDING_TYPE + 0x5BC, dwords(case["max_debris"], case["min_debris"]))
    machine.vector(BUILDING_TYPE + 0x5C4, debris_types, VECTORS)
    machine.vector(BUILDING_TYPE + 0x72C, explosion_types, VECTORS + 0x100)
    uc.mem_write(BUILDING, bytes(0x800))
    uc.mem_write(BUILDING, dwords(BUILDING_VT))
    uc.mem_write(BUILDING + 0x9C, struct.pack("<iii", *case["location"]))
    uc.mem_write(BUILDING + 0x520, dwords(BUILDING_TYPE))

    uc.mem_write(SMUDGE_ITEMS, dwords(SMUDGES))
    uc.mem_write(SMUDGE_COUNT, dwords(len(SMUDGE_TYPES)))
    for index, (_, burn, crater, width, height) in enumerate(SMUDGE_TYPES):
        smudge = SMUDGES + 0x100 + index * 0x400
        uc.mem_write(SMUDGES + index * 4, dwords(smudge))
        uc.mem_write(smudge + 0x298, dwords(width, height))
        uc.mem_write(smudge + 0x2A0, bytes([crater, burn]))
        uc.mem_write(smudge + 0x3F0, dwords(index))

    before = machine.rng()
    machine.events.clear()
    machine.advances = 0
    uc.reg_write(UC_X86_REG_ESP, launch.SP - 0x400)
    uc.reg_write(UC_X86_REG_ESI, BUILDING)
    uc.reg_write(UC_X86_REG_EBX, 0)
    run_checked(uc, DEBRIS_BEGIN, DEBRIS_END, count=20_000_000,
                required_addresses=(0x7022C8, 0x702473, 0x7024AA))
    after_debris = machine.rng()
    debris = [dict(ctor=event, **launch.anim_state(uc, anim))
              for event, anim in zip([e for e in machine.events if e["call"] == "anim_ctor"],
                                     machine.constructed)]

    frame = launch.SP - 0x800
    uc.mem_write(frame + 0x64, dwords(launch.STOP, 0, 0, 0,
                                      FOUNDATION_LISTS + case["foundation"] * FOUNDATION_STRIDE))
    uc.reg_write(UC_X86_REG_ESP, frame)
    uc.reg_write(UC_X86_REG_ESI, BUILDING)
    uc.reg_write(UC_X86_REG_EBX, 0)
    run_checked(uc, EFFECTS_BEGIN, EFFECTS_END, count=20_000_000,
                required_addresses=(0x441819, 0x4419DC, 0x441A1F))
    return dict(input=case, events=machine.events, debris=debris,
                raw_draw_count=machine.advances, rng_before=before,
                rng_after_debris=after_debris, rng_after=machine.rng())


# The retail Dustbowl fixture's power plant: origin cell (68, 40) at level 1. That cell
# carries ore (TIB01), and every candidate's footprint starts there, so CanPlace
# (OverlayTypeIndex +0x44 must be -1, 0x006B6002) admits none: the mask is 0, and the
# placer draws nothing (0x006B5BF0). On a clean Morphable origin the force-1 placer
# prefers types over one cell each way (0x006B5B55) and otherwise picks among every
# admitted one, 1x1 included (0x006B5C1A); `anim_middle` covers that pick for supplied
# masks, and no row here places a mark.
DUSTBOWL_LOCATION = [68 * 256 + 0x80, 40 * 256 + 0x80, 104]
# 22 and 49 construct a zero-delay `gtpowexp` (Start runs Middle) ahead of later cells.
SEEDS = [1, 7, 22, 31, 42, 49, 1000, 0x5CA1AB1E, 0xDEADBEEF, 2024]


def cases():
    for seed in SEEDS:
        yield dict(GAPOWR, location=DUSTBOWL_LOCATION, admit_mask=0, seed=seed)


def generate():
    return dict(smudge_types=[dict(name=name, burn=burn, crater=crater, width=width, height=height)
                              for name, burn, crater, width, height in SMUDGE_TYPES],
                rows=[execute(case) for case in cases()])


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("A retail GAPOWR's death anims: TechnoClass::ReceiveDamage's debris block "
               "0x00702281..0x00702572 (count draw, DebrisAnims arm, each piece's AnimClass "
               "constructor and Start) then BuildingClass::DestructionEffects steps 7 and 8 "
               "0x0044177E..0x00441A2B (the centre scorch/crater roll and real placer, the "
               "per-foundation-cell scatter, delay and Explosion= pick, constructor and Start), "
               "at the retail Dustbowl fixture's Location over ten Scenario seeds; its origin "
               "cell carries ore, so CanPlace's supplied answer admits no SmudgeType, step 7 "
               "draws only its roll and no row places a mark."),
        assumptions=[
            "x87 control word 0x0E7F (PC53, chop) on entry, as anim_bouncer_launch.",
            "No Scenario draw separates the debris block from step 7 for a GAPOWR (the death "
            "sounds use the NonCritical stream; steps 1-6 of DestructionEffects draw nothing "
            "for it); the two blocks run back to back on one stream.",
            "Steps 9 (Explodes=), 10 (stored ore), 13 (DestroyAnim=) draw nothing for GAPOWR "
            "and are not executed; SpawnSurvivors follows and is not executed.",
            "AnimType fields are supplied: retail artmd.ini Bouncer=, Report=, Scorch=, Crater= "
            "and, for the Bouncer=yes debris, RandomRate=, Elasticity=, MaxXYVel=, MinZVel=; the "
            "image-bound types get 16 frames (MiddleFrameIndex 8, nonzero like their retail "
            "images'), the art-less gtpowexp 0.",
            "Audio disabled (0x008464AC clear): the Report sound returns at its gate.",
        ],
        substitutions=[
            "anim_bouncer_launch.Machine's constructor environment (MapClass cell lookup "
            "0x00565730, 0x005F5850, layer submit 0x004A9720, operator new 0x007C8E17 as a bump "
            "allocator)",
            "SmudgeTypeClass CanPlace 0x006B5F80 answers the case's admit mask",
            "MapClass::GetCell 0x005657A0 returns a fixed cell; the floor height 0x00578080 "
            "returns 0",
            "operator delete 0x007C8B3D is a no-op; the SmudgeClass constructor 0x006B4A50 "
            "records its type and coordinate",
        ],
        entry_points={"debris_block": DEBRIS_BEGIN, "destruction_effects_step7": EFFECTS_BEGIN,
                      "foundation_lists": FOUNDATION_INIT, "anim_ctor": launch.CTOR,
                      "anim_start": launch.START, "scorch": 0x6B59A0, "crater": 0x6B5C90,
                      "seed": launch.SEED},
    ))
