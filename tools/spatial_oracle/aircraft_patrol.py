"""Execute retail AircraftClass::Mission_Patrol417300 on supplied worlds.

Reuse aircraft_move.Run's native flat map, Find_Attack_Cell, landing, coordinates,
MissionControl, ftol and Scenario RNG. The destination/idle/locomotor boundaries
remain recorded substitutes. FindDock, GreatestThreat and Override are recorded
answers, not executions of those separate mechanisms. A Team attached by the
destination substitute exercises the handler's live Team/NavCom reread; its
AssignMissionTarget is also a recorded boundary. Legacy House+210 paths stay empty.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.aircraft_move import (
    AIRCRAFT_VT, BLOCKER, LOCO, LOCO_VT, MISSION_CONTROL, OWNER, STUB_AT, STUBS,
    VTABLE, Run, dwords, i32, name, word,
)

PATROL, ENTRY, TEAM = 25, 0x417300, SCRATCH + 0xA25000
EXTRA = {STUBS + 0x50: ('find_dock', 12), STUBS + 0x60: ('scan', 12),
         STUBS + 0x70: ('override', 12), 0x6E9050: ('team_target', 4)}


class Patrol(Run):
    def __init__(self, row):
        super().__init__(row)
        u = self.f.u
        vt = bytearray(u.mem_read(AIRCRAFT_VT, 0x600))
        assert word(u, AIRCRAFT_VT + 0x25C) == ENTRY
        for slot, address in {0x480: STUB_AT['assign_destination'],
                              0x484: STUB_AT['enter_idle_mode'],
                              0x528: STUBS + 0x50, 0x3C4: STUBS + 0x60,
                              0x1F4: STUBS + 0x70}.items():
            struct.pack_into('<I', vt, slot, address)
        u.mem_write(VTABLE, bytes(vt))
        u.mem_write(OWNER, dwords(VTABLE))
        u.mem_write(LOCO, dwords(LOCO_VT))
        u.mem_write(LOCO_VT + 0x10, dwords(STUB_AT['is_moving']))
        u.mem_write(LOCO_VT + 0x44, dwords(STUB_AT['move_to']))
        u.mem_write(OWNER + 0x674, dwords(LOCO))
        for offset, value in {0xAC: PATROL, 0xBC: row['state'],
                              0x2FC: row.get('ammo', 1), 0x5A4: self.param()}.items():
            u.mem_write(OWNER + offset, dwords(value & 0xFFFFFFFF))
        u.mem_write(BLOCKER + 0x14, dwords(2 if row.get('techno', True) else 0))
        for mission, rate in ((PATROL, row.get('rate', .016)), (5, .03)):
            widened = struct.unpack('<f', struct.pack('<f', rate))[0]
            u.mem_write(MISSION_CONTROL + mission * 0x20 + 0x10, struct.pack('<d', widened))
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=0x6E9050, end=0x6E9050)
        u.hook_add(UC_HOOK_CODE, self.patrol_draw, begin=0x417365, end=0x417365)

    def patrol_draw(self, u, _address, _size, _data):
        self.draws.append(['epilogue', i32(u.reg_read(UC_X86_REG_EAX))])

    def on_stub(self, u, address, size, data):
        if address not in EXTRA:
            super().on_stub(u, address, size, data)
            if address == STUB_AT['assign_destination'] and self.row.get('attach_team'):
                # A supplied synchronous destination callback changes both
                # pointers before41738D rereads them. This is not a claim that
                # a particular stock callback attaches teams this way.
                u.mem_write(OWNER + 0x5D4, dwords(TEAM))
                u.mem_write(OWNER + 0x5A4, dwords(BLOCKER))
            if address == STUB_AT['enter_idle_mode'] and self.row.get('idle_guard'):
                u.mem_write(OWNER + 0xAC, dwords(5))
                u.mem_write(OWNER + 0xBC, dwords(0))
            return
        stub, pops = EXTRA[address]
        sp = u.reg_read(UC_X86_REG_ESP)
        args = [word(u, sp + 4 * (n + 1)) for n in range(pops // 4)]
        assert u.reg_read(UC_X86_REG_ECX) == (TEAM if stub == 'team_target' else OWNER)
        result = 0
        if stub == 'find_dock':
            assert args == [word(u, OWNER + 0x6C4) + 0x3E8, 0, 0]
            self.calls.append(['find_dock'])
            result = BLOCKER if self.row.get('dock') else 0
            if 'dock_ammo' in self.row:
                u.mem_write(OWNER + 0x2FC, dwords(self.row['dock_ammo'] & 0xFFFFFFFF))
        elif stub == 'scan':
            assert args[0] == 2 and args[2] == 0
            assert bytes(u.mem_read(args[1], 12)) == bytes(u.mem_read(OWNER + 0x9C, 12))
            self.calls.append(['scan', 2, 0])
            result = BLOCKER if self.row.get('threat') else 0
        elif stub == 'override':
            self.calls.append(['override', i32(args[0]), name(args[1]), name(args[2])])
            # Model a setter callback changing the state. Native must still
            # store0 after the virtual returns.
            u.mem_write(OWNER + 0xBC, dwords(99))
        else:
            self.calls.append(['team_target', name(args[0])])
        u.reg_write(UC_X86_REG_EAX, result)
        u.reg_write(UC_X86_REG_EIP, word(u, sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)


def rows():
    result = [dict(name=f's{state}_{nav}_{moving}', state=state, param=nav, moving=moving)
              for state in (0, 1, 2, 3, 4, 5, 0xFFFFFFFF)
              for nav in (None, [64, 64], 'blocker') for moving in (False, True)]
    result += [dict(name=label, state=2, param=[64, 64], moving=True, **values)
               for label, values in [
                   ('empty_dock', dict(ammo=0, dock=True, threat=True)),
                   ('empty_no_dock', dict(ammo=0, threat=True)),
                   ('dock_callback_ammo', dict(ammo=0, dock_ammo=1, threat=True)),
                   ('negative_ammo', dict(ammo=-1, threat=True)),
                   ('attack', dict(threat=True)),
                   ('non_techno', dict(threat=True, techno=False)),
                   ('reserved', dict(reserved=[[64, 64]])),
               ]]
    result += [dict(name='state4_no_scan', state=4, param=[64, 64], moving=True,
                    ammo=0, dock=True, threat=True),
               dict(name='state3_idle_guard', state=3, param=[64, 64], moving=True, idle_guard=True),
               dict(name='idle_current_rate', state=0, moving=True, idle_guard=True),
               dict(name='team_live_nav', state=0, param=[64, 64], moving=True, attach_team=True),
               dict(name='search_ring', state=0, param=[64, 64], moving=True,
                    occupied=[[64, 64]], rate=.03, seed=99)]
    return result


def generate():
    result = []
    for row in rows():
        run = Patrol(row)
        delay = i32(run.f.call(ENTRY, OWNER, []))
        result.append(run.finish(delay=delay, state=word(run.f.u, OWNER + 0xBC), calls=run.calls))
    return dict(mission_patrol=result)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Aircraft Mission_Patrol states0..5 and UINT_MAX, real attack-cell search/landing/'
              'Rate/RNG, supplied destination/idle/locomotor/dock/scan/override/team boundaries.',
        entry_points={'mission_patrol': ENTRY, 'find_attack_cell': 0x418E20,
                      'cell_free_for_landing': 0x419B00, 'mission_control': 0x5B3A00},
        assumptions=['aircraft_move.Run flat map; no legacy House+210 path; float-widened '
                     'MissionControl rates; source Team null before the search.'],
        substitutions=['Recorded destination, idle and locomotor virtuals; supplied dock and '
                       'scan answers, recorded Override and Team AssignMissionTarget. '
                       'Callback mutations are conditional controls, not observed stock callbacks.'],
    ))
