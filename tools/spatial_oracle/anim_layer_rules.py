"""Original AnimType Layer reader with supplied cached INI entry/index."""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, SP, dwords


def generate():
    rows = []
    for raw in (None, '', 'Underground', 'Surface', 'Ground', 'Air', 'Top',
                'gRoUnD', ' air ', 'None', 'bogus', '-1', '0', '1', '2', '3', '4',
                'Ground' + ' ' * 130, ' ' * 127 + 'Top'):
        f = Fixture()
        # File lexical parsing is excluded; blank values are absent entries.
        f.ini(0x818644, raw.strip() if raw and raw.strip() else None)
        f.u.mem_write(SP, dwords(RET_MAGIC, TYPE + 0x1F8, 0x818644, 3))
        f.u.reg_write(UC_X86_REG_ECX, INI)
        f.u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(f.u, 0x477050, RET_MAGIC, count=100000)
        assert f.u.reg_read(UC_X86_REG_ESP) == SP + 16
        value = struct.unpack('<i', dwords(f.u.reg_read(UC_X86_REG_EAX)))[0]
        rows.append(dict(raw=raw, layer=value))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'read_layer': 0x477050, 'name_to_layer': 0x48E050,
                      'anim_reader_call': 0x427DF2, 'constructor_default': 0x4276D4},
        assumptions=['Cached INI index with one Layer entry; lexical trimming/empty omission supplied.',
                     'Retained default Air3 established at AnimType constructor4276D4. Native128-byte reader and five-name lookup execute unchanged.'],
        substitutions=[],
        scope='19 Anim Layer reads including all native names, absent values, case folding, unknown/numeric tokens and lexical whitespace. Excludes file loading and complete AnimType construction.',
    ))
