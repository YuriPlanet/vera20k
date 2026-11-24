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
from tools.native_oracle import require
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
# at each Location, all at level 1 (z 104). The Rust test asserts the production
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
        # Clean ground at (81, 123), tile 0 on all four cells (the map's only flat,
        # Morphable 2x2 block free of overlay and terrain objects): every candidate fits
        # and the force-1 placer picks among the 2x2 types it prefers.
        dict(x=81, y=123, tile=0),
        dict(x=82, y=123, tile=0),
        dict(x=81, y=124, tile=0),
        dict(x=82, y=124, tile=0),
    ])
LOCATIONS = [[68 * 256 + 0x80, 40 * 256 + 0x80, 104], [73 * 256 + 0x80, 116 * 256 + 0x80, 104],
             [81 * 256 + 0x80, 123 * 256 + 0x80, 104]]
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



# Additive original-execution composition. The primitive owner above and its
# recorded payload are retained. All gameplay calls below enter unmodified PE
# instructions; Python supplies prior data, platform transport and observations.
JOINED_ANIMS = ('DBRIS1LG', 'DBRIS1SM', 'DBRIS4LG', 'DBRIS4SM', 'DBRIS5LG', 'DBRIS5SM',
                'TWLT026', 'TWLT036', 'TWLT070', 'S_BANG48', 'S_BRNL58', 'S_CLSN58',
                'S_TUMU60', 'SMOKEY2', 'S_CLSN16', 'S_CLSN22', 'GUNFIRE')
JOINED_EXTRA_ASSETS = tuple(name + '.SHP' for name in JOINED_ANIMS) + ('GI.SHP', '120MM.SHP')
JOINED_DAMAGE_FIRES = ('FIRE01', 'FIRE02', 'FIRE03')
# These are existing Mission platform/presentation arms, never a replacement
# FireAt, Bullet, entry, damage, Scatter, graph, path, animation or House answer.
JOINED_TRANSPORT = frozenset((0x527AF9, 0x527B0C, 0x41C27D, 0x41C2CB, 0x41C28C, 0x413DE2, 0x413DF1,
    0x46B072, 0x655560, 0x655740, 0x5F8110, 0x5F8CE0, 0x46AFE5, 0x46B007,
    0x55A965, 0x55A987, 0x7CAA5E))


def joined_inputs(root, *, damage_fires=False, stock_smudges=False, damage_sound=False,
                  typed_debris=False, layers=None, wall_autofill=False):
    import hashlib
    from collections import deque
    from types import SimpleNamespace
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_EDI, UC_X86_REG_EAX
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc, engineer_repair_admission as er
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.spatial_oracle.building_body_rules import SP, INI
    from tools.rules_oracle.bridge_child_sound import sections

    anim_names = JOINED_ANIMS + (JOINED_DAMAGE_FIRES if damage_fires else ())
    extra_assets = JOINED_EXTRA_ASSETS + tuple(name + '.SHP' for name in
                                             (JOINED_DAMAGE_FIRES if damage_fires else ()))
    building_names = (*bc.JOINED_NAMES, 'CAMISC03') if damage_sound else bc.JOINED_NAMES
    if wall_autofill:
        building_names = (*building_names, 'GAWALL')
        extra_assets += ('GTWALL.SHP',)
    if damage_sound:
        extra_assets += ('CTMISC03.SHP',)
    m = er.prepare_joined_inputs(root, layers=layers)
    m.input_rule_layers = tuple(layers if layers is not None else
        ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'))
    u, read = m.u, m.read32
    original_ret = m.ret
    m.ret = lambda eax=0, cleanup=0: original_ret(eax, cleanup)
    proxy = SimpleNamespace(m=m, trace=deque(maxlen=40), events=[], pending={}, frame=0, phase='setup')
    executed, platform = set(), []

    def observe(u, pc, size, _):
        executed.add(pc)
        # Engineer Inputs already owns these two signed-CRC string transports.
        if pc in JOINED_TRANSPORT - {0x527AF9, 0x527B0C}:
            Mission.observe(proxy, u, pc, size, None)
        elif pc == 0x7C978A:
            platform.append(dict(pc=f'0x{pc:08X}', kind='CRT_exit_registration',
                                 callback=read(u.reg_read(UC_X86_REG_ESP) + 4)))
            m.ret()

    hook = u.hook_add(UC_HOOK_CODE, observe)
    for entry in (0x4E6F60, 0x40F490, 0x40FB30):
        m.invoke(entry, 0)
    physical_rules = sections((root / 'RULESMD.INI').read_bytes())
    # Older shared Construction/Engineer fixtures use the loose MIX container
    # as their lexical scenario source. This join reads its real Hills.map
    # entry and checks every selected inherited section has the same contents.
    # This does not stand in for a physical map load or whole Rules::Process.
    inherited_map = sections((root / 'Hills.mmx').read_bytes())
    scenario_map = sections((root / 'Hills.map').read_bytes())
    selected_map_sections = (*building_names, 'General', 'AudioVisual',
        'CombatDamage', 'ENGINEER', 'E1', 'CTECH', 'MTNK', '105mm', '105mmE',
        'Cannon', 'AP', 'Americans', 'ElevationModel', 'Radiation')
    inherited_projection = {name: inherited_map.get(name) for name in selected_map_sections}
    scenario_projection = {name: scenario_map.get(name) for name in selected_map_sections}
    assert inherited_projection == scenario_projection
    m.death_scenario_projection = dict(
        inherited_file='Hills.mmx', scenario_file='Hills.map',
        selected_sections=list(selected_map_sections), identical=True,
        projection_sha256=hashlib.sha256(native._canonical(scenario_projection)).hexdigest())
    countries = []
    for name in physical_rules['Countries'].values():
        existing = [read(read(0xA83C9C) + i * 4) for i in range(read(0xA83CA8))]
        pointer = next((p for p in existing if m.string(p + 0x24) == name), None)
        if pointer is None:
            pointer = m.alloc(0x400)
            m.invoke(0x5113F0, pointer, (m.cstring(name),))
        countries.append(dict(name=name, pointer=pointer))
    m.make_ini({'Sides': physical_rules['Sides']})
    side_result = m.invoke(0x672440, 0, (INI,))
    for row in countries:
        row['native_side'] = struct.unpack('<i', u.mem_read(row['pointer'] + 0xBC, 4))[0]
    art = sections((root / 'ARTMD.INI').read_bytes())
    for name in extra_assets:
        path = root / name
        if not path.is_file():
            raise ValueError(f'Missing joined death input: {path}')
        m.assets[name.upper()] = path.read_bytes()
    for name in ('E1', 'CTECH'):
        pointer = m.alloc(0x1900)
        m.invoke(0x5236A0, pointer, (m.cstring(name),))
        m.types[name] = pointer
    for registry in ((0xA83CE0, 0xA8EB00) if layers is None else (0xA83CE0,)):
        u.mem_write(registry, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    # Existing BulletReader unused Color registry prior. The Cannon Color key is
    # absent; no represented color decision or native Color body is replaced.
    color, colors = m.alloc(0x400), m.alloc(4)
    u.mem_write(color + 0x304, dwords(m.cstring('FIXTURE_UNUSED_COLOR')))
    u.mem_write(colors, dwords(color))
    u.mem_write(0xB054D4, dwords(colors))
    u.mem_write(0xB054E0, dwords(1))
    tank = m.alloc(0xF00)
    m.invoke(0x7470D0, tank, (m.cstring('MTNK'),))
    m.types['MTNK'] = tank
    if damage_sound:
        pointer = m.alloc(0x2000)
        m.invoke(0x45DD90, pointer, (m.cstring('CAMISC03'),))
        m.types['CAMISC03'] = pointer
    if wall_autofill:
        pointer = m.alloc(0x2000)
        m.invoke(0x45DD90, pointer, (m.cstring('GAWALL'),))
        m.types['GAWALL'] = pointer
        # The ToOverlay reader must see the real physical registry ordinal,
        # not a one-item replacement catalog. Original constructors own it.
        m.invoke(0x4E71E0, 0)
        m.wall_overlay_names = list(physical_rules['OverlayTypes'].values())
        m.wall_overlay_types = {}
        for name in m.wall_overlay_names:
            typ = m.alloc(0x300)
            m.invoke(0x5FE250, typ, (m.cstring(name),))
            m.wall_overlay_types[name] = typ
        for path in sorted(root.glob('*WALL*.SHP')):
            m.assets[path.name.upper()] = path.read_bytes()
    wanted = {'Dummy', 'BuildingRepaired', 'PowerPlantDie', 'GrizzlyTankAttack'}
    if typed_debris:
        # The fatal source is MTNK here. Bind its actual death sounds before
        # full747620 reads the vectors; unknown registry names are dropped by
        # the native sound-list reader. Historical building controls omit this
        # source-death prerequisite and retain their selected registry exactly.
        for filename in m.input_rule_layers:
            path = root / filename
            if path.exists():
                layer = sections(path.read_bytes()).get('MTNK', {})
                for key in ('VoiceDie', 'DieSound'):
                    wanted.update(name.strip() for name in layer.get(key, '').split(','))
    if damage_sound:
        # Select the actual authored per-type and fallback names in layer
        # order; original7510D0/45FE50/6691E0 still own their binding.
        for filename in m.input_rule_layers:
            path = root / filename
            if path.exists():
                layer = sections(path.read_bytes())
                wanted.update(layer.get('CAMISC03', {}).get('DamageSound', '').split(','))
                wanted.update(layer.get('AudioVisual', {}).get('BuildingDamageSound', '').split(','))
    for name in anim_names:
        for key in ('Report', 'StartSound'):
            wanted.update(art.get(name, {}).get(key, '').split(','))
    wanted.discard('')
    sound = sections((root / 'SOUNDMD.INI').read_bytes())
    selected = {name: sound[name] for name in sound if name in wanted or name == 'Defaults'}
    selected['SoundList'] = {key: value for key, value in sound['SoundList'].items() if value in wanted}
    m.make_ini(selected)
    m.invoke(0x7510D0, INI)
    m.death_sounds = [dict(name=name, fixture_index=m.invoke(0x7514D0, m.cstring(name)))
                      for name in sorted(wanted)]
    selected_art = {name: art[name] for name in (*bc.JOINED_ART_NAMES, *anim_names,
                    'MTNK', 'GTNK', 'Cannon', '120MM', 'GI', 'GISequence',
                    'ENGINEER', 'EngineerSequence', 'CIV3', 'CivilianSequence',
                    *(('CAMISC03',) if damage_sound else ()),
                    *(('GAWALL',) if wall_autofill else ())) if name in art}
    m.death_art_input_sections = dict(selected_art,
        **{name: art.get(name) for name in ('E1', 'GI', 'GISequence', 'GAPOWR', 'gtpowexp')})
    m.make_ini(selected_art)
    art_cache = bytes(u.mem_read(INI, 0x40))
    weapon = m.invoke(0x772FA0, m.cstring('105mm'))
    warhead = m.invoke(0x75E3B0, m.cstring('AP'))

    def rule_cache(retained):
        m.make_ini(retained)
        pointer = m.alloc(0x40)
        u.mem_write(pointer, bytes(u.mem_read(INI, 0x40)))
        u.mem_write(INI, art_cache)
        return pointer

    def rule_block(begin, end, pointer):
        u.mem_write(SP, bytes(0x300))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, m.rules)
        u.reg_write(UC_X86_REG_EDI, pointer)
        native.run_checked(u, begin, end, count=2_000_000)

    m.death_layers = []
    for filename in m.input_rule_layers:
        path = root / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            m.death_layers.append(dict(file=filename, absent=True))
            continue
        physical = sections(path.read_bytes())
        retained = {name: physical[name] for name in (*building_names, 'General',
            'AudioVisual', 'CombatDamage', 'E1', 'CTECH', 'MTNK', '105mm', '105mmE',
            'Cannon', 'AP', 'Americans', 'ElevationModel', 'Radiation') if name in physical}
        # Physical inputs for the Rust fixture's existing production readers.
        # These E1 weapon owners do not execute in the represented escape;
        # keep their authored sections separate from the native-admitted cache.
        reference_keys = ('Primary', 'Secondary', 'ElitePrimary', 'EliteSecondary',
                          'OccupyWeapon', 'EliteOccupyWeapon')
        e1_weapons = {values[key] for values in (physical_rules.get('E1', {}),
                      physical.get('E1', {})) for key in reference_keys if key in values}
        declared_names = set(e1_weapons)
        for name in e1_weapons:
            for values in (physical_rules.get(name, {}), physical.get(name, {})):
                declared_names.update(values[key] for key in ('Projectile', 'Warhead') if key in values)
        declared_e1_weapons = {name: physical[name] for name in sorted(declared_names) if name in physical}
        rules_ini = rule_cache(retained)
        for begin, end in ((0x66ED6C, 0x66ED93), (0x66FC7B, 0x66FCB9),
                           (0x66FCD8, 0x66FCF7), (0x670BEC, 0x670C72)):
            rule_block(begin, end, rules_ini)
        building_reads = {name: m.invoke(0x45FE50, m.types[name], (rules_ini,)) & 255
                          for name in building_names}
        if wall_autofill:
            m.wall_overlay_reads = getattr(m, 'wall_overlay_reads', [])
            m.wall_overlay_reads.append(dict(file=filename,
                result=m.invoke(0x5FE770, m.wall_overlay_types['GAWALL'], (rules_ini,)) & 255,
                physical_keys=physical.get('GAWALL'),
                building_to_overlay=read(m.types['GAWALL'] + 0xE54)))
        e1_result = m.invoke(0x5240A0, m.types['E1'], (rules_ini,)) & 255
        tank_result = m.invoke(0x747620, tank, (rules_ini,)) & 255
        assert read(tank + 0x898) == weapon
        weapon_result = m.invoke(0x772080, weapon, (rules_ini,)) & 255
        projectile = read(weapon + 0xA0)
        projectile_result = m.invoke(0x46BEE0, projectile, (rules_ini,)) & 255
        warhead_result = m.invoke(0x75D3A0, warhead, (rules_ini,)) & 255
        m.invoke(0x7729F0, weapon)
        m.invoke(0x511850, m.country, (rules_ini,))
        for entry in (0x66D530, 0x66D150, 0x66CF70):
            m.invoke(entry, m.rules, (rules_ini,),
                     timeout_us=60_000_000 if layers is not None else 10_000_000)
        if m.invoke(0x526810, rules_ini, (m.cstring('AudioVisual'),)):
            if damage_sound:
                m.invoke(0x6691E0, m.rules, (rules_ini,))
            else:
                rule_block(0x66B3C4, 0x66B3E4, rules_ini)
        if m.invoke(0x526810, rules_ini, (m.cstring('CombatDamage'),)):
            for begin, end in ((0x66CD66, 0x66CD8C), (0x66C184, 0x66C287), (0x66CE2C, 0x66CE57)):
                rule_block(begin, end, rules_ini)
        m.death_layers.append(dict(file=filename, sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
            admitted=dict(buildings=building_reads, e1=e1_result, mtnk=tank_result,
                          weapon=weapon_result, projectile=projectile_result, warhead=warhead_result),
            native=dict(weapon_damage=read(weapon + 0xA4), mtnk_strength=read(tank + 0xA0),
                mtnk_primary_flh=list(struct.unpack('<3i', u.mem_read(tank + 0x89C, 12))),
                e1_strength=read(m.types['E1'] + 0xA0), e1_nominal=u.mem_read(m.types['E1'] + 0xC9E, 1)[0],
                crew_escape_f64_bits=bytes(u.mem_read(m.rules + 0x5C0, 8)).hex(),
                survivor_rate_f64_bits=bytes(u.mem_read(m.rules + 0x14F0, 8)).hex(),
                survivor_divisors=list(struct.unpack('<3i', u.mem_read(m.rules + 0x14F8, 12))),
                allied_crew=read(m.rules + 0xF78), elevation_increment=read(m.rules + 0x1838)),
            physical_keys={name: retained.get(name) for name in
                ('GAPOWR', 'E1', 'MTNK', '105mm', 'Cannon', 'AP', 'Americans', 'ElevationModel')},
            physical_input_sections=retained,
            declared_e1_weapon_sections=declared_e1_weapons))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EBP, tank)
    native.run_checked(u, 0x715B10, 0x715F9E)
    u.mem_write(INI, art_cache)
    m.death_anim_rows = []
    for name in anim_names:
        pointer = m.invoke(0x428B80, m.cstring(name))
        mark = len(m.asset_loaded)
        admitted = m.invoke(0x427D00, pointer, (INI,))
        m.death_anim_rows.append(dict(pointer=pointer, **m.result(name, pointer, admitted),
            assets=m.asset_loaded[mark:], physical_art_keys=art.get(name)))
    m.weapon, m.warhead = weapon, warhead
    if damage_sound:
        m.damage_sound_inputs = dict(target='CAMISC03',
            strength=read(m.types['CAMISC03'] + 0xA0),
            per_type_id=struct.unpack('<i', u.mem_read(m.types['CAMISC03'] + 0x538, 4))[0],
            global_id=struct.unpack('<i', u.mem_read(m.rules + 0x714, 4))[0],
            image_assets=[row for row in m.asset_loaded if 'MISC03' in str(row)])
        m.make_ini({})
        m.voice_theme_ini = m.alloc(0x40)
        u.mem_write(m.voice_theme_ini, bytes(u.mem_read(INI, 0x40)))
    m.death_country_sides = dict(entry='0x00672440', result=side_result, countries=countries,
                                physical_sides=physical_rules['Sides'])
    m.death_input_execution = dict(instruction_addresses=[f'0x{pc:08X}' for pc in sorted(executed)],
                                  platform=platform, transports=proxy.events)
    u.hook_del(hook)
    if stock_smudges:
        joined_stock_inputs(m, root, root)
    if wall_autofill:
        # Reuse the existing clear-tile setup and original theater Boolean
        # reader. This declared component map is not a physical Hills load.
        from tools.spatial_oracle.tmp_clear_render import theater_tile_files
        from tools.spatial_oracle.terrain_recalc import LAT_GLOBALS
        raw = (root / 'TEMPERATMD.INI').read_bytes()
        m.make_ini({'TileSet0000': sections(raw)['TileSet0000']})
        pointer = m.alloc(0x40)
        u.mem_write(pointer, bytes(u.mem_read(INI, 0x40)))
        m.stock_theater_inputs = dict(file='TEMPERATMD.INI',
            sha256=hashlib.sha256(raw).hexdigest(), ini=pointer)
        # Whole545535..545C3F also initializes the role globals to native
        # sentinels; cold zero makes ClearTile0 alias LAT/cliff ranges during
        # Recalc. Reuse the existing original General/ordinal projection
        # owner, not supplied -1 writes or a tile-result callback.
        tiles, mapping = theater_tile_files(m, raw, 0, 'tem')
        m.wall_theater_inputs = dict(mapping=mapping, registered_tile_count=len(tiles),
            physical_general=sections(raw).get('General'),
            roles={f'0x{address:08X}': read(address)
                for address in (*LAT_GLOBALS, 0xAA10B0)},
            boundary='Original theater General/default stores and ordinal projection; imported native role globals. Only the declared ClearTile0 head is constructed in the joined map; no full theater/TMP world load.')
        assert read(0xAA10B0) == 0
        u.mem_write(INI, art_cache)
        # Full existing Overlay theater owner takes the enum in ECX, not an
        # individual type receiver. Only selected GAWALL has been read here;
        # all other physical registry members retain constructor defaults.
        m.invoke(0x5FE620, 0)
        assert read(m.wall_overlay_types['GAWALL'] + 0xA4)
        m.wall_image_assets = [row for row in m.asset_loaded if 'WALL' in str(row)]
    image = native.image_bytes()
    for rva, source, length, _, flags in native._sections(image):
        if length and flags & 0x20000000:
            assert bytes(u.mem_read(native.IMAGE_BASE + rva, length)) == image[source:source + length]
    return m


def joined_stock_inputs(m, root, extra):
    import hashlib
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP,
        UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP)
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc
    from tools.spatial_oracle.building_body_rules import INI, SP, dwords
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.spatial_oracle.refinery_dock import cell
    u,read=m.u,m.read32
    m.assets.update({p.name.upper():p.read_bytes() for p in extra.iterdir() if p.is_file()})
    executed=set(); startups=[]; calls=[]; returns=[]
    def observe(u,pc,size,_):
        executed.add(pc)
        if pc==0x7C978A:
            startups.append(dict(crt_callback=read(u.reg_read(UC_X86_REG_ESP)+4)))
            m.ret()
        if pc in (0x6B5260,0x6B56D0):
            sp=u.reg_read(UC_X86_REG_ESP)
            row=dict(entry=f'0x{pc:08X}',this=u.reg_read(UC_X86_REG_ECX),caller=f'0x{read(sp):08X}')
            calls.append(row);returns.append((read(sp),row))
        for at,row in returns[:]:
            if pc==at:
                row['result']=u.reg_read(UC_X86_REG_EAX);returns.remove((at,row))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    m.invoke(0x4E7260,0)
    stock=sections((root/'RULESMD.INI').read_bytes())
    smudge_names=list(stock['SmudgeTypes'].values())
    art=sections((root/'ARTMD.INI').read_bytes())
    original_art_cache=bytes(u.mem_read(INI,0x40))
    retained_art={name:art[name] for name in smudge_names if name in art}
    m.make_ini(retained_art); art_cache=bytes(u.mem_read(INI,0x40))
    layers=[]
    for filename in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
        path=root/filename
        if not path.exists():
            assert filename=='LANGRULE.INI';layers.append(dict(file=filename,absent=True));continue
        raw=path.read_bytes();physical=sections(raw)
        names=list(physical.get('SmudgeTypes',{}).values())
        existing=[m.string(read(read(0xA8EC1C)+i*4)+0x24) for i in range(read(0xA8EC28))]
        retained={name:physical[name] for name in ('SmudgeTypes',*names,*existing) if name in physical}
        m.make_ini(retained)
        rules_ini=m.alloc(0x40);u.mem_write(rules_ini,bytes(u.mem_read(INI,0x40)));u.mem_write(INI,art_cache)
        u.mem_write(SP,bytes(0x300));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,rules_ini)
        native.run_checked(u,0x668DD2,0x668E23,count=2_000_000)
        mark=len(m.asset_loaded)
        u.mem_write(SP,bytes(0x300));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,rules_ini)
        native.run_checked(u,0x679BE2,0x679C06,count=2_000_000)
        types=[]
        for i in range(read(0xA8EC28)):
            p=read(read(0xA8EC1C)+i*4)
            types.append(dict(name=m.string(p+0x24),pointer=p,index=read(p+0x294),width=read(p+0x298),height=read(p+0x29C),crater=u.mem_read(p+0x2A0,1)[0],burn=u.mem_read(p+0x2A1,1)[0],theater=u.mem_read(p+0x22C,1)[0],image=m.string(p+0x1F8),image_pointer=read(p+0xA4)))
        layers.append(dict(file=filename,sha256=hashlib.sha256(raw).hexdigest(),physical_smudge_registry_entries=[list(pair) for pair in physical.get('SmudgeTypes',{}).items()],physical_input_sections=retained,registry=types,assets=m.asset_loaded[mark:]))
    theater_raw=(extra/'TEMPERATMD.INI').read_bytes();theater=sections(theater_raw)
    m.make_ini({'TileSet0000':theater['TileSet0000']})
    theater_ini=m.alloc(0x40);u.mem_write(theater_ini,bytes(u.mem_read(INI,0x40)))
    u.mem_write(INI,original_art_cache)
    u.hook_del(hook)
    assert not returns
    m.stock_smudge_inputs=dict(startup_entry='0x004E7260',startups=startups,layers=layers,physical_art_input_sections=retained_art,calls=calls,executed=[f'0x{pc:08X}' for pc in sorted(executed)])
    m.stock_theater_inputs=dict(file='TEMPERATMD.INI',sha256=hashlib.sha256(theater_raw).hexdigest(),physical_input_sections={'TileSet0000':theater['TileSet0000']},ini=theater_ini)
    return m

def joined_stock_flat_map(f,m):
    import hashlib
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP,
        UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP)
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc
    from tools.spatial_oracle.building_body_rules import INI, SP, dwords
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.spatial_oracle.refinery_dock import cell
    u,read=f.u,f.read32
    before=f.rng();f.phase='setup_stock_smudge_map'
    u.mem_write(0xA8EC18,bytes(m.u.mem_read(0xA8EC18,24)))
    for entry in (0x4E76E0,0x4E69E0,0x6B5210):
        bc.invoke(u,entry,0)
    tile=f.allocate(0x400); name=f.allocate(32)
    u.mem_write(name,b'Clear01\0')
    result=bc.invoke(u,0x5447C0,tile,0,0xFFFFFFBF,0,name,0)
    assert result==tile and read(0xA8ED38)==1 and read(read(0xA8ED2C))==tile
    constructor_morphable=u.mem_read(tile+0x2E0,1)[0]
    # Selected original theater reader and its original result-store fragment.
    # The complete loader's local-frame cache and section text are supplied;
    # neither a parsed Boolean nor a chosen smudge answer is supplied.
    u.mem_write(SP,bytes(0x800));u.mem_write(SP+0x38,bytes(m.u.mem_read(m.stock_theater_inputs['ini'],0x40)))
    u.mem_write(SP+0x1B4,b'TileSet0000\0');u.reg_write(UC_X86_REG_ESP,SP)
    native.run_checked(u,0x546124,0x54614C,count=10_000,required_addresses=(0x54613F,0x5295F0))
    u.reg_write(UC_X86_REG_EBP,tile)
    native.run_checked(u,0x54642A,0x54642E,count=10)
    native.run_checked(u,0x54644C,0x546452,count=10)
    assert u.mem_read(tile+0x2E0,1)[0]==1
    # Explicit already-admitted legal flat ClearTile0 physical cells. Shared
    # original Cell constructors already own slope/overlay/smudge defaults.
    for y in range(1,32):
        for x in range(1,32):
            u.mem_write(cell(x,y)+0x38,dwords(0))
    f.stock_smudge_setup=dict(registry_count=read(0xA8EC28),tile_constructor=dict(entry='0x005447C0',pointer=tile,index=0,result=result,constructor_morphable=constructor_morphable),theater_morphable=dict(reader='0x00546124..0x0054614C',store='0x0054644C..0x00546452',value=u.mem_read(tile+0x2E0,1)[0]),rng_before=before,rng_after=f.rng())
    assert before==f.rng()
    f.phase='setup'
    return f


