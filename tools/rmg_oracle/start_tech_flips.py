"""Original map-type-0 tech coin flips at the end of CreateStartingPoints.

`python -m tools.rmg_oracle.start_tech_flips --check` replays
`RandomMapGenerator::CreateStartingPoints @ 0x00594B50` from the map-type
gate at 0x00594E80 to 0x00594EE3: for each region in the sorted bucket list
with no start quota, one draw `r * (1 + 2^-32) * 2^-32 < 0.5`, and on heads
the original `PlaceTechBuildingInRegion @ 0x00595400` at once. `--write`
replaces the corpus `tech_buildings::tests::native_start_tech_flip_controls`
consumes. The building constructor, foundation read and Unlimbo are the
`tech_buildings` fixture's declared substitutions.
"""
from pathlib import Path

GATE, END = 0x594E80, 0x594EE3
SORTED_ITEMS, SORTED_COUNT = 0x50, 0x5C   # [ESP+50]/[ESP+5C], the bucket list 594E93 walks


def controls():
    """Declared regions `[quota, cells]` in vector order and `order`, the
    sorted bucket list as region indices; the other inputs as `tech_buildings`."""
    from tools.rmg_oracle.start_points import box
    from tools.rmg_oracle.tech_buildings import rect
    three = [rect(2, 2), rect(1, 1), rect(3, 2)]
    a, b, c, d = box(12, 12, 15, 15), box(17, 12, 21, 15), box(12, 17, 15, 21), box(17, 17, 21, 21)
    rows = [
        # Quota-bearing regions draw nothing; the others flip in list order.
        dict(id='mixed_quotas', seed=12, types=three,
             regions=[[1, a], [0, b], [0, c], [2, d]], order=[3, 1, 0, 2]),
        # Every region flips; heads place at once, between the flips.
        dict(id='all_zero_quota', seed=8, types=three,
             regions=[[0, a], [0, b], [0, c], [0, d]], order=[0, 1, 2, 3]),
        dict(id='all_zero_quota_other_seed', seed=29, types=three,
             regions=[[0, a], [0, b], [0, c], [0, d]], order=[2, 0, 3, 1]),
        # A region with no admissible 2x2 spends its 100 attempts.
        dict(id='exhausted_region_then_flips', seed=6, types=[rect(2, 2)],
             rough=[[x, y] for x, y in box(11, 11, 17, 17) if (x + y) % 2], regions=[[0, a], [0, d], [0, b]],
             order=[0, 1, 2]),
        # Map type 1 skips the loop: no draw.
        dict(id='other_map_type', seed=3, map_type=1, types=three,
             regions=[[0, a], [0, b]], order=[0, 1]),
        dict(id='no_regions', seed=5, types=three, regions=[], order=[]),
    ]
    rows = [dict(gen=[12, 4], map_type=0, rough=[], occupied=[], rock=[], locked=[],
                 terrain=[]) | row for row in rows]
    from tools.rmg_oracle.tiberium_field import in_diamond
    for row in rows:
        row['rough'] = [xy for xy in row['rough'] if in_diamond(*xy)]
    return rows


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool, words
    from tools.rmg_oracle.region_split import MAP_TYPE
    from tools.rmg_oracle.tech_buildings import TechBuildingsFixture
    fixture, pool = TechBuildingsFixture(), StatePool()
    m, u = fixture.m, fixture.u

    def slice_frame(case):
        records = m.read32(0xABDF94)
        items = m.alloc(4 * max(1, len(case['order'])))
        for n, index in enumerate(case['order']):
            u.mem_write(items + n * 4, words(m.read32(records + index * 4)))
        frame = m.alloc(0x400) + 0x200
        u.mem_write(frame + SORTED_ITEMS, words(items))
        u.mem_write(frame + SORTED_COUNT, words(len(case['order'])))
        u.mem_write(MAP_TYPE, words(case['map_type']))
        return GATE, END, frame

    rows = [fixture.run(case, pool, slice_frame) for case in controls()]
    return dict(schema=1, scene=dict(rough=fixture.terrain.bases['cliff']),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='6 runs of CreateStartingPoints 594E80..594EE3: quota-bearing regions skipped, every-region flips under two seeds and list orders, a region exhausted to its 100-attempt cap between flips, a nonzero map type and an empty list; every flip, type and anchor draw and each building\'s cell',
        assumptions=[
            'The tech_buildings fixture and scene (16x16 map, local rect (2, 5, 12, 4), declared foundations, CliffSet-base rough cells, region records with +20 quota and +2C/+38 cell list).',
            'The slice starts at 594E80 on a fresh frame whose [ESP+50]/[ESP+5C] hold the sorted bucket list as declared region records; ABE014 the map type. FPCW 0E7F. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS with guarded blocks; native code bytes unchanged.',
            'The tech_buildings substitutions: BuildingClass constructor 43B740, type vtable +90 foundation, building vtable +D8 Unlimbo and +20 destroy.'],
        entry_points=dict(create_starting_points=0x594B50, gate=GATE, end=END,
                          place_tech_building_in_region=0x595400, next=0x65C780))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
