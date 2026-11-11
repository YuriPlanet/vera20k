"""Original region partition stages over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.region_partition --check` runs the original
`RandomMapGenerator::Generate @ 0x00598960` slice 0x00598C89..0x00598D01:
`PartitionRegions @ 0x0058CF90`, then for every listed class region
`ExpandRegionRings @ 0x0058E740` and `PropagateRegionWater @ 0x0058E9B0`,
then `FloodUnownedRegions @ 0x0058D010` (each flood is
`FloodBuildRegion @ 0x0058C800`). Hooks observe the state between the calls;
nothing is substituted. `--write` replaces the corpus that
`regions::tests::native_partition_stage_controls` consumes. The scenes are
declared component inputs, not a complete Generate prefix.
"""
from pathlib import Path
import math
import struct

from tools.rmg_oracle.region_split import (
    GEN_W, MAP_TYPE, NEXT_REGION_ID, RNG_NEXT, STRIDE, in_diamond)

SLICE, SLICE_END = 0x598C89, 0x598D01
AFTER_PARTITION, EXPAND_CALL, AFTER_EXPAND = 0x598C94, 0x598CE4, 0x598CE9
PROPAGATE_CALL, LOOP_NEXT = 0x598CEB, 0x598CF0
PARTITION, EXPAND, PROPAGATE, FLOOD_UNOWNED = 0x58CF90, 0x58E740, 0x58E9B0, 0x58D010
FLOOD_BUILD, BORDER_FOR_REGION, BORDER_BY_ID = 0x58C800, 0x58D410, 0x5A0700
DEFAULT_LEVEL_AT = 0xABE2E4  # MapSeed +0x30C, the dissolve/merge 0xFF fallback
DEFAULT_LEVEL = 4
GEN = (16, 16)
WATER_FLAG = 0x4B
# Branch sites, counted for the sidecar scope only (not published).
SITES = dict(dissolve_draw=0x58CAFE, dissolve_done=0x58CC6E, small_blob_exit=0x58CC70,
             merge_path=0x58CCA0, merge_done=0x58CD3A, construct=0x58CDF6,
             expand_abort=0x58E986)
TILE_GLOBALS = dict(water=0xAA0738, shore=0xABAD28, green=0xAA0E18, green_lat=0xAA0748,
                    waterfalls=(0xAA073C, 0xABB110, 0xAA1050, 0xAA10A0))


def diamond():
    return [(x, y) for y in range(STRIDE) for x in range(STRIDE) if in_diamond(x, y)]


def near(x, y, cx, cy, r):
    return math.hypot(x - cx, y - cy) <= r


def lakes(x, y):
    """Two lakes with shore rings on two plateaus, a green meadow, a small
    level-12 rise, cliff and 0xFFFF cells inside the rings, and water-flagged
    level-4 cells beside the west lake's level-0 plateau."""
    level = 4 if x - y >= 4 else 8 if y - x >= 6 else 0
    flag = level == 4 and x - y <= 5 and 9 <= y <= 17
    if near(x, y, 12, 14, 2.5):
        return 'W' if x == 12 else 'w', level, False
    if near(x, y, 12, 14, 3.6):
        return 'S' if y == 14 else 's', level, False
    if near(x, y, 25, 15, 2):
        return 'f' if (x, y) == (25, 16) else 'w', level, False
    if near(x, y, 25, 15, 3):
        return 's', level, False
    if near(x, y, 11, 24, 2.5):
        return 'g' if (x, y) == (11, 24) else 'L' if x > 11 else 'l', level, False
    if near(x, y, 20, 28, 2.2):
        return '.', 12, False
    if (x, y) in ((7, 9), (8, 9), (6, 18), (16, 20)):
        return 'c', level, False
    if (x, y) in ((15, 13), (14, 17), (9, 11)):
        return 'x', level, flag
    return '.', level, flag


def rings(x, y):
    """Two same-level lakes three cells apart and a meadow touching the east
    one, so expansion meets foreign regions, and a pond alone on the level-8
    corner whose rings complete; flags chain beside the meadow."""
    level = 8 if x + y >= 40 else 0
    if near(x, y, 12, 13, 2) or near(x, y, 19, 13, 2) or near(x, y, 25, 21, 1.5):
        return 'w', level, False
    if near(x, y, 23, 13, 1.5):
        return 'l', level, False
    flag = 25 <= x <= 27 and 10 <= y <= 18 and x + y < 38
    return '.', level, flag


def dry(x, y):
    """No class cells: a 25-cell rise over the top corner (the first flood)
    between a level-0 west and a level-4 east, small rises on the diamond's
    edges and in the open, and a level -1 plateau west of one rise."""
    level = 0 if x < 16 else 4
    flag = False
    if y <= 5:
        level, flag = 8, y == 3
    elif (x, y) in ((3, 18), (4, 18), (3, 19), (4, 19), (5, 19)):
        level = 12                       # bottom-left edge: (x-1, y) is outside
    elif x + y == 17 and 6 <= y <= 9:
        level = 12                       # top-left edge: both candidates outside
    elif near(x, y, 12, 24, 1.5):
        level = 12
    elif 12 <= y <= 20 and 18 <= x <= 26:
        level = -1
    elif 14 <= y <= 16 and 27 <= x <= 28:
        level = 8                        # merges into the level -1 plateau
    return '.', level, flag