def joined_stock_observer(f):
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX,
        UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP)
    from tools.spatial_oracle import building_construction as bc
    from tools.spatial_oracle.refinery_dock import cell
    from tools.spatial_oracle.unit_source_scatter import SCENARIO
    u,read=f.u,f.read32
    entries={0x6B59A0:'burn_placer',0x6B5C90:'crater_placer',0x6B5F80:'can_place',0x6B4A50:'smudge_constructor',0x6B4BE0:'smudge_mark',0x6B4FA0:'smudge_scalar_destructor',0x6B6080:'smudge_place',0x5F65F0:'uninit',0x5F3B80:'object_destructor',0x68BCB0:'unique_id',0x7258D0:'expiry',0x7446E0:'unit_pointer_expired',0x4D9960:'foot_pointer_expired',0x7077C0:'techno_pointer_expired',0x43F180:'building_mark'}
    calls=[];pending=[];writes=[];smudges=set();pcs=set()
    smudge_fields={cell(x,y)+off:(x,y,off) for y in range(1,32) for x in range(1,32) for off in (0x48,0x11F)}
    def identity(p):
        return dict(pointer=p,id=read(p+0x10),health=struct.unpack('<i',u.mem_read(p+0x6C,4))[0],location=list(struct.unpack('<3i',u.mem_read(p+0x9C,12))),vtable=f'0x{read(p):08X}',abstract_flags=read(p+0x14),alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],marked=u.mem_read(p+0x74,1)[0],logic_member_98=u.mem_read(p+0x98,1)[0])
    def members(items,count):
        return [identity(read(items+i*4)) for i in range(count)]
    def snapshot():
        cells=[]
        for y in range(1,32):
            for x in range(1,32):
                p=cell(x,y);kind=struct.unpack('<i',u.mem_read(p+0x48,4))[0]
                if kind!=-1:cells.append(dict(cell=[x,y],type_index=kind,data=u.mem_read(p+0x11F,1)[0]))
        return dict(unique_id_cursor=read(SCENARIO+0x214),rng=f.rng(),pending_count=read(0xB0F6A8),pending_members=members(read(0xB0F69C),read(0xB0F6A8)),smudge_object_count=read(0xA8B1F0),smudge_objects=members(read(0xA8B1E4),read(0xA8B1F0)),generic_object_count=read(0xB0F730),generic_object_members=members(read(0xB0F724),read(0xB0F730)),logic_members=members(read(0x87F77C),read(0x87F788)),smudge_cells=cells,attacker=dict(target=read(f.source+0x2B4),passive_scan_timer=list(struct.unpack('<3i',u.mem_read(f.source+0x180,12)))))
    def observe(u,pc,size,_):
        pcs.add(pc)
        for row in pending[:]:
            if pc==row['return_pc']:
                row['after']=snapshot();row['result']=u.reg_read(UC_X86_REG_EAX)
                if row['kind']=='smudge_constructor':
                    row['id']=read(row['this']+0x10);row['abstract_flags']=read(row['this']+0x14)
                    row['alive']=u.mem_read(row['this']+0x90,1)[0];row['limbo']=u.mem_read(row['this']+0x81,1)[0];row['constructed_object']=identity(row['this'])
                pending.remove(row)
        if pc not in entries:return
        this=u.reg_read(UC_X86_REG_ECX);sp=u.reg_read(UC_X86_REG_ESP)
        if pc in (0x5F65F0,0x5F3B80) and this not in smudges:return
        if pc==0x7258D0 and this not in smudges:return
        if pc in (0x7446E0,0x4D9960,0x7077C0) and read(sp+4) not in smudges:return
        row=dict(sequence=len(calls),kind=entries[pc],pc=f'0x{pc:08X}',caller=f'0x{read(sp):08X}',phase=f.phase,frame=read(bc.FRAME),this=this,return_pc=read(sp),before=snapshot())
        if pc==0x6B4A50:
            smudges.add(this);p=read(sp+4);coord=read(sp+8)
            row.update(type=p,type_name=f.string(p+0x24),coordinate=list(struct.unpack('<3i',u.mem_read(coord,12))),house=read(sp+12))
        if pc==0x6B5F80:
            row.update(type_name=f.string(this+0x24),origin=list(struct.unpack('<2h',u.mem_read(read(sp+4),4))),force=read(sp+8)&255)
        if pc in (0x6B4BE0,0x43F180):row['mark_argument']=read(sp+4)
        if pc==0x7258D0:row.update(control_dl=u.reg_read(UC_X86_REG_EDX)&255,object=identity(this))
        if pc in (0x7446E0,0x4D9960,0x7077C0):row.update(expired=identity(read(sp+4)),control=read(sp+8)&255)
        calls.append(row);pending.append(row)
    def write(u,access,address,size,value,_):
        if address in smudge_fields:
            x,y,off=smudge_fields[address]
            writes.append(dict(sequence=len(calls),phase=f.phase,frame=read(bc.FRAME),pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',cell=[x,y],offset=off,size=size,before=int.from_bytes(u.mem_read(address,size),'little'),value=value&((1<<(size*8))-1)))
    u.hook_add(UC_HOOK_CODE,observe);u.hook_add(UC_HOOK_MEM_WRITE,write)
    return dict(f=f,calls=calls,writes=writes,pcs=pcs,pending=pending,snapshot=snapshot)

def joined_fixture(inputs, health, *, seed=31, building_ai=False, drive_startup=False,
                   damage_sound=False, unit_startup=False):
    import hashlib
    from collections import deque
    from types import SimpleNamespace
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_EAX
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc, engineer_repair_admission as er
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.spatial_oracle.anytown_damage.navigation import Navigation, MAP
    from tools.spatial_oracle.refinery_dock import cell, place_building
    from tools.spatial_oracle.building_body_rules import SP
    from tools.spatial_oracle.fire_error import LEVEL_HEIGHT_INITIALIZERS
    from tools.spatial_oracle.infantry_entry_raw import STARTUP as INFANTRY_STARTUP

    class Fixture(er.EngineerJoinedFixture):
        transport_entries = JOINED_TRANSPORT

        def __init__(self):
            self.transport = SimpleNamespace(m=self, trace=deque(maxlen=40), events=[],
                                            pending={}, frame=0, phase='setup')
            self.building_constructor = None
            super().__init__(inputs, seed=seed, arena_size=0x1000000)
            if unit_startup:
                # CRT813234 -> whole4E6C60 constructs DynamicVector<Unit*>
                # at8B4108 before Unit7353C0 registers the selected source.
                # Its cold inherited header has no virtual removal owner;
                # Unit7358AB's destructor needs this actual initializer.
                # Keep historical fixture defaults, never supply a vtable or
                # emulate removal. The shared VM still owns every native call.
                self.phase = 'setup_unit_registry_crt'
                assert 0x7353C0 not in self.executed
                assert self.read32(0x813234) == 0x4E6C60
                before = self.rng()
                bc.invoke(self.u, 0x4E6C60, 0)
                assert before == self.rng()
                self.phase = 'setup'
            if building_ai:
                # Original process CRT table813490 owns Infantry's separate
                # level104/bridge416 globals. Techno6F28xx does not initialize
                # A8F234: cold0 makes521850 clear deck at flatZ0 while5217C0
                # puts ground. Run the existing original startup owner before
                # any joined Foot constructor; never supply a bitmap answer.
                self.phase = 'setup_infantry_crt_startup'
                u, read = self.u, self.read32
                assert not {0x4D31E0, 0x517A50, 0x7353C0} & self.executed
                table = list(struct.unpack('<10I', u.mem_read(0x813490, 40)))
                assert table == INFANTRY_STARTUP
                self.infantry_startup = dict(table='0x00813490',
                    initializers=[f'0x{entry:08X}' for entry in table],
                    before_joined_foot_constructor=True,
                    before=dict(base_height_a8f240=read(0xA8F240),
                        bridge_threshold_a8f234=read(0xA8F234), rng=self.rng()))
                visited = []

                def observe_startup(_u, pc, _size, _data):
                    visited.append(pc)

                observer = u.hook_add(UC_HOOK_CODE, observe_startup)
                try:
                    for entry in INFANTRY_STARTUP:
                        bc.invoke(u, entry, 0)
                finally:
                    u.hook_del(observer)
                self.infantry_startup.update(
                    executed_initializers=[f'0x{pc:08X}' for pc in visited if pc in table],
                    executed_pcs=[f'0x{pc:08X}' for pc in sorted(set(visited))],
                    after=dict(base_height_a8f240=read(0xA8F240),
                        bridge_threshold_a8f234=read(0xA8F234), rng=self.rng()))
                assert self.infantry_startup['executed_initializers'] == self.infantry_startup['initializers']
                assert self.infantry_startup['before']['rng'] == self.infantry_startup['after']['rng']
                assert (read(0xA8F240), read(0xA8F234)) == (104, 416)
                self.phase = 'setup'

            if drive_startup:
                # Selected-source startup, after the inherited supplied dock
                # Drive and before source7353C0. Keep historical defaults;
                # execute the registered group, never another scalar104 prior.
                from unicorn import UC_HOOK_MEM_READ, UC_MEM_READ
                from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW
                from tools.spatial_oracle.unit_scatter_state import LOCO as PRIOR_DRIVE
                assert building_ai and not {0x4D31E0, 0x7353C0} & self.executed
                u, read = self.u, self.read32
                begin, end = 0x812D2C, 0x812D64
                expected = (0x4AF330, 0x4AF360, 0x4AF380, 0x4AF3A0,
                    0x4AF3C0, 0x4AF3E0, 0x4AF400, 0x4AF440, 0x4AF470,
                    0x4AF4A0, 0x4AF4D0, 0x4AF4E0, 0x4AF500, 0x4AF520)
                original_table = native.file_span(native.image_bytes(), begin, end-begin)[1]
                assert struct.unpack('<14I', original_table) == expected
                assert bytes(u.mem_read(begin, end-begin)) == original_table
                self.phase = 'setup_selected_source_drive_crt_startup'
                calls, accesses, pending = [], [], []

                def state():
                    return dict(globals_8a0758_8a07d4=bytes(u.mem_read(0x8A0758, 0x7C)).hex(),
                        level_height=read(0x8A07D0), bridge_height=read(0x8A07C4),
                        empty=list(struct.unpack('<3i', u.mem_read(0x8A0790, 12))),
                        origin=list(struct.unpack('<3i', u.mem_read(0x8A07B8, 12))),
                        fpcw=u.reg_read(UC_X86_REG_FPCW), fpsw=u.reg_read(UC_X86_REG_FPSW),
                        rng=self.rng())

                def code(_u, pc, _size, _data):
                    sp = u.reg_read(UC_X86_REG_ESP)
                    for row in pending[:]:
                        if pc == row['return_pc'] and sp > row['sp']:
                            row.update(returned_sp=sp, after=state())
                            pending.remove(row)
                    if pc in expected or pc in (0x7CBED3, 0x7C8FB0, 0x4CAC40, 0x4CAD50, 0x4CADE0, 0x7C5F00):
                        row = dict(pc=pc, sp=sp, return_pc=read(sp), before=state())
                        calls.append(row); pending.append(row)

                def memory(_u, access, address, size, value, _data):
                    accesses.append(dict(pc=u.reg_read(UC_X86_REG_EIP), access='read' if access==UC_MEM_READ else 'write',
                        address=address, size=size, value=value,
                        before=bytes(u.mem_read(address,size)).hex()))

                prior = bytes(u.mem_read(PRIOR_DRIVE, 0x70))
                self.drive_startup = dict(dispatcher=0x7CBED3, table_begin=begin, table_end=end,
                    table_bytes=original_table.hex(), entries=list(expected), before=state(),
                    inherited_prior_drive=dict(pointer=PRIOR_DRIVE, raw_before=prior.hex(),
                        origin='Separate supplied Unit/dock fixture: original4AF540 before selected startup; bridge globals104/416 were supplied there'),
                    calls=calls, accesses=accesses)
                hooks = [u.hook_add(UC_HOOK_CODE, code),
                    u.hook_add(UC_HOOK_MEM_READ, memory, begin=0x8A0758, end=0x8A07D3),
                    u.hook_add(UC_HOOK_MEM_WRITE, memory, begin=0x8A0758, end=0x8A07D3)]
                try:
                    bc.invoke(u, 0x7CBED3, 0, begin, end)
                    sp = u.reg_read(UC_X86_REG_ESP)
                    for row in pending[:]:
                        if row['return_pc'] == native.RET_MAGIC:
                            row.update(returned_sp=sp, after=state()); pending.remove(row)
                    self.drive_startup.update(after=state(), returned_sp=sp,
                        executed_entries=[r['pc'] for r in calls if r['pc'] in expected],
                        native_code_unchanged=self.code_unchanged(), pending_calls=pending,
                        inherited_prior_drive_raw_after=bytes(u.mem_read(PRIOR_DRIVE,0x70)).hex())
                    assert sp == bc.SP+4  # Original7CBEEC C3: cdecl caller owns args.
                    assert self.drive_startup['executed_entries'] == list(expected)
                    assert not pending and self.drive_startup['native_code_unchanged']
                    assert bytes(u.mem_read(PRIOR_DRIVE,0x70)) == prior
                    print('original selected Drive CRT height '+str(read(0x8A07D0))+
                        ', bridge '+str(read(0x8A07C4))+', FPCW '+hex(u.reg_read(UC_X86_REG_FPCW)), flush=True)
                except Exception as exc:
                    self.drive_startup.update(failed_state=state())
                    exc.original_drive_startup = self.drive_startup
                    raise
                finally:
                    for hook in hooks: u.hook_del(hook)
                    self.phase = 'setup'

        def string(self, pointer):
            data = bytearray()
            while (value := self.u.mem_read(pointer + len(data), 1)[0]):
                data.append(value)
            return data.decode('latin1')

        def hook(self, u, pc, size, data):
            if pc in JOINED_TRANSPORT:
                self.executed.add(pc)
                self.transport.phase = 'setup' if self.phase.startswith('setup') else self.phase
                self.transport.frame = self.read32(bc.FRAME)
                Mission.observe(self.transport, u, pc, size, data)
                return
            super().hook(u, pc, size, data)

        def building(self, name, pointer, radio_items, coord=(6, 9), health=None, *, constructor_only=False):
            u = self.u
            u.mem_write(pointer, bytes(0x800))
            self.phase = 'setup_full_building_constructor'
            before = self.rng()
            mark = len(self.draws)
            result = bc.invoke(u, 0x43B740, pointer, self.types[name], er.HOUSE)
            self.building_constructor = dict(entry='0x0043B740', pointer=pointer,
                result=result, native_id=self.read32(pointer + 0x10),
                rng_before=before, rng_after=self.rng(), requests=self.draws[mark:])
            if building_ai:
                self.building_constructor['logic_member_98'] = u.mem_read(pointer + 0x98, 1)[0]
                assert self.building_constructor['logic_member_98'] == 0
                self.building_constructor['abstract_flags'] = self.read32(pointer + 0x14)
            if damage_sound or constructor_only:
                # Original43B740/6F2B40 own +6C and43B78F owns +544=0.
                # No post-constructor Strength/health/UID/marked prior.
                self.building_constructor.update(type=name,
                    health=self.read32(pointer + 0x6C),
                    sampled_health=self.read32(pointer + 0x544))
                self.phase = 'setup'
                return
            # Main retains the actual whole43B740 constructor result. The
            # historical direct control deliberately keeps its partial prior.
            place_building(u, pointer, coord, constructor_abstract_flags=(
                self.building_constructor['abstract_flags'] if building_ai else None))
            u.mem_write(pointer + 0x520, dwords(self.types[name]))
            u.mem_write(pointer + 0x6C, dwords(self.read32(self.types[name] + 0xA0) if health is None else health))
            for off, value in ((0x90, 1), (0x74, 1), (0x81, 0), (0x98, 1)):
                u.mem_write(pointer + off, bytes([value]))
            u.mem_write(cell(*coord) + 0x12C, dwords(0x18))
            u.mem_write(pointer + 0x534, dwords(-1, -1))
            u.mem_write(pointer + 0x6E9, b'\1\1')
            u.mem_write(pointer + 0xE0, dwords(0x7E180C, radio_items, 1, 1))
            u.mem_write(radio_items, dwords(0))
            if building_ai:
                # The inherited admitted prior sets98 without a vector entry.
                # Original55BAA0 owns the actual Layer5519B0 insertion and98
                # write. Run it before the prior's damaged active Anim is born.
                u.mem_write(pointer + 0x98, b'\0')
                before = self.read32(0x87F788)
                self.phase = 'setup_building_logic_admission'
                result = bc.invoke(u, 0x55BAA0, 0x87F778, pointer, 0)
                after = self.read32(0x87F788)
                assert result & 255 == 1 and after == before + 1
                self.logic_registration = dict(entry='0x0055BAA0', before=before,
                    after=after, result=result, retained_98=u.mem_read(pointer + 0x98, 1)[0],
                    actor=pointer, native_id=self.read32(pointer + 0x10))

    f = Fixture()
    u, read = f.u, f.read32
    for address in (0x8B4120, 0x8874C0, 0x887568, 0xA83CE0, 0xB054D0):
        u.mem_write(address, bytes(inputs.u.mem_read(address, 24)))
    for entry in (0x4E6860, 0x4E6B60, 0x4E7BE0, 0x725550, 0x7254D0):
        bc.invoke(u, entry, 0)
    for table, count in ((0x8127F4, 13), (0x8129FC, 13), (0x812A40, 14), (0x813C2C, 13)):
        for entry in struct.unpack('<' + 'I' * count, u.mem_read(table, count * 4)):
            bc.invoke(u, entry, 0)
    for entry in LEVEL_HEIGHT_INITIALIZERS:
        bc.invoke(u, entry, 0)
    # Real process CRT producer for Cell::PlaceInfantryInCell's five retained
    # coordinates. Cold offsets are an invalid ordinary placement premise.
    # CRT entry812B28 points to48E480; no coordinates are supplied here.
    before = f.rng()
    before_offsets = bytes(u.mem_read(0x89E9F0, 60)).hex()
    assert read(0x812B28) == 0x48E480
    bc.invoke(u, 0x48E480, 0)
    f.subcell_startup = dict(entry='0x0048E480', crt_pointer='0x00812B28',
        original_crt_entry=read(0x812B28),
        before_hex=before_offsets, after_hex=bytes(u.mem_read(0x89E9F0, 60)).hex(),
        rng_before=before, rng_after=f.rng())
    assert before == f.rng()
    if damage_sound:
        # Original CRT entry813AA4 -> thunk561900 -> whole561910 owns
        # AreaDamage's369 signed CellSpread pairs at ABD490..ABDA50.
        # A cold BSS table repeats the centre cell. Keep legacy fixtures'
        # declared cold premise; this selected ordinary shot executes its
        # prerequisite instead of supplying offsets or deduplicating hits.
        f.phase = 'setup_selected_cell_spread_crt_startup'
        before = f.rng()
        draws, advances = len(f.draws), len(f.advances)
        before_table = bytes(u.mem_read(0xABD490, 369 * 4))
        crt_entry = read(0x813AA4)
        assert crt_entry == 0x561900
        returned = bc.invoke(u, 0x561910, 0)
        after_table = bytes(u.mem_read(0xABD490, 369 * 4))
        f.cell_spread_startup = dict(entry=0x561910, crt_pointer=0x813AA4,
            original_crt_entry=crt_entry, original_thunk=0x561900,
            table_address=0xABD490, pair_count=369, returned=returned,
            before_hex=before_table.hex(), after_hex=after_table.hex(),
            before_pairs=[list(pair) for pair in struct.iter_unpack('<hh', before_table)],
            after_pairs=[list(pair) for pair in struct.iter_unpack('<hh', after_table)],
            rng_before=before, rng_after=f.rng(),
            requests=f.draws[draws:].copy(), advances=f.advances[advances:].copy(),
            limits='Whole original initializer at the selected fixture startup; full CRT/process/Scenario admission remains outside this fixture.')
        f.phase = 'setup'
        # Execute the registered Tactical CRT group in its original table
        # order. In particular6D1BB0 derives B0CD48 from earlier original
        # initializers; the inherited presentation scale is not a golden.
        entries = struct.unpack('<20I', u.mem_read(0x814E58, 80))
        assert entries[-2:] == (0x6D1BB0, 0x6D1BF0)
        before, globals_before = f.rng(), bytes(u.mem_read(0xB0CD40, 0x120)).hex()
        for entry in entries:
            bc.invoke(u, entry, 0)
        f.tactical_projection_startup = dict(crt_table=0x814E58,
            entries=list(entries), before_hex=globals_before,
            after_hex=bytes(u.mem_read(0xB0CD40, 0x120)).hex(),
            z_scale_f64_bits=bytes(u.mem_read(0xB0CD48, 8)).hex(),
            rng_before=before, rng_after=f.rng())
    bc.invoke(u, FOUNDATION_INIT, 0)
    bc.invoke(u, 0x42E6F0, er.HOUSE + 0x5700)
    f.repair_target_prior(target_name='CAMISC03' if damage_sound else 'GAPOWR',
                          native_unlimbo=damage_sound)
    # The reused Engineer command fixture supplies UID1000 and a duplicate
    # lookup row. Retain the actual full-ctor assigned ID instead; the native
    # registry row already exists. This is declared admitted-state setup.
    if not damage_sound:
        original_id = f.building_constructor['native_id']
        assert read(er.BLD + 0x10) == 1000 and read(0xB0E844) >= 2
        lookup, count = read(0xB0E840), read(0xB0E844)
        assert list(struct.unpack('<2I', u.mem_read(lookup + (count - 1) * 8, 8))) == [1000, er.BLD]
        u.mem_write(er.BLD + 0x10, dwords(original_id))
        u.mem_write(0xB0E844, dwords(count - 1))
        u.mem_write(er.BLD + 0x6E8, b'\0')  # supplied ordinary prior: no paid repair active.
    bc.invoke(u, 0x455F10, er.BLD, 0)
    owner = f.second_house_prior(allied=False, native_building_counts=damage_sound)
    f.owner_houses = [er.HOUSE, owner]
    u.mem_write(owner + 0x1EC, b'\1')
    bc.invoke(u, 0x42E6F0, owner + 0x5700)
    bc.invoke(u, 0x455F10, er.BLD, 0)
    for house in f.owner_houses:
        for off in (0x188, 0x1A8):
            u.mem_write(house + off, struct.pack('<d', 1.0))
        for off in (0x5514, 0x5564):
            u.mem_write(house + off, dwords(0x7E449C, f.allocate(4096), 1024, 1, 1, 10))
    if not damage_sound:
        u.mem_write(er.BLD + 0x6C, dwords(health))
        u.mem_write(er.BLD + 0x544, dwords(health))
    u.mem_write(owner + 0x5778, b'\1')
    surface = f.allocate(0x40)
    u.mem_write(surface, dwords(0x7E2070, 640, 480, 0, 2, 0, 0, 0))
    u.mem_write(0x880A04, dwords(surface))
    u.mem_write(MAP + 0xEC, dwords(0, 0, 16, 16))
    u.mem_write(MAP + 0xFC, dwords(0, 0, 32, 32))
    plane = bytearray(b'\x07\x00\x00\x00' * (33 * 33))
    for y in range(1, 32):
        for x in range(1, 32):
            struct.pack_into('<BBH', plane, (y * 33 + x) * 4, 0, 0, 0)
    f.phase = 'setup_flat_connectivity'
    nav = Navigation.on_existing_map(f, size=(16, 16), class_height_plane=bytes(plane))
    nav.finish_graphs()
    Navigation.setup_pathfinder(nav.call)
    f.flat_map = dict(size=[16, 16], coordinate_bounds=[0, 0, 32, 32],
        class_height_plane_sha256=hashlib.sha256(plane).hexdigest(),
        prior='33x33 class/height samples; class0/level0 interior1..31, class7/level0 border; supplied flat clear Cell data; original connectivity and Pathfinder execute',
        native_zone_count=read(MAP + 0x4C))
    if building_ai and not damage_sound:
        # +12C is the already-revealed client view (original4876F0 ORs18).
        # Infantry raw occupation is independent at+124/+128; only native
        # admissions/marking/placement below may populate these words.
        f.flat_map['client_view_prior'] = dict(cell_flags_12c=0x18,
            native_projection_entry='0x004876F0', visible_reader='0x005656D0',
            shrouded_reader='0x00586360', raw_foot_ground_offset='0x124',
            raw_foot_deck_offset='0x128', raw_foot_initial=0,
            occupied_ground_list='Building2x2 foundation; later admitted MTNK and escaped E1 only')
    f.phase = 'setup_unit_constructor'
    f.source = f.allocate(0x1000)
    if drive_startup:
        f.source_arena = dict(pointer=f.source, size=0x1000,
            origin='Supplied fixture arena; native7353C0 constructor, not original operator-new allocation')
    before, start = f.rng(), len(f.draws)
    result = bc.invoke(u, 0x7353C0, f.source, f.types['MTNK'], er.HOUSE)
    f.unit_constructor = dict(entry='0x007353C0', pointer=f.source,
        native_id=read(f.source + 0x10), rng_before=before, rng_after=f.rng(), requests=f.draws[start:])
    if building_ai:
        f.unit_constructor['result'] = result
        f.unit_constructor['abstract_flags'] = read(f.source + 0x14)
        f.unit_constructor['passive_scan_timer'] = list(struct.unpack('<3i', u.mem_read(f.source + 0x180, 12)))
    coord = f.allocate(0x20)
    u.mem_write(coord, dwords(11 * 256 + 128, 8 * 256 + 128, 0))
    f.phase = 'setup_unit_unlimbo'
    before = f.rng()
    result = bc.invoke(u, 0x737BA0, f.source, coord, 128, timeout_us=60_000_000)
    assert result & 255 == 1
    if building_ai:
        f.unit_unlimbo = dict(entry='0x00737BA0', result=result,
            native_id=read(f.source + 0x10), coord=list(struct.unpack('<3i', u.mem_read(coord, 12))),
            facing=128, rng_before=before, rng_after=f.rng(),
            retained_logic_98=u.mem_read(f.source + 0x98, 1)[0],
            abstract_flags=read(f.source + 0x14),
            passive_scan_timer=list(struct.unpack('<3i', u.mem_read(f.source + 0x180, 12))))
    bc.invoke(u, 0x6FCDB0, f.source, er.BLD)
    if building_ai:
        f.unit_target_assignment = dict(entry='0x006FCDB0', target=read(f.source + 0x2B4),
            abstract_flags=read(f.source + 0x14),
            passive_scan_timer=list(struct.unpack('<3i', u.mem_read(f.source + 0x180, 12))))
    heading = f.allocate(4)
    bc.invoke(u, 0x5F3DB0, f.source, heading, er.BLD)
    for off in (0x388, 0x3A0):
        bc.invoke(u, 0x4C9300, f.source + off, heading)
    f.ready_facing = read(heading)
    if building_ai and not damage_sound:
        # The inherited target prior supplies marked74 and the ground list,
        # but does not execute BuildingMark's raw+124 producer. Reinsert the
        # already-admitted main target through the original owner: Mark0 ->
        # PlaceUp/RemoveContent, then Mark3 -> PlaceDown/AddContent/vtF0.
        # Building453D60 sets bit80; no occupation/path answer is assigned.
        def mark_state():
            return dict(marked=u.mem_read(er.BLD + 0x74, 1)[0],
                location=list(struct.unpack('<3i', u.mem_read(er.BLD + 0x9C, 12))),
                discovery_41a_41b_41c=list(u.mem_read(er.BLD + 0x41A, 3)),
                foundation={f'{x},{y}':dict(ground_head=read(cell(x, y) + 0xE4),
                    ground_raw=read(cell(x, y) + 0x124), deck_raw=read(cell(x, y) + 0x128))
                    for y in range(10, 12) for x in range(10, 12)}, rng=f.rng())

        before = mark_state()
        f.phase = 'setup_building_mark_remove'
        removed = bc.invoke(u, 0x43F180, er.BLD, 0)
        after_remove = mark_state()
        f.phase = 'setup_building_mark_put3'
        inserted = bc.invoke(u, 0x43F180, er.BLD, 3)
        after_put = mark_state()
        assert removed & 255 == 1 and inserted & 255 == 1
        assert before['rng'] == after_remove['rng'] == after_put['rng']
        f.building_mark_admission = dict(before=before,
            remove=dict(entry='0x0043F180', mode=0, result=removed, after=after_remove),
            put=dict(entry='0x0043F180', mode=3, result=inserted, after=after_put))
        f.flat_map['client_view_prior']['occupied_ground_list'] = (
            'original BuildingMark0/3 reinserts the admitted2x2 foundation; native MTNK and escaped E1 admissions')
        f.phase = 'setup'
    if hasattr(inputs, 'stock_smudge_inputs'):
        joined_stock_flat_map(f, inputs)
    return f


def joined_execute(inputs, *, projectile, seed=31, building_ai=False, damage_sound=False, post_drain_put=False):
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import UC_X86_REG_EDI, UC_X86_REG_EAX, UC_X86_REG_EIP
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc, engineer_repair_admission as er
    from tools.spatial_oracle.refinery_dock import cell
    from tools.spatial_oracle.building_body_rules import SP
    from tools.spatial_oracle.unit_source_scatter import SCENARIO
    f = joined_fixture(inputs, None if damage_sound else (38 if projectile else 374),
        seed=seed, building_ai=building_ai, damage_sound=damage_sound)
    audio = None
    if damage_sound:
        from tools.input_oracle.unit_voice_playback import VoiceHistory
        audio = VoiceHistory(inputs, 'CAMISC03_' + ('FireAt' if projectile else 'receiver'),
            fixture=f, actor_count=0, coordinate_profile=True)
    stock = joined_stock_observer(f) if hasattr(inputs, 'stock_smudge_inputs') else None
    u, read = f.u, f.read32
    watched = {0x741340: 'unit_fire', 0x6FDD50: 'techno_fire', 0x46B050: 'bullet_allocate',
        0x4664C0: 'bullet_initialize', 0x468670: 'bullet_fire', 0x4666E0: 'bullet_ai',
        0x4690B0: 'bullet_detonate', 0x489280: 'area_damage', 0x442230: 'building_receive_damage',
        0x702D40: 'record_the_kill', 0x4415F0: 'building_destroy_effects',
        0x442D90: 'spawn_survivors', 0x451330: 'survivor_count', 0x44EB10: 'building_crew_type',
        0x707D20: 'techno_crew_type', 0x517A50: 'crew_ctor', 0x51DFF0: 'crew_unlimbo',
        0x51D0D0: 'crew_scatter', 0x75AEC0: 'immediate_walk', 0x4D3920: 'foot_find_path',
        0x42C900: 'pathfinder_find', 0x5F65F0: 'object_uninit', 0x7258D0: 'expiry',
        0x725C70: 'drain', 0x43BCF0: 'building_destructor', 0x451E40: 'slot_destructor',
        0x426590: 'anim_scalar', 0x4255B0: 'anim_destroy', 0x421EA0: 'anim_ctor',
        0x55BAA0: 'logic_append', 0x55B610: 'logic_visit', 0x5025F0: 'remove_live',
        0x4FF550: 'remove_tracking', 0x4FF980: 'remove_owned', 0x43F180: 'building_mark',
        0x47E390: 'cell_remove', 0x508C30: 'house_power_assessment'}
    if building_ai:
        watched.update({0x43FB20: 'building_ai', 0x43C0D0: 'damage_fire_producer',
                        0x423AC0: 'anim_ai', 0x4228E0: 'anim_destructor'})
    ordered, crew, returns, checkpoints, raw_next, writes = [], [], [], [], [], []
    attacker_expiry, attacker_expiry_returns = [], []

    def signed(at):
        return struct.unpack('<i', u.mem_read(at, 4))[0]

    def actor(pointer):
        interface = read(pointer + 0x674)
        return dict(pointer=pointer, id=read(pointer + 0x10), type=read(pointer + 0x6C0),
            nominal=u.mem_read(pointer + 0x6D9, 1)[0], health=signed(pointer + 0x6C),
            position=list(struct.unpack('<3i', u.mem_read(pointer + 0x9C, 12))),
            alive=u.mem_read(pointer + 0x90, 1)[0], limbo=u.mem_read(pointer + 0x81, 1)[0],
            marked=u.mem_read(pointer + 0x74, 1)[0], doing=signed(pointer + 0x6C4),
            current_mission=signed(pointer + 0xAC), queued_mission=signed(pointer + 0xB4),
            mission_timer=list(struct.unpack('<3i', u.mem_read(pointer + 0xC8, 12))),
            destination=read(pointer + 0x5A4), path=list(struct.unpack('<24i', u.mem_read(pointer + 0x5E0, 96))),
            locomotor=None if not interface else dict(interface=interface,
                head=list(struct.unpack('<3i', u.mem_read(interface - 4 + 0x28, 12))),
                moving=u.mem_read(interface + 0x30, 1)[0]))

    def snapshot():
        state = dict(frame=read(bc.FRAME), unique_id_cursor=read(SCENARIO + 0x214),
            building_coord=list(struct.unpack('<3i', u.mem_read(er.BLD + 0x9C, 12))),
            attacker_coord=list(struct.unpack('<3i', u.mem_read(f.source + 0x9C, 12))),
            building=f.snapshot(er.BLD), health=signed(er.BLD + 0x6C), sampled_health=signed(er.BLD + 0x544),
            id=read(er.BLD + 0x10), alive=u.mem_read(er.BLD + 0x90, 1)[0],
            limbo=u.mem_read(er.BLD + 0x81, 1)[0], marked=u.mem_read(er.BLD + 0x74, 1)[0],
            damage_fire_slots=[read(er.BLD + 0x5C8 + i * 4) for i in range(8)],
            pending=[read(read(0xB0F69C) + i * 4) for i in range(read(0xB0F6A8))],
            escape_bracket=read(0xA8E7AC), crew=[actor(c) for c in crew],
            houses=[dict(pointer=h, building_members=[read(read(h + 0x6C) + i * 4) for i in range(read(h + 0x78))],
                tracked_buildings=read(h + 0x2F0), owned_buildings=read(read(h + 0x5504)),
                live_buildings=read(read(h + 0x5554)), power=signed(h + 0x53A4), drain=signed(h + 0x53A8),
                dirty=list(u.mem_read(h + 0x5778, 2)), score=read(h + 0x54E8), losses=read(h + 0x5488),
                last_killer_house_index=signed(h + 0x548C),
                kills_by_house=[read(h + 0x5438 + i * 4) for i in range(20)]) for h in f.owner_houses],
            ground={f'{x},{y}':dict(head=read(cell(x, y) + 0xE4), mask=read(cell(x, y) + 0xDC))
                    for y in range(7, 15) for x in range(7, 15)}, rng=f.rng())
        if building_ai:
            state.update(game_speed=read(0xA8EB60),
                building_abstract_flags=read(er.BLD + 0x14),
                building_discovery_41a_41b_41c=list(u.mem_read(er.BLD + 0x41A, 3)),
                damage_fire_active_5e8=u.mem_read(er.BLD + 0x5E8, 1)[0],
                attacker=dict(pointer=f.source, id=read(f.source + 0x10),
                    # Original Unit type getter741490 reads6C4;6C0 is its
                    # independent firing frame, unlike Infantry's type6C0.
                    type=read(f.source + 0x6C4), type_name=f.string(read(f.source + 0x6C4) + 0x24),
                    health=signed(f.source + 0x6C),
                    position=list(struct.unpack('<3i', u.mem_read(f.source + 0x9C, 12))),
                    alive=u.mem_read(f.source + 0x90, 1)[0], limbo=u.mem_read(f.source + 0x81, 1)[0],
                    marked=u.mem_read(f.source + 0x74, 1)[0], logic_member=u.mem_read(f.source + 0x98, 1)[0],
                    abstract_flags=read(f.source + 0x14),
                    body_facing_words=[struct.unpack('<H', u.mem_read(f.source + o, 2))[0]
                                       for o in (0x388, 0x38C)],
                    turret_facing_words=[struct.unpack('<H', u.mem_read(f.source + o, 2))[0]
                                         for o in (0x3A0, 0x3A4)],
                    body_facing_hex=bytes(u.mem_read(f.source + 0x388, 24)).hex(),
                    turret_facing_hex=bytes(u.mem_read(f.source + 0x3A0, 24)).hex(),
                    target=read(f.source + 0x2B4),
                    passive_scan_timer=list(struct.unpack('<3i', u.mem_read(f.source + 0x180, 12))),
                    current_mission=signed(f.source + 0xAC), queued_mission=signed(f.source + 0xB4),
                    mission_status=signed(f.source + 0xBC), mission_visit_count=read(f.source + 0xC4),
                    mission_timer=list(struct.unpack('<3i', u.mem_read(f.source + 0xC8, 12))),
                    rearm_timer=list(struct.unpack('<3i', u.mem_read(f.source + 0x2EC, 12))),
                    burst_index=signed(f.source + 0x3B8), ammo=signed(f.source + 0x2FC),
                    last_fire_frame=signed(f.source + 0x120), firing_frame=signed(f.source + 0x6C0)),
                raw_foot_occupation={f'{x},{y}':dict(ground=read(cell(x, y) + 0x124),
                    deck=read(cell(x, y) + 0x128), client_flags=read(cell(x, y) + 0x12C))
                    for y in range(7, 15) for x in range(7, 15)})
        return state

    def checkpoint(label):
        checkpoints.append(dict(label=label, sequence=len(ordered), state=snapshot()))

    tracked_returns = {0x741340, 0x4664C0, 0x468670, 0x442230, 0x702D40, 0x4415F0,
        0x442D90, 0x451330, 0x44EB10, 0x707D20, 0x517A50, 0x51DFF0, 0x51D0D0,
        0x75AEC0, 0x5F65F0, 0x43BCF0, 0x421EA0}
    state_returns = {0x442230, 0x442D90, 0x51DFF0, 0x51D0D0, 0x75AEC0, 0x5F65F0, 0x43BCF0}
    if building_ai:
        tracked_returns.update((0x43FB20, 0x43C0D0))
        state_returns.update((0x43C0D0,))

    def observe(u, pc, size, _):
        if building_ai:
            for pending in attacker_expiry_returns[:]:
                if pc == pending['pc']:
                    pending['row']['after_return'] = dict(
                        target=read(f.source + 0x2B4), abstract_flags=read(f.source + 0x14),
                        passive_scan_timer=list(struct.unpack('<3i', u.mem_read(f.source + 0x180, 12))),
                        rng=f.rng())
                    attacker_expiry_returns.remove(pending)
            if pc == 0x7077C0 and u.reg_read(UC_X86_REG_ECX) == f.source:
                sp = u.reg_read(UC_X86_REG_ESP)
                expired = read(sp + 4)
                row = dict(entry='0x007077C0', caller=f'0x{read(sp):08X}',
                    sequence=len(ordered), phase=f.phase, frame=read(bc.FRAME),
                    listener=f.source, expired=expired, expired_id=read(expired + 0x10),
                    expired_abstract_flags=read(expired + 0x14), control=read(sp + 8) & 255,
                    target_before=read(f.source + 0x2B4),
                    passive_scan_timer_before=list(struct.unpack('<3i', u.mem_read(f.source + 0x180, 12))),
                    escape_bracket=read(0xA8E7AC), rng_before=f.rng(), guards=[])
                attacker_expiry.append(row)
                attacker_expiry_returns.append(dict(pc=read(sp), row=row))
            if pc in (0x7079A1, 0x7079F0, 0x707A0D, 0x707A2E) and u.reg_read(UC_X86_REG_ESI) == f.source:
                assert attacker_expiry_returns
                # PUSH4/PUSH8 precede707A0D; use the actual native local frame.
                local_sp = u.reg_read(UC_X86_REG_ESP) + (8 if pc == 0x707A0D else 0)
                attacker_expiry_returns[-1]['row']['guards'].append(dict(
                    pc=f'0x{pc:08X}', allow_clear=u.mem_read(local_sp + 0x24, 1)[0],
                    control=u.mem_read(local_sp + 0x28, 1)[0],
                    eax_signed=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0],
                    escape_bracket=read(0xA8E7AC), target=read(f.source + 0x2B4),
                    passive_scan_timer=list(struct.unpack('<3i', u.mem_read(f.source + 0x180, 12))),
                    rng=f.rng()))
        for pending in returns[:]:
            if pc == pending['pc']:
                row = pending['row']
                row['result'] = u.reg_read(UC_X86_REG_EAX)
                if pending['entry'] in (0x421EA0, 0x517A50):
                    row['assigned_id'] = read(row['this'] + 0x10)
                    row['unique_id_cursor_after'] = read(SCENARIO + 0x214)
                if pending['entry'] == 0x421EA0:
                    row['constructed'] = dict(**f.anim_type_identity(read(row['this'] + 0xC8)),
                        **launch.constructor_state(u, row['this']))
                returns.remove(pending)
                if pending['entry'] in state_returns:
                    checkpoint('after_' + row['kind'])
        if pc == 0x65C79D:
            raw_next.append(dict(phase=f.phase, frame=read(bc.FRAME), this=u.reg_read(UC_X86_REG_ECX),
                                 raw=u.reg_read(UC_X86_REG_ESI)))
        if pc in (0x443147, 0x44321D, 0x443288, 0x44328E):
            checkpoint({0x443147:'escape_bracket_set', 0x44321D:'after_scatter_before_queue',
                        0x443288:'after_queue_before_bracket_clear', 0x44328E:'escape_bracket_cleared'}[pc])
        if pc not in watched:
            return
        this, sp = u.reg_read(UC_X86_REG_ECX), u.reg_read(UC_X86_REG_ESP)
        row = dict(sequence=len(ordered) + 1, phase=f.phase, frame=read(bc.FRAME),
                   pc=f'0x{pc:08X}', kind=watched[pc], this=this, caller=f'0x{read(sp):08X}',
                   escape_bracket=read(0xA8E7AC), unique_id_cursor=read(SCENARIO + 0x214))
        if pc in (0x421EA0, 0x517A50):
            pointer = read(sp + 4)
            row.update(type=pointer, type_name=f.string(pointer + 0x24))
            if pc == 0x421EA0:
                row.update(delay=read(sp + 12), loops=read(sp + 16), flags=read(sp + 20),
                           coordinate=list(struct.unpack('<3i', u.mem_read(read(sp + 8), 12))))
            else:
                crew.append(this)
                row['house'] = read(sp + 8)
        if pc == 0x55BAA0:
            pointer = read(sp + 4)
            row.update(actor=pointer, id=read(pointer + 0x10), logic_count_before=read(0x87F788))
        if pc == 0x55B610:
            row.update(actor_id=read(this + 0x10), index=u.reg_read(UC_X86_REG_ESI), count=read(0x87F788))
        if pc == 0x442230:
            row.update(raw_damage=signed(read(sp + 4)), distance=read(sp + 8), warhead=read(sp + 12),
                       source=read(sp + 16), attacking_house=read(sp + 28))
            checkpoint('before_building_receive_damage')
        if pc == 0x451E40:
            row['slot_argument'] = signed(sp + 4)
        if pc in tracked_returns:
            returns.append(dict(pc=read(sp), entry=pc, row=row))
        ordered.append(row)

    def write(u, access, address, size, value, _):
        if address == 0xA8E7AC:
            writes.append(dict(phase=f.phase, frame=read(bc.FRAME),
                pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}', value=value))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, write)

    def power_prefix():
        # Global Factory list is empty prior. The represented power gate ends
        # before Radar/SuperWeapon continuation4F84F1; whole HouseAI is excluded.
        for house in f.owner_houses:
            u.mem_write(SP, dwords(native.RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, house)
            native.run_checked(u, 0x4F8440, 0x4F84F1, count=2_000_000)

    f.phase = 'initial_house_power_prefix'
    power_prefix()
    if damage_sound:
        audio.invoke('whole_original_building_initial_health_sample', 0x43FB20, er.BLD)
        assert read(er.BLD + 0x6C) == read(er.BLD + 0x544) == 100
    result = dict(input=dict(route='mtnk_105mm_ap_fatal' if projectile else 'direct_ap_multi_hit_control',
        seed=seed, health=38 if projectile else 374, paid_repair=False, tag=None,
        building_owner='human Allied enemy House', ready_mtnk=bool(projectile),
        building_id_prior='retained full original constructor ID; Engineer command override omitted',
        building_coord=list(struct.unpack('<3i', u.mem_read(er.BLD + 0x9C, 12))),
        mtnk_coord=list(struct.unpack('<3i', u.mem_read(f.source + 0x9C, 12))),
        mtnk_ready_heading_from_original_getter=f.ready_facing, flat_map=f.flat_map),
        building_constructor=f.building_constructor, unit_constructor=f.unit_constructor,
        subcell_startup=f.subcell_startup,
        nominal_inputs=dict(building_6e9=u.mem_read(er.BLD + 0x6E9, 1)[0],
                            e1_type_c9e=u.mem_read(f.types['E1'] + 0xC9E, 1)[0]), before=snapshot())
    if building_ai:
        smudge_items, smudge_count = read(smudge_can_place.SMUDGE_ITEMS), read(smudge_can_place.SMUDGE_COUNT)
        result['input'].update(building_ai=True, game_speed=read(0xA8EB60),
            rng_seeds=dict(scenario=seed, main=seed, mapgen=31),
            smudge_registry_prior=dict(items_address=f'0x{smudge_can_place.SMUDGE_ITEMS:08X}',
                count_address=f'0x{smudge_can_place.SMUDGE_COUNT:08X}',
                items=smudge_items, count=smudge_count,
                types=[dict(pointer=read(smudge_items + i * 4),
                            name=f.string(read(smudge_items + i * 4) + 0x24)) for i in range(smudge_count)],
                selected_smudge_readers_executed=False,
                prior='inherited empty SmudgeType registry; physical SmudgeTypes reader/map-smudge startup excluded; original center roll and reached placers execute'))
        if damage_sound:
            result['input'].update(route='mtnk_105mm_ap_ordinary' if projectile else 'direct_ap_ordinary',
                health=100, admission='whole original440580, supplied admitted map/House inputs')
            result['building_native_admission'] = f.building_native_admission
            result['tactical_projection_startup'] = f.tactical_projection_startup
        else:
            result['building_mark_admission'] = f.building_mark_admission
            result['building_logic_registration'] = f.logic_registration
        result['unit_unlimbo'] = f.unit_unlimbo
        result['infantry_startup'] = f.infantry_startup
        result['unit_target_assignment'] = f.unit_target_assignment
        listener_items, listener_count = read(0xB0F724), read(0xB0F730)
        result['object_expiry_listener_prior'] = dict(address='0x00B0F720',
            items=listener_items, count=listener_count, capacity=read(0xB0F728), grow_by=read(0xB0F734),
            members=[dict(pointer=read(listener_items + i * 4),
                          native_id=read(read(listener_items + i * 4) + 0x10))
                     for i in range(listener_count)])
    f.events.clear();f.draws.clear();f.advances.clear();f.executed.clear();f.trace.clear()
    ordered.clear();checkpoints.clear();raw_next.clear();writes.clear()
    if projectile:
        f.phase = 'original_unit_fire'
        slot = (audio.invoke('whole_original_weapon_slot', 0x746CD0, f.source, er.BLD)
                if damage_sound else bc.invoke(u, 0x746CD0, f.source, er.BLD))
        error = (audio.invoke('whole_original_can_fire', 0x740FD0, f.source, er.BLD, slot, 1)
                 if damage_sound else bc.invoke(u, 0x740FD0, f.source, er.BLD, slot, 1))
        assert error == 0, error
        bullet = (audio.invoke('whole_original_unit_fire', 0x741340, f.source, er.BLD, slot)
                  if damage_sound else bc.invoke(u, 0x741340, f.source, er.BLD, slot))
        assert bullet
        result['shot'] = dict(slot=slot, fire_error=error, bullet=bullet,
            bullet_id=read(bullet + 0x10), raw_damage=signed(bullet + 0x6C),
            launch_position=list(struct.unpack('<3i', u.mem_read(bullet + 0x9C, 12))))
        result['after_launch'] = snapshot()
        frames = []
        for visit in range(30):
            f.phase = f'logic_frame_{read(bc.FRAME)}'
            u.mem_write(SP, dwords(native.RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EDI, 0x87F778)
            native.run_checked(u, 0x55B5FF, 0x55B61B, count=10_000_000)
            after_objects = snapshot()
            f.phase = 'house_power_prefix_after_objects'
            power_prefix()
            frames.append(dict(after_objects=after_objects, after_power=snapshot()))
            if not u.mem_read(bullet + 0x90, 1)[0]:
                break
            f.phase = 'main_frame_increment_and_deferred_drain'
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EDI, 0)
            native.run_checked(u, 0x55DE73, 0x55DE87)
            bc.invoke(u, 0x725C70, 0)
        else:
            raise AssertionError('No impact in30original dynamic Logic visits')
        if damage_sound:
            # Record the original outcome rather than supply a direct-receiver
            # answer to this separate FireAt/AreaDamage composition.
            result['native_impact_state'] = snapshot()
            # Preserve this independent FireAt outcome before the later PCM
            # transport phase, including on a bounded observer failure there.
            import json, sys
            print('NATIVE_FIRE_IMPACT ' + json.dumps(dict(shot=result['shot'],
                frame=result['native_impact_state']['frame'],
                health=result['native_impact_state']['health'],
                sampled_health=result['native_impact_state']['sampled_health'],
                alive=result['native_impact_state']['alive'],
                location=result['native_impact_state']['building_coord'],
                rng=result['native_impact_state']['rng'])), file=sys.stderr, flush=True)
        else:
            assert not u.mem_read(er.BLD + 0x90, 1)[0], 'represented shot was not fatal'
        result['frames'] = frames
    else:
        damage_pointer = f.allocate(4)
        result['receiver_visits'] = []
        for visit in range(1, 2 if damage_sound else 30):
            f.phase = f'direct_AP_receiver_{visit}'
            u.mem_write(damage_pointer, dwords(read(inputs.weapon + 0xA4)))
            before = snapshot()
            answer = (audio.invoke('whole_original_building_receiver', 0x442230,
                      er.BLD, damage_pointer, 0, inputs.warhead, f.source, 0, 0, 0)
                      if damage_sound else bc.invoke(u, 0x442230, er.BLD,
                      damage_pointer, 0, inputs.warhead, 0, 0, 0, 0))
            after_receiver = snapshot()
            f.phase = 'direct_control_health_sample_and_power'
            if damage_sound:
                audio.invoke('whole_original_building_post_damage_health_sample', 0x43FB20, er.BLD)
            else:
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_ESI, er.BLD)
                native.run_checked(u, 0x440042, 0x440072)
            power_prefix()
            result['receiver_visits'].append(dict(before=before, result=answer,
                after_receiver=after_receiver, after_sample_and_power=snapshot()))
            if not u.mem_read(er.BLD + 0x90, 1)[0]:
                break
        else:
            if damage_sound:
                assert answer == 1 and read(er.BLD + 0x6C) == 62
            else:
                raise AssertionError('No fatal result in30direct stock AP controls')
    if stock is not None:
        result['input']['smudge_registry_prior'].update(
            selected_smudge_readers_executed=True,
            prior='Original stock Rules/ART SmudgeType constructors/readers; original selected TEMPERAT ClearTile Morphable reader/store on an admitted flat clear map')
    result['before_drain'] = snapshot()
    f.phase = 'main_frame_increment'
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EDI, 0)
    native.run_checked(u, 0x55DE73, 0x55DE87)
    result['after_frame_increment'] = snapshot()
    f.phase = 'deferred_drain'
    bc.invoke(u, 0x725C70, 0)
    result['after_drain'] = snapshot()
    assert f.code_unchanged()
    if building_ai:
        assert not attacker_expiry_returns
        result['attacker_pointer_expiry'] = attacker_expiry
    result.update(ordered_native_calls=ordered, checkpoints=checkpoints,
        escape_bracket_writes=writes, rng_requests=f.draws, ranged_raw_advances=f.advances,
        native_next_raw_advances=raw_next, events=f.events, state_writes_and_native_trace=f.trace,
        platform_transports=f.transport.events,
        executed=[f'0x{pc:08X}' for pc in sorted(f.executed)],
        original_executable_sections_unchanged=True)
    if stock is not None:
        assert not stock['pending']
        result['stock_runtime'] = dict(setup=f.stock_smudge_setup,
            calls=stock['calls'],cell_writes=stock['writes'],
            executed=[f'0x{pc:08X}' for pc in sorted(stock['pcs'])],
            final=stock['snapshot']())
    if damage_sound:
        result['cell_spread_startup'] = f.cell_spread_startup
        result['coordinate_audio_lifecycle'] = audio.natural_coordinate_end()
        result['damage_sound_audio'] = audio.project()
    if post_drain_put:
        import copy
        assert stock is not None and projectile and building_ai and seed == 1
        # Observers retain mutable lists. Preserve every existing fatal/drain
        # field before continuing this same native world with another object.
        result = copy.deepcopy(result)
        result['post_drain_put'] = post_drain_smudge_put(f, stock)
    return result


def post_drain_smudge_put(f, stock):
    """Fresh original GAPOWR admission after the real stock Smudge drain.

    This optional continuation shares the existing Runtime/ctor/House/map; it
    never revives the destroyed target or overwrites fresh constructor fields.
    """
    import copy
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_ESI
    from tools.spatial_oracle import building_construction as bc
    from tools.spatial_oracle.refinery_dock import cell
    from tools.spatial_oracle.unit_source_scatter import SCENARIO
    from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry, DiscoveryFixture
    from types import SimpleNamespace
    u, read = f.u, f.read32
    before = stock['snapshot']()
    player = read(0xA83D4C)
    proxy = SimpleNamespace(uc=u, read32=read)
    client_priors = dict(player_ptr=player,game_mode=read(0xA8B238),
        scenario_init=read(0xA8E7AC),active_game=u.mem_read(0xA8E9A0,1)[0],
        player_flags_1ec_1ed=list(u.mem_read(player+0x1EC,2)),
        cells=[DiscoveryFixture.pointer_state(proxy,cell(x,y),include_dirty=True)
               for x,y in ((10,10),(11,10),(10,11),(11,11))])
    assert before['pending_count'] == before['smudge_object_count'] == 0
    assert [(r['cell'],r['type_index'],r['data']) for r in before['smudge_cells']] == [
        ([10,10],44,0),([11,10],44,1),([10,11],44,2),([11,11],44,3)]
    calls, writes, timer_writes, discovery_calls = [], [], [], []
    request_start, advance_start = len(f.draws), len(f.advances)
    event_start = len(f.events)
    fresh = f.allocate(0x800)
    position = f.allocate(12)
    selected_cells = {cell(x,y):(x,y) for y in range(9,13) for x in range(9,13)}
    entries = {0x43B740:'building_ctor',0x440580:'building_unlimbo',
        0x43F180:'building_mark',0x43F8A6:'clear_smudge_type',
        0x4852D0:'cell_redraw_rect',0x486E70:'cell_dirty_ring',0x6DA7D0:'tactical_dirty',
        0x5F65F0:'uninit',0x7258D0:'expiry',0x725C70:'deferred_drain'}
    def actor():
        return dict(pointer=fresh,id=read(fresh+0x10),health=read(fresh+0x6C),
            sampled_health=read(fresh+0x544),alive=u.mem_read(fresh+0x90,1)[0],
            limbo=u.mem_read(fresh+0x81,1)[0],marked=u.mem_read(fresh+0x74,1)[0],
            logic=u.mem_read(fresh+0x98,1)[0],placed=u.mem_read(fresh+0x6E4,1)[0],
            anim_slots=[read(fresh+0x55C+i*4) for i in range(21)],abstract_flags=read(fresh+0x14),
            owner=read(fresh+0x21C),discovery_41a_41b_41c=list(u.mem_read(fresh+0x41A,3)),
            location=list(struct.unpack('<3i',u.mem_read(fresh+0x9C,12))),
            mission=list(struct.unpack('<4i',u.mem_read(fresh+0xAC,16))),
            mission_timer=list(struct.unpack('<3i',u.mem_read(fresh+0xC8,12))),
            stage_timer=list(struct.unpack('<5i',u.mem_read(fresh+0xFC,20))))
    def state():
        # Existing shared snapshot owns AnimType identity and native runtime fields.
        presentation = f.snapshot(fresh)
        return dict(stock=stock['snapshot'](),building=actor(),anims=presentation['anims'],
            unique_id_cursor=read(SCENARIO+0x214),
            attacker_passive_scan_timer=list(struct.unpack('<3i',u.mem_read(f.source+0x180,12))))
    discovery_entries = {0x44D5D0:'building_discovered',0x445F80:'grand_opening',
        0x70AF50:'update_reveal',0x5B35E0:'queue_mission',0x5B3570:'commence'}
    discovery_sites = {0x6F6E2A:'before_idle',0x6F6E4F:'after_idle_commence',
        0x440D61:'unlimbo_explicit_discovery'}
    def code(_u,pc,_size,_data):
        sp=u.reg_read(UC_X86_REG_ESP);this=u.reg_read(UC_X86_REG_ECX)
        if pc in discovery_entries and this==fresh:
            row=dict(entry=f'{pc:08X}',kind=discovery_entries[pc],
                caller=f'{read(sp):08X}',building_before=actor())
            if pc in (0x44D5D0,0x5B35E0):row['arg0']=read(sp+4)
            discovery_calls.append(row)
        elif pc in discovery_sites and u.reg_read(UC_X86_REG_ESI)==fresh:
            discovery_calls.append(dict(site=f'{pc:08X}',kind=discovery_sites[pc],building_before=actor()))
        if pc not in entries:return
        row=dict(entry=f'{pc:08X}',kind=entries[pc],phase=f.phase)
        if pc != 0x43F8A6:
            row.update(this=this,caller=f'{read(sp):08X}')
        if pc==0x43F180:row['mode']=read(sp+4)
        if pc in (0x4852D0,0x486E70) and this in selected_cells:row['cell']=list(selected_cells[this])
        if pc==0x43F8A6:
            pointer=u.reg_read(UC_X86_REG_EAX)
            row.update(cell=list(struct.unpack('<hh',u.mem_read(pointer+0x24,4))),
                before_type=struct.unpack('<i',u.mem_read(pointer+0x48,4))[0],
                before_data=u.mem_read(pointer+0x11F,1)[0])
        if pc in (0x440580,0x7258D0):row['arguments']=list(struct.unpack('<2I',u.mem_read(sp+4,8)))
        calls.append(row)
    def memory(_u,_access,address,size,value,_data):
        row=dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08X}',phase=f.phase,address=address,
                 size=size,value=value,before=bytes(u.mem_read(address,size)).hex())
        for pointer,xy in selected_cells.items():
            if pointer+0x48<=address<pointer+0x4C or address==pointer+0x11F:
                row.update(cell=list(xy),offset=address-pointer);writes.append(row);return
        if (fresh+0xC8<=address<fresh+0xD4 or fresh+0xFC<=address<fresh+0x110
                or f.source+0x180<=address<f.source+0x18C):
            timer_writes.append(row)
    hooks=[u.hook_add(UC_HOOK_CODE,code),u.hook_add(UC_HOOK_MEM_WRITE,memory)]
    try:
        f.phase='post_drain_put_original_geometry_startup'
        geometry=initialize_discovery_geometry(lambda a,obj,args:bc.invoke(u,a,obj,*args),read,u)
        f.building('GAPOWR',fresh,f.allocate(4),constructor_only=True)
        constructor=copy.deepcopy(f.building_constructor)
        after_constructor=state()
        f.phase='post_drain_put_original_cell_coordinates'
        # Actual Cell GetCoords486840, also used by original waypoint68BD00.
        result=bc.invoke(u,0x486840,cell(10,10),position)
        coordinates=dict(entry='00486840',receiver=cell(10,10),result=result,
                         xyz=list(struct.unpack('<3i',u.mem_read(position,12))))
        f.phase='post_drain_put_original_building_unlimbo'
        result=bc.invoke(u,0x440580,fresh,position,0)
        after_unlimbo=state()
        assert result&255==1,after_unlimbo
        assert any(r['kind']=='clear_smudge_type' for r in calls)
        assert not after_unlimbo['stock']['smudge_cells']
        assert all(u.mem_read(cell(x,y)+0x11F,1)[0]==data
                   for x,y,data in ((10,10,0),(11,10,1),(10,11,2),(11,11,3)))
        assert f.code_unchanged()
    finally:
        for hook in hooks:u.hook_del(hook)
    owner = after_constructor['building']['owner']
    house_type = read(owner+0x34)
    return dict(input=dict(type_name='GAPOWR',coords=[10,10],facing=0,
            owner=dict(pointer=owner,house_type_pointer=house_type,house_type_name=f.string(house_type+0x24)),
            boundary='Existing supplied Runtime House; fresh original constructor receives this owner. XYZ comes from original Cell GetCoords, not a supplied height.'),
        before=before,client_priors=client_priors,discovery_calls=discovery_calls,
        geometry_startup=geometry,source_coordinates=coordinates,
        constructor=constructor,after_constructor=after_constructor,
        unlimbo=dict(entry='00440580',receiver=fresh,facing=0,result=result),
        after_unlimbo=after_unlimbo,calls=calls,cell_writes=writes,timer_writes=timer_writes,
        construction_events=copy.deepcopy([e for e in f.events[event_start:] if e['event'] in
            ('grand_opening','slot_allocate','anim_ctor','anim_start','unlimbo','logic_add','layer_submit','mark')]),
        rng_requests=copy.deepcopy(f.draws[request_start:]),raw_advances=copy.deepcopy(f.advances[advance_start:]),
        persistent_data_after=[dict(cell=[x,y],data=u.mem_read(cell(x,y)+0x11F,1)[0])
                               for x,y in ((10,10),(11,10),(10,11),(11,11))],
        limits='Same declared already-loaded flat Runtime/House/map after actual drain. Fresh constructor and whole Unlimbo run unchanged, with original Cell coordinates and successful native AL1. No paid production/input or full native world bootstrap. Existing downstream presentation/platform transport boundaries remain.')


