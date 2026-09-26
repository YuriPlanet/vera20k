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
   0x64 at the Location cell's centre, whose CanPlace 0x006B5F80 runs over
   MapClass's cell table (`smudge_can_place`); step 8 over the caller's fourth argument,
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

The cell table is `smudge_can_place`'s: MapClass's Size and CellClass pointer
array with the retail Dustbowl cells each Location's CanPlace reads (VERA20k's
retail load of the map: IsoTileTypeIndex, overlay, smudge and slope, and the
TEMPERATE theater's Morphable ranges; the Rust test asserts the production map
holds them), cells and the dummy built by the original CellClass constructor.
Every GetCell 0x005657A0 on the path runs natively over it. After the run the
recorded SmudgeClass's SmudgeTypeClass::Place 0x006B6080 executes on the table
(`marked`).

Supplied: GAPOWR's retail values (rulesmd.ini [GAPOWR], artmd.ini Foundation=2x2
and the AnimTypes' Bouncer/RandomRate/Elasticity/MaxXYVel/MinZVel/Report/
Scorch/Crater), the [SmudgeTypes] table (ArrayIndex +0x294, Width +0x298, Height
+0x29C, Crater +0x2A0, Burn +0x2A1), the anim-constructor environment of
`anim_bouncer_launch.Machine`, the floor height 0x00578080 (height 0),
operator delete 0x007C8B3D (no-op) and the SmudgeClass constructor 0x006B4A50
(recorded; `smudge_can_place` reads its Unlimbo -> Mark -> Place path).

Schema: `smudge_types` is the supplied [SmudgeTypes] table (index order);
`map` the Dustbowl cell table (`smudge_can_place.maps` schema); `rows[]` hold
`input`, `events` (ordered `ranged`/`next` draws with call site and result,
`can_place` {type, origin, force, result, dummy_read}, `smudge` {type, coord,
house}, `anim_ctor` {type, coord, delay, loop, flags}, `start`, `middle`,
`unlimbo`, ...), `debris` (per debris piece: its constructor event and
`location`/`bounce` state), `marked` (Place's writes: cell, dummy, type index,
SmudgeData), `raw_draw_count`, and the Scenario RNG states `rng_before`,
`rng_after_debris` and `rng_after` (0x3F4-byte hex).

