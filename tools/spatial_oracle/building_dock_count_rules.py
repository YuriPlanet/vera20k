"""Original signed dock-count read and separate Building contact-capacity gate."""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, SP, dwords


def generate():
    f = Fixture()
    u = f.u
    rows = []
    for initial in [1, 4, -7]:
        for raw in [None, '', '0', '-7', '1', '255', '256', '300', '$100',
                    '100h', '-2147483648', '2147483647', '2147483648',
                    '4294967295', '12junk', 'junk']:
            f.ini(0x8194C4, raw)
            u.mem_write(TYPE + 0x24, b'TEST\0')
            f.write(INI + 4, TYPE + 0x24)
            f.write(TYPE + 0x1780, initial)
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EBP, TYPE)
            u.reg_write(UC_X86_REG_ESI, INI)
            run_checked(u, 0x46492E, 0x46494B,
                        required_addresses=[0x464940, 0x5276D0, 0x464945])
            count = struct.unpack('<i', u.mem_read(TYPE + 0x1780, 4))[0]
            assert u.reg_read(UC_X86_REG_ESP) == SP
            u.reg_write(UC_X86_REG_EAX, TYPE)
            run_checked(u, 0x43BCBD, 0x43BCCD, required_addresses=[0x43BCC3])
            capacity = u.reg_read(UC_X86_REG_EAX)
            rows.append(dict(initial=initial, raw=raw, count=count, capacity=capacity))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original BuildingType46492E..46494B signed NumberOfDocks read and Building43BCBD..43BCCD contact capacity gate; not array allocation or full Rules pass lifetime.',
        assumptions=['Cached TEST INI index uses native CRC and supplied raw input.',
                     'Current type count supplied as reader default; contact constructor is stopped before allocation.'],
        substitutions=[], entry_points={'count_reader': 0x46492E, 'integer_reader': 0x5276D0,
                                         'contact_capacity': 0x43BCBD}))