def joined_generate(*, stock_smudges=False):
    import hashlib, json, os
    root = Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    # One reader owner, two explicit configurations. Full FIRE inputs and
    # whole target AI are required by the ordinary shot. The historical
    # receiver control keeps its original input/allocator/type identities.
    m = joined_inputs(root, damage_fires=True, stock_smudges=stock_smudges)
    direct = joined_inputs(root)
    receipts_path = root / 'receipts.json'
    receipts = json.loads(receipts_path.read_text()) if receipts_path.is_file() else []
    files = [dict(name=p.name, size=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
             for p in sorted(root.iterdir()) if p.is_file() and p.suffix.lower() != '.json']
    stock_context = (dict(native_stock_smudge_inputs=m.stock_smudge_inputs,
        native_stock_theater_inputs=m.stock_theater_inputs) if stock_smudges else {})
    return dict(schema_version=1, source='unicorn/gamemd.exe',
        **stock_context, retail_files=files,
        extraction_receipts=[{k:v for k,v in row.items() if k in
            ('asset','source_archive','entry_id','size','sha256','catalog_lookup_only')} for row in receipts],
        construction_layers=m.layers, engineer_layers=m.engineer_layers, death_layers=m.death_layers,
        inherited_scenario_section_projection=m.death_scenario_projection,
        native_country_sides=m.death_country_sides, selected_sounds=m.death_sounds,
        physical_art_input_sections=m.death_art_input_sections,
        native_fatal_anim_inputs=m.death_anim_rows, native_input_execution=m.death_input_execution,
        native_code_sections=m.code_identity,
        direct_receiver_input_context=dict(selected_sounds=direct.death_sounds,
            physical_art_input_sections=direct.death_art_input_sections,
            native_fatal_anim_inputs=direct.death_anim_rows,
            native_country_sides=direct.death_country_sides,
            death_layers=direct.death_layers, native_input_execution=direct.death_input_execution,
            native_code_sections=direct.code_identity),
        routes=([joined_execute(m, projectile=True, seed=seed, building_ai=True, post_drain_put=seed == 1)
                 for seed in (2,1,31)] if stock_smudges else
                [joined_execute(m, projectile=True, seed=2, building_ai=True),
                 joined_execute(direct, projectile=False),
                 joined_execute(m, projectile=True, seed=1, building_ai=True),
                 joined_execute(m, projectile=True, seed=31, building_ai=True)]))


def joined_metadata():
    return provenance(scope='One ordinary untagged human Allied GAPOWR38HP fatal from real ready MTNK/105mm/AP launch, whole target Building AI and stock damage fires, Bullet/area damage and nonnull killer, real debris/foundation/one E1 escape through dynamic Logic, sampled power and deferred destruction; independent direct374HP AP control',
        entry_points=dict(unit_type_ctor=0x7470D0, unit_type_reader=0x747620,
            infantry_reader=0x5240A0, building_reader=0x45FE50, weapon_reader=0x772080,
            projectile_reader=0x46BEE0, warhead_reader=0x75D3A0, anim_reader=0x427D00,
            elevation_reader=0x66D150, country_sides_reader=0x672440,
            building_ctor=0x43B740, house_base_ctor=0x42E6F0, reservation_mark=0x455F10,
            building_mark=0x43F180, map_place_down=0x5683C0, map_place_up=0x5687F0,
            cell_add_content=0x47E8A0, cell_remove_content=0x47EA90,
            building_set_raw_occupation=0x453D60, building_clear_raw_occupation=0x453DC0,
            unit_ctor=0x7353C0, unit_unlimbo=0x737BA0, set_target=0x6FCDB0,
            get_direction=0x5F3DB0, set_facing=0x4C9300, unit_fire=0x741340,
            object_ctor_flags=0x5F3B37, techno_ctor_flags=0x6F322F,
            object_expiry_flag_guard=0x72592D, unit_pointer_expired=0x7446E0,
            foot_pointer_expired=0x4D9960, techno_pointer_expired=0x7077C0,
            target_equality_guard=0x7079A1, passive_timer_guard=0x7079D1,
            passive_priority_guard=0x7079F5, passive_timer_rng=0x707A0D,
            bullet_factory=0x6C5090, bullet_initialize=0x4664C0, bullet_fire=0x468670,
            bullet_ai=0x4666E0, bullet_detonate=0x4690B0, area_damage=0x489280,
            building_receive_damage=0x442230, record_the_kill=0x702D40,
            destruction_effects=0x4415F0, spawn_survivors=0x442D90, crew_ctor=0x517A50,
            crew_unlimbo=0x51DFF0, scatter=0x51D0D0, immediate_walk=0x75AEC0,
            escape_increment=0x443141, escape_decrement=0x443288,
            pathfinder_ctor=0x42A6D0, pathfinder_resize=0x42AC00, pathfinder_find=0x42C900,
            logic_dynamic_loop=0x55B5FF, logic_append=0x55BAA0, frame_increment=0x55DE73,
            building_ai=0x43FB20, damage_fire_producer=0x43C0D0,
            anim_ai=0x423AC0, anim_destructor=0x4228E0,
            cell_reveal_shroud_flags=0x4876F0, map_is_visible=0x5656D0,
            house_power_prefix=0x4F8440, house_assessment=0x508C30, sample_health=0x440042,
            uninit=0x5F65F0, expiry=0x7258D0, deferred_drain=0x725C70,
            building_destructor=0x43BCF0, ordinary_slot_destructor=0x451E40,
            damage_fire_destroy=0x4255B0, unique_id_producer=0x68BCB0,
            subcell_startup=0x48E480, subcell_startup_crt_pointer=0x812B28,
            infantry_startup_table=0x813490, infantry_startup_first=0x517840,
            infantry_startup_last=0x5179B0, infantry_base_height_producer=0x517910,
            infantry_bridge_threshold_producer=0x5179B0,
            infantry_set_raw_occupation=0x5217C0, infantry_clear_raw_occupation=0x521850),
        assumptions=[
            'Exact lexical selected sections enter existing original signed-CRC caches. Complete original selected BuildingType/InfantryType/UnitType/weapon/projectile/warhead/AnimType/country/Sides/General/ElevationModel/Radiation readers and stated surviving key blocks execute in RULESMD, optional LANGRULE, Battle mode, Hills scenario order; whole Rules::Process and physical INI cache construction are excluded. The actual Hills.map MIX entry is read; every selected inherited Construction/Engineer scenario section is compared with their Hills.mmx lexical source and is identical.',
            'Layer physical_input_sections and physical_art_input_sections retain the exact selected physical reader maps, including General/CombatDamage/AudioVisual and GI/Sequence. Separate declared_e1_weapon_sections retain physically authored E1 referenced weapons and their projectile/warhead inputs only for the Rust fixture production readers; these weapon readers/firing paths were not executed by this native comparison and are not admission or parity evidence.',
            'Actual physical SHP/INI/audioIDX/BAG bytes and catalog receipts are retained externally; corpus contains identities/readbacks, no retail byte payload. Original selected asset requests/frame headers are recorded. MTNK VXL loading and final drawing are presentation transport; no rendered parity claim.',
            'Full Building43B740 and Unit7353C0 constructors execute. Existing admitted Building/Foundation/House/Cell/DrawingLayer priors are supplied after Building construction; actual embedded Base42E6F0/MarkReservation455F10 produce reservation state. Original Logic55BAA0 admits the main target before its initial damaged active Anim, preserving Building-before-Anim-before-MTNK order. Its constructor98 default and retained admission98 state are recorded. The main passes the actually constructed AbstractFlags+14 into the shared supplied-placement helper, retaining original Object5F3B37 OR2 and Techno6F322F OR1 instead of its historical partial flag1. Native72592D..38 dispatches the actual registered listeners; original Unit7446E0/Foot4D9960/Techno7077C0 clears its current target and conditionally shortens its180/188 timer. The supplied128-capacity generic Object vector is retained; whole native constructors own actual registration, not raw listener rows. Original7252D0 startup was a disproved research lead and is not added. The Engineer command UID1000 override is omitted in favor of the original constructor ID and its pre-existing native registry row. Other whole House construction/admission, production, vision and earlier volleys are outside this comparison.',
            'The ordinary projectile case supplies a previously damaged GAPOWR38HP and sampled38, no paid repair, two human enemy Allied House priors, no Tags/factories. Full original UnitUnlimbo and SetTarget execute; original getter/facing setter establish an already ready/faced shooter. Earlier target acquisition, physical approach, turret/body turning and weapon cadence over repeated volleys are excluded.',
            'The declared flat admitted32x32 map has clear supplied Cell data and a33x33 class/height plane with blocked7 outer border and class0/level0 interior. Existing Navigation runs original connected zones/hierarchy and original Pathfinder ctor/resize/workspace/find in the same VM; no cell-entry, Scatter, path, connectivity or graph answer is synthesized. Cell+12C18 is the declared already-revealed client view, the same flag projection original4876F0 produces; it is not raw occupation. Independent raw Foot words+124/+128 start empty. The main reinserts the inherited admitted Building through original43F180 Mark0 then Mark3 after the ready attacker prior, executing PlaceUp/PlaceDown/RemoveContent/AddContent and Building453D60 raw bit80 writes across its actual foundation. Before/remove/put occupation, discovery41A/B/C and all three RNG states are recorded. Native MTNK/E1 admissions populate their own occupation; no raw bit or path answer is supplied. The historical direct receiver control retains its supplied raw-zero Building prior and excludes normal Mark admission and ordinary crew-path equivalence.',
            'GameOptions+A8EB60 is supplied3 by the reused Construction fixture and read back in main input/snapshots; complete options loading is excluded. The main input records the actual inherited SmudgeType itemsA8EC1C/countA8EC28 and selected names, which remain0/0/empty. Selected physical SmudgeType constructors/readers and physical map-smudge startup are not executed. Original center0..99 roll and reached Burn/Crater placers execute against that empty registry; real retail type/placement coverage remains the separate preserved primitive physical-smudge controls, not a full joined smudge admission claim.',
            'Original process CRT producer48E480, reached through table pointer812B28 before WinMain, initializes all five retained Cell::PlaceInfantryInCell coordinates at89E9F0. Before/after bytes and unchanged full three RNG streams are recorded; no subcell coordinate answer is supplied. Earlier research captures with cold offsets are superseded.',
            'Only the three ordinary main shot routes execute the existing infantry_entry_raw.STARTUP owner: all ten original table813490 entries run in table order before any joined Foot constructor or gameplay. Original517910/5179B0 produce A8F240104/A8F234416 from cold0/0; actual entry/full-PC visits, scalar readbacks and unchanged full Scenario/Main/MapGen states are recorded in infantry_startup. No scalar or raw bitmap answer is supplied. Original5217C0 and521850 consequently select the normal ground plane at flatZ0, including the successful immediate Walk clear while A8E7AC remains1. Earlier ordinary-main captures with cold Infantry thresholds are superseded. The historical direct receiver route deliberately omits this startup and retains A8F2400/A8F2340; its ground PUT/deck CLEAR and retained current bit are cold-threshold isolation evidence only, not ordinary crew occupation equivalence.',
            'Real UnitFireAt741340, Bullet factory/initialize/fire/AI/detonate, AreaDamage489280 and whole Building ReceiveDamage execute. The dynamic original55B5FF..55B61B Logic loop re-reads append/removal count and executes actual live objects; death Anims/escaped E1 and reachable projectile aftermath therefore receive their actual same-frame visits. Only reached collateral calls are recorded.',
            'The main target receives whole original43FB20 Building AI visits through the actual dynamic Logic loop before impact. General DamageFireTypes and physical FIRE01/02/03 ART/SHP/StartSound readers execute. Original43C0D0 produces the reached fires; whole native Anim updates, fatal4255B0 UnInit/expiry and later scalar/destructor cleanup execute. The historical no-target-AI main capture is superseded and retained externally; it is not the accepted ordinary main path. An independent reader configuration keeps the direct receiver control and its recorded type/sound/pointer identities unchanged; that context is retained in direct_receiver_input_context.',
            'Whole fatal, survivors, UnInit/expiry and deferred725C70/Building destructor execute. Ordinary21slot pointers, eight damage-fire5C8 pointers, attached ownerCC, native identities/cursor, House live/owned/tracking/statistics, ground occupancy, crew NavCom/path/Doing/Walk state are sampled at before-fatal, after-UnInit, before/after-frame-increment and drain boundaries. Crew mission_timer is the raw three-dword native CdTimer atC8: only startC8 and durationD0 are timer values; middleCC is retained uninterpreted storage. Main attacker passive_scan_timer retains raw180/184/188 at every boundary and ctor/Unlimbo/SetTarget receipts; only180 start and188 duration are logical timer fields,184 remains opaque stack-derived storage. Attacker pointer-expiry entry/return and real guard observations retain target equality, control, native timer/RNG states and A8E7AC; its timer draw precedes death sound/debris.',
            'House4F8440..4F84F1 executes the real dirty-gated power prefix after each original object visit, in supplied House order, with empty Factory prior. Radar/SuperWeapon continuation and rest of HouseAI/other global phases are excluded. Main frame increment55DE73..55DE87 executes before direct native725C70, as55DE9F caller;55E160 and final presentation/global phases are excluded.',
            'The direct AP control starts374HP with stock65 damage/nullSource/nullAttackingHouse, calls whole receiver followed by explicit original health sampling and House prefix. Its historical partial AbstractFlags1 intentionally excludes the generic Object expiry callback arm; it is not ordinary full-constructor target-expiry evidence. It independently bounds health/slot/power/cleanup and is not ordinary weapon timing or kill-credit evidence. Its historical manual Building ground-list/marked prior leaves raw Cell+124 zero, with ordinary Mark0/3 admission deliberately excluded; its retained crew path is only that isolation-control outcome.',
            'Scenario/Main/MapGen full0x3F4 states, each Next/Ranged request before/after/result, ranged raw/mask/rejection and separate rawNext advance are retained. Original seeder65C6D0 supplies Scenario/Main seed2 and MapGen seed31 for the one-E1 main coverage control. Two additional identical-prior shot controls retain native seed1/31 crew refusals after correct pointer expiry; the direct control remains seed31 in all three streams. Setup constructor draws are separate. PC53/chop FPCW0E7F and every original executable PE section are checked; no original code instruction is patched. Stock E1+C9E0 leaves copied Nominal branch inactive; CTECH full civilian lifetime remains excluded.'
        ],
        substitutions=[
            'Existing Engineer/Construction reader-cache, physical requested-file byte IO, bump allocator/delete, COM/CRT TLS, original audioIDX/qsort/BAG/backend device transport boundaries are reused. Native Drive/Bullet/Walk COM factories execute rather than fabricated locomotor/projectile responses.',
            'Only listed existing Mission OS/COM/presentation transport arms are delegated. Supplied640x480surface prior executes original vtable rectangle getter; original sound registry/readers/event allocation execute, hardware sample output is not audible parity. Existing UpdateAnimation presentation callbacks are retained; no selected gameplay body or numeric decision is replaced.'
        ])


def joined_source_paths():
    root = Path(__file__).resolve().parents[2]
    sources = ('tools/spatial_oracle/building_death_anims.py', 'tools/native_oracle.py',
        'tools/spatial_oracle/engineer_repair_admission.py', 'tools/spatial_oracle/building_construction.py',
        'tools/spatial_oracle/anytown_damage/mission.py', 'tools/spatial_oracle/anytown_damage/navigation.py',
        'tools/spatial_oracle/shrapnel_repair/zone_composition.py', 'tools/spatial_oracle/fire_error.py',
        'tools/spatial_oracle/infantry_entry_raw.py',
        'tools/spatial_oracle/refinery_dock.py', 'tools/spatial_oracle/anim_bouncer_launch.py',
        'tools/spatial_oracle/track_destination.py', 'tools/spatial_oracle/unit_entry.py',
        'tools/spatial_oracle/unit_scatter_state.py', 'tools/spatial_oracle/unit_source_scatter.py',
        'tools/spatial_oracle/building_body_rules.py', 'tools/spatial_oracle/map_queries.py',
        'tools/spatial_oracle/building_slot_replacement.py',
        'tools/rules_oracle/bridge_anim_inputs.py', 'tools/rules_oracle/bridge_child_sound.py',
        'tools/rules_oracle/bridge_anim_lists.py', 'tools/projectile_oracle/flat_art.py')
    return {name:root / name for name in sources}


def joined_main(argv):
    from tools.native_oracle import finish_vectors
    finish_vectors(joined_generate, Path(__file__).with_name('building_death_anims_joined.json'),
                   provenance=joined_metadata, argv=argv,source_paths=joined_source_paths())


def damage_sound_object_identity(f, pointer):
    """Raw identity inputs shared by this producer's whole receiver controls."""
    owner = f.read32(pointer + 0x21C)
    house_type = f.read32(owner + 0x34)
    return dict(pointer=pointer, native_id=f.read32(pointer + 0x10),
        location=list(struct.unpack('<3i', f.u.mem_read(pointer + 0x9C, 12))),
        owner=owner, house_type_pointer=house_type, house_type_name=f.string(house_type + 0x24))


def damage_sound_receiver_controls(inputs):
    """Whole receivers with explicit packets, retaining native health history."""
    from tools.input_oracle.unit_voice_playback import VoiceHistory
    from tools.spatial_oracle import engineer_repair_admission as er
    f = joined_fixture(inputs, None, building_ai=True, damage_sound=True)
    h = VoiceHistory(inputs, 'CAMISC03_packet_admission_controls', fixture=f,
                     actor_count=0, coordinate_profile=True)
    h.invoke('whole_original_initial_health_sample', 0x43FB20, er.BLD)
    projection = h.projection_controls()
    packet = f.allocate(4)
    result = dict(input=dict(target='CAMISC03', strength=f.read32(f.types['CAMISC03'] + 0xA0),
        can_c4=f.u.mem_read(f.types['CAMISC03'] + 0x1577, 1)[0],
        per_type_id=struct.unpack('<i', f.u.mem_read(f.types['CAMISC03'] + 0x538, 4))[0],
        packet_boundary='Declared signed int receiver ABI inputs; health only from constructor or previous whole receiver.',
        source='original admitted MTNK', warhead='original stock AP', distance=0),
        building_constructor=f.building_constructor,
        building_native_admission=f.building_native_admission,
        cell_spread_startup=f.cell_spread_startup,
        tactical_projection_startup=f.tactical_projection_startup,
        projection_controls=projection, steps=[])
    result['input'].update(source_object=damage_sound_object_identity(f, f.source),
                          target_object=damage_sound_object_identity(f, er.BLD))
    for label, entry, raw, ignore in (
        ('Object_raw_zero', 0x5F5390, 0, 1),
        ('Techno_raw_zero', 0x701900, 0, 1),
        ('Building_raw_zero', 0x442230, 0, 1),
        ('Building_full_health_heal', 0x442230, -1, 0),
        ('Techno_defended_zero', 0x701900, 0, 0),
        ('Building_defended_zero', 0x442230, 0, 0),
        ('Building_heal_after_zero', 0x442230, -1, 0),
        ('Building_stock_105mm_packet', 0x442230, f.read32(inputs.weapon + 0xA4), 0),
        ('Building_heal_after_stock_damage', 0x442230, -1, 0),
        ('Building_heal_below_strength', 0x442230, -7, 0),
    ):
        f.u.mem_write(packet, dwords(raw))
        before, call_start = h.snapshot(), len(h.events)
        draw_start, advance_start = len(f.draws), len(f.advances)
        answer = h.invoke(label, entry, er.BLD, packet, 0, inputs.warhead,
                          f.source, ignore, 0, 0)
        after, call_end = h.snapshot(), len(h.events)
        import copy
        receiver_draws = copy.deepcopy(f.draws[draw_start:])
        receiver_advances = copy.deepcopy(f.advances[advance_start:])
        h.invoke(label + '_whole_AI_health_sample', 0x43FB20, er.BLD)
        calls = h.events[call_start:call_end]
        result['steps'].append(dict(label=label, entry=entry, supplied_damage=raw,
            ignore_defenses=bool(ignore), damage_packet_after=struct.unpack('<i', f.u.mem_read(packet, 4))[0],
            result=answer, before=before, after_receiver=after, after_sample=h.snapshot(),
            requests=receiver_draws, advances=receiver_advances,
            coordinate_attempts=[{k: v for k, v in row.items() if k not in ('before', 'after')}
                                 for row in calls if row.get('kind') == 'CoordinateRequest'],
            packet_readbacks=[{k: v for k, v in row.items() if k not in ('before', 'after')}
                for row in calls if row.get('kind') in ('BuildingReceiver', 'TechnoReceiver',
                    'ObjectReceiver', 'WarheadDamageKernel', 'receiver_packet_site')],
            ordered_receiver_calls=[dict(kind=row.get('kind'), pc=row.get('pc'),
                phase=row.get('phase')) for row in calls if row.get('kind') in
                ('BuildingReceiver', 'TechnoReceiver', 'ObjectReceiver', 'CoordinateRequest')]))
    h.invoke('original_request_clear', 0x750FA0, 0)
    h.audio(1034)
    result['audio'] = h.project()
    return result


def damage_sound_generate(*, profile='all'):
    """Selected ordinary DamageSound path on the existing joined world."""
    import hashlib, os
    root = Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    inputs = joined_inputs(root, damage_fires=True, damage_sound=True)
    result = dict(schema_version=1, source='unicorn/gamemd.exe',
        selected_profile=profile,
        retail_files=[dict(name=p.name, size=p.stat().st_size,
            sha256=hashlib.sha256(p.read_bytes()).hexdigest())
            for p in sorted(root.iterdir()) if p.is_file() and p.suffix.lower() != '.json'],
        selected_sounds=inputs.death_sounds, selected_inputs=inputs.damage_sound_inputs,
        death_layers=inputs.death_layers, physical_art_input_sections=inputs.death_art_input_sections,
        native_asset_loads=inputs.asset_loaded,
        routes=[joined_execute(inputs, projectile=projectile, building_ai=True, damage_sound=True)
                for projectile in {'all': (False, True), 'direct': (False,),
                                   'fire': (True,), 'receiver': (), 'audio': (),
                                   'threshold': ()}[profile]])
    if profile in ('all', 'receiver'):
        result['receiver_controls'] = damage_sound_receiver_controls(inputs)
    if profile in ('all', 'audio'):
        from tools.input_oracle.unit_voice_playback import VoiceHistory
        f = joined_fixture(inputs, None, building_ai=True, damage_sound=True)
        h = VoiceHistory(inputs, 'CAMISC03_shared_audio_owner_controls', fixture=f,
                         actor_count=0, coordinate_profile=True)
        ids = {row['name']: row['fixture_index'] for row in inputs.death_sounds}
        result['audio_core_controls'] = h.audio_core_controls(
            ids['BuildingMetalDamaged'], ids['GrizzlyTankAttack'])
        result['audio_core_controls']['cell_spread_startup'] = f.cell_spread_startup
    if profile in ('all', 'threshold'):
        result['threshold_controls'] = damage_sound_threshold_controls(inputs)
    return result


def damage_sound_threshold_controls(inputs):
    """Synthetic reference controls through existing full native readers."""
    import os
    from tools.input_oracle.unit_voice_playback import VoiceHistory
    from tools.spatial_oracle import engineer_repair_admission as er
    from tools.spatial_oracle.building_body_rules import INI
    from tools.rules_oracle.bridge_child_sound import sections
    m = inputs
    original_type = m.types['CAMISC03']
    physical = sections((Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS']) / 'RULESMD.INI').read_bytes())
    raw_type = dict(physical['CAMISC03'])
    rows, global_reads = [], []
    for label, reference in (('absent', None), ('empty', ''),
            ('unregistered', 'UNREGISTERED_DAMAGE_SOUND_CONTROL'),
            ('registered_Dummy', 'Dummy'), ('registered_stock', raw_type['DamageSound'])):
        name = 'DSC_' + label
        pointer = m.alloc(0x2000)
        m.invoke(0x45DD90, pointer, (m.cstring(name),))
        initial = struct.unpack('<i', m.u.mem_read(pointer + 0x538, 4))[0]
        raw = dict(raw_type, Image='CAMISC03')
        raw.pop('DamageSound', None)
        if reference is not None:
            raw['DamageSound'] = reference
        # Declared section-name/Image translation admits the same physical
        # type fields. Only the reference under test differs; no native ID,
        # health/sample, threshold, RNG or timer result is supplied.
        m.make_ini({name: raw})
        admitted = m.invoke(0x45FE50, pointer, (INI,)) & 255
        assert admitted == 1, (name, admitted)
        bound = struct.unpack('<i', m.u.mem_read(pointer + 0x538, 4))[0]
        m.types['CAMISC03'] = pointer
        f = joined_fixture(m, None, building_ai=True, damage_sound=True)
        h = VoiceHistory(m, 'reference_' + label, fixture=f, actor_count=0, coordinate_profile=True)
        h.invoke('initial_native_health_sample', 0x43FB20, er.BLD)
        packet, steps = f.allocate(4), []
        for order in range(3):
            f.u.mem_write(packet, dwords(50))
            before, start = h.snapshot(), len(h.events)
            draw_start, advance_start = len(f.draws), len(f.advances)
            call_label = 'whole_receiver_threshold_' + str(order)
            result = h.invoke(call_label, 0x442230,
                             er.BLD, packet, 0, m.warhead, f.source, 0, 0, 0)
            calls, after = h.events[start:], h.snapshot()
            steps.append(dict(label=call_label, entry=0x442230, ignore_defenses=False,
                supplied_damage=50, result=result,
                damage_packet_after=struct.unpack('<i', f.u.mem_read(packet, 4))[0],
                before=before, after_receiver=after,
                requests=f.draws[draw_start:].copy(), advances=f.advances[advance_start:].copy(),
                coordinate_attempts=[{k: v for k, v in call.items() if k not in ('before', 'after')}
                    for call in calls if call.get('kind') == 'CoordinateRequest'],
                packet_readbacks=[{k: v for k, v in call.items() if k not in ('before', 'after')}
                    for call in calls if call.get('kind') in ('BuildingReceiver', 'TechnoReceiver',
                    'ObjectReceiver', 'WarheadDamageKernel', 'receiver_packet_site')]))
            h.invoke('native_health_sample_after_threshold_' + str(order), 0x43FB20, er.BLD)
            steps[-1]['after_sample'] = h.snapshot()
        h.invoke('original_clear_threshold_requests', 0x750FA0, 0)
        rows.append(dict(label=label, synthetic_input_section={name: raw},
            input=dict(source_object=damage_sound_object_identity(f, f.source),
                target_object=damage_sound_object_identity(f, er.BLD), warhead='original stock AP', distance=0),
            reference=reference, constructor_id=initial, reader_result=admitted, native_bound_id=bound,
            global_bound_id=struct.unpack('<i', m.u.mem_read(m.rules + 0x714, 4))[0],
            building_constructor=f.building_constructor, building_native_admission=f.building_native_admission,
            cell_spread_startup=f.cell_spread_startup,
            steps=steps, audio=h.project(),
            limits='Synthetic name/Image/reference controls over physical CAMISC03 fields; original full type/receiver bodies, not a stock alternate type or scenario producer.'))
    # Later empty/invalid passes retain the registered global ID. Full AV
    # owns this same selected fallback; no field-specific resolver is used.
    for label, raw in (('empty', ''), ('unregistered', 'UNREGISTERED_DAMAGE_SOUND_CONTROL'),
                      ('registered_stock', physical['AudioVisual']['BuildingDamageSound'])):
        before = struct.unpack('<i', m.u.mem_read(m.rules + 0x714, 4))[0]
        m.make_ini({'AudioVisual': {'BuildingDamageSound': raw}})
        returned = m.invoke(0x6691E0, m.rules, (INI,))
        global_reads.append(dict(label=label, raw=raw, before=before, returned=returned,
            after=struct.unpack('<i', m.u.mem_read(m.rules + 0x714, 4))[0]))
    m.types['CAMISC03'] = original_type
    return dict(histories=rows, global_later_passes=global_reads,
        limits='Constructor defaults/parser capacity/trim/case reader controls remain separately preserved in the accepted original reader packet. These are new full-reader/receiver integration controls.')


def damage_sound_projection(data):
    """Select original observations; the formatter archives every raw byte.

    Ordinary receiver/projection and shared-core Rust comparisons consume
    named boundaries, not each of the300/299/201 admission-loop snapshots.
    No HP, event, timer, PCM or RNG answer is computed by this projection.
    """
    import copy
    result = copy.deepcopy(data)
    bulk_prefixes = ('full_pool_nonpositional_', 'state4_pool_nonpositional_',
                     'inaudible_request_capacity_')
    def retained(row):
        label = row['label']
        return not any(label.startswith(prefix) and label[len(prefix):].isdigit()
                       for prefix in bulk_prefixes)
    def history(value, *, boundaries=True, extra=()):
        keys = ('name', 'initial', 'inputs', 'final', 'complete_rng_states',
                'original_code_unchanged', 'platform_transport')
        selected = {key: value[key] for key in keys if key in value}
        if boundaries:
            prefixes = ('audio_pump_', 'original_worker_', 'whole_game_pause_',
                        'whole_game_resume_', 'registered_Limit2_request_')
            names = {'coordinate_natural_end', 'coordinate_before_nested_pause',
                     'new_request_during_nested_pause_depth_zero',
                     'original_request_clear_while_live'}
            selected['boundaries'] = [row for row in value['boundaries'] if
                row['label'].startswith(prefixes) or row['label'] in names]
        # IDs are original observer references. Keep the original full bytes
        # for every referenced stream; unused state copies stay in the archive.
        states = selected.pop('complete_rng_states')
        references = set()
        def collect(value):
            if isinstance(value, str) and value in states:
                references.add(value)
            elif isinstance(value, dict):
                for item in value.values(): collect(item)
            elif isinstance(value, list):
                for item in value: collect(item)
        collect(selected)
        collect(extra)
        selected['complete_rng_states'] = {key: states[key] for key in sorted(references)}
        return selected
    result['receiver_controls']['audio'] = history(result['receiver_controls']['audio'],
        boundaries=False, extra=result['receiver_controls']['steps'])
    for row in result['threshold_controls']['histories']:
        row['audio'] = history(row['audio'], boundaries=False, extra=row['steps'])
    # Rust consumes the direct receiver/threshold histories and named core
    # controls. The executed Fire routes and these controls have no Rust
    # consumer yet; they stay in the checked raw archive.
    for key in ('routes', 'native_asset_loads', 'physical_art_input_sections'):
        del result[key]
    core = result['audio_core_controls']
    del core['cell_spread_startup']
    archived_controls = {
        'position_out_of_range_id_unbinds', 'position_valid_after_invalid',
        'disabled_position_retains_handle', 'disabled_nonpositional_retains_handle',
        'position_inaudible_stops_retains_entry', 'out_of_range_request_node_no_event',
        'inaudible_request_node_no_event', 'invalid_inaudible_nodes_reaped',
        'suspended_request_service_and_new_request_redrive',
        'clear_requests_and_dead_event_reap'}
    core['controls'] = [row for row in core['controls']
                        if retained(row) and row['label'] not in archived_controls]
    core['history'] = history(core['history'], extra=[core['controls'], core['natural_end']])
    def lifecycle(value):
        selected = {key: item for key, item in value.items()
                    if key not in ('all_native_rng_requests', 'all_raw_advances')}
        pcm = selected['pcm_observation']
        for key in ('callbacks', 'blocks'):
            pcm[key] = [{name: item for name, item in row.items()
                         if name not in ('output_writes', 'nibble_calls', 'count_writes')}
                        for row in pcm[key]]
        return selected
    core['natural_end'] = lifecycle(core['natural_end'])
    result['projection_manifest'] = dict(
        retained_core_control_labels=[row['label'] for row in core['controls']],
        retained_core_boundary_labels=[row['label'] for row in core['history']['boundaries']],
        omitted_bulk_admission_prefixes=list(bulk_prefixes),
        archived_core_control_labels=sorted(archived_controls),
        limits='Named exact observations and their referenced full RNG states only; repeated raw traces, scoped timer/detach writes, every bulk admission, the executed Fire routes and the archived controls remain in the checked full raw archive. The Fire fixture executes original CellSpread startup before the ready/faced shot; full scenario/input acquisition/approach/turning and later weapon cadence remain fixture boundaries.')
    return result


def damage_sound_metadata():
    return provenance(scope='Stock MTNK/105mm/AP ordinary damage to genuine CAMISC03, per-type retained-coordinate DamageSound requests',
        entry_points=dict(building_ctor=0x43B740, building_unlimbo=0x440580,
            cell_spread_startup=0x561910,
            building_ai=0x43FB20, health_sample_store=0x44006C,
            building_receiver=0x442230, techno_receiver=0x701900,
            object_receiver=0x5F5390, unit_fire=0x741340,
            coordinate_request=0x750E20, positional_play=0x7509E0,
            coordinate_service=0x750EC0, positional_redrive=0x750D40,
            spatial_gain=0x750AC0, coords_to_client=0x6D2140),
        assumptions=[
            'Existing joined_inputs owns original selected full constructors/readers and lexical RULESMD/optional LANGRULE/Battle/Hills layers. Complete scenario/House/factory admission and physical cache loading are fixture boundaries.',
            'Distinct genuine CAMISC03 retains original43B740 health100/sample0/UID/flags. Original440580 owns Unlimbo/Mark/Logic admission against supplied clear Cells/House. Whole43FB20 owns the first sample100; no post-constructor health or sampled-health value is supplied.',
            'Selected joined-fixture startup invokes whole original561910 via the existing invocation owner, recording CRT813AA4/thunk561900, all369 signed CellSpread pairs and full RNG/draw readbacks. This prerequisite is separate from full CRT/process/Scenario admission. Historical default fixtures keep their cold uninitialized CellSpread premise and cannot establish ordinary AreaDamage parity.',
            'Direct receiver and original ready/faced MTNK FireAt/Bullet/AreaDamage are separate routes on the same existing fixture implementation. Earlier target acquisition/input/approach/turning and subsequent weapon cadence remain outside the native fixture.',
            'VoiceHistory attaches to the same Unicorn world, existing NativeAudioPlatform, invocation/worker and observer. It does not create a second world, listener, registry, platform or audio service.',
            'Viewport/camera/surface values are declared consumer inputs. Native projection readbacks/constants/outputs are observed; no native camera producer or rendered/audible hardware parity is claimed.',
            'Full Scenario/Main/MapGen states and original RNG requests, native request/event readbacks and scoped memory writes are retained. Opaque CdTimer middle words are not numeric timer assertions.'],
        substitutions=['Existing joined lexical cache/file/allocator/COM/CRT/platform boundaries only; original gameplay/audio/projection instructions are unchanged.'])


def damage_sound_main(argv):
    # Load inherited observer/PCM owners before finish_vectors' source guard.
    from tools.input_oracle import unit_voice_playback as voice
    sources = dict(joined_source_paths(), **voice.source_paths())
    root = Path(__file__).resolve().parents[2]
    sources['tools/input_oracle/unit_voice_playback.py'] = root / 'tools/input_oracle/unit_voice_playback.py'
    selected = [arg for arg in argv if arg.startswith('--damage-sound-profile=')]
    if len(selected) > 1:
        raise ValueError('Select one damage-sound profile')
    profile = selected[0].split('=', 1)[1] if selected else 'all'
    if profile not in ('all', 'direct', 'fire', 'receiver', 'audio', 'threshold'):
        raise ValueError('Unknown damage-sound profile: ' + profile)
    argv = [arg for arg in argv if not arg.startswith('--damage-sound-profile=')]
    def metadata():
        result = damage_sound_metadata()
        result['selected_profile'] = profile
        if profile == 'all':
            result['projection_limits'] = ('Existing finish_vectors retains the complete raw archive. '
                'Projection selects named original controls/receiver steps/24projection outputs and referenced full RNG states/PCM bytes; no derived answers. '
                'Selected original CellSpread startup/ready-faced FireAt/impact and full receiver are executed on declared fixture inputs; the Fire routes are archived, not projected. '
                'Full scenario admission and input/approach/turning/rearm cadence are outside this comparison.')
        return result
    finish_vectors(lambda: damage_sound_generate(profile=profile),
        Path(__file__).with_name('building_damage_sound_joined.json'),
        provenance=metadata, argv=argv, source_paths=sources,
        projection=damage_sound_projection if profile == 'all' else None)



def stock_joined_generate():
    return joined_generate(stock_smudges=True)


def stock_joined_metadata():
    base = joined_metadata()
    assumptions = [text for text in base['assumptions']
        if not text.startswith('GameOptions+A8EB60')
        and not text.startswith('The direct AP control')
        and not text.startswith('Scenario/Main/MapGen')]
    assumptions.extend([
        'Three ready-MTNK/105mm/AP routes use Scenario/Main seeds2,1,31 and MapGen31. The four inherited empty-registry/direct controls remain a separate unchanged payload. Stock seed2 admits one E1; seeds1/31 refuse crew.',
        'Original SmudgeType registry CRT4E7260, active Rules::Process668DD2..668E23 and Rules::ReadTypeData679BE2..679C06 execute over physical RULESMD, optional LANGRULE, Battle mode and inner Hills.map caches. Whole ctor6B5260/ReadINI6B56D0 and ObjectType5F92D0 execute; Image comes from rules and Theater from physical ARTMD. Native requested-file IO returns exact extracted retail image bytes; old unflagged CR/BURN names legitimately have no images.',
        'The additional stock ClearTile0 prior executes real IsoTileType registry CRT4E76E0, ctor5447C0 and original TEMPERATMD TileSet0000 Morphable Boolean block546124..54614C plus its original54644C result store. The admitted flat interior cells carry TileIndex0, no overlay, slope0 and initial SmudgeIndex-1. Actual full theater/TMP/map loading is excluded; class/height, connectivity and all gameplay owners retain the shared joined fixture.',
        'Original SmudgeClass registry CRT4E69E0 and sentinel6B5210 execute. Actual selected Burn/Crater placers, CanPlace, full Smudge6B4A50, Mark6B4BE0, Place6B6080, UnInit5F65F0, repeated expiry7258D0, deferred scalar6B4FA0 and Object5F3B80 execute, with IDs/flags, generic/Smudge/Logic registries, ordered queue members, cell writes and full three RNG states recorded. Physical SmudgeTypes key/value pairs retain declaration order as an array. Every registry/queue object and expiry boundary retains signed Health and raw Location. No selected type, mark, lifecycle result or RNG draw is supplied.',
        'The transient Smudge consumes original Scenario uniqueID14, queues after dead Anim7/8, never joins Logic, retires before Building1/Bullet4 and leaves its persistent2x2 cells marked after drain. This is native executed state/lifecycle evidence, not native pixel/audio or full map-loader parity.',
        'Only stock seed1 adds a post_drain_put continuation after preserving every old prefix field. The same Runtime executes shared original Discovery geometry initialization, fresh GAPOWR43B740, Cell GetCoords486840 and full440580 with successful AL1. Native43F8A6 clears type48 across the existing2x2 footprint while preserving raw data11F. Existing supplied House/map/registry bounds remain; no paid production/input or full native world load is claimed. Native newID23 is active GAPOWR_A in slot3 with actual ctor/Start events, not GAPOWRMK buildup. Sparse ART inputs exclude GAPOWRMK; later whole construction/animation AI is not certified. Measured client priors retain original GameMode1, PlayerPtr/current-owner and House1EC/1ED, plus known Cell12C18/counter0 inputs. Original early CellPUT47E9E2 discovery opens before Techno idle; no manual opening or expected flag write is supplied.',
    ])
    base['scope'] = 'Stock SmudgeTypes/clear-Morphable ready-MTNK fatal GAPOWR through footprint, crew and drain, plus seed1 fresh successful normal PUT cleanup'
    base['assumptions'] = assumptions
    base['entry_points'].update(smudge_registry_startup='0x004E7260',
        smudge_list_process='0x00668DD2', smudge_type_ctor='0x006B5260',
        smudge_type_reader='0x006B56D0', smudge_runtime_ctor='0x006B4A50',
        smudge_mark='0x006B4BE0', smudge_place='0x006B6080',
        smudge_runtime_scalar='0x006B4FA0', tile_type_ctor='0x005447C0',
        tile_morphable_reader='0x00546124', tile_morphable_store='0x0054644C',
        post_put_cell_coordinates='0x00486840', post_put_unlimbo='0x00440580',
        post_put_clear_type='0x0043F8A6', post_put_redraw='0x004852D0',
        post_put_dirty_ring='0x00486E70')
    return base


def stock_joined_main(argv):
    finish_vectors(stock_joined_generate,
        Path(__file__).with_name('building_death_anims_joined_stock_smudges.json'),
        provenance=stock_joined_metadata,argv=argv,source_paths=joined_source_paths())


def constructed_cancellation_execute(inputs, mode):
    """Original create/cancel routes; observes their one shared destructor."""
    from collections import deque
    import traceback
    from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
    from unicorn.x86_const import (UC_X86_REG_EAX,UC_X86_REG_EBP,UC_X86_REG_EBX,
        UC_X86_REG_ECX,UC_X86_REG_EDI,UC_X86_REG_EDX,UC_X86_REG_EIP,
        UC_X86_REG_ESI,UC_X86_REG_ESP)
    from tools.spatial_oracle import building_construction as bc,engineer_repair_admission as er
    from tools.spatial_oracle.building_body_rules import SP
    from tools.spatial_oracle.unit_source_scatter import SCENARIO
    entries={0x4E6E60:'factory_crt_initializer',0x4C98B0:'factory_constructor',0x4C9C70:'start_production',0x43B740:'building_constructor',0x4C9FF0:'abandon_production',0x459F20:'building_scalar',0x43BCF0:'building_destructor',0x7258D0:'pointer_expiry',0x445880:'base_reservation_release',0x451E40:'ordinary_slot_destructor',0x406060:'sound_handle_release',0x405C00:'sound_handle_destructor',0x4FF550:'remove_tracking',0x4FF980:'remove_owned',0x5025F0:'remove_live',0x6F4500:'techno_destructor',0x5F3B80:'object_destructor',0x7C8B3D:'arena_release',0x4FAA10:'house_abandon_production',0x4CA760:'factory_scalar',0x4F9950:'refund_add_credits',0x6FCDB0:'assign_observer_target',0x7446E0:'unit_pointer_expired',0x4D9960:'foot_pointer_expired',0x7077C0:'techno_pointer_expired'}
    pcs={0x7079D1:'passive_scan_timer_guard',0x7079F5:'passive_scan_priority_guard',0x707A0D:'passive_scan_rng_call',0x4CA0E3:'before_priority_increment',0x4CA0EB:'priority_increment_store',0x4CA0FC:'product_scalar_call',0x4CA0FF:'clear_factory_product',0x4CA109:'priority_decrement_store',0x43BEF5:'building_active_global_gate',0x43BF07:'building_compare_health_sample',0x43BF11:'building_health_mismatch_dirty_write',0x43BF34:'building_tracking_call',0x43C022:'clear_building_type',0x440055:'health_sampling_power_dirty_store',0x440062:'health_sampling_radar_dirty_store',0x44006C:'health_sampling_health_store',0x4FABA6:'human_house_cancel_leaf_call',0x4FAC6A:'house_building_factory_scalar_call',0x4FAC85:'house_infantry_factory_scalar_call'}
    regs={'eax':UC_X86_REG_EAX,'ebx':UC_X86_REG_EBX,'ecx':UC_X86_REG_ECX,'edx':UC_X86_REG_EDX,'esi':UC_X86_REG_ESI,'edi':UC_X86_REG_EDI,'ebp':UC_X86_REG_EBP,'esp':UC_X86_REG_ESP}
    f=joined_fixture(inputs,38,seed=2,building_ai=True)
    u,read=f.u,f.read32
    owner=f.owner_houses[1]
    if mode=='factory_leaf_ai':
        # Explicit AI-house prior, not a synthesized destructor answer.
        u.mem_write(owner+0x1EC,b'\0')
    product=0
    factory=0
    recent=deque(maxlen=64)
    events=[];writes=[];pending=[];expiry_returns=[];phase='observe_setup';boundaries=[]
    def signed(at):return struct.unpack('<i',u.mem_read(at,4))[0]
    def vector(base):
        ptr,count=read(base+4),read(base+16)
        return dict(base=base,pointer=ptr,count=count,items=[] if not count else list(struct.unpack('<'+'I'*count,u.mem_read(ptr,count*4))))
    def state():
        b=product or (read(factory+0x58) if factory else 0)
        return dict(phase=phase,frame=read(bc.FRAME),scenario_init_priority=read(0xA8E7AC),active_game=u.mem_read(0xA8E9A0,1)[0],house=dict(pointer=owner,dirty=list(u.mem_read(owner+0x5778,2)),building_count=signed(owner+0x2F0),chosen_building=signed(owner+0x564C),credits=signed(owner+0x30C),held_building_factory=read(owner+0x53BC),buildings=vector(owner+0x68)),factory=None if not factory else dict(pointer=factory,id=read(factory+0x10),held=read(factory+0x58),owner=read(factory+0x6C),stage=read(factory+0x24),balance=signed(factory+0x60),suspended=u.mem_read(factory+0x70,1)[0],queue_count=read(factory+0x50)),building=None if not b else dict(pointer=b,id=read(b+0x10),abstract_flags=read(b+0x14),type=read(b+0x520),owner=read(b+0x21C),health=signed(b+0x6C),sampled_health=signed(b+0x544),limbo=u.mem_read(b+0x81,1)[0],marked=u.mem_read(b+0x74,1)[0],alive=u.mem_read(b+0x90,1)[0],logic_member=u.mem_read(b+0x98,1)[0],mission=signed(b+0xAC),queued=signed(b+0xB4),body=signed(b+0x534),slots=list(struct.unpack('<21I',u.mem_read(b+0x55C,84))),damage_fires=list(struct.unpack('<8I',u.mem_read(b+0x5C8,32))),sound_handles=[bytes(u.mem_read(b+o,20)).hex() for o in (0x6A0,0x6B4)]),unique_id_cursor=read(SCENARIO+0x214),observer=dict(pointer=f.source,id=read(f.source+0x10),abstract_flags=read(f.source+0x14),target=read(f.source+0x2B4),passive_scan_timer=list(struct.unpack('<3i',u.mem_read(f.source+0x180,12)))),rng=f.rng())
    def code(_u,pc,size,_):
        for row in expiry_returns[:]:
            if pc==row['return_pc']:
                row['event']['after_return']=state();expiry_returns.remove(row)
        for row in pending[:]:
            row['after_at_next_pc']=f'0x{pc:08X}';row['after']=state();pending.remove(row)
        if pc in entries or pc in pcs:
            sp=u.reg_read(UC_X86_REG_ESP)
            row=dict(pc=f'0x{pc:08X}',kind=entries.get(pc,pcs.get(pc)),phase=phase,this=u.reg_read(UC_X86_REG_ECX),caller=f'0x{read(sp):08X}' if pc in entries else None,registers={k:u.reg_read(v) for k,v in regs.items()},state=state())
            if pc==0x7C8B3D:row['pointer']=read(sp+4)
            events.append(row)
            if pc==0x7077C0 and row['this']==f.source:
                row.update(expired_pointer=read(sp+4),control_word=read(sp+8),removed=read(sp+8)&255,target_matches=read(f.source+0x2B4)==read(sp+4))
                expiry_returns.append(dict(return_pc=read(sp),event=row))
        recent.append(dict(pc=f'0x{pc:08X}',bytes=bytes(u.mem_read(pc,size)).hex()))
    def write(_u,access,address,size,value,_):
        if (address<owner+0x577A and owner+0x5778<address+size) or (address<0xA8E7B0 and 0xA8E7AC<address+size):
            row=dict(pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',address=address,size=size,value=value,before=state(),recent_pcs=list(recent))
            writes.append(row);pending.append(row)
    u.hook_add(UC_HOOK_CODE,code);u.hook_add(UC_HOOK_MEM_WRITE,write)
    def invoke(label,entry,this,*args):
        nonlocal phase
        phase=label;f.phase=label
        before=state();draw_start=len(f.draws)
        result=bc.invoke(u,entry,this,*args)
        after=state()
        boundaries.append(dict(label=label,entry=f'0x{entry:08X}',this=this,args=list(args),result=result,before=before,after=after,rng_requests=f.draws[draw_start:]))
        return result
    fault=None
    try:
        # Existing original House prefix, stopped before unrelated House AI.
        phase='clean_house_through_original_power_prefix';f.phase=phase
        before=state();draw_start=len(f.draws)
        u.reg_write(UC_X86_REG_ECX,owner);u.reg_write(UC_X86_REG_ESP,SP)
        bc.run_checked(u,0x4F8440,0x4F84F1)
        boundaries.append(dict(label=phase,entry='0x004F8440',stop='0x004F84F1',before=before,after=state(),rng_requests=f.draws[draw_start:]))
        assert u.mem_read(owner+0x5778,1)==b'\0'
        invoke('factory_vector_original_startup',0x4E6E60,0)
        factory=f.allocate(0x80)
        invoke('factory_original_constructor',0x4C98B0,factory)
        require(invoke('original_start_production_gapowr',0x4C9C70,factory,f.types['GAPOWR'],owner,0)&255==1, 'native 0x4C9C70 returned an unexpected result')
        product=read(factory+0x58)
        assert product and read(product+0x520)==f.types['GAPOWR']
        type_kind=invoke('original_building_type_rtti',read(read(f.types['GAPOWR'])+0x2C),f.types['GAPOWR'])
        object_kind=invoke('original_building_object_rtti',read(read(product)+0x2C),product)
        assert (type_kind,object_kind)==(7,6)
        boundaries.append(dict(label='held_ctor_complete_gapowr',state=state()))
        if mode.endswith('equal_sample'):
            phase='original_health_sampling_rung_equal_control';f.phase=phase
            before=state();draw_start=len(f.draws)
            u.reg_write(UC_X86_REG_ESI,product)
            bc.run_checked(u,0x440042,0x440072)
            boundaries.append(dict(label=phase,entry='0x00440042',stop='0x00440072',before=before,after=state(),rng_requests=f.draws[draw_start:],bound='Isolated original Building health-sampling rung establishes equal544 without changing health; limbo products do not receive an ordinary Logic visit. This is an asymmetric equal-health guard control.'))
            phase='original_house_power_clean_after_equal_sample';f.phase=phase
            before=state();draw_start=len(f.draws)
            u.reg_write(UC_X86_REG_ECX,owner);u.reg_write(UC_X86_REG_ESP,SP)
            bc.run_checked(u,0x4F8440,0x4F84F1)
            boundaries.append(dict(label=phase,entry='0x004F8440',stop='0x004F84F1',before=before,after=state(),rng_requests=f.draws[draw_start:]))
            assert read(product+0x544)==read(product+0x6C) and u.mem_read(owner+0x5778,1)==b'\0'
        if mode.endswith('targeted_observer'):
            invoke('original_assign_limbo_product_to_real_mtnk_observer',0x6FCDB0,f.source,product)
            assert read(f.source+0x2B4)==product
            boundaries.append(dict(label='asymmetric_targeted_limbo_observer_prior',state=state(),bound='Actual MTNK ctor/Unlimbo is reused; original SetTarget assigns the limbo product. This is an explicit callback guard control, not an ordinary player acquisition claim for a limbo target.'))
        if mode.startswith('house_cancel'):
            u.mem_write(owner+0x53BC,bc.dwords(factory))
            boundaries.append(dict(label='supplied_existing_house_factory_link',state=state(),bound='Original Factory ctor/start executed; House BeginProduction/link producer not included. This is an existing-linked-factory prior for whole HouseAbandonProduction. Noncurrent human avoids client sidebar transports.'))
            invoke('original_house_cancel_current_product',0x4FAA10,owner,type_kind,-1,0,0)
        else:
            invoke('original_factory_abandon_product',0x4C9FF0,factory)
        assert read(factory+0x58)==0
        assert read(product+0x520)==0
        assert read(0xA8E7AC)==0
    except Exception as exc:
        fault=dict(type=type(exc).__name__,message=str(exc),pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',registers={k:u.reg_read(v) for k,v in regs.items()},recent=list(recent),state=state(),traceback=traceback.format_exc())
    assert f.code_unchanged()
    assert fault is None, fault
    return dict(input=dict(route='constructed_limbo_gapowr_cancellation',control=mode,
        seed=2,owner_human=mode!='factory_leaf_ai',existing_house_factory_link=mode.startswith('house_cancel'),
        equal_sample_control=mode.endswith('equal_sample'),targeted_observer_control=mode.endswith('targeted_observer')),
        boundaries=boundaries,ordered_native_calls=events,priority_and_house_dirty_writes=writes,
        final=state(),rng_requests=f.draws,ranged_raw_advances=f.advances,
        runtime_events=f.events,native_trace=f.trace,
        executed=[f'0x{pc:08X}' for pc in sorted(f.executed)],original_executable_sections_unchanged=True)


def constructed_cancellation_generate():
    import hashlib,json,os
    root=Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    inputs=joined_inputs(root,damage_fires=True)
    receipts_path=root/'receipts.json'
    receipts=json.loads(receipts_path.read_text()) if receipts_path.is_file() else []
    return dict(schema_version=1,source='unicorn/gamemd.exe',
        retail_files=[dict(name=p.name,size=p.stat().st_size,
            sha256=hashlib.sha256(p.read_bytes()).hexdigest())
            for p in sorted(root.iterdir()) if p.is_file() and p.suffix.lower()!='.json'],
        extraction_receipts=[{k:v for k,v in row.items() if k in
            ('asset','source_archive','entry_id','size','sha256','catalog_lookup_only')} for row in receipts],
        construction_layers=inputs.layers,engineer_layers=inputs.engineer_layers,
        death_layers=inputs.death_layers,selected_sounds=inputs.death_sounds,
        native_country_sides=inputs.death_country_sides,
        physical_art_input_sections=inputs.death_art_input_sections,
        native_fatal_anim_inputs=inputs.death_anim_rows,
        native_input_execution=inputs.death_input_execution,native_code_sections=inputs.code_identity,
        controls=[constructed_cancellation_execute(inputs,mode) for mode in (
            'factory_leaf_human','house_cancel_noncurrent_human','factory_leaf_ai',
            'factory_leaf_human_equal_sample','factory_leaf_human_targeted_observer')])


def constructed_cancellation_metadata():
    return provenance(scope='Constructor-complete stock GAPOWR cancellation through whole original Factory/House cancellation and Building scalar/destructor; shared limbo destructor consumer of ordinary fatal cleanup',
        entry_points=dict(factory_vector_startup=0x4E6E60,factory_ctor=0x4C98B0,
            start_production=0x4C9C70,building_ctor=0x43B740,
            house_abandon=0x4FAA10,factory_abandon=0x4C9FF0,
            priority_increment=0x4CA0EB,priority_decrement=0x4CA109,
            building_scalar=0x459F20,building_destructor=0x43BCF0,
            building_expiry=0x43BD67,expiry=0x7258D0,sound_release=0x406060,
            ordinary_slot_destructor=0x451E40,health_mismatch_dirty=0x43BF11,
            remove_tracking=0x4FF550,health_sampling_rung=0x440042,
            house_prefix=0x4F8440,assign_target=0x6FCDB0,
            unit_pointer_expired=0x7446E0,techno_pointer_expired=0x7077C0,
            passive_priority_guard=0x7079F5),
        assumptions=[
            'Reuses joined_inputs and joined_fixture unchanged: physical selected retail Rules/ART/weapon/sound readers, whole constructors, same allocator, original flat-map connectivity/navigation and declared House/admitted target/MTNK priors. No fatal shot or live-object loop runs in this control.',
            'Whole original4E6E60 Factory vector initializer,4C98B0 Factory ctor and4C9C70 StartProduction create a separate real GAPOWR. The product remains constructor-complete in limbo with original fields and AbstractFlags; it receives no supplied placement, Mark, Logic visit or UnInit. Final Building bytes are read from retained freed arena storage only, not from a live entity.',
            'The whole House cancel supplies only the pre-existing House+53BC factory link: House BeginProduction/link and earlier queue/UI producer are excluded. The owner is a noncurrent human House, so client sidebar paths remain dormant. Actual BuildingType vt2C returns7; native ordinary non-Defense switch selects53BC. Full House4FAA10 cancels index-1 and deletes the now-empty Factory.',
            'The AI control supplies House1EC0 in GameMode1; original StartProduction and Abandon execute their actual AI branches. The full BuildingFactoryAI/ExitObject/AI site-failure producer is not executed; original45022C reaches the same whole cancellation/destructor leaf.',
            'The equal-health control executes only original440042..440072 health-sampling rung and the original dirty-gated House prefix before cancel. Limbo products normally receive no Logic visit; this is an explicit asymmetric equality guard control, not ordinary limbo scheduling evidence.',
            'The target control uses the real MTNK ctor/Unlimbo and original SetTarget to point it at the limbo product. It bounds destructor callback/priority/timer behavior, not ordinary player acquisition of limbo objects. Target/raw180184188/full3RNG are retained at every boundary and actual Techno callback return.',
            'Ordinary GAPOWR constructor slots21 and damage-fire8 pointers and both Building sound handles are inactive. Original slot/sound owners still execute in destructor order; this control does not create artificial active limbo animation/sound state. Loss booking, fatal effects/survivors, ObjectUnInit and deferred drain are not reached.'
        ],substitutions=[
            'Only existing joined fixture reader-cache/physical IO/allocator/platform/COM/audio presentation transports are reused. No new decision thunk or patched original instruction is added. Every original PE code section remains byte-identical.'
        ])


def constructed_cancellation_main(argv):
    from tools.native_oracle import finish_vectors
    finish_vectors(constructed_cancellation_generate,
        Path(__file__).with_name('building_death_anims_limbo_cancel.json'),
        provenance=constructed_cancellation_metadata,argv=argv,
        source_paths=joined_source_paths())


def wall_autofill_execute(inputs, control):
    """Whole588750 in the existing joined VM; all hooks below only observe.

    Endpoints are original BuildingType::CreateOne -> Building::Unlimbo
    results. The one custom-land control is a declared pre-entry input, read
    by674000, never a post-scan answer or a hook mutation.
    """
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX,
        UC_X86_REG_EIP, UC_X86_REG_ESP)
    from tools.spatial_oracle import building_construction as bc, engineer_repair_admission as er
    from tools.spatial_oracle.building_body_rules import INI
    from tools.spatial_oracle.refinery_dock import cell
    from tools.spatial_oracle.unit_source_scatter import SCENARIO
    from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry

    origin = (20, 14)
    endpoints = {'no_gap': [(20, 13)], 'two_empty': [(20, 11)],
        'middle_refusal': [(20, 10), (22, 14)]}[control]
    f = joined_fixture(inputs, 750, seed=31, building_ai=True)
    u, read = f.u, f.read32
    owner, typ = er.HOUSE, f.types['GAWALL']
    f.phase = 'setup_wall_autofill'
    u.mem_write(0xA83D80, bytes(inputs.u.mem_read(0xA83D80, 24)))
    joined_stock_flat_map(f, inputs)
    # These are actual original-reader outputs, including native sentinels,
    # copied with the same input-Reader -> joined-runtime transport as types.
    # Never make a clear tile appear valid by bypassing native LAT/Recalc.
    for address, value in inputs.wall_theater_inputs['roles'].items():
        u.mem_write(int(address, 16), dwords(value))
    assert read(0xAA10B0) == 0 and read(0xA8ED38) == 1
    clear_head = read(read(0xA8ED2C))
    assert read(clear_head + 0x294) == 0
    assert read(read(clear_head) + 0x9C) == 0x544CB0
    assert read(clear_head + 0xA4) == 0  # explicitly nonresident component prior
    # Original Overlay registry and translation-unit startup. In particular
    # EmptyCell must agree with the constructor's own sentinel; don't assign
    # a special coordinate or rewrite a constructor branch to bypass Mark.
    for entry in (0x4E68E0, 0x5FC310, 0x5FC320, 0x5FC340, 0x5FC360):
        bc.invoke(u, entry, 0)
    assert bytes(u.mem_read(0xAC1608, 4)) == bytes(u.mem_read(0xAC1528, 4))
    before_geometry = f.rng()
    geometry = initialize_discovery_geometry(
        lambda entry, receiver, args: bc.invoke(u, entry, receiver, *args), read, u)
    assert before_geometry == f.rng()
    overlay = read(typ + 0xE54)
    assert overlay == inputs.wall_overlay_types['GAWALL']
    assert u.mem_read(overlay + 0x2A8, 1) == b'\1'
    assert read(read(typ) + 0x8C) == 0x45E880
    assert read(read(overlay) + 0x8C) == 0x5FE570

    def signed(address):
        return struct.unpack('<i', u.mem_read(address, 4))[0]

    rng_states, rng_indexes = [], {}
    def rng_index():
        state = f.rng()
        key = tuple(state.items())
        if key not in rng_indexes:
            rng_indexes[key] = len(rng_states)
            rng_states.append(state)
        return rng_indexes[key]

    def members(base):
        pointer, count = read(base + 4), read(base + 16)
        assert count <= 4096, (hex(base), count)
        return [] if not count else [dict(pointer=read(pointer + i * 4),
            id=read(read(pointer + i * 4) + 0x10)) for i in range(count)]

    objects, calls, active, boundaries, dirty_writes = {}, [], [], [], []
    phase = 'setup_endpoints'
    draw_start, advance_start = len(f.draws), len(f.advances)
    # Full RNG states are interned, not dropped or reconstructed. Native draw
    # requests retain their own complete before/after buffers via the owner.
    registries = dict(building=0xA8EB40, overlay=0xA8EC50,
        generic_object=0xB0F720, abstract=0xB0F670, logic=0x87F778,
        pending=0xB0F698, house_live=owner + 0x68)

    def object_state(pointer):
        kind = objects[pointer]
        return dict(pointer=pointer, kind=kind, id=read(pointer + 0x10),
            vtable=read(pointer), abstract_flags=read(pointer + 0x14),
            location=list(struct.unpack('<3i', u.mem_read(pointer + 0x9C, 12))),
            health=signed(pointer + 0x6C), alive=u.mem_read(pointer + 0x90, 1)[0],
            limbo=u.mem_read(pointer + 0x81, 1)[0], marked=u.mem_read(pointer + 0x74, 1)[0],
            logic=u.mem_read(pointer + 0x98, 1)[0],
            **(dict(type=read(pointer + 0x520), owner=read(pointer + 0x21C),
                sampled_health=signed(pointer + 0x544),
                constructor_ax=struct.unpack('<H', u.mem_read(pointer + 0x3C8, 2))[0],
                raw_words_c0_cc=list(struct.unpack('<3i', u.mem_read(pointer + 0xC0, 12))),
                passive_scan_timer=list(struct.unpack('<3i', u.mem_read(pointer + 0x180, 12))),
                radio_capacity=read(pointer + 0xE8),
                radio_contacts=list(struct.unpack('<' + 'I' * read(pointer + 0xE8),
                    u.mem_read(read(pointer + 0xE4), read(pointer + 0xE8) * 4)))
                    if read(pointer + 0xE8) else []) if kind == 'building'
              else dict(type=read(pointer + 0xAC))))

    def cell_state(x, y):
        p = cell(x, y)
        return dict(coord=[x, y], tile=signed(p + 0x38), land=signed(p + 0xEC),
            level=u.mem_read(p + 0x11B, 1)[0], slope=u.mem_read(p + 0x11C, 1)[0],
            overlay=signed(p + 0x44), data=u.mem_read(p + 0x11E, 1)[0], owner=signed(p + 0x50),
            ground_head=read(p + 0xE4), ground_raw=read(p + 0x124), deck_raw=read(p + 0x128),
            retained_neighbor=u.mem_read(p + 0x122, 1)[0], flags=read(p + 0x12C),
            ground_frame=struct.unpack('<b', u.mem_read(p + 0x120, 1))[0])

    def state():
        return dict(rng_state=rng_index(), frame=read(bc.FRAME),
            unique_id_cursor=read(SCENARIO + 0x214),
            scenario_init_priority=read(0xA8E7AC),
            wall_owner_globals={f'0x{address:08X}': read(address)
                for address in (0x8333BC, 0x880994)},
            house=dict(pointer=owner, index=read(owner + 0x30),
                human=u.mem_read(owner + 0x1EC, 1)[0],
                dirty=list(u.mem_read(owner + 0x5778, 2)),
                building_count=signed(owner + 0x2F0), credits=signed(owner + 0x30C),
                tracked=counts(owner + 0x5500), owned=counts(owner + 0x5550)),
            registries={name: members(base) for name, base in registries.items()},
            objects=[object_state(pointer) for pointer in objects],
            cells=[cell_state(x, y) for y in range(9, 17) for x in range(18, 24)])

    # Scalar count vectors store integers, not pointers. Do not dereference
    # them through the pointer-vector observer used by registries/radio.
    def counts(base):
        count = read(base + 16)
        assert count <= 4096
        return [] if not count else list(struct.unpack('<' + 'i' * count,
            u.mem_read(read(base + 4), count * 4)))

    def snapshot():
        # Keep the same state shape at all boundaries without serializing a
        # full 32x32 object image, instruction trace or repeated RNG buffers.
        return state()

    entries = {0x588750: 'autofill', 0x45E880: 'building_create',
        0x43B740: 'building_constructor', 0x440580: 'building_unlimbo',
        0x5FE570: 'overlay_create', 0x5FC380: 'overlay_constructor',
        0x5FD270: 'overlay_unlimbo', 0x5FC570: 'overlay_mark',
        0x47D2B0: 'cell_recalc', 0x47CA80: 'cell_lat_recalc',
        0x5471B0: 'tile_slope', 0x544CB0: 'tile_image_getter',
        0x47C620: 'cell_clearance', 0x5678E0: 'map_reveal_area',
        0x65ACB0: 'radio_first_contact', 0x65A970: 'radio_transmit',
        0x5F65F0: 'object_uninit', 0x7258D0: 'pointer_expiry',
        0x725C70: 'deferred_drain', 0x459F20: 'building_scalar',
        0x43BCF0: 'building_destructor', 0x5FDF70: 'overlay_scalar',
        0x4FF700: 'house_add_tracking', 0x4FF550: 'house_remove_tracking',
        0x4FFA50: 'house_add_owned', 0x4FF980: 'house_remove_owned',
        0x502A80: 'house_add_live', 0x5025F0: 'house_remove_live'}
    executed, measured, recalc_dispatches = set(), set(), []

    def diagnostic():
        from unicorn.x86_const import (UC_X86_REG_EBX, UC_X86_REG_EDX,
            UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP)
        def safe(address, size=4):
            try:
                return bytes(u.mem_read(address & 0xFFFFFFFF, size)).hex()
            except Exception as exc:
                return dict(unreadable=f'0x{address & 0xFFFFFFFF:08X}', error=str(exc))
        regs = {name: u.reg_read(register) for name, register in (
            ('eax', UC_X86_REG_EAX), ('ebx', UC_X86_REG_EBX), ('ecx', UC_X86_REG_ECX),
            ('edx', UC_X86_REG_EDX), ('esi', UC_X86_REG_ESI), ('edi', UC_X86_REG_EDI),
            ('ebp', UC_X86_REG_EBP), ('esp', UC_X86_REG_ESP), ('eip', UC_X86_REG_EIP))}
        table, count = read(0xA8ED2C), read(0xA8ED38)
        return dict(phase=phase, registers=regs,
            cell_at_esi_24=safe(regs['esi'] + 0x24), cell_at_esi_38=safe(regs['esi'] + 0x38),
            tile_registry=dict(table=table, count=count, header=safe(0xA8ED28, 24),
                eax_slot=safe(table + regs['eax'] * 4),
                clear_slot=safe(table + read(0xAA10B0) * 4)),
            edi_head=safe(regs['edi'], 16), edx_vtable_9c=safe(regs['edx'] + 0x9C),
            roles={address: read(int(address, 16))
                for address in inputs.wall_theater_inputs['roles']},
            recent_recalc_dispatches=recalc_dispatches[-8:])
    def complete_returns(pc):
        sp = u.reg_read(UC_X86_REG_ESP)
        for pending in active[:]:
            if pending['return_pc'] == pc and sp > pending['sp']:
                row = pending['row']
                row['result'] = u.reg_read(UC_X86_REG_EAX)
                if row['kind'] in ('building_constructor', 'building_unlimbo',
                        'overlay_constructor', 'overlay_unlimbo', 'object_uninit',
                        'deferred_drain'):
                    row['after'] = snapshot()
                if row['kind'] in ('building_constructor', 'overlay_constructor'):
                    row['constructed_body_hex'] = bytes(u.mem_read(row['this'],
                        0x720 if row['kind'] == 'building_constructor' else 0xB0)).hex()
                active.remove(pending)

    def observe(_u, pc, size, _data):
        executed.add(pc)
        if pc in (0x5471B8, 0x5471DC):
            for pending in reversed(active):
                if pending['row']['kind'] == 'tile_slope':
                    if pc == 0x5471B8:
                        pending['row']['native_image_result'] = u.reg_read(UC_X86_REG_EAX)
                    else:
                        pending['row']['zero_branch_pc'] = '0x005471DC'
                    break
        if pc == 0x47D1E7:
            # Observe the actual dispatch before it executes. No change to
            # EAX/tile, head/vtable, callback address or return is allowed.
            row = diagnostic()
            row.pop('recent_recalc_dispatches')
            recalc_dispatches.append(row)
        if phase == 'whole_autofill_588750':
            measured.add(pc)
        complete_returns(pc)
        sp, receiver = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        if pc in (0x43B740, 0x5FC380):
            objects[receiver] = 'building' if pc == 0x43B740 else 'overlay'
        if pc not in entries:
            return
        row = dict(pc=f'0x{pc:08X}', kind=entries[pc], phase=phase,
            this=receiver, return_pc=f'0x{read(sp):08X}')
        if pc == 0x440580:
            row.update(coord=list(struct.unpack('<3i', u.mem_read(read(sp + 4), 12))),
                facing=read(sp + 8))
        elif pc == 0x5678E0:
            row.update(coord=list(struct.unpack('<3i', u.mem_read(read(sp + 4), 12))),
                args=[read(sp + 4 + i * 4) for i in range(8)])
        elif pc == 0x47C620:
            row.update(coord=list(struct.unpack('<hh', u.mem_read(receiver + 0x24, 4))),
                args=[read(sp + 4 + i * 4) for i in range(3)])
        elif pc == 0x5471B0:
            row.update(subtile=read(sp + 4), head_vtable=read(receiver),
                resident_image=read(receiver + 0xA4))
        elif pc in (0x47D2B0, 0x47CA80):
            row.update(coord=list(struct.unpack('<hh', u.mem_read(receiver + 0x24, 4))),
                tile_before=signed(receiver + 0x38))
        elif pc in (0x65ACB0, 0x65A970, 0x7258D0):
            row['args'] = [read(sp + 4 + i * 4) for i in range(3 if pc == 0x65A970 else 2 if pc == 0x7258D0 else 1)]
        calls.append(row)
        active.append(dict(return_pc=read(sp), sp=sp, row=row))

    def watch_dirty(_u, _access, address, size, value, _data):
        pc = u.reg_read(UC_X86_REG_EIP)
        # Keep original constructor field writes, including timer starts and
        # the AX store, as compact raw observations. No guessed timer layout
        # is required to recover a field which the Rust consumer needs.
        for pending in reversed(active):
            row = pending['row']
            if row['kind'] in ('building_constructor', 'overlay_constructor'):
                length = 0x720 if row['kind'] == 'building_constructor' else 0xB0
                if row['this'] <= address and address + size <= row['this'] + length:
                    row.setdefault('field_writes', []).append(
                        [pc, address - row['this'], size, value])
                    break
        if address < owner + 0x577A and owner + 0x5778 < address + size:
            dirty_writes.append(dict(phase=phase, pc=f'0x{pc:08X}',
                address=address, size=size, value=value,
                before=list(u.mem_read(owner + 0x5778, 2))))

    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, watch_dirty)]
    def invoke(label, entry, receiver, *args):
        nonlocal phase
        phase = label
        f.phase = label
        before = snapshot()
        mark = len(f.draws)
        try:
            result = bc.invoke(u, entry, receiver, *args)
        except Exception as exc:
            import json
            print('WALL_AUTOFILL_NATIVE_FAULT ' + json.dumps(diagnostic(), sort_keys=True), flush=True)
            raise RuntimeError(f'{label}: original native entry0x{entry:08X} failed') from exc
        # A top-level return stops before RET_MAGIC's code hook. Resolve only
        # that real completed call using the actual returned ESP/EAX.
        complete_returns(native_ret)
        boundaries.append(dict(label=label, entry=f'0x{entry:08X}', this=receiver,
            args=list(args), result=result, before=before, after=snapshot(),
            rng_request_range=[mark - draw_start, len(f.draws) - draw_start]))
        return result

    from tools.native_oracle import RET_MAGIC as native_ret
    prior_land = None
    try:
        # Reuse full native wall entry for imported endpoint/origin material.
        # They are separate Building products, not factory-paid objects.
        for index, xy in enumerate((*endpoints, origin)):
            p = invoke(f'setup_wall_{index}_create', 0x45E880, typ, owner)
            coord = f.allocate(12)
            u.mem_write(coord, dwords(xy[0] * 256 + 128, xy[1] * 256 + 128, 0))
            require(invoke(f'setup_wall_{index}_unlimbo', 0x440580, p, coord, 0) & 255 == 1, 'native 0x440580 returned an unexpected result')
            if index + 1 == len(endpoints):
                invoke('setup_endpoint_deferred_drain', 0x725C70, 0)
                assert read(0xB0F6A8) == 0
        # HousePlace calls588750 after origin Unlimbo and before its drain.
        # Keep that origin's actual transient queue entries alive here.
        assert read(0xB0F6A8) > 0
        assert all(signed(cell(*xy) + 0x44) == signed(overlay + 0x294)
            and read(cell(*xy) + 0x50) == read(owner + 0x30) for xy in (*endpoints, origin))
        if control == 'middle_refusal':
            # Supported custom land input: GAWALL's native SpeedType=-1
            # reads Buildable while Overlay Mark uses Track1. Keep every
            # other cell's physical land table row unchanged.
            assert signed(typ + 0x67C) == -1
            land_names = [inputs.string(inputs.read32(0x839D68 + i * 4)) for i in range(12)]
            land_index = land_names.index('Rough')
            inputs.make_ini({'Rough': {'Buildable': 'yes', 'Track': '0%'}})
            inputs.invoke(0x674000, 0, (INI,))
            table = 0x89EA40 + land_index * 36
            old = bytes(u.mem_read(table, 36))
            raw = bytes(inputs.u.mem_read(table, 36))
            u.mem_write(table, raw)
            u.mem_write(cell(20, 12) + 0xEC, dwords(land_index))
            prior_land = dict(coord=[20, 12], land=land_index,
                source='Declared custom INI/map prior before whole588750; not stock-map admission',
                ini={'Rough': {'Buildable': 'yes', 'Track': '0%'}},
                reader='0x00674000', physical_row_before=old.hex(), supplied_native_row=raw.hex())
        coord = f.allocate(4)
        u.mem_write(coord, struct.pack('<hh', *origin))
        measurement_call_start = len(calls)
        measurement_draw_start = len(f.draws) - draw_start
        invoke('whole_autofill_588750', 0x588750, 0x87F7E8, coord, owner, typ)
        invoke('autofill_deferred_drain', 0x725C70, 0)
        assert not active, active
        assert f.code_unchanged()
        assert 0x588750 in measured
        if control != 'no_gap':
            assert {0x43B740, 0x440580, 0x5FC380, 0x5FD270, 0x5678E0} <= measured
        # These assert the intended branch was actually admitted, not a
        # manually supplied expected body/RNG/identity result.
        attempts = [r for r in calls[measurement_call_start:] if r['kind'] == 'building_unlimbo']
        if control == 'no_gap':
            assert not attempts
        elif control == 'two_empty':
            assert len(attempts) == 2 and all(r['result'] & 255 for r in attempts)
        else:
            results = [bool(r['result'] & 255) for r in attempts]
            assert results == [True, False, True, True], results
        return dict(input=dict(control=control, seed=31, origin=list(origin),
            endpoints=[list(xy) for xy in endpoints], owner=owner,
            type='GAWALL', custom_middle_land=prior_land,
            map=f.flat_map, geometry_startup=geometry,
            theater_roles=inputs.wall_theater_inputs,
            selected_tile=dict(clear_base=read(0xAA10B0), registry_count=read(0xA8ED38),
                head=read(read(0xA8ED2C)),
                vtable=read(read(read(0xA8ED2C))),
                resident_image=read(read(read(0xA8ED2C)) + 0xA4),
                boundary='Declared original ClearTile0 head; no resident TMP supplied. Original image/slope getters execute and their null-image branch is observed if entered.'),
            current_viewer=read(0xA83D4C), game_mode=read(0xA8B238),
            active_game=u.mem_read(0xA8E9A0, 1)[0], scenario_init_priority=read(0xA8E7AC),
            wall_owner_globals={f'0x{address:08X}': read(address)
                for address in (0x8333BC, 0x880994)},
            scenario_flags=read(SCENARIO),
            reveal_by_height=u.mem_read(f.rules + 0x17EE, 1)[0],
            boundary='Declared joined32x32 flat clear map and House/target/MTNK priors; full native endpoint wall entry/drain, origin wall entry remains pending, then whole588750 and drain. No Factory payment, HousePlace caller or physical Hills bootstrap.'),
            rng_states=rng_states, boundaries=boundaries, ordered_calls=calls,
            measurement_call_start=measurement_call_start,
            measurement_rng_request_start=measurement_draw_start,
            rng_requests=f.draws[draw_start:], ranged_raw_advances=f.advances[advance_start:],
            house_dirty_writes=dirty_writes, final=snapshot(),
            recalc_tile_dispatches=recalc_dispatches,
            measured_body_entries=[f'0x{pc:08X}' for pc in entries if pc in measured],
            original_executable_sections_unchanged=True)
    finally:
        for hook in hooks:
            u.hook_del(hook)


def wall_autofill_project(raw):
    """Pure selection/interning of original observations; no engine answers.

    Pointer identities become typed references, unknown values remain raw.
    Patch membership is the observed changed-cell union at retained states,
    plus declared origin/endpoints. Full buffers and patch rows are interned.
    """
    import json
    streams, stream_index, patch_values, patch_index = [], {}, [], {}
    object_states, object_indexes = [], {}
    call_kinds, call_kind_indexes, phases, phase_indexes = [], {}, [], {}
    patch_fields = ('tile', 'land', 'level', 'slope', 'overlay', 'data', 'owner',
        'retained_neighbor', 'flags', 'ground_frame', 'ground_raw', 'deck_raw', 'ground_head')
    def intern(value, values, indexes):
        key = tuple(value) if isinstance(value, list) else value
        if key not in indexes:
            indexes[key] = len(values)
            values.append(value)
        return indexes[key]
    controls = []
    for control in raw['controls']:
        incoming = control['input']
        bounds = control['boundaries']
        origin_label = 'setup_wall_' + str(len(incoming['endpoints'])) + '_unlimbo'
        origin = next(b for b in bounds if b['label'] == origin_label)
        selected = {'origin_unlimbo': origin,
            'autofill': next(b for b in bounds if b['label'] == 'whole_autofill_588750'),
            'drain': next(b for b in bounds if b['label'] == 'autofill_deferred_drain')}
        constructors = [c for c in control['ordered_calls']
            if c['kind'] in ('building_constructor', 'overlay_constructor')]
        refs, ids, counters = {}, {}, {'building_constructor': 0, 'overlay_constructor': 0}
        for call in constructors:
            kind = call['kind']
            ref = ('B' if kind == 'building_constructor' else 'O') + str(counters[kind])
            counters[kind] += 1
            refs[call['this']] = ref
            obj = next(o for o in call['after']['objects'] if o['pointer'] == call['this'])
            ids[ref] = obj['id']
        known = dict(refs)
        known.update({incoming['owner']: 'House:owner', 0x87F7E8: 'Map',
            raw['type']['pointer']: 'BuildingType:GAWALL',
            raw['type']['to_overlay']: 'OverlayType:GAWALL'})
        def reference(value):
            return known.get(value, value)
        def obj_state(obj):
            keep = ('kind', 'id', 'abstract_flags', 'location', 'health', 'alive',
                'limbo', 'marked', 'logic', 'sampled_health', 'constructor_ax',
                'raw_words_c0_cc', 'passive_scan_timer', 'radio_capacity')
            result = {k: obj[k] for k in keep if k in obj}
            result['ref'] = refs[obj['pointer']]
            result['type'] = reference(obj['type'])
            if 'owner' in obj:
                result['owner'] = reference(obj['owner'])
                result['radio_contacts'] = [reference(p) for p in obj['radio_contacts']]
            key = json.dumps(result, sort_keys=True, separators=(',', ':'))
            if key not in object_indexes:
                object_indexes[key] = len(object_states)
                object_states.append(result)
            return object_indexes[key]
        stages = [s for transition in selected.values() for s in
            (transition['before'], transition['after'])]
        baseline = {tuple(c['coord']): c for c in stages[0]['cells']}
        coords = {tuple(incoming['origin']), *(tuple(c) for c in incoming['endpoints'])}
        for stage in stages:
            for cell_row in stage['cells']:
                if cell_row != baseline[tuple(cell_row['coord'])]:
                    coords.add(tuple(cell_row['coord']))
        coords = sorted(coords, key=lambda c: (c[1], c[0]))
        def state(stage):
            rng = control['rng_states'][stage['rng_state']]
            cells = {tuple(c['coord']): c for c in stage['cells']}
            result = dict(cursor=stage['unique_id_cursor'], frame=stage['frame'],
                rng={name: intern(rng[name], streams, stream_index)
                    for name in ('main', 'scenario', 'mapgen')},
                objects=[obj_state(o) for o in stage['objects']],
                registries={name: [refs.get(p['pointer'], 'prior:' + str(p['id']))
                    for p in members] for name, members in stage['registries'].items()
                    if name != 'abstract'},
                house={k: v for k, v in stage['house'].items() if k != 'pointer'},
                patch=[intern([reference(cells[xy][field]) if field == 'ground_head'
                    else cells[xy][field] for field in patch_fields], patch_values,
                    patch_index) for xy in coords])
            for key in ('scenario_init_priority', 'wall_owner_globals'):
                if key in stage:
                    result[key] = stage[key]
            return result
        retained = []
        omit = {'cell_clearance', 'cell_recalc', 'cell_lat_recalc', 'tile_slope', 'tile_image_getter'}
        for call in control['ordered_calls']:
            if call['kind'] in omit or call['phase'] not in (
                    origin_label, 'whole_autofill_588750', 'autofill_deferred_drain'):
                continue
            kind = (call['kind'], call['pc'])
            if kind not in call_kind_indexes:
                call_kind_indexes[kind] = len(call_kinds)
                call_kinds.append(dict(kind=call['kind'], pc=call['pc']))
            item = [call_kind_indexes[kind], intern(call['phase'], phases, phase_indexes),
                reference(call['this']), call['return_pc'], call['result']]
            extra = {}
            if 'coord' in call:
                extra['coord'] = call['coord']
            if 'args' in call:
                extra['args'] = [reference(a) for a in call['args']]
                if call['kind'] == 'map_reveal_area':
                    extra['args'][0] = {'coord': call['coord']}
            if 'facing' in call:
                extra['facing'] = call['facing']
            if call.get('result') in refs:
                extra['result_ref'] = refs[call['result']]
            if extra:
                item.append(extra)
            retained.append(item)
        ctor_rows, unlimbo_rows = [], []
        for call in control['ordered_calls']:
            if call['kind'] in ('building_constructor', 'overlay_constructor',
                    'building_unlimbo', 'overlay_unlimbo'):
                obj = next(o for o in call['after']['objects'] if o['pointer'] == call['this'])
                row = dict(ref=refs[call['this']], phase=call['phase'],
                    result=call['result'], state=obj_state(obj))
                if 'coord' in call:
                    row['coord'] = call['coord']
                if call['kind'] == 'building_constructor':
                    body = bytes.fromhex(call['constructed_body_hex'])
                    row['mission_timer_raw_c8'] = list(struct.unpack_from('<3i', body, 0xC8))
                (ctor_rows if call['kind'].endswith('constructor') else unlimbo_rows).append(row)
        projected_input = {k: v for k, v in incoming.items()
            if k not in ('owner', 'current_viewer', 'theater_roles')}
        projected_input.update(owner=reference(incoming['owner']),
            current_viewer=reference(incoming['current_viewer']))
        requests = [{k: r[k] for k in ('entry', 'site', 'this', 'frame', 'args', 'result')}
            for r in control['rng_requests']]
        controls.append(dict(input=projected_input, native_ids=ids,
            origin_ref=refs[origin['this']], patch_coordinates=[list(xy) for xy in coords],
            transitions={name: dict(before=state(b['before']), after=state(b['after']),
                result=b['result']) for name, b in selected.items()},
            constructors=ctor_rows, unlimbo=unlimbo_rows,
            semantic_calls=retained, rng_requests=requests,
            origin_unlimbo_rng_request_range=origin['rng_request_range'],
            autofill_rng_request_range=selected['autofill']['rng_request_range'],
            house_dirty_writes=[dict(phase=w['phase'], pc=w['pc'],
                offset=w['address'] - incoming['owner'], size=w['size'],
                value=w['value'], before=w['before']) for w in control['house_dirty_writes']],
            original_executable_sections_unchanged=control['original_executable_sections_unchanged'],
            observation_gaps=[] if 'wall_owner_globals' in incoming else ['wall_owner_globals']))
    return dict(schema_version=2, source=raw['source'],
        retail_files=raw['retail_files'], type=raw['type'], physical_art=raw['physical_art'],
        native_theater_inputs=raw['native_theater_inputs'],
        native_wall_image_assets=raw['native_wall_image_assets'],
        native_overlay_layers=raw['native_overlay_layers'],
        overlay_registry_names=raw['overlay_registry_names'],
        input_layers=[{k: layer[k] for k in ('file', 'sha256', 'absent') if k in layer}
            | (dict(physical_GAWALL=layer['physical_input_sections'].get('GAWALL'),
                admitted_GAWALL=layer['admitted']['buildings']['GAWALL'])
                if 'physical_input_sections' in layer else {}) for layer in raw['input_layers']],
        rng_streams=streams, object_states=object_states,
        semantic_call_kinds=call_kinds, phases=phases,
        semantic_call_fields=['kind_index', 'phase_index', 'this', 'return_pc', 'result', 'optional_fields'],
        patch_fields=list(patch_fields), patch_values=patch_values,
        controls=controls)


def wall_autofill_generate(*, detail=False):
    import hashlib, os
    root = Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    inputs = joined_inputs(root, wall_autofill=True)
    p, u = inputs.types['GAWALL'], inputs.u
    signed = lambda offset: struct.unpack('<i', u.mem_read(p + offset, 4))[0]
    overlay = inputs.read32(p + 0xE54)
    raw = dict(schema_version=1, source='unicorn/gamemd.exe',
        retail_files=[dict(name=path.name, bytes=path.stat().st_size,
            sha256=hashlib.sha256(path.read_bytes()).hexdigest())
            for path in sorted(root.iterdir()) if path.is_file() and path.suffix.lower() != '.json'],
        input_layers=inputs.death_layers, native_overlay_layers=inputs.wall_overlay_reads,
        overlay_registry_names=inputs.wall_overlay_names,
        physical_art=inputs.death_art_input_sections.get('GAWALL'),
        native_theater_inputs=inputs.wall_theater_inputs,
        native_wall_image_assets=inputs.wall_image_assets,
        type=dict(name='GAWALL', pointer=p, strength=signed(0xA0),
            to_overlay=overlay, overlay_index=inputs.read32(overlay + 0x294),
            guard_range=signed(0x5B8), sight=signed(0x5E8), speed=signed(0x67C),
            insignificant=u.mem_read(p + 0x232, 1)[0],
            reveal_by_height=u.mem_read(inputs.rules + 0x17EE, 1)[0]),
        controls=[wall_autofill_execute(inputs, control) for control in
            ('no_gap', 'two_empty', 'middle_refusal')])
    return raw if detail else wall_autofill_project(raw)


def wall_autofill_metadata():
    return provenance(scope='Whole original588750 GAWALL autofill: native temporary Building/Overlay construction, ignored failure and deferred cleanup in existing joined component world',
        entry_points=dict(autofill=0x588750, building_create=0x45E880,
            building_ctor=0x43B740, constructor_rng=0x6F3254, building_unlimbo=0x440580,
            overlay_create=0x5FE570, overlay_ctor=0x5FC380, overlay_unlimbo=0x5FD270,
            cell_clearance=0x47C620, land_reader=0x674000, sight=0x5678E0,
            radio=0x65ACB0, uninit=0x5F65F0, expiry=0x7258D0, drain=0x725C70),
        assumptions=[
            'Existing physical layered Building/Overlay/Rules readers, joined House and flat clear map, native graph/pathfinder and target/MTNK setup are reused. This is not a paid Factory/HousePlace or full Hills execution.',
            'Original BuildingType45E880 and Building440580 create endpoint marks and drain them. A fresh origin wall enters Unlimbo and remains pending before whole588750, matching its immediate HousePlace seam; the final drain includes origin and successful fillers. No body result, ID, RNG draw, pending item, sight answer or Unlimbo result is supplied.',
            'The middle refusal is an explicit pre-entry custom Rough Buildable=yes/Track=0% original674000 output and one supplied Cell land value. It is a native branch control, not a stock map occurrence; original588750 must continue the later attempts.',
            'All three full RNG streams are interned and referenced at boundaries, with original draw requests/raw advances retained. Final freed-arena readbacks are historical bytes, not live entities. Failure survivors remain constructor-complete in limbo; no eventual shutdown claim.',
            'Default schema2 is a pure observed-data selection/interning/typed-identity projection; original producer/control execution is unchanged. Optional --autofill-detail retains the full trace only in external scratch. No expected Rust state, RNG continuation or callback answer is calculated.'
        ], substitutions=[
            'Only existing joined reader cache, physical-file/allocator/CRT/COM/audio and documented presentation transports. No new VM, instruction patch, native gameplay result substitution, fake iterator or constructor RNG stub.'
        ])


def wall_autofill_main(argv):
    import argparse
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--autofill-output-dir', type=Path, required=True)
    parser.add_argument('--autofill-detail', action='store_true')
    args, remaining = parser.parse_known_args(argv)
    repository = Path(__file__).resolve().parents[2]
    output = args.autofill_output_dir.expanduser().resolve()
    if output.is_relative_to(repository) or any(
            arg == '--output' or arg.startswith('--output=') for arg in remaining):
        raise SystemExit('Wall autofill generation requires an external scratch --autofill-output-dir; --output is not accepted')
    sources = joined_source_paths()
    sources['tools/spatial_oracle/trigger_action_values.py'] = repository / 'tools/spatial_oracle/trigger_action_values.py'
    for name in ('tools/spatial_oracle/tmp_clear_render.py',
                 'tools/rules_oracle/theater_general_reader.py',
                 'tools/spatial_oracle/terrain_recalc.py'):
        sources[name] = repository / name
    name = 'building_death_anims_wall_autofill' + ('_detail' if args.autofill_detail else '')
    finish_vectors(lambda: wall_autofill_generate(detail=args.autofill_detail), output / (name + '.json'),
        provenance=wall_autofill_metadata, argv=remaining, source_paths=sources)


def typed_debris_generate():
    """Actual admitted MTNK fatal receiver and adjacent death-Anim allocation."""
    import os, hashlib
    from collections import deque
    from types import SimpleNamespace
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.spatial_oracle.building_body_rules import SP, INI, dwords
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.projectile_oracle.ifv_isolated_tail import impact
    from tools.spatial_oracle.anim_bouncer_launch import body_fields, constructor_state
    from tools.procedural_drawing_oracle.electric_bolt import RngTrace
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_ESI
    root = Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    m = joined_inputs(root, typed_debris=True); u = m.u
    proxy = SimpleNamespace(m=m, trace=deque(maxlen=40), events=[], pending={}, frame=0, phase='setup')
    def platform(_u, pc, size, data):
        if pc in JOINED_TRANSPORT - {0x527AF9, 0x527B0C}:
            Mission.observe(proxy, _u, pc, size, data)
        elif pc == 0x7C978A:
            m.ret(0)
    hook = u.hook_add(UC_HOOK_CODE, platform)
    m.invoke(0x4E7660, 0); art = bytes(u.mem_read(INI, 0x40))
    patch = {'DebrisTypes':'TIRE', 'DebrisMaximums':'4', 'MinDebris':'1', 'MaxDebris':'3'}
    m.make_ini({'MTNK':patch}); rules=m.alloc(0x40)
    u.mem_write(rules, bytes(u.mem_read(INI, 0x40)));u.mem_write(INI, art)
    m.invoke(0x747620, m.types['MTNK'], (rules,))
    tire=m.read32(m.read32(m.types['MTNK']+0x318))
    sections,_=lexical((root/'RULESMD.INI').read_bytes(), {'TIRE'})
    m.make_ini(sections);rules=m.alloc(0x40)
    u.mem_write(rules, bytes(u.mem_read(INI, 0x40)));u.mem_write(INI, art)
    u.mem_write(SP,dwords(native.RET_MAGIC,rules));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,tire)
    native.run_checked(u,0x74B050,0x74B56C);u.mem_write(INI,art);u.hook_del(hook)
    f=joined_fixture(m,38,seed=1,unit_startup=True);u=f.u;read=f.read32
    bc.invoke(u,0x40EE40,0)
    scenario=read(0xA8B230)
    # Attach the existing trace observer without reseeding the admitted world.
    rng=RngTrace.__new__(RngTrace);rng.u=u
    rng.streams={'main':0x886B88,'scenario':scenario+0x218,'mapgen':0xABE890}
    rng.requests=[];rng.pending=[];u.hook_add(UC_HOOK_CODE,rng.observe)
    before=rng.states();cursor=read(scenario+0x214)
    def xyz(p):return list(struct.unpack('<3i',u.mem_read(p,12)))
    def member(p,base):return p in [read(read(base+4)+4*i) for i in range(read(base+16))]
    owner=read(f.source+0x21C)
    old_rng=f.rng();owner_human=bool(bc.invoke(u,0x50B6F0,owner)&255)
    assert old_rng==f.rng()
    def sound_name(index):
        if index < 0:return None
        assert index < read(0xB1D388)
        return f.string(read(read(read(0xB1D37C)+index*4))+0x6C)
    sound_vectors={}
    for key,offset in [('voice_die',0x4BC),('die_sound',0x510)]:
        t=f.types['MTNK'];indices=[read(read(t+offset+4)+i*4) for i in range(read(t+offset+16))]
        sound_vectors[key]=dict(count=len(indices),entries=[dict(index=i,name=sound_name(i)) for i in indices])
    prior=dict(type='MTNK',native_id=read(f.source+0x10),health=read(f.source+0x6C),location=xyz(f.source+0x9C),
        owner_native_id=read(owner+0x10),owner_human=owner_human,house_human_flags=list(u.mem_read(owner+0x1EC,2)),
        session_mode=read(0xA8B238),sound_vectors=sound_vectors,logic_registered=member(f.source,0x87F778),
        alive=u.mem_read(f.source+0x90,1)[0],limbo=u.mem_read(f.source+0x81,1)[0],marked=u.mem_read(f.source+0x74,1)[0])
    events=[];pending=[];voxels=[];anims=[]
    def event(kind,**kw):events.append(dict(event=kind,rng_request_count=len(rng.requests),native_cursor=read(scenario+0x214),**kw))
    def observe(_u,pc,size,data):
        sp=u.reg_read(UC_X86_REG_ESP);this=u.reg_read(UC_X86_REG_ECX)
        if pending and (pc,sp)==pending[-1][:2]:
            _,_,kind,p=pending.pop()
            if kind=='VoxelCtor':
                raw=bytes(u.mem_read(p+0xB0,0x50))
                row=dict(type=f.string(read(p+0x104)+0x24),native_id=read(p+0x10),owner_is_null=read(p+0x10C)==0,
                    owner_native_id=read(read(p+0x10C)+0x10) if read(p+0x10C) else None,
                    duration=read(p+0x140),location=xyz(p+0x9C),bounce_hex=raw.hex(),bounce=body_fields(raw),
                    logic_registered=member(p,0x87F778),registry_registered=member(p,0x887388),
                    display_layers=[i for i in range(5) if member(p,0x8A0360+24*i)],marked_for_deletion=bool(u.mem_read(p+0x110,1)[0]))
                voxels.append(row);event('VoxelCtor_return',native_id=row['native_id'],rng_scenario=rng.states()['scenario'])
            else:
                row=dict(pointer=p,type=f.string(read(p+0xC8)+0x24),**constructor_state(u,p));anims.append(row)
                event('AnimCtor_return',native_id=row['native_id'],rng_scenario=rng.states()['scenario'])
        kinds={0x737C90:'UnitReceiveDamage',0x701900:'TechnoReceiveDamage',0x7493B0:'VoxelCtor',
            0x749453:'VoxelIdentity',0x410230:'CreateId',0x68BCB0:'NextNativeId',0x421EA0:'AnimCtor',
            0x70D690:'DeathWeapon',0x466380:'BulletCtor',0x4397E0:'BounceInit',0x7509E0:'Sound',0x517A50:'InfantryCtor',0x707D20:'CrewType'}
        if pc not in kinds:return
        kind=kinds[pc];kw=dict(pc=f'{pc:08X}',caller=f'{read(sp):08X}')
        if kind in ('VoxelCtor','AnimCtor'):
            kw.update(type=f.string(read(sp+4)+0x24),position=xyz(read(sp+8)),rng_scenario=rng.states()['scenario'])
            pending.append((read(sp),sp+(16 if kind=='VoxelCtor' else 32),kind,this))
        if kind=='VoxelIdentity':kw['native_id']=read(u.reg_read(UC_X86_REG_ESI)+0x10)
        if kind=='Sound':
            index=struct.unpack('<i',dwords(this))[0];kw.update(index=index,name=sound_name(index))
        event(kind,**kw)
    u.hook_add(UC_HOOK_CODE,observe)
    damage=f.allocate(4);u.mem_write(damage,dwords(1000))
    result=bc.invoke(u,0x737C90,f.source,damage,0,m.warhead,0,1,0,0)
    assert not pending and not rng.pending
    for row in anims:
        p=row.pop('pointer');row.update(constructor_state(u,p),logic_registered=member(p,0x87F778),
            display_layers=[i for i in range(5) if member(p,0x8A0360+24*i)],observation='receiver_return')
    assert [v['native_id'] for v in voxels]==[cursor+1,cursor+2]
    assert [a['type'] for a in anims]==['S_BRNL58'] and anims[0]['native_id']==cursor+3
    return dict(sound_sha256=hashlib.sha256((root/'SOUNDMD.INI').read_bytes()).hexdigest(),rows=[dict(
        input=dict(name='fatal_mtnk_typed_then_death_anim',seed=1,damage=1000,distance=0,warhead=f.string(m.warhead+0x24),
            ignore_defenses=True,prevent_passenger_escape=False,attacker_is_null=True,source_house_is_null=True,
            binary_frame=read(bc.FRAME),debris_types=['TIRE'],debris_maximums=[4],minimum=1,maximum=3),
        source_before=prior,result=result,damage_after=read(damage),events=events,voxels=voxels,anims=anims,
        rng_before=before,rng_after=rng.states(),rng_requests=rng.requests,native_cursor_before=cursor,native_cursor_after=read(scenario+0x214),
        source_after=dict(health=read(f.source+0x6C),alive=u.mem_read(f.source+0x90,1)[0],limbo=u.mem_read(f.source+0x81,1)[0]))])


def typed_debris_metadata():
    return provenance(scope='One whole native MTNK fatal737C90→701900 typed TIRE construction and adjacent S_BRNL58 identity/RNG control',
        assumptions=[
            'Existing physical joined_inputs/joined_fixture own all reader/VM/asset/House/map/registry transport. Actual UnitType reader747620 reads authored MTNK DebrisTypes=TIRE/Maximums4/Min1/Max3. Original TIRE scalar reader74B050 stops at74B56C before VXL I/O, with no completed reader return supplied and no model-binding claim.',
            'Full source MTNK7353C0/Unlimbo737BA0 run in the supplied admitted flat-map/House world. Actual recorded prior is health300, nativeID3, ownerID0, coordinate2944,2176,0, frame0. Direct damage1000/AP/distance0/ignoreDefenses1/null attacker/null sourceHouse enters whole737C90. This is a receiver control, not a full launch or earlier-world identity-prefix claim.',
            'Actual full fatal body emits TIRE4/TIRE5 before ordinary S_BRNL58 Anim6. CreateID precedes seven Voxel constructor draws. Full three-stream before/after state, ordered original requests/raw words and Bounce bodies are retained. Voxel state is constructor-return; Anim state/membership is receiver-return. No stock MTNK DeathWeapon is reached. Observation ends at receiver return; later Logic/deferred deletion is excluded.',
            'The typed-only physical selected SoundList includes the actual MTNK VoiceDie/DieSound references before full UnitType read. Retail VoiceDie is empty and DieSound resolves GenVehicleDie; original7021E4 consumes Main2025287381 before the17 Scenario requests. Original7509E0 executes without a supplied playback result; actual sound vectors, owner IsHuman50B6F0 and sound entries are retained.',
            'All allocations succeed. Null-model TIRE numeric/admission fields are bounded here; actual Rust physical loading and GPU output are independent checks. No Voxel/Anim/ID/RNG/GetCoords/Unlimbo/registry success is supplied.',
        ], substitutions=joined_metadata()['substitutions'], entry_points={
            'unit_type_reader':0x747620,'tire_scalar_reader':0x74B050,'model_boundary':0x74B56C,
            'unit_ctor':0x7353C0,'unit_unlimbo':0x737BA0,'unit_damage':0x737C90,'techno_damage':0x701900,
            'typed_selector':0x70232B,'voxel_ctor':0x7493B0,'create_id':0x410230,'bounce':0x4397E0,'death_anim_ctor':0x421EA0})


def typed_debris_main(argv):
    finish_vectors(typed_debris_generate,Path(__file__).with_name('building_death_anims_typed_debris.json'),
                   provenance=typed_debris_metadata,argv=argv)



def grapple_inputs(root, *, drone=False, firing=False):
    """Extend the existing physical reader for the selected stock SQD/DEST route."""
    from collections import deque
    from types import SimpleNamespace
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_ESI,UC_X86_REG_EDI
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.spatial_oracle.building_body_rules import INI,SP,dwords
    import hashlib
    m=joined_inputs(root);u=m.u;read=m.read32
    m.grapple_layers=[]
    proxy=SimpleNamespace(m=m,trace=deque(maxlen=64),events=[],pending={},frame=0,phase='setup')
    def transport(u,pc,n,d):
     if pc in JOINED_TRANSPORT-{0x527af9,0x527b0c}:Mission.observe(proxy,u,pc,n,d)
     elif pc==0x7c978a:m.ret(0)
    h=u.hook_add(UC_HOOK_CODE,transport)
    m.invoke(0x4e73e0,0)
    actors=['DRON','MTNK'] if drone else ['SQD','DEST']
    weapon,projectile,warhead=('DroneJump','JUMP','Parasite') if drone else ('SquidGrab','SQDJUMP','ParasitePlus')
    artall=sections((root/'ARTMD.INI').read_bytes());animnames=[] if drone else ['SQDG','WAKE1','H2O_EXP1','H2O_EXP2','H2O_EXP3']
    artnames=['DRON','GTNK'] if drone else ['SQD','DEST','ASW']
    if drone and firing:
     artnames.append('DRONP')
    art={k:v for k,v in m.death_art_input_sections.items() if v};art.update({k:artall[k] for k in artnames+animnames if k in artall})
    for n in animnames:m.assets[n+'.SHP']=(root/(n+'.SHP')).read_bytes()
    sound=sections((root/'SOUNDMD.INI').read_bytes());wanted=set()
    for n in animnames:
     for k in ['Report','StartSound']:wanted.update(art.get(n,{}).get(k,'').split(','))
    if drone:
     for filename in ['RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map']:
      path=root/filename
      if not path.exists():continue
      physical=sections(path.read_bytes())
      for name in actors:
       for key,value in physical.get(name,{}).items():
        if key.startswith('Voice') or key.endswith('Sound'):wanted.update(value.split(','))
    wanted.discard('');selected={k:v for k,v in sound.items() if k in wanted|{'Defaults'}};selected['SoundList']={k:v for k,v in sound['SoundList'].items() if v in wanted};m.make_ini(selected);m.invoke(0x7510d0,INI)
    m.make_ini(art);artcache=bytes(u.mem_read(INI,0x40))
    for name in actors:
     t=m.alloc(0xf00);m.invoke(0x7470d0,t,(m.cstring(name),));m.types[name]=t
    w=m.invoke(0x772fa0,m.cstring(weapon))
    for fn in ['RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map']:
     p=root/fn
     if not p.exists():
      assert fn=='LANGRULE.INI';m.grapple_layers.append(dict(file=fn,absent=True));continue
     m.grapple_layers.append(dict(file=fn,sha256=hashlib.sha256(p.read_bytes()).hexdigest()))
     all=sections(p.read_bytes());sel={k:v for k,v in all.items() if k in {*actors,weapon,projectile,warhead,'ASW','General','CombatDamage'}}
     m.make_ini(sel);rules=m.alloc(0x40);u.mem_write(rules,bytes(u.mem_read(INI,0x40)));u.mem_write(INI,artcache)
     for name in actors:m.invoke(0x747620,m.types[name],(rules,))
     if not drone:
      asw=read(m.types['DEST']+0xd58);m.types['ASW']=asw;m.invoke(0x41cc20,asw,(rules,))
     m.invoke(0x772080,w,(rules,));m.invoke(0x46bee0,read(w+0xa0),(rules,));m.invoke(0x75d3a0,read(w+0xac),(rules,));m.invoke(0x7729f0,w)
     for a,b in [(0x66d847,0x66d894),(0x66c184,0x66c287)]:
      u.mem_write(SP-4,dwords(0));u.reg_write(UC_X86_REG_ESP,SP-4 if a==0x66d847 else SP);u.reg_write(UC_X86_REG_ESI,m.rules);u.reg_write(UC_X86_REG_EDI,rules);run_checked(u,a,b)
    u.mem_write(INI,artcache)
    anim_rows=[]
    for name in animnames:
     t=m.invoke(0x428b80,m.cstring(name));result=m.invoke(0x427d00,t,(INI,));anim_rows.append(dict(pointer=t,**m.result(name,t,result)))
    u.hook_del(h)
    if drone:
     from tools.procedural_drawing_oracle.electric_bolt import read_spark_types,prepare_spark_capacity
     prepare_spark_capacity(m,(0xA83C98,0xA83D98,0xA83D68,0x8874C0))
     layers=[(fn,None,(root/fn).read_bytes() if (root/fn).exists() else None)
         for fn in ['RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map']]
     h=u.hook_add(UC_HOOK_CODE,transport)
     read_spark_types(m,layers,cache=m.make_ini,ini_pointer=INI)
     u.hook_del(h)
    m.grapple_weapon=w;m.grapple_anims=anim_rows
    return m


def grapple_fixture(inputs, root, *, seed=1, drone=False, start_attached=True):
    """Fresh stock actors on the shared runtime's declared water projection.

    By default this calls Object::Limbo directly before AttachTo; native FireAt
    and the Bullet producer are excluded. With start_attached=False both actors
    remain at their original constructor/Unlimbo boundary so the caller can run
    the ordinary FireAt/Bullet producer. Every actor remains genuinely alive.
    """
    from types import SimpleNamespace
    import hashlib
    from tools.spatial_oracle import building_construction as bc
    from tools.spatial_oracle.refinery_dock import cell
    from tools.spatial_oracle.anytown_damage import next_family_native as identity
    from tools.spatial_oracle.anytown_damage.navigation_inputs import bind_primary_tiles
    from tools.rules_oracle.bridge_anim_inputs import Reader
    previous=identity.ri.ASSETS;identity.ri.ASSETS=root
    try:theater=identity.theater()
    finally:identity.ri.ASSETS=previous
    f=joined_fixture(inputs,38,seed=seed,unit_startup=True);u=f.u;read=f.read32
    u.mem_write(bc.FRAME,dwords(0))
    u.mem_write(0xA8B218,bytes(inputs.u.mem_read(0xA8B218,24)))
    for entry in (0x69EBF0,0x4E6BE0,0x629190,0x6B6C10):bc.invoke(u,entry,0)
    for house in f.owner_houses:
        for offset in (0x553C,0x558C):bc.invoke(u,0x49F9B0,house+offset)
        bc.invoke(u,0x748FD0,house+0x328)
    # Adapt only transport API names to the same VM and allocator. No native
    # object constructor, map query, numeric kernel or gameplay answer is supplied.
    view=SimpleNamespace(u=u,alloc=f.allocate,read32=read)
    view.invoke=lambda address,obj,args=():bc.invoke(u,address,obj,*args)
    view.cstring=lambda name:Reader.cstring(view,name)
    u.mem_write(0xA8ED28,dwords(0x7EB6D4,0x44000000,theater['count'],1,theater['count'],10))
    water=0 if drone else theater['globals'][0xAA0738]
    tiles=bind_primary_tiles(view,theater,[water],preserve_abstract_registry=True)
    for address,value in theater['globals'].items():u.mem_write(address,dwords(value))
    for y in range(7,10):
        for x in range(7,11):
            p=cell(x,y);u.mem_write(p+0x38,dwords(water));u.mem_write(p+0x11A,b'\0')
    # Derive LandType from physical TMP through the real owner before admission.
    # Otherwise Ship Unlimbo correctly refuses the inherited clear-land prior.
    for y in range(7,10):
        for x in range(7,11):bc.invoke(u,0x47D2B0,cell(x,y),0xFFFFFFFF)
    def actor_state(p):
        owner=read(p+0x21C)
        # FacingClass::Current4C93D0 returns the supplied DirStruct pointer;
        # Parasite helper629736 passes output storage, then reads that DWORD.
        current=f.allocate(4);u.mem_write(current,dwords(0))
        require(bc.invoke(u,0x4C93D0,p+0x388,current)==current, 'native 0x4C93D0 returned an unexpected result')
        result=dict(native_id=read(p+0x10),alive=u.mem_read(p+0x90,1)[0],
            limbo=u.mem_read(p+0x81,1)[0],health=read(p+0x6C),
            location=list(struct.unpack('<3i',u.mem_read(p+0x9C,12))),
            owner_native_id=read(owner+0x10),owner_slot=read(owner+0x30),
            owner_alliances=read(owner+0x5788),
            primary_facing_hex=bytes(u.mem_read(p+0x388,20)).hex(),
            primary_current_raw=read(current))
        if drone:
            require(bc.invoke(u,0x4C93D0,p+0x3A0,current)==current, 'native 0x4C93D0 returned an unexpected result')
            result.update(secondary_current_raw=read(current),secondary_facing_hex=bytes(u.mem_read(p+0x3A0,20)).hex())
        return result
    def manager_timers(p):
        return {name:dict(start=read(p+offset),opaque=read(p+offset+4),duration=read(p+offset+8))
            for name,offset in [('timer_2c',0x2C),('timer_38',0x38)]}
    actors=[];admissions=[]
    names=['DRON','MTNK'] if drone else ['SQD','DEST']
    for name,xy,house in [(names[0],(8,8),f.owner_houses[0]),(names[1],(9,8),f.owner_houses[1])]:
        p=f.allocate(0x1000);bc.invoke(u,0x7353C0,p,f.types[name],house)
        c=cell(*xy);before_tile=read(c+0x38);coord=f.allocate(12)
        u.mem_write(coord,dwords(xy[0]*256+128,xy[1]*256+128,0))
        answer=bc.invoke(u,0x737BA0,p,coord,128)&255
        assert answer==1 and read(c+0x38)==water and read(c+0xEC)==(0 if drone else 2)
        assert actor_state(p)['alive']==1 and actor_state(p)['limbo']==0
        actors.append(p);admissions.append(dict(type=name,**actor_state(p),
            unlimbo_al=answer,unlimbo_facing_input=128,tile_before=before_tile,tile_after=read(c+0x38),land_after=read(c+0xEC)))
    source,victim=actors;manager=read(source+0x69C)
    if drone:
        from tools.voxel_oracle.raster import render_native
        from tools.procedural_drawing_oracle.electric_bolt import prepare_spark_capacity
        from tools.native_slope import initialize_slope_tables
        from tools.native_oracle import RET_MAGIC
        from tools.spatial_oracle.building_body_rules import SP
        initialize_slope_tables(u,SP-0x1000,RET_MAGIC)
        for address in (0xA83C98,0xA83D98):u.mem_write(address,bytes(inputs.u.mem_read(address,24)))
        prepare_spark_capacity(SimpleNamespace(u=u,alloc=f.allocate),(0xA83D68,0x8874C0))
        files={name:(root/name).read_bytes() for name in ['GTNK.VXL','GTNK.HVA','VOXELS.VPL']}
        loaded=render_native(files['GTNK.VXL'],files['GTNK.HVA'],files['VOXELS.VPL'],drive_pose=dict(frame=0))['drive']
        header=f.allocate(32);u.mem_write(header,bytes.fromhex(loaded['model_header_hex']))
        u.mem_write(f.types['MTNK']+0xB0,dwords(header))
        for offset,bits in zip((0x360,0x368),loaded['half_size_bits']):u.mem_write(f.types['MTNK']+offset,struct.pack('<Q',int(bits,16)))
        admitted=bc.invoke(u,0x6F9E10,victim)&255;assert admitted==1
        f.drone_model_input=dict(**{key:loaded[key] for key in ('model_header_hex','is_voxel','first_section_size','half_size_bits')},
            joined_is_voxel_al=admitted,files={name:dict(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest()) for name,raw in files.items()})
    assert read(manager+0x10)==0xFFFFFFFF
    if start_attached:
        limbo=bc.invoke(u,0x5F4D30,source)&255;assert limbo==1
        attach_entry=[actor_state(p) for p in actors]
        timers_before_attach=manager_timers(manager)
        assert [(a['alive'],a['limbo']) for a in attach_entry]==[(1,1),(1,0)]
        can_infect=bc.invoke(u,0x62A8E0,manager,victim)&255;assert can_infect==1
        bc.invoke(u,0x62A980,manager,victim)
        assert read(manager+0x28)==victim and read(victim+0x694)==source
    else:
        limbo,can_infect=None,None
        attach_entry=[actor_state(p) for p in actors]
        timers_before_attach=manager_timers(manager)
    grapple_entry=[actor_state(p) for p in actors]
    assert [(a['alive'],a['limbo']) for a in grapple_entry]==[(1,int(start_attached)),(1,0)]
    scenario=read(0xA8B230)
    # This component does not run Scenario map lighting or a cell relight.
    # Preserve that supplied lighting prior, rather than imply reset defaults.
    lighting=dict(normal_profile={name:struct.unpack('<i',dwords(read(scenario+offset)))[0]
        for name,offset in [('ambient_percent',0x352C),('red_percent',0x3534),
            ('green_percent',0x3538),('blue_percent',0x353C),('ground_units',0x3540),('level_units',0x3544)]},
        scenario_fields={f'{offset:04X}':read(scenario+offset)
            for offset in range(0x3528,0x3548,4)},
        effect_globals={f'{address:08X}':read(address) for address in (0xA9FAB4,0xA9FABC,0xA9FAC0)},
        cells=[dict(cell=[x,8],level=u.mem_read(cell(x,8)+0x11B,1)[0],
            fields_104=list(struct.unpack('<i4h',u.mem_read(cell(x,8)+0x104,12))),
            ground_z_adjust=struct.unpack('<h',u.mem_read(cell(x,8)+0x10A,2))[0]) for x in (8,9)])
    f.grapple_source,f.grapple_victim,f.grapple_manager=source,victim,manager
    f.grapple_context=dict(seed=seed,frame=0,
        game_speed=struct.unpack('<i',dwords(read(0xA8EB60)))[0],actor_admissions=admissions,
        source_limbo_al=limbo,attach_entry=attach_entry,can_infect_al=can_infect,
        grapple_entry=grapple_entry,manager_native_id=struct.unpack('<i',dwords(read(manager+0x10)))[0],
        manager_timers_before_attach=timers_before_attach,manager_timers_grapple_entry=manager_timers(manager),
        lighting=lighting,
        water=dict(tile=water,theater_count=theater['count'],theater_sha256=theater['sha256'],
            selected_tmp=tiles['added_rows'],projection_bounds=[7,7,10,9],
            recalc_entry='0047D2B0'),native_cursor=read(read(0xA8B230)+0x214))
    if drone:
        w=inputs.grapple_weapon;wh=read(w+0xAC)
        f.grapple_context.update(model_input=f.drone_model_input,
            type_inputs=[dict(name=name,strength=read(f.types[name]+0xA0),
                weight_bits=f'{struct.unpack("<Q",u.mem_read(f.types[name]+0x370,8))[0]:016x}',
                primary_flh=list(struct.unpack('<3i',u.mem_read(f.types[name]+0x89C,12)))) for name in names],
            weapon_input=dict(name=f.string(w+0x24),damage=read(w+0xA4),rof=read(w+0xB0),
                warhead=f.string(wh+0x24),paralyzes=read(wh+0x170)),
            spark_type_inputs=inputs.effective_types,sound_names=inputs.sound_names)
    if not start_attached:
        f.grapple_context['entry_boundary']='original constructors and Unlimbo; before FireAt, Limbo and AttachTo'
    assert f.code_unchanged()
    return f



# Selected Suicide controls reuse this module's joined readers/runtime.


def suicide_fire_inputs(root, scenario_map):
    """Original stock TERROR readers on the existing joined input VM.

    Native: InfantryType5236A0/5240A0, Weapon772080, C4 block66C304 and
    fixed ART427D00. Existing IO/parser/registry owners remain authoritative.
    """
    import hashlib
    from collections import deque
    from types import SimpleNamespace
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ESI, UC_X86_REG_EDI
    from tools import native_oracle as native
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.spatial_oracle.building_body_rules import INI, SP
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.rules_oracle.weapon_speed_order import RetailRulesInputs
    from tools.rules_oracle.weapon_laser import physical_layers

    m = joined_inputs(root, damage_sound=True)
    m.reserve_arena(0x1000000)
    u, read = m.u, m.read32
    rules = sections((root / 'RULESMD.INI').read_bytes())
    art = sections((root / 'ARTMD.INI').read_bytes())
    sound = sections((root / 'SOUNDMD.INI').read_bytes())
    catalog = RetailRulesInputs(native.configured_gamemd().parent)
    # The shared world still owns its historical Hills input projection. Check
    # that replacing the scenario's selected rules with this map is identical
    # for all shared fields; no map geometry/loading answer is supplied here.
    shared_names = ('General', 'AudioVisual', 'CombatDamage', 'GAPOWR', 'E1',
        'MTNK', '105mm', '105mmE', 'Cannon', 'AP', 'Americans', 'ElevationModel', 'Radiation')
    base_map = sections((root / 'Hills.map').read_bytes())
    selected_map = sections(scenario_map.read_bytes())
    assert {n: base_map.get(n) for n in shared_names} == {
        n: selected_map.get(n) for n in shared_names}
    asset_rows = []
    corpse_names = [m.string(read(read(m.rules + 0x124) + i * 4) + 0x24)
                    for i in range(read(m.rules + 0x130))]
    for name in ('TRST', 'MININUKE', *corpse_names):
        filename = name + '.SHP'
        raw = catalog.lookup(filename)
        assert raw is not None, filename
        m.assets[filename] = raw
        source = catalog.requests[filename][0]
        asset_rows.append(dict(file=filename, bytes=len(raw),
            sha256=hashlib.sha256(raw).hexdigest(),
            sources=[v['source'] for v in source['candidates']]))
    wanted = set()
    for key, value in rules['TERROR'].items():
        if key.startswith('Voice') or key in ('DieSound', 'CrushSound'):
            wanted.update(v.strip() for v in value.split(','))
    wanted.update(art['MININUKE'].get('Report', '').split(','))
    wanted.discard('')
    selected_sound = {n: sound[n] for n in ('Defaults', *sorted(wanted)) if n in sound}
    selected_sound['SoundList'] = {k: v for k, v in sound['SoundList'].items() if v in wanted}
    m.make_ini(selected_sound)
    m.invoke(0x7510D0, INI)
    selected_art = {n: art[n] for n in ('TRST', art['TRST']['Sequence'],
                    'MININUKE', *corpse_names) if n in art}
    m.make_ini(selected_art)
    art_cache = bytes(u.mem_read(INI, 0x40))
    actor_type = m.alloc(0x1900)
    m.invoke(0x5236A0, actor_type, (m.cstring('TERROR'),))
    m.types['TERROR'] = actor_type
    weapon = m.invoke(0x772FA0, m.cstring('TerrorBomb'))
    ctor = dict(strength=read(actor_type + 0xA0),
                suicide=u.mem_read(weapon + 0x144, 1)[0])
    country = next(v['pointer'] for v in m.death_country_sides['countries']
                   if v['name'] == 'Confederation')
    proxy = SimpleNamespace(m=m, trace=deque(maxlen=40), events=[], pending={}, frame=0, phase='setup')

    def transport(_u, pc, size, data):
        if pc in JOINED_TRANSPORT - {0x527AF9, 0x527B0C}:
            Mission.observe(proxy, _u, pc, size, data)

    hook = u.hook_add(UC_HOOK_CODE, transport)
    layers = physical_layers()
    layers[-1] = (scenario_map.name, 'declared_scenario_file', scenario_map.read_bytes())
    rows = []
    consumed_keys = dict(TERROR=('Image', 'Strength', 'Primary', 'DeathWeapon', 'Explodes',
        'DeathWeaponDamageModifier', 'DieSound', 'VoiceFeedback', 'RequiredHouses'),
        TerrorBomb=('Damage', 'ROF', 'Range', 'Projectile', 'Warhead', 'Suicide', 'Anim'),
        TerrorBombWH=('CellSpread', 'PercentAtMax', 'InfDeath', 'Fire', 'Bright', 'AnimList', 'Verses'),
        Super=('InfDeath', 'ProneDamage', 'Verses'), CombatDamage=('C4Warhead',))
    try:
        for filename, archive, raw in layers:
            if raw is None:
                assert not (root / filename).exists()
                rows.append(dict(file=filename, absent=True))
                continue
            if filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini'):
                assert (root / filename).read_bytes() == raw
            physical = sections(raw)
            selected = {n: physical[n] for n in ('TERROR', 'TerrorBomb', 'TerrorBombWH',
                        'Super', 'Invisible', 'Confederation', 'CombatDamage') if n in physical}
            m.make_ini(selected)
            ini = m.alloc(0x40)
            u.mem_write(ini, bytes(u.mem_read(INI, 0x40)))
            u.mem_write(INI, art_cache)
            admitted = dict(infantry=m.invoke(0x5240A0, actor_type, (ini,)) & 255,
                            weapon=m.invoke(0x772080, weapon, (ini,)) & 255)
            admitted['projectile'] = m.invoke(0x46BEE0, read(weapon + 0xA0), (ini,)) & 255
            if 'CombatDamage' in selected:
                u.mem_write(SP, bytes(0x300))
                for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, m.rules), (UC_X86_REG_EDI, ini)):
                    u.reg_write(reg, value)
                native.run_checked(u, 0x66C304, 0x66C34C)
            for wh in sorted({read(m.rules + 0xFA8), read(weapon + 0xAC)}):
                if wh:
                    m.invoke(0x75D3A0, wh, (ini,))
            m.invoke(0x7729F0, weapon)
            m.invoke(0x511850, country, (ini,))
            rows.append(dict(file=filename, archive=archive, bytes=len(raw),
                sha256=hashlib.sha256(raw).hexdigest(), admitted=admitted,
                physical={n: {k: physical[n][k] for k in keys if k in physical[n]}
                          for n, keys in consumed_keys.items() if n in physical},
                native_after=dict(strength=read(actor_type + 0xA0),
                    suicide=u.mem_read(weapon + 0x144, 1)[0],
                    death_weapon=m.string(read(actor_type + 0xD18) + 0x24))))
        u.mem_write(INI, art_cache)
        animation_rows = []
        for name in ('MININUKE', *corpse_names):
            pointer = m.invoke(0x428B80, m.cstring(name))
            admitted = m.invoke(0x427D00, pointer, (INI,)) & 255
            shape = read(pointer + 0xA4)
            assert shape
            animation_rows.append(dict(name=name, admitted=admitted,
                frames=struct.unpack('<H', u.mem_read(shape + 6, 2))[0], art=art[name]))
    finally:
        u.hook_del(hook)
    bank = read(actor_type + 0xE3C)
    m.suicide_country, m.suicide_weapon = country, weapon
    m.suicide_inputs = dict(layers=rows, constructor=ctor, assets=asset_rows,
        shared_map_rules_projection_equal=True, shared_map_sections=list(shared_names),
        native=dict(image=m.string(actor_type + 0x1F8), strength=read(actor_type + 0xA0),
            primary=m.string(read(actor_type + 0x898) + 0x24),
            death_weapon=m.string(read(actor_type + 0xD18) + 0x24),
            explodes=u.mem_read(actor_type + 0xD15, 1)[0],
            death_modifier_f32_bits=bytes(u.mem_read(actor_type + 0xD1C, 4)).hex(),
            suicide=u.mem_read(weapon + 0x144, 1)[0], damage=read(weapon + 0xA4),
            rof=read(weapon + 0xB0), range_leptons=read(weapon + 0xB4),
            projectile=m.string(read(weapon + 0xA0) + 0x24),
            warhead=m.string(read(weapon + 0xAC) + 0x24),
            c4_warhead=m.string(read(m.rules + 0xFA8) + 0x24),
            fire_up=read(actor_type + 0xE40),
            die2=list(struct.unpack('<9i', u.mem_read(bank + 12 * 36, 36))),
            corpse_names=corpse_names), fixed_art={name:{k:v for k,v in values.items()
                if name not in ('TRST',art['TRST']['Sequence']) or k in
                ('Sequence','FireUp','Crawls','Ready','Guard','Die2')}
                for name,values in selected_art.items()},
        fixed_inis=[dict(file=name,bytes=(root/name).stat().st_size,
            sha256=hashlib.sha256((root/name).read_bytes()).hexdigest())
            for name in ('ARTMD.INI','SOUNDMD.INI')],
        sound_names=sorted(n for n in wanted if n in sound))
    return m


