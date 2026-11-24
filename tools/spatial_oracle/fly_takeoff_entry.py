"""Original BeginTakeoff4CF950, optionally reached through complete MoveTo.

Real AirTracker, Facing and sound-dispatch receiver execute; no gameplay calls
are replaced. Sound index-1 excludes mixer/assets, not the dispatch call.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.fly_landing_phase import fixture, LOCO, RULES, OWNER, TYPE, dwords

ARG = SCRATCH + 0xA0000


def execute(case):
    f, state = fixture(dict(case, landing=False, latched=True))
    u = f.u
    u.mem_write(TYPE + 0x52C, dwords(-1))
    u.mem_write(TYPE + 0x618, dwords(case.get('flight_level', -1)))
    u.mem_write(RULES + 0x7B4, dwords(1500))
    u.mem_write(LOCO + 0x10, bytes([case.get('powered', True)]))
    u.mem_write(LOCO + 0x38, dwords(37))
    u.mem_write(LOCO + 0x34, b'\0')
    u.mem_write(LOCO + 0x1C, dwords(0, 0, 0))
    u.mem_write(0xA8ED84, dwords(90))
    for offset, initial, target in ((0x388, 0x4000, 0xC000), (0x3A0, 0x6000, 0x2000)):
        f.call(0x4C91C0, OWNER + offset, [])
        # Facing rate is supplied as in Aircraft InitFromType; original Snap
        # and Set initialize retained destination, history and countdown.
        u.mem_write(OWNER + offset + 20, dwords(5 << 8))
        u.mem_write(ARG, dwords(initial))
        f.call(0x4C9300, OWNER + offset, [ARG])
        u.mem_write(ARG, dwords(target))
        f.call(0x4C9220, OWNER + offset, [ARG])
    u.mem_write(0xA8ED84, dwords(100))
    events = []

    def controls():
        facings = []
        for offset in (0x388, 0x3A0):
            data = list(struct.unpack('<6I', u.mem_read(OWNER + offset, 24)))
            facings.append(dict(destination=data[0] & 65535, previous=data[1] & 65535,
                                start=data[2], duration=data[4], rate=data[5] & 65535))
        return dict(state=state(), target_height=struct.unpack('<i', u.mem_read(LOCO + 0x38, 4))[0],
                    facings=facings, mode=bool(u.mem_read(LOCO+0x5C,1)[0]))

    def observe(_u, pc, _size, _data):
        if pc == 0x4134A0:
            events.append('air_add')
        elif pc == 0x4C9300:
            assert u.reg_read(UC_X86_REG_ECX) == OWNER + 0x388
            events.append('primary_snap')
        elif pc == 0x7509E0:
            events.append('takeoff_sound')
        elif pc == 0x578080:
            sp = u.reg_read(UC_X86_REG_ESP)
            ptr = struct.unpack('<I', u.mem_read(sp+4,4))[0]
            events.append(['ground', list(struct.unpack('<iii',u.mem_read(ptr,12)))])

    before = controls()
    u.hook_add(UC_HOOK_CODE, observe)
    if case.get('move'):
        request = [16768, 16512, 0]
        u.mem_write(f.sp, dwords(RET_MAGIC, LOCO+4, *request))
        u.reg_write(UC_X86_REG_ESP, f.sp)
        run_checked(u,0x4CCC80,RET_MAGIC,count=500000)
        assert u.reg_read(UC_X86_REG_ESP) == f.sp + 20
    else:
        f.call(0x4CF950, LOCO, [])
    return dict(input=case, before=before, after=controls(), events=events)


def generate():
    cases = [dict(z=z, air_registered=registered, move=move)
             for z in (0, 1, 208, 900) for registered in (False, True) for move in (False, True)]
    cases += [dict(z=0, air_registered=False, move=move, **extra)
              for move in (False, True) for extra in
              (dict(powered=False), dict(health=0), dict(flight_level=40000),
               dict(bridge=True, on_bridge=True), dict(level=2))]
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'begin_takeoff':0x4CF950,'move_to':0x4CCC80,'air_add':0x4134A0,
                      'facing_snap':0x4C9300},
        assumptions=['Real Aircraft, Fly and spatial callees from landing fixture. No Team, cargo, target or contacts.',
                     'Supplied height, power, existing air membership and original Facing Snap/Set at frame90, observation frame100.',
                     'AuxSound1 index-1; no playback/asset claim. Warp, EMP and Foot+6A0 refusal producers excluded.'],
        substitutions=[],
        scope='26 complete BeginTakeoff or enclosing non-null MoveTo calls: air add only when unregistered, height-zero Primary snap to Secondary destination, height/type reads, power versus health admission and final stored-coordinate ground-query order. Excludes complete Fly Process, docking/landing entry and rendered output.'))
