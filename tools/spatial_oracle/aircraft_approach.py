"""Full original Aircraft Mission_Attack states0/3, including facing and setters.

Uses the real classifier, Fly IsMovingNow, DistanceTo/GetFLH, Facing Set and
Aircraft/Foot destination chain. The shared fixture substitutes only OS atomics.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX
from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.aircraft_reengagement import execute as dispatch
from tools.spatial_oracle.aircraft_reengagement import OWNER, TARGET, TYPE, LOCO, dwords, cell

ARG = SCRATCH + 0xA0000


def execute(case):
    retained = {}

    def configure(f):
        retained['fixture'] = f
        u = f.u
        u.mem_write(OWNER + 0xBC, dwords(case.get('state', 3)))
        u.mem_write(TYPE + 0xE0E, bytes([case.get('fighter', False)]))
        u.mem_write(LOCO + 0x48, struct.pack('<d', case.get('speed', 1.0)))
        u.mem_write(LOCO + 0x34, bytes([case.get('moving', True)]))
        u.mem_write(OWNER + 0x5A4, dwords(cell(*case['nav']) if case.get('nav') else 0))
        u.mem_write(TARGET + 0x74, bytes([case.get('target_marked', True)]))
        u.mem_write(TYPE + 0x720, dwords(case.get('turret_offset', 0)))
        u.mem_write(OWNER + 0x3B8, dwords(case.get('burst_index', 0)))
        for slot, flh in ((0x898, case.get('flh', [0, 0, 0])),
                          (0xA94, case.get('elite_flh', case.get('flh', [0, 0, 0])))):
            u.mem_write(TYPE + slot + 4, dwords(*flh))
        for offset, initial in ((0x388, case.get('primary', 0)),
                                (0x3A0, case.get('secondary', 0))):
            f.call(0x4C91C0, OWNER + offset, [])
            f.call(0x4C9680, OWNER + offset, [5])
            u.mem_write(ARG, dwords(initial))
            f.call(0x4C9300, OWNER + offset, [ARG])
        def observe(_u, pc, _size, _data):
            if pc == 0x41822F:
                retained['flh'] = list(struct.unpack('<iii', u.mem_read(u.reg_read(UC_X86_REG_EAX), 12)))
        u.hook_add(UC_HOOK_CODE, observe)

    result = dispatch(case, configure)
    f = retained['fixture']
    result['facings'] = []
    for offset in (0x388, 0x3A0):
        raw = struct.unpack('<6i', f.u.mem_read(OWNER + offset, 24))
        result['facings'].append(dict(destination=raw[0] & 65535, previous=raw[1] & 65535,
                                      start=raw[2], duration=raw[4], rate=raw[5] & 65535))
    result['flh'] = retained.get('flh')
    return result


def generate():
    cases = []
    for fighter in (False, True):
        for rot, inviso in ((0, False), (1, False), (1, True), (3, False)):
            for near in (False, True):
                cases.append(dict(name=f'class_{fighter}_{rot}_{inviso}_{near}', fighter=fighter,
                                  rot=rot, inviso=inviso,
                                  target=[11000 if near else 16512, 16512 if near else 18048, 0]))
    for speed, moving in ((0, True), (0, False), (1, False), (-0.25, True)):
        cases.append(dict(name=f'speed_{speed}_{moving}', speed=speed, moving=moving, nav=[45, 64]))
    for delta in (0, 15, 16, 17, 255, 511, 512, 513):
        cases.append(dict(name=f'nav_distance_{delta}', nav=[45, 64],
                          aircraft=[45 * 256 + 128 - delta, 16512, 500], target=[16512, 18048, 0]))
    for values in (dict(null_target=True), dict(ammo=0), dict(ammo=1, pending=True),
                   dict(ammo=2, pending=True), dict(ammo=-1), dict(rot=0, swap=True),
                   dict(rot=0, target=[16512, 16512, 208]),
                   dict(rot=0, target=[16512, 16512, 208], target_marked=False)):
        cases.append(dict(name=f'gate_{len(cases)}', nav=[45, 64], **values))
    for primary, secondary, burst in ((0, 0, 0), (0x4000, 0, 1), (0x1234, 0xCAFE, 2),
                                       (0x7FFF, 0x8000, -1)):
        cases.append(dict(name=f'flh_{primary}_{secondary}_{burst}', nav=[45, 61],
                          primary=primary, secondary=secondary, burst_index=burst,
                          flh=[150, 32, 20], turret_offset=25))
    cases.append(dict(name='elite_flh', nav=[45, 61], veterancy=2,
                      flh=[100, 20, 30], elite_flh=[200, 40, 60]))
    for null in (False, True):
        for ammo in (0, 2):
            for pending in (False, True):
                cases.append(dict(name=f'entry_{null}_{ammo}_{pending}', state=0,
                                  null_target=null, ammo=ammo, pending=pending))
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='49 full original Mission_Attack calls: 8 initial state0 transitions and 41 state3 calls covering classifier precedence, speed-vs-request distinction, strict NavCom thresholds, live target/weapon FLH facing, ammo prefix, destination/timers and unchanged Scenario RNG. Flat supplied Fly matrix; not complete flight/attack parity.',
        entry_points={'attack': 0x417FE0, 'strafe': 0x41B7F0, 'fighter': 0x41B840,
                      'moving_now': 0x4CCAC0, 'distance': 0x5F6440, 'get_flh': 0x6F3AD0,
                      'facing_set': 0x4C9220, 'aircraft_destination': 0x41AA80, 'foot_destination': 0x4D94B0},
        assumptions=['Inherited re-engagement map/COM/airborne fixture; no queued Enter, radio contacts, linked lift, fire particles or6AC latch.',
                     'Original Facing constructors, SetROT5 and Snap initialize both headings; frame100.',
                     'Supplied current Fly speed, Fighter, primary/elite FLH, turret offset and burst parity; no pitch/roll tilt.'],
        substitutions=['Inherited OS Interlocked imports only; no gameplay function replaced.']))