def suicide_fire_execute(inputs, *, ordinary=False, wounded=False, seed=31):
    """Actual whole source receivers/FireAt/AI on the shared admitted world.

    Native self-call6FDDFD uses Strength, not Health; the75/17 controls share
    one original receiver. No gameplay result, RNG word or sequence is supplied.
    """
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_EDI, UC_X86_REG_EIP
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc, engineer_repair_admission as er
    from tools.input_oracle.unit_voice_playback import VoiceHistory
    from tools.procedural_drawing_oracle.electric_bolt import RngTrace

    f = joined_fixture(inputs, 750, seed=seed, building_ai=True, unit_startup=True)
    u, read = f.u, f.read32
    bc.invoke(u, 0x561910, 0)  # Original369-pair CellSpread CRT prerequisite.
    u.mem_write(er.HOUSE + 0x34, dwords(inputs.suicide_country))
    actor = f.allocate(0x1000)
    bc.invoke(u, 0x517A50, actor, f.types['TERROR'], er.HOUSE)
    position = f.allocate(12)
    u.mem_write(position, dwords(2496, 2752, 0))
    admitted = bc.invoke(u, 0x51DFF0, actor, position, 128) & 255
    assert admitted == 1 and list(struct.unpack('<3i', u.mem_read(actor + 0x9C, 12))) == [2496, 2752, 0]
    audio = VoiceHistory(inputs, 'stock_suicide', fixture=f, actor_count=0, coordinate_profile=True)
    audio.listener_input('declared_flat_suicide_listener', viewport=(640,480), backbuffer=(640,480), view=(0,0))
    audio.actors.append(actor)
    audio.actor = actor
    rng = RngTrace.__new__(RngTrace)
    rng.u, rng.streams = u, {'main':0x886B88, 'scenario':read(0xA8B230)+0x218, 'mapgen':0xABE890}
    rng.requests, rng.pending = [], []
    u.hook_add(UC_HOOK_CODE, rng.observe)
    phase = 'preparation'
    pending, packets, target_calls, calls, anims, lifecycle = [], [], [], [], [], []
    timer_writes, fire_writes, target_writes = [], [], []
    bullets, phases = [], []
    normal = dict(get_rof=0, bullet_emission_tail=0, burst_increment=0, rearm_tail=0, last_fire_tail=0)
    normal_sites = {0x6FCFA0:'get_rof',0x6FE562:'bullet_emission_tail',0x6FF274:'burst_increment',0x6FF2B2:'rearm_tail',0x6FF743:'last_fire_tail'}

    def signed(at):
        return struct.unpack('<i', u.mem_read(at,4))[0]

    def coords(at):
        return list(struct.unpack('<3i',u.mem_read(at,12)))

    def role(pointer):
        return 'null' if not pointer else 'source' if pointer==actor else 'target' if pointer==er.BLD else 'source_house' if pointer==er.HOUSE else 'other'

    def member(pointer, base):
        return pointer in [read(read(base+4)+i*4) for i in range(read(base+16))]

    def state():
        return dict(frame=read(bc.FRAME),health=signed(actor+0x6C),alive=u.mem_read(actor+0x90,1)[0],
            limbo=u.mem_read(actor+0x81,1)[0],marked=u.mem_read(actor+0x74,1)[0],logic=member(actor,0x87F778),
            location=coords(actor+0x9C),target=role(read(actor+0x2B4)),current_weapon=signed(actor+0x138),
            mission=signed(actor+0xAC),pending=u.mem_read(actor+0x68D,1)[0],doing=signed(actor+0x6C4),stage=signed(actor+0xF8),
            sequence=dict(start=signed(actor+0x100),duration=signed(actor+0x108),rate=signed(actor+0x10C)),
            rearm=dict(start=signed(actor+0x2EC),duration=signed(actor+0x2F4),rof_copy=signed(actor+0x2F8)),
            burst=signed(actor+0x3B8),last_fire=signed(actor+0x120),pending_deletes=read(0xB0F6A8),
            target_health=signed(er.BLD+0x6C),target_alive=u.mem_read(er.BLD+0x90,1)[0])

    def boundary(label):
        row=dict(label=label,source=state(),rng=rng.states(),rng_request_count=len(rng.requests))
        phases.append(row)
        return row

    observed = {0x51DF60:'infantry_fire_at',0x6FDD50:'techno_fire_at',0x5206B0:'fire_at_target',
        0x51B1F0:'set_target',0x5218E0:'select_weapon',0x51C8B0:'get_fire_error',0x51D6F0:'do_action',
        0x70D690:'death_weapon',0x46B050:'bullet_factory',0x466380:'bullet_ctor',0x4690B0:'bullet_detonate',
        0x489280:'area_damage',0x421EA0:'anim_ctor',0x4DE5D0:'foot_uninit',0x5F65F0:'object_uninit',
        0x7258D0:'announce_expired',0x725C70:'deferred_drain',0x517D90:'infantry_destructor',0x481670:'target_scatter',
        0x517FA0:'source_receiver',0x442230:'target_receiver'}

    def observe(_u, pc, size, data):
        sp=u.reg_read(UC_X86_REG_ESP)
        for item in pending[:]:
            if pc==item['return_pc'] and sp==item['after_sp']:
                row=item['row'];name=row['kind']
                if name in ('source_receiver','target_receiver'):
                    receiver=actor if name=='source_receiver' else er.BLD
                    row.update(health_after=signed(receiver+0x6C),result=u.reg_read(UC_X86_REG_EAX))
                elif name in ('infantry_fire_at','techno_fire_at','select_weapon','get_fire_error','death_weapon'):
                    row['result']=u.reg_read(UC_X86_REG_EAX)
                elif name=='do_action':row['result_al']=u.reg_read(UC_X86_REG_EAX)&255
                if name=='foot_uninit':boundary('after_original_uninit')
                pending.remove(item)
        if pc in normal_sites:normal[normal_sites[pc]]+=1
        if pc not in observed:return
        this=u.reg_read(UC_X86_REG_ECX);name=observed[pc]
        if name=='source_receiver' and this!=actor:return
        if name=='target_receiver' and this!=er.BLD:return
        row=dict(kind=name,phase=phase,frame=read(bc.FRAME))
        cleanup=0
        if name in ('source_receiver','target_receiver'):
            args=list(struct.unpack('<7I',u.mem_read(sp+4,28)))
            row.update(damage=signed(args[0]),distance=signed(sp+8),warhead=f.string(args[2]+0x24),
                attacker=role(args[3]),ignore_defenses=bool(args[4]),arg6=bool(args[5]),attacking_house=role(args[6]),
                aliases_health=args[0]==this+0x6C,health_before=signed(this+0x6C))
            (packets if name=='source_receiver' else target_calls).append(row)
            cleanup=28
        elif name in ('infantry_fire_at','techno_fire_at'):
            row.update(target=role(read(sp+4)),weapon_index=signed(sp+8));cleanup=8
            calls.append(row)
        elif name=='set_target':
            row['target']=role(read(sp+4));cleanup=4;calls.append(row)
        elif name=='select_weapon':cleanup=4;calls.append(row)
        elif name=='get_fire_error':cleanup=12;calls.append(row)
        elif name=='do_action':
            row.update(action=signed(sp+4),force=bool(read(sp+8)));cleanup=12
            if phase!='dying_ai':calls.append(row)
        elif name=='death_weapon':row['extra_damage']=signed(sp+4);cleanup=4;calls.append(row)
        elif name=='bullet_factory':
            row['caller']=f'{read(sp):08X}';calls.append(row)
        elif name=='bullet_ctor':bullets.append(this);calls.append(row)
        elif name=='bullet_detonate':row['location']=coords(read(sp+4));calls.append(row)
        elif name=='area_damage':
            row.update(location=coords(this),damage=u.reg_read(UC_X86_REG_EDX),warhead=f.string(read(sp+8)+0x24));calls.append(row)
        elif name=='anim_ctor':
            row.update(type=f.string(read(sp+4)+0x24),location=coords(read(sp+8)));anims.append(row)
        elif name=='announce_expired':
            if this!=actor and this not in bullets:return
            row.update(object='source' if this==actor else 'death_bullet',caller=f'{read(sp):08X}');lifecycle.append(row)
        elif name in ('foot_uninit','object_uninit','infantry_destructor'):
            if this!=actor:return
            lifecycle.append(row)
        elif name=='deferred_drain':lifecycle.append(row)
        elif name=='target_scatter':calls.append(row)
        elif name=='fire_at_target':
            if phase!='dying_ai':calls.append(row)
        if name in ('source_receiver','target_receiver','infantry_fire_at','techno_fire_at',
                    'select_weapon','get_fire_error','do_action','death_weapon','foot_uninit') and read(sp)!=native.RET_MAGIC:
            pending.append(dict(row=row,return_pc=read(sp),after_sp=sp+4+cleanup))

    def written(_u, access, address, size, value, data):
        if not actor<=address<actor+0x1000:return
        off=address-actor
        row=dict(phase=phase,frame=read(bc.FRAME),pc=f'{u.reg_read(UC_X86_REG_EIP):08X}',offset=off,value=value)
        if off in (0xC8,0xD0,0xF8,0x100,0x108,0x10C,0x174,0x17C):timer_writes.append(row)
        if off in (0x2EC,0x2F4,0x2F8,0x120,0x3B8):fire_writes.append(row)
        if off==0x2B4:row['value']=role(value);target_writes.append(row)

    hooks=[u.hook_add(UC_HOOK_CODE,observe),u.hook_add(UC_HOOK_MEM_WRITE,written)]
    try:
        playfield=f.allocate(12);u.mem_write(playfield,dwords(*coords(actor+0x9C)))
        require(bc.invoke(u,0x568350,0x87F7E8,playfield)&255==1, 'native 0x568350 returned an unexpected result')
        boundary('before_preparation')
        preparation=None
        if wounded:
            packet=f.allocate(4);u.mem_write(packet,dwords(58))
            result=bc.invoke(u,0x517FA0,actor,packet,0,read(f.rules+0xFA8),0,1,0,0)
            packets[-1].update(health_after=signed(actor+0x6C),result=result)
            preparation=dict(damage=58,result=result,health=signed(actor+0x6C),strength=read(f.types['TERROR']+0xA0))
            assert preparation['health']==17 and u.mem_read(actor+0x90,1)[0]==1
        bc.invoke(u,0x51B1F0,actor,er.BLD)
        if ordinary:
            bc.invoke(u,0x5B35E0,actor,1,0);bc.invoke(u,0x5B3570,actor)
        boundary('before_fire')
        phase='ordinary_start' if ordinary else 'direct_fire';f.phase=phase
        if ordinary:
            bc.invoke(u,0x5206B0,actor,timeout_us=60_000_000)
            boundary('after_schedule_start')
            for visit in range(1,13):
                if signed(actor+0x6C)==0:break
                phase='ordinary_ai';f.phase=phase
                u.mem_write(bc.SP,dwords(native.RET_MAGIC));u.reg_write(UC_X86_REG_ESP,bc.SP);u.reg_write(UC_X86_REG_EDI,0)
                native.run_checked(u,0x55DE73,0x55DE87)
                bc.invoke(u,0x51BAB0,actor,timeout_us=60_000_000)
            assert signed(actor+0x6C)==0,'Ordinary schedule did not fire within12 visits'
        else:
            returned=bc.invoke(u,0x51DF60,actor,er.BLD,0,timeout_us=60_000_000)
            next(v for v in calls if v['kind']=='infantry_fire_at')['result']=returned
        boundary('after_fire')
        phase='end_fire_deferred_drain';f.phase=phase
        bc.invoke(u,0x725C70,0,timeout_us=60_000_000)
        visits=[]
        for visit in range(1,161):
            phase='dying_ai';f.phase=phase
            u.mem_write(bc.SP,dwords(native.RET_MAGIC));u.reg_write(UC_X86_REG_ESP,bc.SP);u.reg_write(UC_X86_REG_EDI,0)
            native.run_checked(u,0x55DE73,0x55DE87)
            bc.invoke(u,0x51BAB0,actor,timeout_us=60_000_000)
            sample=state()
            visits.append({k:sample[k] for k in ('frame','health','alive','logic','doing','stage','pending_deletes')})
            if not sample['alive']:break
        assert not state()['alive'],'Native dying sequence did not UnInit'
        boundary('before_deferred_destruction')
        phase='final_deferred_drain';f.phase=phase
        bc.invoke(u,0x725C70,0,timeout_us=60_000_000)
        boundary('after_deferred_destruction')
        assert not pending and f.code_unchanged()
        assert read(0xB0F6A8)==0
        return dict(name='ordinary_5206b0' if ordinary else 'direct_wounded17' if wounded else 'direct_stock75',
            input=dict(seed=seed,source_xyz=[2496,2752,0],target_xyz=coords(er.BLD+0x9C),
                target_foundation=list(f.building_foundation_dimensions),source_country='Confederation',
                distinct_target_house=read(er.BLD+0x21C)!=er.HOUSE,unlimbo_admitted=admitted,
                guard_a8ed6b=u.mem_read(0xA8ED6B,1)[0],session_literal_a8b238=read(0xA8B238)),
            preparation=preparation,phases=phases,source_packets=packets,target_receivers=target_calls,
            calls=calls,anims=[v for v in anims if v['type']!='BEHIND'],
            observed_excluded_anims=sorted({v['type'] for v in anims if v['type']=='BEHIND'}),
            normal_fire_tail_visits=normal,fire_writes=fire_writes,target_writes=target_writes,
            timer_writes=[v for v in timer_writes if v['phase']!='dying_ai'],
            rng_requests=rng.requests,lifecycle=lifecycle,death_visits=visits,
            original_code_unchanged=f.code_unchanged(),
            reached_presentation_boundaries=sorted({f'{v["address"]:08X}' for v in f.events if v.get('event')=='presentation_boundary'}),
            coordinate_sound_request_count=read(0xB1D3A8),
            post_destructor_state_bound='retained arena bytes only; source is no longer a live object')
    finally:
        for h in hooks:u.hook_del(h)


