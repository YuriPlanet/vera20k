"""Original Unit73F0A0 numeric answers for the existing repair arguments.

Runs complete class/Foot admission, real Drive and original object/house/type
readers. Supplied ordered lists and raw occupation are intentionally independent.
No gameplay method is replaced; scatter fixture dispatch observers are not called.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, SP
from tools.spatial_oracle.unit_source_scatter import make_source_fixture, CELLS

EXTRA = SCRATCH + 0xA0000
HOUSE, ENEMY, WEAPON, PROJECTILE, WARHEAD = [EXTRA + n for n in (0, 0x6000, 0xC000, 0xD000, 0xE000)]
CELL = CELLS + (10 * 32 + 11) * 0x200


def query(case):
    u, call, read32 = make_source_fixture(dict(case, live_entry=True))
    u.mem_map(EXTRA, 0x30000)
    u.mem_write(0xA8E9A0, b'\x01')
    u.mem_write(TYPE, dwords(0x7F6218))
    u.mem_write(ACTOR + 0x14, dwords(5))  # Techno and Foot abstract flags
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    for address, index in ((HOUSE, 0), (ENEMY, 1)):
        u.mem_write(address + 0x30, dwords(index))
    for x, y, slope in case.get('slopes', []):
        u.mem_write(CELLS + (y * 32 + x) * 0x200 + 0x11C, bytes([slope]))
    if 'restricted_land' in case:
        u.mem_write(TYPE + 0xDFC, dwords(case['restricted_land']))
        u.mem_write(CELL + 0xEC, dwords(case.get('land', 0)))
        u.mem_write(0x89EA40, struct.pack('<108f', *([1.0] * 108)))
        tile = EXTRA + 0x28000
        u.mem_write(0xA8ED2C, dwords(tile + 0x400))
        u.mem_write(tile + 0x400, dwords(tile))
        u.mem_write(tile + 0x2E4, dwords(*case.get('tile_size', [1, 1])))
        u.mem_write(CELL + 0x11A, bytes([case.get('subtile', 0)]))
    if 'tubes' in case:
        table = EXTRA + 0x29000
        u.mem_write(0x8B413C, dwords(table))
        u.mem_write(0x8B4148, dwords(len(case['tubes'])))
        for y in range(32):
            for x in range(32):
                u.mem_write(CELLS + (y * 32 + x) * 0x200 + 0x116, b'\xff\xff')
        for index, tube in enumerate(case['tubes']):
            address = EXTRA + 0x2A000 + index * 0x100
            u.mem_write(table + index * 4, dwords(address))
            x, y = tube['cell']
            u.mem_write(CELLS + (y * 32 + x) * 0x200 + 0x116, struct.pack('<h', index))
            u.mem_write(address + 0x24, packed(*tube.get('entry', [x, y]))
                        + packed(*tube.get('exit', [20, 20])) + dwords(tube['direction']))
    u.mem_write(TYPE + 0xD28, bytes([case.get('crusher', False), case.get('omni', False)]))
    # Original GetWeapon(0)->TechnoType7177C0: type +898 + index*28.
    u.mem_write(TYPE + 0x898, dwords(WEAPON if case.get('armed') else 0))
    u.mem_write(WEAPON + 0xA0, dwords(PROJECTILE))
    u.mem_write(WEAPON + 0xAC, dwords(WARHEAD))
    u.mem_write(PROJECTILE + 0x2A5, bytes([case.get('ag', True)]))
    if 'overlay' in case:
        overlay = EXTRA + 0xF000
        u.mem_write(0xA83D84, dwords(overlay + 0x400))
        u.mem_write(overlay + 0x400 + case['overlay'] * 4, dwords(overlay))
        u.mem_write(CELL + 0x44, dwords(case['overlay']))
    if 'wall' in case:
        overlay = EXTRA + 0xF000
        u.mem_write(0xA83D84, dwords(overlay + 0x400))
        u.mem_write(overlay + 0x400, dwords(overlay))
        u.mem_write(CELL + 0x44, dwords(0))
        u.mem_write(CELL + 0x50, dwords(case.get('wall_owner', -1)))
        u.mem_write(overlay + 0x2A8, b'\x01')
        u.mem_write(overlay + 0x22D, bytes([case['wall']]))
        u.mem_write(WARHEAD + 0x144, bytes([case.get('wall_weapon', False)]))
        u.mem_write(TYPE + 0x5B4, dwords(12 if case.get('crusher_all') else 0))
    u.mem_write(CELL + 0x124, dwords(case.get('bits', 0), case.get('deck_bits', 0)))
    u.mem_write(CELL + 0x54, dwords(case.get('owner', -1), case.get('deck_owner', -1)))
    nodes = []
    for index, node in enumerate(case.get('objects', [])):
        if node.get('self'):
            address = ACTOR
        else:
            address = EXTRA + 0x10000 + index * 0x5000
            typ, loco = address + 0x1000, address + 0x3000
            building = node.get('building', False)
            u.mem_write(address, dwords(0x7E3EBC if building else 0x7F5C70))
            u.mem_write(address + (0x520 if building else 0x6C4), dwords(typ))
            u.mem_write(typ, dwords(0x7E4570 if building else 0x7F6218))
            u.mem_write(address + 0x14, dwords(1 if building else 5))
            u.mem_write(address + 0x21C, dwords(ENEMY if node.get('enemy') else HOUSE))
            u.mem_write(address + 0x220, dwords(2 if node.get('cloaked') else 0))
            u.mem_write(address + 0x9C, dwords(2944, 2688, 0))
            u.mem_write(typ + 0x22D, bytes([node.get('crushable', False)]))
            u.mem_write(typ + 0xD2A, bytes([node.get('resistant', False)]))
            u.mem_write(address + 0xAC, dwords(5))
            u.mem_write(address + 0xB4, dwords(-1))
            if building and node.get('gate'):
                u.mem_write(typ + 0x16B7, b'\x01')
                if node.get('open'):
                    u.mem_write(address + 0xAC, dwords(24))
                    u.mem_write(address + 0x368, b'\x00\x01')
            if not building:
                call(0x4AF540, loco, [])
                u.mem_write(loco + 0xC, dwords(address))
                u.mem_write(address + 0x674, dwords(loco + 4))
                u.mem_write(address + 0x6B6, bytes([node.get('occupation', True)]))
                u.mem_write(address + 0x5A4, dwords(CELL if node.get('nav') else 0))
                if node.get('turn'):
                    call(0x4C91C0, address + 0x388, [])
                    call(0x4C9680, address + 0x388, [5])
                    u.mem_write(loco + 0x100, struct.pack('<H', 0x4000))
                    call(0x4C9220, address + 0x388, [loco + 0x100])
        nodes.append(address)
    u.mem_write(CELL + (0xE8 if case.get('deck') else 0xE4), dwords(nodes[0] if nodes else 0))
    for index, address in enumerate(nodes):
        u.mem_write(address + 0x30, dwords(nodes[index + 1] if index + 1 < len(nodes) else 0))
    if case.get('deck'):
        u.mem_write(CELL + 0x140, dwords(0x100))
    if 'nav_target' in case:
        u.mem_write(ACTOR + 0x5A4, dwords(nodes[case['nav_target']]))
    seen = []
    def observe(_u, address, _size, _data):
        if address in (0x73F0A0, 0x4D9C10, 0x55ABF0, 0x73FC24, 0x47EBA0):
            seen.append(hex(address))
    u.hook_add(UC_HOOK_CODE, observe)
    before = bytes(u.mem_read(EXTRA, 0x30000))
    previous = case.get('previous')
    previous_ptr = CELLS + (previous[1] * 32 + previous[0]) * 0x200 if previous else 0
    call(0x73F0A0, ACTOR, [CELL, case.get('direction', -1), case.get('height', -1), previous_ptr, 1])
    assert u.reg_read(UC_X86_REG_ESP) == SP + 24
    assert bytes(u.mem_read(EXTRA, 0x30000)) == before
    return dict(input=case, result=u.reg_read(UC_X86_REG_EAX), calls=seen)


def generate():
    cases = [dict(bits=bits, owner=owner, armed=armed, ag=ag, crusher=crusher)
             for bits in (0, 0x20, 0x1C, 0x3F) for owner in (-1, 0, 1)
             for armed, ag, crusher in ((False, True, False), (True, True, False),
                                       (True, False, False), (False, True, True))]
    cases += [dict(objects=[node], bits=bits, armed=armed)
              for node in ({}, {'nav': True}, {'enemy': True}, {'enemy': True, 'cloaked': True},
                           {'self': True}, {'building': True})
              for bits in (0, 0x20) for armed in (False, True)]
    cases += [dict(objects=nodes, bits=bits, crusher=True, armed=True)
              for nodes in ([{'enemy': True, 'crushable': True}],
                            [{'enemy': True, 'crushable': True}, {'enemy': True}],
                            [{'enemy': True}, {'enemy': True, 'crushable': True}],
                            [{'enemy': True, 'crushable': True}, {'nav': True}])
              for bits in (0, 0x20)]
    cases += [dict(mission=7, nav_target=0, objects=[{}, {'building': True}], bits=0x20),
              dict(deck=True, deck_bits=0x20, bits=0),
              dict(deck=True, deck_bits=0, bits=0x20)]
    cases += [dict(objects=[dict(building=True, gate=True, enemy=enemy, open=opened)], armed=armed)
              for enemy in (False, True) for opened in (False, True) for armed in (False, True)]
    cases += [dict(wall=crushable, wall_owner=owner, crusher=crusher,
                   crusher_all=all_walls, armed=armed, wall_weapon=weapon)
              for crushable in (False, True) for owner in (-1, 0, 1)
              for crusher, all_walls, armed, weapon in (
                  (False, False, False, False), (False, False, True, True),
                  (True, False, False, False), (False, True, False, False))]
    cases += [dict(objects=[dict(turn=True)]),
              dict(objects=[dict(nav=True, occupation=False)]),
              dict(crusher=True, armed=True, objects=[dict(enemy=True, crushable=True), dict(enemy=True, cloaked=True)])]
    cases += [dict(crusher=crusher, omni=omni, objects=[dict(enemy=True, crushable=crushable,
                   resistant=resistant, building=building)])
              for crusher in (False, True) for omni in (False, True)
              for crushable in (False, True) for resistant in (False, True)
              for building in (False, True)]
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Complete Unit73F0A0 numeric results with existing repair arguments(-1,-1,NULL,true): raw occupation, ordered Unit/Building lists, wall and Gate arms; excludes non-Drive locomotors, direction/height traversal and full repair/scatter effects.',
        entry_points={'unit_entry': 0x73F0A0, 'foot_entry': 0x4D9C10,
                      'drive_entry': 0x55ABF0, 'tail': 0x73FC24, 'get_unit': 0x47EBA0},
        assumptions=['Shared Unit/Drive/source map fixture; actual vtables and typed prestates. Type IsTrain=false, unrestricted land, no tubes; land speed1.0. Raw owner indices -1/0/1 and independent supplied lists. Optional Wall overlay/owner/Crushable, primary warhead Wall and MovementZone CrusherAll are supplied.',
                     'Objects are supplied Unit or Building nodes; Drive constructed, unpowered/turning/lifecycle producers excluded. Ordinary bodies/owner queries/GetWeapon/CrushableBy execute. No native scenario load.',
                     'Object and house memory remains unchanged. Source fixture QueueMission/SetDestination observer slots are never reached.'],
        substitutions=['Only OS Interlocked imports from the shared Unit fixture. No gameplay callable substitution.']))
