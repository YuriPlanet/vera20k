"""Original Fly Process's effective-Mission cruise-mode reset.

Execute4CD664..4CD67F and its real owner virtual GetMission5B3040. This is an
interior range with supplied full Fly receiver; no gameplay call is replaced.
"""
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.crate_speed_effect import fixture
from tools.spatial_oracle.map_queries import dwords

LOCO = SCRATCH + 0x1B000


def generate():
    rows = []
    for current in (-1, -2, 0, 5, 7):
        for queued in (-1, 5, 7):
            for mode in (False, True):
                u, sp, actors, _ = fixture(dict(actors=[dict(kind='aircraft')]))
                owner = actors[0]
                u.reg_write(UC_X86_REG_ECX, LOCO)
                u.reg_write(UC_X86_REG_ESP, sp)
                u.mem_write(sp, dwords(RET_MAGIC))
                run_checked(u, 0x4CC9A0, RET_MAGIC, count=1000)
                assert u.mem_read(LOCO + 0x5C, 1) == b'\0'
                u.mem_write(LOCO + 0xC, dwords(owner))
                u.mem_write(LOCO + 0x5C, bytes([mode]))
                u.mem_write(owner + 0xAC, dwords(current))
                u.mem_write(owner + 0xB4, dwords(queued))
                u.reg_write(UC_X86_REG_ESI, LOCO)
                u.reg_write(UC_X86_REG_ESP, sp)
                run_checked(u, 0x4CD664, 0x4CD67F, count=1000,
                            required_addresses=[0x5B3040])
                assert u.reg_read(UC_X86_REG_ESP) == sp
                rows.append(dict(current=current, queued=queued, before=mode,
                                 after=bool(u.mem_read(LOCO + 0x5C, 1)[0])))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'process_range': 0x4CD664, 'stop_before': 0x4CD67F,
                      'constructor': 0x4CC9A0, 'mission_getter': 0x5B3040},
        assumptions=['Real Aircraft vtable+184 and Fly ctor; supplied owner, current/queued Mission dwords and previous mode.',
                     'Range starts after tracker synchronization and stops before power/health branching. No callbacks or RNG calls occur.'],
        substitutions=[],
        scope='30 original mode-reset ranges including ctor zero, Enter/current/queued precedence and unknown negative Mission. Excludes the rest of Process, null MoveTo and landing callbacks.',
    ))