def suicide_fire_generate(root=None, scenario_map=None):
    import os
    root=Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS']) if root is None else Path(root)
    scenario_map=root/'Hills.map' if scenario_map is None else Path(scenario_map)
    inputs=suicide_fire_inputs(root,scenario_map)
    return dict(schema_version=1,inputs=inputs.suicide_inputs,
        rows=[suicide_fire_execute(inputs),suicide_fire_execute(inputs,wounded=True),
              suicide_fire_execute(inputs,ordinary=True)])


def suicide_fire_metadata():
    return provenance(scope='Stock TERROR synchronous Suicide FireAt, explicit DeathWeapon/real2x2 GAPOWR area receivers and native Infantry sequence/UnInit/deferred cleanup; three prepared direct/ordinary controls.',
        entry_points=dict(type_ctor=0x5236A0,type_reader=0x5240A0,weapon_reader=0x772080,
            infantry_ctor=0x517A50,infantry_unlimbo=0x51DFF0,fire_at_target=0x5206B0,
            infantry_ai=0x51BAB0,infantry_fire=0x51DF60,techno_fire=0x6FDD50,
            infantry_receiver=0x517FA0,techno_receiver=0x701900,death_weapon=0x70D690,
            area_damage=0x489280,corpse_completion=0x520AE0,deferred_drain=0x725C70),
        assumptions=['Existing joined_inputs/joined_fixture owns the VM, original readers/registries/constructors, clear flat admitted map/House priors and original CellSpread/Infantry/subcell startup. Original full scenario/House/target production/Unlimbo and input/approach are excluded. The supplied target750HP/2x2 foundation is reinserted by original Mark0/3.',
            'Fixed ART/SHP is physical TRST/TerroristSequence, MININUKE and six Rules DeadBodies assets. Raw guardA8ED6B=0 and sessionA8B238=1 retain literal identities, with no Editor/session enum claim. Wounded17 is reached by original58/Super/ignoreDefenses1/arg6false receiver before measured FireAt75.',
            'Seed31; source placement[2496,2752,0] is independently admitted by original Infantry ctor/Unlimbo, with target[2688,2688,0]. Other objects, House/effect Anim AI, global Logic interleaving/audio service/PCM/render are excluded. Behind remains explicitly excluded; incidental constructor observations do not establish its reader/assets/render parity.',
            'Full three RNG states at named boundaries and native request/raw draws are observed; timer middle words/void EAX/DoAction high bytes and freed-object interpretations are excluded. Post-destructor bytes are retained arena storage.'],
        substitutions=['Existing source-order lexical/physical-file IO, successful arena allocation/no-op release, CRT/TLS/SEH/Windows COM/audio device/clock/listener boundaries. Actual original Walk/Drive/Bullet factories execute. Remap705D70/radar655560/655740 remain presentation sinks. No selected FireAt, receiver, fatal, Bullet/AoE, source AI, corpse or cleanup result is supplied.'])


