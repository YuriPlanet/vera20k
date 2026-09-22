"""Original Aircraft4196B0 landing-cell query, including Cell486840 and586360.

Supplied per-viewer Cell+12C flags distinguish retained ground knowledge from
current sight. No function is replaced. Team script admission remains excluded.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX
from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, OWNER, BLOCKER, cell, dwords

OUT = SCRATCH + 0x30000
HOUSE = SCRATCH + 0x31000


def execute(case):
    f = Fixture(dict(game_mode=case.get('game_mode', 0)))
    u = f.u
    target = cell(64, 64)
    u.mem_write(target, dwords(0x7E4EEC))
    u.mem_write(target + 0x11B, bytes([case['level'], case['slope']]))
    u.mem_write(OWNER + 0x41A, bytes([case.get('owned', True)]))
    u.mem_write(OWNER + 0x3D4, bytes([case.get('mission_only', False)]))
    u.mem_write(OWNER + 0x21C, dwords(HOUSE))
    for address in (0x89E7C0, 0xAC13C8, 0xABDE88):
        u.mem_write(address, dwords(104))
    f.call(0x49F2F0, 0, [])
    query = case.get('query', [16512, 16512, 0])
    u.mem_write(OUT+16, dwords(*query))
    assert f.call(0x565730, 0x87F7E8, [OUT+16]) == target
    f.call(0x486840, target, [OUT])
    coord = list(struct.unpack('<iii', u.mem_read(OUT, 12)))
    # Set known input knowledge on both possible projected cells. This setup
    # uses the original coordinate getter;4196B0/586360 decide admission.
    q = int(coord[2] / 104)
    offset = int(q / 2) + (q & 1)
    projected = [64-offset, 64-offset]
    diagonal = [projected[0]+1, projected[1]+1]
    for xy in (projected, diagonal):
        u.mem_write(cell(*xy) + 0x12C, b'\0')
    exposed = projected if case.get('first', True) else diagonal
    u.mem_write(cell(*exposed) + 0x12C, bytes([case['bits']]))
    if case.get('occupant'):
        u.mem_write(target + 0xE4, dwords(BLOCKER))
        u.mem_write(BLOCKER + 0x21C, dwords(HOUSE))
    events = []

    def observe(_u, pc, _size, _data):
        if pc == 0x4834A0:
            events.append('winged_passability')
        elif pc == 0x586360:
            events.append('shroud')
        elif pc == 0x486840:
            assert u.reg_read(UC_X86_REG_ECX) == target
            events.append('cell_coordinate')

    u.hook_add(UC_HOOK_CODE, observe)
    result = f.call(0x4196B0, OWNER, [target, -1, -1, 0, 1])
    return dict(input=case, query=query, coordinate=coord, projected=projected, diagonal=diagonal,
                exposed=exposed, result=result, events=events)


def generate():
    cases = [dict(level=l, slope=s, bits=b, first=first)
             for l in (0, 1, 2, 3) for s in (0, 1, 4)
             for b in (0, 8, 16, 24) for first in (False, True)]
    cases += [dict(level=1, slope=0, bits=0, **extra) for extra in
              (dict(game_mode=1), dict(owned=False), dict(mission_only=True), dict(occupant=True))]
    # Fixed512-row indexing aliases requested cell576,63 to actual Cell64,64.
    cases += [dict(level=1, slope=0, bits=8, first=first, query=[147584,16256,0])
              for first in (False, True)]
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'can_enter': 0x4196B0, 'cell_coord': 0x486840, 'shroud': 0x586360},
        assumptions=['Real Aircraft/Cell vtables; no Team. Mode0 owned aircraft except explicit gate cases.',
                     'Ground104 globals initialized, supplied levels/slopes and raw Cell+12C bits. Same-house occupant only.',
                     'No visibility lifecycle, current-sight producer or complete movement transaction is claimed.'],
        substitutions=[],
        scope='102 full Aircraft Can_Enter_Cell calls: ground shroud bit8 versus bit16, even/odd projected height, two projected cells, mode/owner/mission gates, same-house occupant and two fixed-grid alias inputs through original MapAtCoord. Excludes Team6EC300, hostile occupant side effects, absent Cell slots and full Fly landing/Process.'))
