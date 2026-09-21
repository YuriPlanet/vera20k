"""Original Anim owner attachment, live detachment and owner expiry.

Supplies real-vtable objects, marking/type state and preallocated display vectors.
No native calls are replaced. This is a dependency witness for the pending Rust
animation display migration, not a claim that the migration is implemented.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.crate_speed_effect import fixture
from tools.spatial_oracle.crate_ground_membership import LAYERS, BUFFERS
from tools.spatial_oracle.map_queries import dwords


def execute(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='unit'), dict(kind='unit')]))
    owner, anim = actors
    config = anim + 0x800
    output = SCRATCH + 0x1C000

    def call(address, receiver, args=()):
        u.reg_write(UC_X86_REG_ECX, receiver)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        run_checked(u, address, RET_MAGIC, count=10000)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + 4 * len(args)
        return struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_EAX)))[0]

    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x4A8630, 0x4A866D, count=100)
    for layer in range(5):
        u.mem_write(LAYERS + layer * 24 + 4, dwords(BUFFERS + layer * 0x100, 16))
    u.mem_write(anim, dwords(0x7E3354))
    u.mem_write(anim + 0x74, bytes([int(case['marked'])]))
    u.mem_write(anim + 0x94, dwords(-1))
    u.mem_write(anim + 0xC8, dwords(config, 0))
    u.mem_write(anim + 0x104, dwords(case['adjust']))
    u.mem_write(config + 0x340, dwords(999))  # Deliberately differs from retained instance.
    u.mem_write(config + 0x364, dwords(case['layer']))
    u.mem_write(anim + 0x9C, dwords(2816, 2944, 300))
    u.mem_write(0xA8E9AC, dwords(output + 0x100))
    u.mem_write(0xA8E9B8, dwords(1))
    u.mem_write(output + 0x100, dwords(anim))

    def observe():
        call(0x422BE0, anim, (output,))
        layers = []
        for layer in range(5):
            count = struct.unpack('<i', u.mem_read(LAYERS + layer * 24 + 16, 4))[0]
            assert count in (0, 1)
            if count:
                assert bytes(u.mem_read(BUFFERS + layer * 0x100, 4)) == dwords(anim)
            layers.append(count)
        return dict(stored=list(struct.unpack('<iii', u.mem_read(anim + 0x9C, 12))),
                    absolute=list(struct.unpack('<iii', u.mem_read(output, 12))),
                    owner=bool(struct.unpack('<I', u.mem_read(anim + 0xCC, 4))[0]),
                    marked=bool(u.mem_read(anim + 0x74, 1)[0]),
                    detached=bool(u.mem_read(anim + 0x19B, 1)[0]),
                    queried_layer=call(0x424CB0, anim),
                    y_sort=call(0x422BC0, anim), layers=layers)

    observations = []
    for op in ['submit', 'attach', 'move_owner', 'detach', 'attach', 'expire']:
        if op == 'submit':
            call(0x4A9720, 0x87F7E8, (anim,))
        elif op == 'attach':
            call(0x424B50, anim, (owner,))
        elif op == 'move_owner':
            u.mem_write(owner + 0x9C, dwords(3000, 3100, 700))
        elif op == 'detach':
            call(0x424B50, anim, (0,))
        elif op == 'expire':
            call(0x425150, anim, (owner, 1))
        observations.append(dict(op=op, state=observe()))
    return dict(input=case, observations=observations)


def generate():
    return [execute(dict(name=f'layer_{layer}_marked_{marked}_adjust_{adjust}',
                         layer=layer, marked=marked, adjust=adjust))
            for layer in [-1, 0, 1, 2, 3, 4]
            for marked in [False, True]
            for adjust in [-17, 2147483647]]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'set_owner': 0x424B50, 'expiry': 0x425150,
                      'coords': 0x422BE0, 'sort': 0x422BC0, 'layer': 0x424CB0,
                      'mark_wrapper': 0x4238B0, 'mark': 0x5F5850,
                      'submit': 0x4A9720, 'remove': 0x4A9770},
        assumptions=['Real Anim/Unit vtables and one-entry Anim registry. Initial object/type fields, marking and coordinates are supplied; constructors and native INI readers are excluded.',
                     'Five preallocated display buffers exclude allocator growth/failure. Type YSortAdjust999 deliberately differs from supplied retained instance+104.',
                     'Owner is Unit; the shared-owner scan sees no other attached Anim. Mark executes its original Anim/Object paths without substitutions.'],
        substitutions=[],
        scope='24 original six-step animation display/coordinate histories: six layers, marked/unmarked detach gating, signed/wrapping instance sort adjustment, moving Unit owner, normal detach and owner expiry. Excludes Building owner coordinates, multiple attached anims, constructor admission and AI deletion. Rust integration remains pending.',
    ))