def suicide_fire_main(argv):
    import argparse
    from tools.native_oracle import finish_vectors
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--suicide-map',type=Path)
    args,remaining=parser.parse_known_args(argv)
    sources=joined_source_paths()
    repo=Path(__file__).resolve().parents[2]
    for name in ('tools/input_oracle/unit_voice_playback.py',
                 'tools/procedural_drawing_oracle/electric_bolt.py',
                 'tools/rules_oracle/weapon_laser.py','tools/rules_oracle/weapon_speed_order.py'):
        sources[name]=repo/name
    finish_vectors(lambda:suicide_fire_generate(scenario_map=args.suicide_map),
        Path(__file__).with_name('suicide_fire.json'),provenance=suicide_fire_metadata,
        argv=remaining,source_paths=sources)


def garrison_fatal_inputs(root):
    """Extend the existing joined readers with stock CAHSE01 and C4 inputs."""
    from collections import deque
    from types import SimpleNamespace
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI
    from tools import native_oracle as native
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.spatial_oracle.building_body_rules import INI, SP
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.rules_oracle.weapon_speed_order import RetailRulesInputs
    from tools.sidebar_oracle.stock import mix, mix_hash

    m = joined_inputs(root, layers=('RULESMD.INI', 'LANGRULE.INI',
                                  'MPBattleMD.ini', 'Hills.map'))
    m.reserve_arena(0x1000000)
    u, read = m.u, m.read32
    art = sections((root / 'ARTMD.INI').read_bytes())
    catalog = RetailRulesInputs(native.configured_gamemd().parent)
    # NewTheater/DemandLoad image bytes come from the actual temperate MIX;
    # RetailRulesInputs deliberately excludes theater archive registration.
    archive = mix((native.configured_gamemd().parent / 'ra2.mix').read_bytes())
    theater = mix(archive[mix_hash('isotemp.mix')])
    m.assets['CTHSE01.SHP'] = theater[mix_hash('CTHSE01.SHP')]
    # Read back the native General vector, not the authored twenty-token CSV.
    # ReadGeneral66DA90 uses ReadString capacity80h, producing fifteen entries
    # ending in D. CAHSE01's singular DebrisAnim key is not DebrisAnims.
    names = [m.string(read(read(m.rules + 0x140) + i * 4) + 0x24)
             for i in range(read(m.rules + 0x14C))]
    for name in names:
        raw = catalog.lookup(name + '.SHP')
        if raw is not None:
            m.assets[name + '.SHP'] = raw
    m.make_ini({name: art[name] for name in ('CAHSE01', *names) if name in art})
    proxy = SimpleNamespace(m=m, trace=deque(maxlen=40), events=[], pending={},
                            frame=0, phase='setup')

    def transport(vm, pc, size, _):
        if pc in JOINED_TRANSPORT - {0x527AF9, 0x527B0C}:
            Mission.observe(proxy, vm, pc, size, None)

    hook = u.hook_add(UC_HOOK_CODE, transport)
    try:
        for name in names:
            pointer = m.invoke(0x428B80, m.cstring(name))
            admitted = m.invoke(0x427D00, pointer, (INI,)) & 255
            assert admitted == int(name in art), (name, admitted)
        art_cache = bytes(u.mem_read(INI, 0x40))
        typ = m.alloc(0x2000)
        m.invoke(0x45DD90, typ, (m.cstring('CAHSE01'),))
        m.types['CAHSE01'] = typ
        physical_rules = sections((root / 'RULESMD.INI').read_bytes())
        wh_name = physical_rules['CombatDamage']['C4Warhead']
        for filename in m.input_rule_layers:
            path = root / filename
            if not path.exists():
                assert filename == 'LANGRULE.INI'
                continue
            physical = sections(path.read_bytes())
            m.make_ini({name: physical[name] for name in
                        ('CAHSE01', 'CombatDamage', wh_name) if name in physical})
            ini = m.alloc(0x40)
            u.mem_write(ini, bytes(u.mem_read(INI, 0x40)))
            u.mem_write(INI, art_cache)
            m.invoke(0x45FE50, typ, (ini,))
            if 'CombatDamage' in physical:
                u.mem_write(SP, bytes(0x300))
                for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, m.rules),
                                   (UC_X86_REG_EDI, ini)):
                    u.reg_write(reg, value)
                native.run_checked(u, 0x66C304, 0x66C34C)
            warhead = read(m.rules + 0xFA8)
            if warhead:
                assert m.string(warhead + 0x24) == wh_name
                m.invoke(0x75D3A0, warhead, (ini,))
        # Both debris vectors are empty:7024BB takes General MetallicDebris.
        assert read(typ + 0x5D4) == read(typ + 0x324) == 0
        assert u.mem_read(typ + 0x157B, 1) == b'\1'
        m.garrison_fatal_context = dict(seed=31, building_type='CAHSE01',
            origin_cell=[18, 16], strength=read(typ + 0xA0),
            min_debris=read(typ + 0x5C0), max_debris=read(typ + 0x5BC),
            metallic_debris=names, warhead=m.string(warhead + 0x24))
    finally:
        u.hook_del(hook)
    return m


