"""Read-only reproduction of three bounded native coordinate witnesses.

Run as a module with VERA20K_GAMEMD_EXE set. --check writes no output files;
neither mode patches code, substitutes callees, invokes Cargo, or writes Ghidra.
This is evidence preservation, not a complete production parity harness.
"""
from pathlib import Path
import hashlib
import json
import struct


from tools.native_oracle import NATIVE_SHA256, SCRATCH, RET_MAGIC, run_checked
from tools.spatial_oracle.object_health import Fixture, OBJ, TYPE, SP, dwords, i32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)

# Include entered corridors and consequential native callees in the unchanged
# byte check. Fixture.reset supplies data and an original copied Building VT;
# it leaves Fixture.stubs empty, so no reached callback is replaced.
SPANS = (
    (0x6D1DC5, 0x6D1E24), (0x4518E3, 0x451932),
    (0x6D2360, 0x6D241C), (0x5AFB80, 0x5AFBF6),
    (0x459EF0, 0x459F1B), (0x7C5F00, 0x7C5F2C),
    (0x72AD20, 0x72AD82), (0x424507, 0x42464C),
    (0x422BE0, 0x422C4B), (0x5F65A0, 0x5F65D0),
)


def read_xyz(u, address):
    return list(struct.unpack('<iii', u.mem_read(address, 12)))


def generate():
    f = Fixture()  # load_image verifies the actual PE bytes against NATIVE_SHA256
    f.reset('building', 100, 100)
    u = f.u
    original = [bytes(u.mem_read(a, b-a)) for a, b in SPANS]
    tactical = SCRATCH + 0x10000
    u.mem_map(tactical, 0x2000)

    # Execute the authentic matrix stores. EBX is the constructor's zero;
    # the stop precedes its stack epilogue. This also sets global887324.
    u.reg_write(UC_X86_REG_ESI, tactical)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, 0x6D1DC5, 0x6D1E24)
    u.mem_write(0xB0CE30, dwords(640, 480))
    u.mem_write(0xB0CE08, dwords(0, 0, 0))
    constructors = []
    for offset, expected in [
        ((640, 0), [2560, 5120, 104]),
        ((-16777217, -16777219), [-214747488, -71578240, 104]),
        ((30, 15), [2816, 5120, 104]),
    ]:
        f.reset('building', 100, 100)
        # reset preserves separately mapped Tactical and image runtime globals.
        u.mem_write(OBJ+0x9C, dwords(2688, 5248, 104))
        u.mem_write(TYPE+0xF7C+3*0x44, dwords(*offset))
        u.mem_write(SP+0x3C, dwords(3))
        u.reg_write(UC_X86_REG_ESI, OBJ)
        u.reg_write(UC_X86_REG_ESP, SP)
        # Start after type lookup/admission. Stop before allocator invocation;
        # ECX,EDX,EAX are the exact constructor XYZ just computed.
        run_checked(u, 0x4518E3, 0x451932,
                    required_addresses=[0x6D2360, 0x459EF0])
        result = [i32(u.reg_read(r)) for r in
                  (UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX)]
        assert result == expected
        constructors.append(dict(offset=offset, raw_building=[2688,5248,104],
                                 supplied_tactical_bounds=[640,480], world=result))

    rectangles = []
    # Explicit ordinary sidebar flags; the original loaded PE/BSS supplied
    # the same zero values in the ephemeral probe. Only X origin uses them.
    u.mem_write(0xA8EB7C, b'\0')
    u.mem_write(0xA8ED6B, b'\0')
    for width, height, expected in [
        (640,480,[168,0,472,448]),
        (800,600,[168,0,632,568]),
        (1024,768,[168,0,856,736]),
    ]:
        u.mem_write(0x886FB0, dwords(0,0,width,height))
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ECX, OBJ)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x72AD20, RET_MAGIC)
        result = list(struct.unpack('<iiii', u.mem_read(OBJ,16)))
        assert result == expected
        rectangles.append(dict(visible=[width,height], rectangle=result))

    damage = []
    for coord in [(2560,5120,104),(5290,2390,104)]:
        f.reset('building',100,100)
        # Data-shaped Anim receiver with authentic Anim VT. This is entry
        # after the earlier AI admission, not a complete constructor or AI.
        u.mem_write(OBJ, dwords(0x7E3354))
        u.mem_write(OBJ+0xC8, dwords(TYPE))
        u.mem_write(OBJ+0xCC, dwords(0))
        u.mem_write(OBJ+0x9C, dwords(*coord))
        u.mem_write(TYPE+0x24, b'TEST\0')
        u.mem_write(TYPE+0x2A8, struct.pack('<d',1.0))
        u.mem_write(OBJ+0x188, struct.pack('<d',0.0))
        u.reg_write(UC_X86_REG_ESI, OBJ)
        u.reg_write(UC_X86_REG_ESP, SP)
        # Stop at DamageArea ENTRY: inspect its arguments without executing
        # map lookup/damage. No damage callee result is supplied or assumed.
        run_checked(u, 0x424507, 0x489280, required_addresses=[0x422BE0])
        result = read_xyz(u, u.reg_read(UC_X86_REG_ECX))
        amount = u.reg_read(UC_X86_REG_EDX)
        assert result == list(coord) and amount == 1
        damage.append(dict(stored_xyz=coord, owner_pointer=0,
                           supplied_type_damage=1, supplied_accumulator=0,
                           damage_area_xyz=result, damage_amount=amount))

    assert not f.stubs
    assert original == [bytes(u.mem_read(a,b-a)) for a,b in SPANS]
    return dict(
        native_sha256=NATIVE_SHA256, fpcw='0E7F',
        substitutions=[],
        unchanged_spans=[dict(start=f'{a:08X}', end_exclusive=f'{b:08X}',
                              sha256=hashlib.sha256(data).hexdigest())
                         for (a,b),data in zip(SPANS,original)],
        constructor_rows=constructors, rectangle_rows=rectangles,
        damage_argument_rows=damage,
        limits=[
            'Constructor witness starts after slot/type admission and stops before allocation.',
            'Tactical640x480 is a supplied witness input, not visible640x480 or a proposed default.',
            'Rectangle witness runs arithmetic only, not actual display initialization.',
            'Damage witness supplies admitted Anim data, starts at damage arm, and stops at DamageArea entry.',
            'Damage accumulator0 is supplied; ordinary Anim constructor seeds1. No cadence claim.',
            'No full renderer, map damage, save loader, or all-caller parity claim.',
        ],
    )


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors, provenance
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Building pixel-coordinate constructor inputs, tactical bounds, and Anim damage-coordinate forwarding',
        assumptions=['Original matrix initialization; supplied admitted Building/Anim receivers and tactical bounds. No allocation, damage callee, or complete lifecycle execution. Detailed limits and code hashes are in the payload.'],
        substitutions=['None; unchanged native corridors and callees execute.'],
        entry_points={'matrix': 0x6D1DC5, 'building_coordinate': 0x4518E3, 'bounds': 0x72AD20, 'damage_argument': 0x424507},
    ))