SCENES = dict(lakes=lakes, rings=rings, dry=dry)


def controls():
    rows = [dict(id='lakes_type_1', scene='lakes', seed=11, map_type=1),
            dict(id='lakes_type_3', scene='lakes', seed=11, map_type=3),
            dict(id='lakes_type_2', scene='lakes', seed=11, map_type=2),
            dict(id='rings_type_1', scene='rings', seed=5, map_type=1),
            dict(id='rings_type_4', scene='rings', seed=5, map_type=4)]
    rows += [dict(id=f'dry_seed_{seed}', scene='dry', seed=seed, map_type=1)
             for seed in (1, 7, 42, 3)]
    return rows


def signed(v):
    return v - (1 << 32) if v & 0x80000000 else v


class PartitionFixture:
    def __init__(self):
        from tools.rmg_oracle.region_split import SplitFixture
        self.split = SplitFixture()
        self.terrain, self.m, self.u = self.split.terrain, self.split.m, self.split.u
        read = self.m.read32
        t = {k: signed(read(a)) for k, a in TILE_GLOBALS.items() if k != 'waterfalls'}
        t['waterfalls'] = [signed(read(a)) for a in TILE_GLOBALS['waterfalls']]
        t['cliff'] = self.terrain.bases['cliff']
        self.bases = t
        self.codes = {'.': 0, 'x': 0xFFFF, 'w': t['water'], 'W': t['water'] + 13,
                      's': t['shore'], 'S': t['shore'] + 41, 'f': t['waterfalls'][0] + 3,
                      'g': t['green'], 'l': t['green_lat'], 'L': t['green_lat'] + 15,
                      'c': t['cliff']}
        self.tiles = {tile: code for code, tile in self.codes.items()}
        assert len(self.tiles) == len(self.codes)
        self.cells = diamond()
        self.sites = dict.fromkeys(SITES, 0)

    def scene(self, name):
        at = SCENES[name]
        planes = [at(x, y) for x, y in self.cells]
        return dict(tiles=''.join(code for code, _, _ in planes),
                    levels_hex=bytes(level & 255 for _, level, _ in planes).hex(),
                    water=[list(xy) for xy, (_, _, flag) in zip(self.cells, planes) if flag])

    def prepare(self, case, scene):
        from tools.rmg_oracle.cliff_materialization import words
        f, u = self.terrain, self.u
        self.split.reset(case['seed'])
        levels = bytes.fromhex(scene['levels_hex'])
        water = {tuple(xy) for xy in scene['water']}
        for (x, y), code, level in zip(self.cells, scene['tiles'], levels):
            cell = f.cells[(x, y)]
            u.mem_write(cell + 0x38, words(self.codes[code]))
            u.mem_write(cell + 0x11A, bytes([0, level]))
            u.mem_write(self.split.record(x, y) + WATER_FLAG, bytes([(x, y) in water]))
        u.mem_write(MAP_TYPE, words(case['map_type']))
        u.mem_write(DEFAULT_LEVEL_AT, words(DEFAULT_LEVEL))
        # The generated size: CollectRegionBorderCells @ 0x005A0700 grows its
        # list by MapSeed +0x180 * +0x184 (0x005A0737); zero refuses every add.
        u.mem_write(GEN_W, words(*GEN))

    def observe(self):
        """Owners as signed bytes, levels, tile codes and flagged cells."""
        f, u = self.terrain, self.u
        raw = bytes(u.mem_read(f.scratch, STRIDE * STRIDE * 0x50))
        owners, flags = bytearray(), []
        for x, y in self.cells:
            at = (y * STRIDE + x) * 0x50
            owner = struct.unpack_from('<i', raw, at + 0x38)[0]
            assert -1 <= owner < 0x80, owner
            owners.append(owner & 255)
            if raw[at + WATER_FLAG]:
                flags.append([x, y])
        levels = bytes(u.mem_read(f.cells[xy] + 0x11B, 1)[0] for xy in self.cells)
        tiles = ''.join(self.tiles[self.m.read32(f.cells[xy] + 0x38)] for xy in self.cells)
        return dict(owners_hex=bytes(owners).hex(), levels_hex=levels.hex(), tiles=tiles,
                    water=flags)

    def claimed(self, before, after):
        return [list(xy) for xy, a, b in zip(self.cells, bytes.fromhex(before),
                                             bytes.fromhex(after)) if a != b]

    def run(self, case, scene, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.gen_rng_vectors import check_code
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
                                      UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP)
        f, m, u, split = self.terrain, self.m, self.u, self.split
        self.prepare(case, scene)
        before = pool.refs(f.states())
        out = dict(input=case, rng_before=before['mapgen'], expansions=[])
        pending = {}
        # Generate's frame: [ESP+0x18] is the dialog record whose +0x3C is the
        # map type (0x00598CB2).
        record = m.alloc(0x40)
        u.mem_write(record + 0x3C, words(case['map_type']))
        frame = m.alloc(0x400) + 0x200
        u.mem_write(frame + 0x18, words(record))

        def owners():
            return self.observe()['owners_hex']

        def observe(machine, pc, _size, _data):
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                return
            for name, site in SITES.items():
                if pc == site:
                    self.sites[name] += 1
            if pc == RNG_NEXT:
                native.require(machine.reg_read(UC_X86_REG_ECX) == f.rngs['mapgen'],
                               'Unexpected RNG stream')
            elif pc == AFTER_PARTITION:
                state = self.observe()
                out['partition'] = dict(owners_hex=state['owners_hex'], listed=split.listed(),
                                        next_region_id=m.read32(NEXT_REGION_ID))
            elif pc == EXPAND_CALL:
                region = machine.reg_read(UC_X86_REG_ESI)
                passes = machine.reg_read(UC_X86_REG_EAX)
                pending.update(region=m.read32(region + 8), passes=passes, owners=owners())
            elif pc == AFTER_EXPAND:
                now = owners()
                pending.update(result=machine.reg_read(UC_X86_REG_EAX) & 255,
                               rings=self.claimed(pending['owners'], now), owners=now)
            elif pc == LOOP_NEXT and 'result' in pending:
                entry = dict(region=pending['region'], passes=pending['passes'],
                             result=pending['result'], rings=pending['rings'],
                             water=self.claimed(pending['owners'], owners()))
                out['expansions'].append(entry)
                pending.clear()

        hooks = [u.hook_add(UC_HOOK_CODE, observe, begin=pc, end=pc)
                 for pc in (RNG_NEXT, AFTER_PARTITION, EXPAND_CALL, AFTER_EXPAND, LOOP_NEXT,
                            *SITES.values())]
        f.active = True
        try:
            u.reg_write(UC_X86_REG_ESP, frame)
            u.reg_write(UC_X86_REG_EBX, 0)
            native.run_checked(u, SLICE, SLICE_END, count=500_000_000, timeout_us=600_000_000,
                               context=dict(control=case['id']))
            native.require(u.reg_read(UC_X86_REG_ESP) == frame,
                           f"{case['id']}: the slice left its frame unbalanced")
            native.require(not pending, f"{case['id']}: an expansion did not finish")
            check_code(u, f.code)
            after = pool.refs(f.states())
            native.require(all(before[name] == after[name] for name in ('main', 'scenario')),
                           f"{case['id']}: unexpected main/scenario draw")
            out.update(rng_after=after['mapgen'], final=dict(
                self.observe(), listed=split.listed(), next_region_id=m.read32(NEXT_REGION_ID)))
            return out
        finally:
            f.active = False
            for hook in hooks:
                u.hook_del(hook)