def garrison_fatal_execute(inputs, occupants):
    """Whole442230 through Techno death and garrison release; stop at4415F0."""
    from unicorn import UC_HOOK_CODE
    from tools import native_oracle as native
    from tools.spatial_oracle import building_construction as bc, engineer_repair_admission as er
    from tools.spatial_oracle.building_body_rules import SP
    from tools.spatial_oracle.refinery_dock import cell_xy
    from tools.procedural_drawing_oracle.electric_bolt import RngTrace

    context = inputs.garrison_fatal_context
    f = joined_fixture(inputs, 374, seed=context['seed'], building_ai=True, unit_startup=True)
    u, read = f.u, f.read32
    # Explicit stock-skirmish scenario prior; the legacy shared fixture uses1.
    u.mem_write(0xA8B238, dwords(5))
    target = f.allocate(0x800)
    f.building(context['building_type'], target, f.allocate(0x20), constructor_only=True)
    position = f.allocate(12)
    u.mem_write(position, dwords(*(c * 256 + 128 for c in context['origin_cell']), 0))
    require(bc.invoke(u, 0x440580, target, position, 0) & 255 == 1,
            'native CAHSE01 Unlimbo failed')
    feet = []
    for _ in range(occupants):
        foot = f.allocate(0x800)
        bc.invoke(u, 0x517A50, foot, f.types['E1'], er.HOUSE)
        feet.append(foot)
    # Declared already-garrisoned membership, not an admission comparison.
    # Original vector resize45A560 owns storage; original constructors own Foot.
    vector = target + 0x684
    if read(target + 0x68C) < occupants:
        require(bc.invoke(u, read(read(vector) + 8), vector, occupants, 0) & 255 == 1,
                'native occupant vector resize failed')
    if feet:
        u.mem_write(read(target + 0x688), dwords(*feet))
    u.mem_write(target + 0x694, dwords(occupants))
    for foot in feet:
        u.mem_write(foot + 0x9C, bytes(u.mem_read(target + 0x9C, 12)))
        u.mem_write(foot + 0x11C, dwords(target))
        u.mem_write(foot + 0xAC, dwords(5))  # prior Guard, still in constructor limbo.
    assert read(bc.FRAME) == 0 and read(0xA8B238) == 5
    assert read(0xA83D4C) == er.HOUSE
    assert f.string(read(er.HOUSE + 0x34) + 0x24) == 'Americans'
    assert bytes(u.mem_read(er.HOUSE + 0x1EC, 2)) == b'\1\1'
    before_human_query = f.rng()
    require(bc.invoke(u, 0x50B6F0, er.HOUSE) & 255 == 1,
            'native current House is not human')
    assert f.rng() == before_human_query
    streams = {'main': 0x886B88, 'scenario': read(0xA8B230) + 0x218, 'mapgen': 0xABE890}
    for pointer in streams.values():
        bc.invoke(u, 0x65C6D0, pointer, context['seed'])
    rng = RngTrace.__new__(RngTrace)
    rng.u, rng.streams, rng.requests, rng.pending = u, streams, [], []
    u.hook_add(UC_HOOK_CODE, rng.observe)
    before = rng.states()
    boundaries = []
    watched = {0x442230: 'building_receive', 0x701900: 'techno_receive',
        0x702281: 'debris', 0x44242A: 'techno_return', 0x457DE0: 'eject',
        0x51D0D0: 'scatter', 0x442640: 'after_eject'}

    def observe(_u, pc, _size, _data):
        if pc in watched:
            boundaries.append((watched[pc], sum(len(r['raw_draws']) for r in rng.requests)))

    u.hook_add(UC_HOOK_CODE, observe)
    damage = f.allocate(4)
    u.mem_write(damage, dwords(read(target + 0x6C)))
    u.mem_write(SP, dwords(native.RET_MAGIC, damage, 0, read(inputs.rules + 0xFA8), 0, 1, 0, 0))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, target)
    f.phase = 'garrison_fatal'
    native.run_checked(u, 0x442230, 0x4415F0, count=10_000_000, timeout_us=60_000_000)
    assert f.code_unchanged() and not rng.pending
    assert [name for name, _ in boundaries] == ['building_receive', 'techno_receive',
        'debris', 'techno_return', 'eject', *(['scatter'] if occupants else []), 'after_eject']
    split = dict(boundaries)['techno_return']
    assert dict(boundaries)['eject'] == split
    if occupants:
        assert dict(boundaries)['scatter'] == split
    assert all(row['stream'] == 'scenario' for row in rng.requests)
    after = rng.states()
    assert all(before[name] == after[name] for name in ('main', 'mapgen'))
    words = [word for row in rng.requests for word in row['raw_draws']]
    return dict(occupants=occupants, debris_raw_words=words[:split],
        garrison_raw_words=words[split:], released=[dict(
            nav_com=cell_xy(read(foot + 0x5A4)),
            limbo=bool(u.mem_read(foot + 0x81, 1)[0]),
            alive=bool(u.mem_read(foot + 0x90, 1)[0])) for foot in feet])


