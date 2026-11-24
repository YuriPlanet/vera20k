"""Original Anim owner expiry and Building damage-fire reference cleanup.

Executes full Anim425150 and its real Building-vtable owner callback. The
Building listener's Anim-specific tail44EA07..44EA52 is tested separately;
inherited Techno expiry, complete Building destruction and sound IO are excluded.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.crate_speed_effect import fixture
from tools.spatial_oracle.crate_ground_membership import LAYERS, BUFFERS
from tools.spatial_oracle.map_queries import dwords


def execute(slot):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='unit'), dict(kind='unit')]))
    owner, anim = actors
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x4A8630, 0x4A866D, count=100)
    for layer in range(5):
        u.mem_write(LAYERS + layer * 24 + 4, dwords(BUFFERS + layer * 0x100, 16))
    u.mem_write(owner, dwords(0x7E3EBC))
    u.mem_write(owner + 0x5C8 + slot * 4, dwords(anim))
    u.mem_write(anim, dwords(0x7E3354, 0x7E3338))
    u.mem_write(anim + 0x74, b'\x01')
    u.mem_write(anim + 0xCC, dwords(owner))
    u.mem_write(anim + 0x9C, dwords(128, 256, 300))

    def call(address, receiver, args):
        u.reg_write(UC_X86_REG_ECX, receiver)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        run_checked(u, address, RET_MAGIC, count=10000)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + 4 * len(args)

    def observe():
        return dict(slots=[bool(x) for x in struct.unpack('<8I', u.mem_read(owner + 0x5C8, 32))],
                    owner=bool(struct.unpack('<I', u.mem_read(anim + 0xCC, 4))[0]),
                    marked=bool(u.mem_read(anim + 0x74, 1)[0]),
                    expired=bool(u.mem_read(anim + 0x19B, 1)[0]),
                    stored=list(struct.unpack('<3i', u.mem_read(anim + 0x9C, 12))),
                    layers=[struct.unpack('<i', u.mem_read(LAYERS + i * 24 + 16, 4))[0] for i in range(5)])

    call(0x4A9720, 0x87F7E8, (anim,))
    call(0x425150, anim, (owner, 1))
    expired = observe()
    u.reg_write(UC_X86_REG_ESI, owner)
    u.reg_write(UC_X86_REG_EBP, anim)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, 0x44EA07, (0x44EA4F, 0x44EA52), count=1000,
                required_addresses=[0x44EA45])
    return dict(slot=slot, owner_expiry=expired, anim_expiry=observe())


if __name__ == '__main__':
    finish_vectors(lambda: [execute(slot) for slot in range(8)], Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            entry_points={'anim_owner_expiry': 0x425150, 'owner_callback': 0x710410,
                          'building_anim_reference_tail': 0x44EA07, 'fire_slot_clear': 0x44EA45},
            assumptions=['Supplied marked Anim, attached Building, one occupied damage-fire slot and real primary/RTTI vtables.',
                         'Preallocated display vectors; no allocator growth. The Building listener tail receives registers from its inherited prefix.'],
            substitutions=[],
            scope='Eight slots: original owner expiry removes Display and retains relative coordinates and the Building slot; subsequent Anim expiry clears that slot. Excludes complete Building/Anim destruction, sound IO and inherited Building listener prefix.'))