def generate():
    import sys
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = PartitionFixture(), StatePool()
    scenes = {name: fixture.scene(name) for name in SCENES}
    rows = [fixture.run(case, scenes[case['scene']], pool) for case in controls()]
    print('branch sites:', fixture.sites, file=sys.stderr)
    return dict(schema=1, scene=dict(stride=STRIDE, bounds=[16, 48], tiles=fixture.bases,
                                     codes={c: t for c, t in fixture.codes.items()},
                                     default_level=DEFAULT_LEVEL),
                scenes=scenes, rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='9 runs of the original Generate region slice 0x00598C89..0x00598D01 over 3 declared scenes: PartitionRegions over water, shore, waterfall and green/LAT seeds; ExpandRegionRings with Generate\'s pass counts for map types 1-4, ring claims and both aborts; PropagateRegionWater over flagged cells; FloodUnownedRegions with first-blob dissolves (draws, success and fall-through), merges into the left or upper cell, edge construction and the MapSeed +0x30C level fallback',
        assumptions=[
            'Existing CliffFixture and SplitFixture own original-constructed Cell backing, admission, the stride-33 scratch with in-diamond coordinates, the native theater tile-set globals and the region list; bounds 16..48. Regions and stamps start -1 and the list empty, as Generate\'s teardown before 0x00598C89 leaves them.',
            'Declared scenes: tile, level and scratch +0x4B flag per in-diamond cell, subtile 0. The tile codes name the native water, shore, waterfall, green, green LAT and CliffSet bases and 0xFFFF. MapSeed +0x30C is 4 and the generated size (+0x180, +0x184) 16 x 16. The dialog record at [ESP+0x18] holds only the map type (+0x3C). FPCW 0E7F. All streams seeded by original 65C6D0.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged. No region, owner, level, tile, flag or RNG result is supplied; hooks only read state between the slice\'s calls.'],
        entry_points=dict(slice=SLICE, partition=PARTITION, flood_build=FLOOD_BUILD,
                          expand=EXPAND, propagate=PROPAGATE, flood_unowned=FLOOD_UNOWNED,
                          border_for_region=BORDER_FOR_REGION, border_by_id=BORDER_BY_ID,
                          next=RNG_NEXT))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
