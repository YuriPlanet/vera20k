"""Original type FlightLevel reader and complete effective-height getter."""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, RULES, SP, dwords


def generate():
    rows = []
    for raw in (None, '', '-1', '0', '1', '500', '1500', '2200', '-2',
                '$600', '600h', '2200junk', 'junk', '2147483647', '-2147483648'):
        for general in (500, 1500):
            f = Fixture()
            f.ini(0x83C854, raw)
            u = f.u
            u.mem_write(TYPE + 0x618, dwords(-1))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EBP, TYPE)
            u.reg_write(UC_X86_REG_ESI, INI)
            u.reg_write(UC_X86_REG_EBX, TYPE + 0x1F8)
            run_checked(u, 0x712336, 0x712350, count=100000,
                        required_addresses=[0x5276D0, 0x71234A])
            assert u.reg_read(UC_X86_REG_ESP) == SP
            stored = struct.unpack('<i', u.mem_read(TYPE + 0x618, 4))[0]
            u.mem_write(0x8871E0, dwords(RULES))
            u.mem_write(RULES + 0x7B4, dwords(general))
            u.mem_write(SP, dwords(RET_MAGIC))
            u.reg_write(UC_X86_REG_ECX, TYPE)
            run_checked(u, 0x717800, RET_MAGIC, count=100)
            assert u.reg_read(UC_X86_REG_ESP) == SP + 4
            value = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
            rows.append(dict(raw=raw, general=general, stored=stored, effective=value))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'read_call': 0x712336, 'read_end': 0x712350,
                      'read_integer': 0x5276D0, 'effective_height': 0x717800},
        assumptions=['Supplied cached INI entry/index and TechnoType constructor default -1 (711050, EBP=-1).',
                     'General FlightLevel supplied as500 or retail1500; original getter executes in full.'],
        substitutions=[],
        scope='30 original FlightLevel reads and effective-height queries. Excludes file loading, full constructors, locomotor state transitions and movement.',
    ))