def garrison_fatal_generate():
    import os
    inputs = garrison_fatal_inputs(Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS']))
    return dict(input=inputs.garrison_fatal_context,
                rows=[garrison_fatal_execute(inputs, count) for count in (0, 1)])


def garrison_fatal_metadata():
    return provenance(scope='Stock occupied CAHSE01 lethal C4 receiver: native Techno debris before SellBuilding/Infantry Scatter',
        entry_points=dict(building_type=0x45FE50, anim_type=0x427D00,
            metallic_debris_reader=0x66DA90, c4_warhead_reader=0x66C304,
            building_ctor=0x43B740, building_unlimbo=0x440580,
            infantry_ctor=0x517A50, occupant_vector_resize=0x45A560,
            building_damage=0x442230, techno_damage=0x701900, debris=0x702281,
            techno_return=0x44242A, sell_building=0x457DE0, infantry_scatter=0x51D0D0,
            stop_before_destruction_effects=0x4415F0),
        assumptions=[
            'Existing joined_inputs/joined_fixture own the original selected readers, actual requested SHP bytes, admitted flat clear32-coordinate map, House priors, startup, navigation and locomotors. RULESMD, optional LANGRULE, Battle mode and Hills map read in their existing order; ARTMD is fixed. Whole physical INI/archive startup and map loading are excluded.',
            'Whole original CAHSE01 constructor and Unlimbo at origin18,16 run alongside the shared background GAPOWR/MTNK. Original E1 constructors execute. Already-garrisoned vector membership, Guard mission, stored position and Transporter are explicit priors; entrance/ownership transfer is not claimed. Original resize owns storage. This selected control supplies runtime gameMode5 (Skirmish) before target/occupant construction, replacing the shared legacy mode1 prior. Actual frame0/mode5, HouseType Americans, House+1EC/+1ED both1 and CurrentHouse equality are asserted from the VM; original House IsHuman50B6F0 returns1 without changing RNG. All allocations succeed.',
            'Actual native MetallicDebris vector is read back at Rules+140/+14C. ReadString capacity80h at66DA9A truncates retail twenty-token CSV to fifteen entries ending D. Actual fixed ART/SHP readers bind these entries; D has no ART/image and retains real constructor defaults. CAHSE01 singular DebrisAnim is ignored by the plural DebrisAnims reader; both actual per-type debris vectors are empty.',
            'All three RNG streams reseed31 after setup. Whole Building442230 receives own health/Super C4Warhead/distance0/null source/null sourceHouse/ignoreDefenses1/arg6false. Original701900 completes debris before original457DE0 and51D0D0. Original RngTrace raw words are split at the actual44242A return boundary, including any Unlimbo draws in the garrison phase. Main/MapGen stay unchanged. Every executable PE section remains unchanged.',
            'Observation ends at4415F0 entry before Building DestructionEffects. Later explosions, survivors, UnInit, deferred destruction, rendering and whole-match parity are excluded. Only Rust-consumed input, Scenario words and released Foot state are retained.'
        ], substitutions=joined_metadata()['substitutions'])


def garrison_fatal_main(argv):
    sources = joined_source_paths()
    repo = Path(__file__).resolve().parents[2]
    for name in ('tools/procedural_drawing_oracle/electric_bolt.py',
                 'tools/rules_oracle/weapon_speed_order.py', 'tools/sidebar_oracle/stock.py'):
        sources[name] = repo / name
    finish_vectors(garrison_fatal_generate,
        Path(__file__).with_name('building_garrison_fatal.json'),
        provenance=garrison_fatal_metadata, argv=argv, source_paths=sources)


if __name__ == "__main__":
    import sys
    if '--garrison-fatal' in sys.argv[1:]:
        if {'--wall-autofill', '--suicide-fire', '--typed-debris', '--damage-sound-joined',
                '--joined-stock-smudges', '--constructed-cancellation', '--joined'} & set(sys.argv[1:]):
            raise SystemExit('Select one native corpus mode')
        garrison_fatal_main([arg for arg in sys.argv[1:] if arg != '--garrison-fatal'])
        raise SystemExit(0)
    if '--wall-autofill' in sys.argv[1:]:
        if {'--suicide-fire', '--typed-debris', '--damage-sound-joined',
                '--joined-stock-smudges', '--constructed-cancellation', '--joined'} & set(sys.argv[1:]):
            raise SystemExit('Select one native corpus mode')
        wall_autofill_main([arg for arg in sys.argv[1:] if arg != '--wall-autofill'])
        raise SystemExit(0)
    if '--suicide-fire' in sys.argv[1:]:
        if {'--typed-debris','--damage-sound-joined','--joined-stock-smudges',
                '--constructed-cancellation','--joined'} & set(sys.argv[1:]):
            raise SystemExit('Select one native corpus mode')
        suicide_fire_main([arg for arg in sys.argv[1:] if arg!='--suicide-fire'])
        raise SystemExit(0)
    if '--typed-debris' in sys.argv[1:]:
        typed_debris_main([arg for arg in sys.argv[1:] if arg!='--typed-debris'])
        raise SystemExit(0)
    if '--damage-sound-joined' in sys.argv[1:]:
        if {'--joined', '--joined-stock-smudges', '--constructed-cancellation'} & set(sys.argv[1:]):
            raise SystemExit('Select one native corpus mode')
        damage_sound_main([arg for arg in sys.argv[1:] if arg != '--damage-sound-joined'])
        raise SystemExit(0)
    if '--joined-stock-smudges' in sys.argv[1:]:
        if '--joined' in sys.argv[1:] or '--constructed-cancellation' in sys.argv[1:]:
            raise SystemExit('Select one native corpus mode')
        stock_joined_main([arg for arg in sys.argv[1:] if arg!='--joined-stock-smudges'])
        raise SystemExit(0)
    if '--constructed-cancellation' in sys.argv[1:]:
        if '--joined' in sys.argv[1:]:
            raise SystemExit('Select one native corpus mode')
        constructed_cancellation_main([arg for arg in sys.argv[1:]
            if arg!='--constructed-cancellation'])
        raise SystemExit(0)
    if "--joined" in sys.argv[1:]:
        joined_main([arg for arg in sys.argv[1:] if arg != "--joined"])
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("A retail GAPOWR's death anims: TechnoClass::ReceiveDamage's debris block "
               "0x00702281..0x00702572 (count draw, DebrisAnims arm, each piece's AnimClass "
               "constructor and Start) then BuildingClass::DestructionEffects steps 7 and 8 "
               "0x0044177E..0x00441A2B (the centre scorch/crater roll and real placer, the "
               "per-foundation-cell scatter, delay and Explosion= pick, constructor and Start), "
               "over ten Scenario seeds at three retail Dustbowl Locations: the fixture's, whose "
               "cells carry ore, so CanPlace admits no SmudgeType and step 7 draws only its "
               "roll, and two clean ones (no 2x2 fits at the first; at the second the 2x2 "
               "types are preferred) where the placer's pick and SmudgeTypeClass::Place "
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
