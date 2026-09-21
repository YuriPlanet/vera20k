"""Original mixed DisplayClass queries and membership, with real class vtables.

Object/type coordinates and lifecycle bytes are supplied. Native Submit/Remove,
GetLayer/GetYSort and the single adjacent Ground sort run without substitutions.
This isolates registration from the constructors' unrelated gameplay side effects.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.crate_speed_effect import fixture, CELL
from tools.spatial_oracle.crate_ground_membership import LAYERS, BUFFERS, LOCOMOTORS
from tools.spatial_oracle.map_queries import dwords, EMPTY_TABLE, TABLE, GLOBAL_TABLE

VTABLES = dict(unit=0x7F5C70, building=0x7E3EBC, terrain=0x7F522C,
               particle=0x7EFB9C, bullet=0x7E46E4, voxel=0x7F6318, wave=0x7F6BF4)


def execute(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='unit') for _ in case['actors']]))

    def call(address, receiver=0, args=()):
        u.reg_write(UC_X86_REG_ECX, receiver)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        run_checked(u, address, RET_MAGIC, count=10000)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + 4 * len(args)
        return struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_EAX)))[0]

    u.mem_write(TABLE, EMPTY_TABLE)
    u.mem_write(GLOBAL_TABLE, dwords(TABLE))
    u.mem_write(TABLE + (10 * 512 + 10) * 4, dwords(CELL))
    u.mem_write(0xAC13C8, dwords(104))
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x4A8630, 0x4A866D, count=100)
    for layer in range(5):
        u.mem_write(LAYERS + 24 * layer + 4, dwords(BUFFERS + 0x100 * layer, 16))

    for index, (actor, spec) in enumerate(zip(actors, case['actors'])):
        kind = spec['kind']
        u.mem_write(actor, dwords(VTABLES[kind]))
        u.mem_write(actor + 0x74, b'\x01')
        u.mem_write(actor + 0x94, dwords(-1))
        u.mem_write(actor + 0x9C, dwords(*spec['xyz']))
        if kind == 'unit':
            loco = LOCOMOTORS + 0x100 * index
            call(0x4AF540, loco)
            u.mem_write(loco + 0xC, dwords(actor))
            u.mem_write(actor + 0x674, dwords(loco + 4))
        if kind == 'bullet':
            u.mem_write(actor + 0xAC, dwords(actor + 0x800))
            u.mem_write(actor + 0x800 + 0x2F7, bytes([int(spec.get('flat', False))]))
        if kind == 'building':
            # Building's inherited Techno type pointer is +520 (GetType70F1A0).
            u.mem_write(actor + 0x520, dwords(actor + 0x800))
            u.mem_write(actor + 0x800 + 0x16C5, bytes([int(spec.get('voxel_turret', False))]))
            u.mem_write(actor + 0x800 + 0x16B7, bytes([int(spec.get('gate', False))]))

    def state():
        layers = []
        for layer in range(5):
            count = struct.unpack('<i', u.mem_read(LAYERS + 24 * layer + 16, 4))[0]
            assert 0 <= count <= 16
            pointers = struct.unpack('<' + 'I' * count,
                                     u.mem_read(BUFFERS + 0x100 * layer, count * 4))
            layers.append([actors.index(pointer) for pointer in pointers])
        return layers

    queries = []
    for actor, spec in zip(actors, case['actors']):
        table = VTABLES[spec['kind']]
        layer = call(struct.unpack('<I', u.mem_read(table + 0x78, 4))[0], actor)
        key = call(struct.unpack('<I', u.mem_read(table + 0xB8, 4))[0], actor) if layer == 2 else None
        queries.append(dict(layer=layer, key=key))
    observations = []
    for step in case['steps']:
        actor = actors[step['actor']] if 'actor' in step else 0
        if step['op'] == 'submit':
            call(0x4A9720, 0x87F7E8, (actor,))
        elif step['op'] == 'remove':
            call(0x4A9770, 0x87F7E8, (actor,))
        elif step['op'] == 'coordinates':
            u.mem_write(actor + 0x9C, dwords(*step['xyz']))
        elif step['op'] == 'sort':
            call(0x551A30, LAYERS + 48)
        else:
            raise AssertionError(step)
        observations.append(state())
    return dict(input=case, queries=queries, after_steps=observations)


def generate():
    cases = []
    for kind in ['particle', 'terrain', 'bullet', 'voxel', 'wave']:
        for flat in ([False, True] if kind == 'bullet' else [False]):
            actors = [dict(kind=kind, xyz=[2688, 2688, 0], flat=flat)]
            cases.append(dict(name=f'{kind}_{flat}', actors=actors,
                              steps=[dict(op='submit', actor=0), dict(op='remove', actor=0)]))
    actors = [dict(kind='unit', xyz=[2688, 2688, 0]),
              dict(kind='particle', xyz=[2688, 2688, 700]),
              dict(kind='terrain', xyz=[2688, 2688, 0]),
              dict(kind='particle', xyz=[-2147483648, -1, 0])]
    insert = [dict(op='submit', actor=i) for i in range(len(actors))]
    cases.append(dict(name='mixed_ties_signed_wrap_and_resubmit', actors=actors,
                      steps=insert + [dict(op='submit', actor=0), dict(op='remove', actor=1),
                                      dict(op='submit', actor=1)]))
    actors = [dict(kind=kind, xyz=[2688 + i * 256, 2688, 0])
              for i, kind in enumerate(['unit', 'particle', 'terrain', 'particle'])]
    steps = [dict(op='submit', actor=i) for i in range(4)]
    steps += [dict(op='coordinates', actor=i, xyz=[2688 + (4 - i) * 256, 2688, 0])
              for i in [0, 1, 3]]  # Terrain stays fixed.
    cases.append(dict(name='mixed_single_pass_not_full_sort', actors=actors,
                      steps=steps + [dict(op='sort')] * 3))
    # Building type memory is larger than the actor fixture's stride. Keep it
    # last so these supplied type bytes cannot overlap a subsequent receiver.
    actors = [dict(kind='particle', xyz=[2560, 2591, 0]),
              dict(kind='building', xyz=[2688, 2688, 0], voxel_turret=True)]
    cases.append(dict(name='building_voxel_adjust_orders_particle', actors=actors,
                      steps=[dict(op='submit', actor=1), dict(op='submit', actor=0)]))
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'submit': 0x4A9720, 'remove': 0x4A9770, 'sort': 0x551A30,
                      'particle_layer': 0x62FE80, 'bullet_layer': 0x468B90,
                      'voxel_layer': 0x74A960, 'wave_layer': 0x75F890,
                      'object_sort': 0x5F6BD0, 'building_sort': 0x449410},
        assumptions=['Real class vtables identified from original constructors. Coordinates, initial marked/layer bytes and relevant type fields are supplied; constructors/Unlimbo are not executed.',
                     'Original display constructor prefix stops before atexit; supplied capacity16 buffers exclude allocation growth/failure.',
                     'Only real Unit Drive locomotors are constructed; no calls or return values are substituted. Terrain remains at supplied cell center/Z=0.'],
        substitutions=[],
        scope='Nine mixed display query/registration/removal/sort sequences: particles, stationary terrain, Flat/non-Flat bullets, voxel debris, waves, Unit peers and voxel-turret Building adjustment. Excludes animation owners, lifecycle marking, render output and full combat parity.',
    ))