Rust consumer: `retail_dustbowl_death_anims_use_the_types_lists` in
src/sim/combat/destruction_effects_tests.rs (the production receiver on the
retail Dustbowl map, reseeded at the kill).
"""
import struct
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle import anim_bouncer_launch as launch
from tools.spatial_oracle import smudge_can_place
from tools.spatial_oracle.anim_bouncer_launch import dwords
from tools.spatial_oracle.smudge_can_place import SMUDGE_TYPES

DEBRIS_BEGIN, DEBRIS_END = 0x702281, 0x702572
EFFECTS_BEGIN, EFFECTS_END = 0x44177E, 0x441A2B
FOUNDATION_INIT, FOUNDATION_LISTS, FOUNDATION_STRIDE = 0x45B1C0, 0x89C900, 0x78
BUILDING_VT = 0x7E3EBC
FLOOR, MIDDLE = 0x578080, 0x424F00

BUILDING = launch.MEM + 0x100000
BUILDING_TYPE = launch.MEM + 0x101000
VECTORS = launch.MEM + 0x104000

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
# The image-bound AnimTypes' frame count in `Machine.anim_type` is 16.
MIDDLE_FRAME = 8


class Machine(smudge_can_place.Machine):
    def __init__(self, seed):
        super().__init__(seed)
        self.constructed = []

    def hook(self, uc, address, size, data):
        if address in (launch.START, MIDDLE):
            # Recorded, then executed.
            self.events.append(dict(call="start" if address == launch.START else "middle",
                                    anim=uc.reg_read(UC_X86_REG_ECX) - launch.HEAP))
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
    machine = Machine(case["seed"])
    uc = machine.uc
    machine.cells.install_map(DUSTBOWL)
    machine.cells.install_smudge_types(SMUDGE_TYPES)
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
                marked=machine.cells.place_recorded(), raw_draw_count=machine.advances,
                rng_before=before, rng_after_debris=after_debris, rng_after=machine.rng())


# VERA20k's retail Dustbowl load (TEMPERATE, Size 70x76; temperatmd.ini's 838
# IsoTileTypes, Morphable=yes on these ranges): the cells step 7's CanPlace reads
# at each Location, both at level 1 (z 104). The Rust test asserts the production
# map holds them.
DUSTBOWL = dict(
    size=[70, 76], allocate_diamond=False, tile_count=838,
    morphable=smudge_can_place.ranges((0, 48), (131, 147), (404, 413), (510, 533), (551, 565)),
    cells=[
        # The fixture's power plant at (68, 40): ore (overlay 102) on all four cells, so
        # CanPlace (OverlayTypeIndex +0x44 must be -1, 0x006B6002) admits no candidate
        # and the placer draws nothing (0x006B5BF0).
        dict(x=68, y=40, tile=135, overlay=102, overlay_data=10),
        dict(x=69, y=40, tile=506, overlay=102, overlay_data=8),
        dict(x=68, y=41, tile=131, overlay=102, overlay_data=11),
        dict(x=69, y=41, tile=133, overlay=102, overlay_data=11),
        # Clean ground at (73, 116), its 0xFFFF tiles read as tile 0; (74, 117) is not
        # Morphable, so no 2x2 type fits and the force-1 placer, with nothing to prefer
        # (0x006B5B55), picks among every admitted 1x1, 2x1 and 1x2 type (0x006B5C1A).
        dict(x=73, y=116, tile=0xFFFF),
        dict(x=74, y=116, tile=0xFFFF),
        dict(x=73, y=117, tile=0xFFFF),
        dict(x=74, y=117, tile=507),
    ])
LOCATIONS = [[68 * 256 + 0x80, 40 * 256 + 0x80, 104], [73 * 256 + 0x80, 116 * 256 + 0x80, 104]]
# 22 and 49 construct a zero-delay `gtpowexp` (Start runs Middle) ahead of later cells.
SEEDS = [1, 7, 22, 31, 42, 49, 1000, 0x5CA1AB1E, 0xDEADBEEF, 2024]


def cases():
    for location in LOCATIONS:
        for seed in SEEDS:
            yield dict(GAPOWR, location=location, seed=seed)


def generate():
    return dict(smudge_types=[dict(name=name, burn=burn, crater=crater, width=width, height=height)
                              for name, burn, crater, width, height in SMUDGE_TYPES],
                map=DUSTBOWL, rows=[execute(case) for case in cases()])


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("A retail GAPOWR's death anims: TechnoClass::ReceiveDamage's debris block "
               "0x00702281..0x00702572 (count draw, DebrisAnims arm, each piece's AnimClass "
               "constructor and Start) then BuildingClass::DestructionEffects steps 7 and 8 "
               "0x0044177E..0x00441A2B (the centre scorch/crater roll and real placer, the "
               "per-foundation-cell scatter, delay and Explosion= pick, constructor and Start), "
               "over ten Scenario seeds at two retail Dustbowl Locations: the fixture's, whose "
               "cells carry ore, so CanPlace admits no SmudgeType and step 7 draws only its "
               "roll, and clean ground where the placer's pick and SmudgeTypeClass::Place "
               "0x006B6080 mark the map; CanPlace 0x006B5F80 and GetCell 0x005657A0 run "
               "natively over MapClass's cell table (smudge_can_place)."),
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
            "The Dustbowl cells (IsoTileTypeIndex, overlay, smudge, slope) and the TEMPERATE "
            "Morphable ranges are VERA20k's retail load of the map and theater, asserted "
            "against the production map by the Rust test; cells step 7 does not read are "
            "not allocated.",
            "The SmudgeClass constructor's Unlimbo -> Mark -> Place path is read, not run "
            "(smudge_can_place).",
        ],
        substitutions=[
            "anim_bouncer_launch.Machine's constructor environment (MapClass cell lookup "
            "0x00565730, 0x005F5850, layer submit 0x004A9720, operator new 0x007C8E17 as a bump "
            "allocator)",
            "the floor height 0x00578080 returns 0",
            "operator delete 0x007C8B3D is a no-op; the SmudgeClass constructor 0x006B4A50 "
            "records its type, coordinate and house; SmudgeTypeClass::Place 0x006B6080 then "
            "runs on the recorded type and truncated cell, its redraw 0x00486E70 recorded",
        ],
        entry_points={"debris_block": DEBRIS_BEGIN, "destruction_effects_step7": EFFECTS_BEGIN,
                      "foundation_lists": FOUNDATION_INIT, "anim_ctor": launch.CTOR,
                      "anim_start": launch.START, "scorch": 0x6B59A0, "crater": 0x6B5C90,
                      "can_place": smudge_can_place.CAN_PLACE, "place": smudge_can_place.PLACE,
                      "seed": launch.SEED},
    ))
