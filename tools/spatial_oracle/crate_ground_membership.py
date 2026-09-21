"""Original Ground display registration needed by crate recipient selection.

Uses real Foot vtables and constructed Walk/Drive locomotors. Executes native
Display Submit/Remove and Layer adjacent-sort bodies without replacing calls.
The display-array constructor prefix executes before its unrelated atexit call;
preallocated buffers avoid the allocator. Coordinates/initial object state are
supplied. Full Unlimbo/Conceal and airborne layer transitions remain outside scope.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.crate_speed_effect import fixture
from tools.spatial_oracle.map_queries import dwords

LAYERS = 0x8A0360
BUFFERS, LOCOMOTORS = SCRATCH + 0x1A000, SCRATCH + 0x1B000


def execute(case):
    u, sp, actors, _ = fixture(case)

    def call(address, receiver=0, args=()):
        u.reg_write(UC_X86_REG_ECX, receiver)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        run_checked(u, address, RET_MAGIC, count=10000)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + 4 * len(args)

    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x4A8630, 0x4A866D, count=100)
    for layer in range(5):
        header = LAYERS + 24 * layer
        assert bytes(u.mem_read(header, 4)) == dwords(0x7E6060)
        u.mem_write(header + 4, dwords(BUFFERS + 0x100 * layer, 16))
    for index, actor in enumerate(actors):
        loco = LOCOMOTORS + 0x100 * index
        kind = case['actors'][index].get('kind', 'infantry')
        assert kind in ('infantry', 'unit')
        call(0x75AA90 if kind == 'infantry' else 0x4AF540, loco)
        u.mem_write(loco + 0xC, dwords(actor))
        u.mem_write(actor + 0x674, dwords(loco + 4))
        u.mem_write(actor + 0x94, dwords(-1))

    def state():
        layers = []
        for layer in range(5):
            count = struct.unpack('<i', u.mem_read(LAYERS + layer * 24 + 16, 4))[0]
            assert 0 <= count <= 16
            pointers = struct.unpack('<' + 'I' * count,
                                     u.mem_read(BUFFERS + layer * 0x100, count * 4))
            layers.append([actors.index(pointer) for pointer in pointers])
        return dict(layers=layers, registered=[
            struct.unpack('<i', u.mem_read(actor + 0x94, 4))[0] for actor in actors])

    observations = []
    for step in case['steps']:
        op = step['op']
        actor = actors[step['actor']] if 'actor' in step else 0
        if op == 'submit':
            call(0x4A9720, 0x87F7E8, (actor,))
        elif op == 'remove':
            call(0x4A9770, 0x87F7E8, (actor,))
        elif op == 'coordinates':
            u.mem_write(actor + 0x9C, dwords(*step['xyz']))
        elif op == 'cached_layer':
            # Explicit malformed-state witness for native removal's fallback.
            u.mem_write(actor + 0x94, dwords(step['layer']))
        elif op == 'sort':
            call(0x551A30, LAYERS + 2 * 24)
        else:
            raise AssertionError(op)
        observations.append(state())
    return dict(input=case, after_steps=observations)


def generate():
    actors = [dict(kind='unit', delta=[256, 0, 0]),
              dict(kind='infantry'), dict(kind='unit')]
    submit = [dict(op='submit', actor=i) for i in range(3)]
    cases = [
        dict(name='sorted_insert_stable_ties', actors=actors, steps=submit),
        dict(name='resubmit_removes_old_and_reinserts_after_ties', actors=actors,
             steps=submit + [dict(op='submit', actor=1)]),
        dict(name='remove_compacts_and_clears_registration', actors=actors,
             steps=submit + [dict(op='remove', actor=1), dict(op='remove', actor=1)]),
        dict(name='wrong_cached_layer_falls_back_to_ground', actors=actors,
             steps=submit + [dict(op='cached_layer', actor=1, layer=4),
                             dict(op='remove', actor=1)]),
        dict(name='null_submit_and_remove', actors=actors,
             steps=submit + [dict(op='submit'), dict(op='remove')]),
    ]
    ordered = [dict(kind='unit', delta=[i * 100, 0, 0]) for i in range(4)]
    steps = [dict(op='submit', actor=i) for i in range(4)]
    steps += [dict(op='coordinates', actor=i, xyz=[2688 + (3 - i) * 100, 2688, 0])
              for i in range(4)]
    steps += [dict(op='sort')] * 3
    cases.append(dict(name='one_adjacent_sort_pass_per_call', actors=ordered, steps=steps))
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'display_array_init': 0x4A8630, 'before_atexit': 0x4A866D,
                      'submit': 0x4A9720, 'remove': 0x4A9770,
                      'layer_insert': 0x5519B0, 'ground_insert': 0x551A90,
                      'adjacent_sort': 0x551A30, 'compare': 0x5F6220},
        assumptions=['Original Unit/Infantry tables and constructed Drive/Walk locomotors; coordinates and initial Object+94=-1 are supplied, not created through Object Unlimbo.',
                     'Actual display-array constructor prefix stops before atexit registration. Five supplied buffers have capacity16; allocator growth/failure is outside scope.',
                     'One case explicitly corrupts cached Object+94 to exercise original removal fallback.'],
        substitutions=[],
        scope='Six original registration/removal/sort sequences for the Ground membership read by crate effects. Covers sorted insertion, stable ties, resubmission, compacting removal, stale-layer fallback, null guards and single-pass sorting. Excludes Unlimbo/Conceal, aircraft/jumpjet transitions, non-Foot object sort keys and production Rust integration.',
    ))
