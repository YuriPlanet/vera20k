"""Original HoverLocomotionClass SpeedUpdate 515ED0 and altitude controller 513D20.

A supplied Hover object (original ILocomotion vtable 0x7EACFC) is linked to a
supplied Foot whose virtual slots are small fixed-answer stubs: GetCoords
(+0x48, the supplied Location), GetHeight (+0x1C8), SetHeight (+0x1CC, recorded)
and the AbstractClass ID (Foot+4 vtable +0x10). Everything else, including
Is_Moving, Is_Powered, FacingClass, the distance and trig helpers, SinFromTable
and the map ground query 578080 over a supplied cell table, executes unmodified.
SpeedUpdate cases always carry a head, so its null-head arm (Set_Speed,
Path_And_Arrival, ProcessMovement) is out of scope.

``python -m tools.spatial_oracle.hover_speed_altitude --drop-as-bomb --check``
replays a separate small corpus in drop_as_bomb.json. It runs ObjectClass's
DropAsBomb and falling AI, the Foot missing-deck gate and Hover water tail,
recording substituted receiver calls. Original Hover startup helpers establish
the 104-lepton water threshold; the native x87 fall-rate conversion runs.
"""
from pathlib import Path
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, NATIVE_FPCW
from tools.native_slope import slope_matrices

MEM = 0x21000000
OBJ, FOOT = MEM + 0x1000, MEM + 0x4000
FOOT_VT, ABSTRACT_VT = MEM + 0x8000, MEM + 0x9000
STUBS, RECORD = MEM + 0xA000, MEM + 0xB000
RULES = MEM + 0x10000
TABLE, CELLS = MEM + 0x40000, MEM + 0x140000
TRAP, STOP = 0x32000000, 0x30000000
DUMMY = 0xABDC50
HOVER_VTABLE = 0x7EACFC
SPEED_UPDATE, ALTITUDE = 0x515ED0, 0x513D20
# Cell deltas x256 in compass order (util::direction_tables::lepton).
LEPTON_DELTAS = [(0, -256), (256, -256), (256, 0), (256, 256),
                 (0, 256), (-256, 256), (-256, 0), (-256, -256)]

# Rules offsets the two functions read.
HOVER_HEIGHT, HOVER_BOB, HOVER_BOOST = 0x5CC, 0x5D0, 0x5D8
HOVER_ACCEL, HOVER_BRAKE, HOVER_DAMPEN, GRAVITY = 0x5E0, 0x5E8, 0x5F0, 0x16B8
# Doubles as bits: the constructor defaults (0x00665E2B..) and the values the
# native INI readers produce from retail rulesmd.ini (.04, 150%, .02, .03, 40%).
RULE_SETS = {
    'constructor': dict(height=120, bob=0x403E000000000000, boost=0x3FF4CCCCCCCCCCCD,
                        accel=0x3F9EB851EB851EB8, brake=0x3F9EB851EB851EB8,
                        dampen=0x3FE999999999999A, gravity=6),
    'retail': dict(height=120, bob=0x3FA47AE140000000, boost=0x3FF8000000000000,
                   accel=0x3F947AE140000000, brake=0x3F9EB851E0000000,
                   dampen=0x3FD9999999999999, gravity=6),
}


def words(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def f64_bits(value):
    return struct.unpack('<Q', struct.pack('<d', value))[0]


def machine(case):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(MEM, 0x200000)
    uc.mem_map(STOP, 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.mem_write(0x822D80, struct.pack('<H', NATIVE_FPCW))
    # Map: 512-wide cell pointer table, level step 104, slope matrices.
    uc.mem_write(0x87F924, words(TABLE, 0x40000))
    uc.mem_write(0x89E7C0, words(104))
    uc.mem_write(0x89C778, words(104))
    for slope, matrix in enumerate(slope_matrices()):
        uc.mem_write(0xB45188 + 48 * slope, struct.pack('<12I', *matrix))
    # g_DirectionDeltaX/Y_Table 0x0089F6D8 is filled at startup; supplied here.
    uc.mem_write(0x89F6D8, struct.pack('<16i', *[v for pair in LEPTON_DELTAS for v in pair]))
    uc.mem_write(DUMMY, bytes(0x200))
    uc.mem_write(DUMMY + 0x44, words(-1))
    for i, (x, y, level) in enumerate(case.get('cells', [])):
        cell = CELLS + i * 0x200
        uc.mem_write(cell + 0x24, struct.pack('<hh', x, y))
        uc.mem_write(cell + 0x44, words(-1))
        uc.mem_write(cell + 0x11B, bytes([level & 255, 0]))
        uc.mem_write(TABLE + (y * 512 + x) * 4, words(cell))
    rules = RULE_SETS[case['rules']]
    uc.mem_write(0x8871E0, words(RULES))
    uc.mem_write(RULES + HOVER_HEIGHT, words(rules['height']))
    for offset, key in ((HOVER_BOB, 'bob'), (HOVER_BOOST, 'boost'), (HOVER_ACCEL, 'accel'),
                        (HOVER_BRAKE, 'brake'), (HOVER_DAMPEN, 'dampen')):
        uc.mem_write(RULES + offset, struct.pack('<Q', rules[key]))
    uc.mem_write(RULES + GRAVITY, words(rules['gravity']))
    uc.mem_write(0xA8ED84, words(case['frame']))
    # Foot stubs: GetCoords copies Location, GetHeight/ID answer, SetHeight records.
    get_coords = (b'\x8b\x44\x24\x04'
                  + b'\x8b\x15' + words(FOOT + 0x9C) + b'\x89\x10'
                  + b'\x8b\x15' + words(FOOT + 0xA0) + b'\x89\x50\x04'
                  + b'\x8b\x15' + words(FOOT + 0xA4) + b'\x89\x50\x08'
                  + b'\xc2\x04\x00')
    get_height = b'\xb8' + words(case['height']) + b'\xc3'
    set_height = b'\x8b\x44\x24\x04\xa3' + words(RECORD) + b'\xc2\x04\x00'
    fetch_id = b'\xb8' + words(case['id']) + b'\xc2\x04\x00'
    stubs = {}
    cursor = STUBS
    for name, code in (('coords', get_coords), ('height', get_height),
                       ('set_height', set_height), ('id', fetch_id)):
        uc.mem_write(cursor, code)
        stubs[name] = cursor
        cursor += 0x40
    uc.mem_write(FOOT_VT, words(*[TRAP] * 0x200))
    uc.mem_write(FOOT_VT + 0x48, words(stubs['coords']))
    uc.mem_write(FOOT_VT + 0x1C8, words(stubs['height']))
    uc.mem_write(FOOT_VT + 0x1CC, words(stubs['set_height']))
    uc.mem_write(ABSTRACT_VT, words(*[TRAP] * 0x20))
    uc.mem_write(ABSTRACT_VT + 0x10, words(stubs['id']))
    uc.mem_write(FOOT, words(FOOT_VT, ABSTRACT_VT))
    uc.mem_write(FOOT + 0x9C, words(*case['location']))
    uc.mem_write(FOOT + 0x5E0, words(*case['path']))
    uc.mem_write(RECORD, words(0x7FFFFFFF))
    # Hover object: interface vtable, Foot, powered byte, coordinates, doubles.
    uc.mem_write(OBJ + 4, words(HOVER_VTABLE))
    uc.mem_write(OBJ + 0xC, words(FOOT))
    uc.mem_write(OBJ + 0x10, bytes([case['powered']]))
    uc.mem_write(OBJ + 0x18, words(*case.get('destination', (0, 0, 0))))
    uc.mem_write(OBJ + 0x24, words(*case.get('head', (0, 0, 0))))
    uc.mem_write(OBJ + 0x48, struct.pack('<Q', case.get('request', 0)))
    uc.mem_write(OBJ + 0x50, struct.pack('<Q', case.get('current', 0)))
    uc.mem_write(OBJ + 0x58, struct.pack('<Q', case.get('mult', f64_bits(1.0))))
    uc.mem_write(OBJ + 0x60, struct.pack('<Q', case.get('bob', 0)))
    uc.mem_write(OBJ + 0x70, bytes([case.get('pushed', 0)]))
    return uc


def read_u64(uc, address):
    return struct.unpack('<Q', uc.mem_read(address, 8))[0]


def run(uc, entry, required):
    uc.mem_write(MEM + 0x1FF000, words(STOP))
    uc.reg_write(UC_X86_REG_ESP, MEM + 0x1FF000)
    uc.reg_write(UC_X86_REG_ECX, OBJ)
    run_checked(uc, entry, STOP, count=200000, required_addresses=required)


def speed_cases():
    one, zero = f64_bits(1.0), 0
    location = (2688, 2688, 0)
    for rules in RULE_SETS:
        for current in (0.0, 0.01, 0.3, 0.5, 0.97, 1.0, 1.3, 1.5):
            for name, head, destination, powered, pushed in (
                    ('far_destination', (2944, 2688, 0), (4224, 2688, 0), 1, 0),
                    ('near_destination', (2944, 2688, 0), (2800, 2688, 0), 1, 0),
                    ('null_destination_far_head', (2944, 2688, 0), None, 1, 0),
                    ('null_destination_near_head', (2780, 2688, 0), None, 1, 0),
                    ('unpowered', (2944, 2688, 0), (4224, 2688, 0), 0, 0),
                    ('pushed_unpowered_far', (2944, 2688, 0), (4224, 2688, 0), 0, 1),
                    ('pushed_near', (2944, 2688, 0), (2800, 2688, 0), 1, 1)):
                for path in ((-1, -1), (2, 2), (2, 3)):
                    yield dict(kind='speed', name=name, rules=rules, frame=100, height=0, id=1,
                               location=location, head=head,
                               destination=destination or (0, 0, 0), powered=powered,
                               pushed=pushed, path=path, request=one if current else zero,
                               current=f64_bits(current), mult=f64_bits(1.3))


def altitude_cases():
    for rules in RULE_SETS:
        for height in (-50, 0, 29, 30, 31, 60, 119, 120, 121, 200, 520):
            for bob in (0.0, -3.7, 5.25, 40.0):
                for ident, frame in ((1, 0), (2, 13), (7, 999), (100000, 123457)):
                    for powered in (0, 1):
                        yield dict(kind='altitude', rules=rules, frame=frame, height=height,
                                   id=ident, location=(2688, 2688, 0), path=(-1, -1),
                                   powered=powered, bob=f64_bits(bob))
        # A path word: the ground one direction step ahead against the ground here.
        for centre, ring in ((0, 1), (1, 0), (2, 2)):
            cells = [(10 + dx, 10 + dy, ring if (dx, dy) != (0, 0) else centre)
                     for dx in (-1, 0, 1) for dy in (-1, 0, 1)]
            for direction in range(8):
                for height in (60, 150):
                    yield dict(kind='altitude', rules=rules, frame=77, height=height, id=3,
                               location=(2688, 2688, centre * 104), path=(direction, direction),
                               powered=1, bob=f64_bits(2.5), cells=cells)


def execute(case):
    uc = machine(case)
    if case['kind'] == 'speed':
        run(uc, SPEED_UPDATE, (SPEED_UPDATE,))
        return dict(input=case, request=read_u64(uc, OBJ + 0x48),
                    current=read_u64(uc, OBJ + 0x50), mult=read_u64(uc, OBJ + 0x58))
    grounds = []

    def observe(u, address, _size, _data):
        if address == 0x513DC8:
            grounds.append([struct.unpack('<i', words(u.reg_read(UC_X86_REG_EDI)))[0],
                            struct.unpack('<i', words(u.reg_read(UC_X86_REG_EAX)))[0]])
    uc.hook_add(UC_HOOK_CODE, observe)
    run(uc, ALTITUDE, (ALTITUDE,))
    visible = struct.unpack('<i', uc.mem_read(RECORD, 4))[0]
    assert visible != 0x7FFFFFFF
    climbing = bool(grounds) and grounds[0][1] > grounds[0][0]
    return dict(input=case, climbing=climbing, visible=visible, bob=read_u64(uc, OBJ + 0x60))


def generate():
    return [execute(case) for case in [*speed_cases(), *altitude_cases()]]


def execute_bomb(case):
    """Original Object bodies and bounded Hover water tail; receiver substitutions
    are explicit controls, not evidence of receiver implementations or play parity.
    The x87 fall conversion and startup threshold execute without substitution.
    """
    uc = machine(dict(rules='retail', frame=100, height=0, id=1,
                      location=(2688, 2688, case.get('z', 416)), path=(-1, -1), powered=0))
    read32 = lambda p: struct.unpack('<I', uc.mem_read(p, 4))[0]
    signed = lambda p: struct.unpack('<i', uc.mem_read(p, 4))[0]
    for entry in (0x513A10, 0x513A40, 0x513A60, 0x513A80, 0x513AA0, 0x513AC0, 0x513AE0):
        run(uc, entry, [entry])
    threshold = signed(0xA8F1C0)
    uc.mem_write(RULES + 0x7B8, words(case.get('parachute_cap', -3), case.get('free_cap', -100)))
    uc.mem_write(RULES + 0xFA8, words(MEM + 0xD000))
    uc.mem_write(FOOT + 0x14, words(4 if case.get('foot', True) else 0))
    uc.mem_write(FOOT + 0x2C, words(case.get('rate', 0)))
    uc.mem_write(FOOT + 0x64, words(-1))
    uc.mem_write(FOOT + 0x6C, words(case.get('health', 100)))
    for offset, key, default in ((0x74, 'marked', 1), (0x81, 'limbo', 0), (0x84, 'parachuted', 0),
                                 (0x8C, 'bridge', 1), (0x8D, 'falling', int(case['kind'] == 'fall')),
                                 (0x8F, 'bomb', int(case['kind'] == 'fall'))):
        uc.mem_write(FOOT + offset, bytes([case.get(key, default)]))
    canopy = MEM + 0xE000
    uc.mem_write(FOOT + 0x88, words(canopy))
    uc.mem_write(canopy + 0x195, b'\x01')
    loco_vt = MEM + 0xC000
    uc.mem_write(OBJ + 4, words(loco_vt))
    uc.mem_write(FOOT + 0x674, words(OBJ + 4))
    callbacks = {}
    for index, (slot, name, cleanup) in enumerate(((0x124, 'mark', 4), (0xF4, 'scatter', 4),
            (0x78, 'layer', 0), (0x1D0, 'z', 0), (0x1C8, 'height', 0),
            (0x1CC, 'set_height', 4), (0x18C, 'mission', 4), (0x16C, 'damage', 28),
            (0x2C, 'class', 0), (0x88, 'type', 0))):
        address = STUBS + 0x200 + index * 0x20
        uc.mem_write(address, b'\xc3')
        uc.mem_write(FOOT_VT + slot, words(address))
        callbacks[address] = (name, cleanup)
    for index, (slot, name, cleanup) in enumerate(((0x10, 'moving', 4), (0x9C, 'move_to', 8),
                                                (0x60, 'powered', 4))):
        address = STUBS + 0x500 + index * 0x20
        uc.mem_write(address, b'\xc3')
        uc.mem_write(loco_vt + slot, words(address))
        callbacks[address] = (name, cleanup)
    events = []
    def ret(cleanup, value=0):
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, read32(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
    def observe(u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address in (0x4A9770, 0x4A9720):
            events.append(dict(kind='display_remove' if address == 0x4A9770 else 'display_submit',
                               bridge=read32(FOOT + 0x8C) & 255))
            ret(4)
        elif address in callbacks:
            name, cleanup = callbacks[address]
            args = [signed(sp + 4 + n * 4) for n in range(cleanup // 4)]
            value = 0
            if name == 'z': value = signed(FOOT + 0xA4)
            elif name == 'height': value = signed(FOOT + 0xA4) - case.get('floor', 0)
            elif name == 'set_height': uc.mem_write(FOOT + 0xA4, words(case.get('floor', 0) + args[0]))
            elif name == 'moving': value = int(case.get('moving', False))
            elif name == 'powered':
                value = int(case.get('powered', False) or signed(FOOT + 0xA4) > case.get('floor', 0))
            elif name == 'damage':
                args[0] = signed(args[0])
                args[2] = 'C4Warhead' if args[2] == MEM + 0xD000 else args[2]
            if name not in ('z', 'height', 'class', 'type', 'layer'):
                # Pointer arguments are receiver identity; record only semantically meaningful values.
                if name in ('moving', 'powered'): args = []
                elif name == 'move_to': args = [args[1]]
                elif name == 'scatter': args = list(struct.unpack('<3i', u.mem_read(args[0], 12)))
                events.append(dict(kind=name, args=args))
            ret(cleanup, value)
        elif address == 0x565730 and case['kind'] in ('hover_water', 'bridge_gate'):
            ret(4, CELLS)
        elif address == 0x578080 and case['kind'] == 'hover_water':
            ret(4, case.get('floor', 0))
        elif address == 0x4C93D0 and case['kind'] == 'hover_water':
            uc.mem_write(read32(sp + 4), words(0))
            ret(4, read32(sp + 4))
        elif address == 0x4C9300 and case['kind'] == 'hover_water':
            ret(4)
    uc.hook_add(UC_HOOK_CODE, observe)
    if case['kind'] == 'bridge_gate':
        uc.mem_write(CELLS + 0x140, words(0x100 if case.get('deck', False) else 0))
        uc.mem_write(FOOT_VT + 0xEC, words(0x5F4160))
        uc.reg_write(UC_X86_REG_ESI, FOOT)
        uc.reg_write(UC_X86_REG_ESP, MEM + 0x1FF000)
        run_checked(uc, 0x4D8D0B, 0x4D8D59, count=1000, required_addresses=[0x4D8D0B])
    elif case['kind'] == 'hover_water':
        uc.mem_write(OBJ + 8, words(FOOT))
        uc.mem_write(OBJ, words(loco_vt))
        uc.mem_write(OBJ + 0x64, bytes([case.get('spinning', 0)]))
        uc.mem_write(OBJ + 0x68, words(case.get('steps', 2)))
        uc.mem_write(CELLS + 0xEC, words(case.get('land', 2)))
        uc.mem_write(FOOT_VT + 0xEC, words(0x5F4160))
        uc.reg_write(UC_X86_REG_ESI, OBJ)
        uc.reg_write(UC_X86_REG_ESP, MEM + 0x1FF000)
        run_checked(uc, 0x514AD0, 0x514C12, count=1000, required_addresses=[0x514AD0])
    else:
        entry = 0x5F4160 if case['kind'] == 'drop' else 0x5F3E70
        uc.mem_write(MEM + 0x1FF000, words(STOP))
        uc.reg_write(UC_X86_REG_ESP, MEM + 0x1FF000)
        uc.reg_write(UC_X86_REG_ECX, FOOT)
        run_checked(uc, entry, STOP, count=2000, required_addresses=[entry])
    return dict(input=case, threshold=threshold, events=events, z=signed(FOOT + 0xA4),
                rate=signed(FOOT + 0x2C), falling=bool(uc.mem_read(FOOT + 0x8D, 1)[0]),
                bomb=bool(uc.mem_read(FOOT + 0x8F, 1)[0]),
                bridge=bool(uc.mem_read(FOOT + 0x8C, 1)[0]),
                canopy_active=bool(uc.mem_read(canopy + 0x195, 1)[0]))


def generate_bomb():
    cases = [dict(kind='drop', name='standing_rate_kept', rate=-17),
             dict(kind='drop', name='moving', moving=True), dict(kind='drop', name='nonfoot', foot=False)]
    cases += [dict(kind='fall', name=f'free_rate_{rate}', rate=rate, z=1000)
              for rate in (0, -1, -2, -98, -99, -100, 2)]
    cases += [dict(kind='fall', name=name, **values) for name, values in (
        ('grounding', dict(z=1, rate=-2)), ('already_grounded', dict(z=0, rate=0)),
        ('nonbomb_grounding', dict(z=0, bomb=0)), ('dead_grounding', dict(z=0, health=0)),
        ('not_falling_bomb', dict(z=0, falling=0)), ('negative_health', dict(z=0, health=-1)),
        ('custom_free_cap', dict(z=1000, rate=-5, free_cap=-6)),
        ('unmarked', dict(z=1000, rate=-2, marked=0)),
        ('limbo_keeps_rate', dict(z=1000, limbo=1, rate=-4)),
        ('parachute_landing', dict(z=0, rate=-2, parachuted=1, bomb=0)))]
    cases += [dict(kind='hover_water', name=f'water_z_{z}', z=z, spinning=1, steps=1)
              for z in (0, 103, 104, 105)]
    cases += [dict(kind='hover_water', name='dry', z=0, land=0, spinning=1),
              dict(kind='hover_water', name='still_spinning', z=0, spinning=1, powered=1),
              dict(kind='hover_water', name='not_spinning', z=0),
              dict(kind='hover_water', name='last_spin_step', z=0, spinning=1, powered=1, steps=1)]
    cases += [dict(kind='bridge_gate', name='missing_deck', bridge=1),
              dict(kind='bridge_gate', name='present_deck', bridge=1, deck=True),
              dict(kind='bridge_gate', name='not_on_bridge', bridge=0)]
    return [execute_bomb(case) for case in cases]


if __name__ == '__main__':
    if '--drop-as-bomb' in sys.argv:
        finish_vectors(generate_bomb, Path(__file__).with_name('drop_as_bomb.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--drop-as-bomb'],
            provenance=lambda: provenance(
                scope='Bounded original DropAsBomb, ObjectAI falling block, Foot missing-deck and Hover Process water gate controls',
                entry_points=dict(drop_as_bomb=0x5F4160, object_ai=0x5F3E70, hover_water=0x514AD0,
                                  foot_missing_deck=0x4D8D0B, hover_threshold_startup=0x513AE0),
                substitutions=['Foot virtual receiver callbacks and display membership record arguments and return supplied values; no receiver parity claim',
                               'Foot/Hover gate Map lookup and Hover floor return supplied cell/floor; DropAsBomb dispatch executes original body',
                               'Hover Is_Powered returns the supplied powered byte OR supplied Location Z above floor (516C70); facing read/update callbacks are substituted while spin-step arithmetic and entry/exit gates execute original bytes'],
                assumptions=['Foot/nonfoot bit, health, Location, canopy, limbo, bridge, parachute, falling and bomb flags are supplied controls',
                             'Rules fall caps -3/-100 and C4Warhead pointer supplied; freefall literal1.4 and x87 conversion execute original bytes',
                             'Original Hover startup helpers513A10..513AE0 execute in order to initialize threshold104; no startup constants supplied']))
        sys.exit(0)
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Hover SpeedUpdate 515ED0 (request, ramp, SpeedMult) and altitude controller '
              '513D20 (visible height, spring offset) over supplied object, Foot and map state',
        entry_points={'speed_update': SPEED_UPDATE, 'altitude': ALTITUDE},
        substitutions=[
            'Foot vt+0x48 GetCoords returns the supplied Location; vt+0x1C8 GetHeight a supplied value; '
            'vt+0x1CC SetHeight records its argument; Foot+4 vt+0x10 returns a supplied ID',
        ],
        assumptions=[
            'Hover object and FacingClass memory start zeroed apart from the supplied fields, so the '
            'steering facing turns instantly; the turn gate is not exercised',
            'SpeedUpdate cases always have a head; the null-head arm is excluded',
            'Rules doubles/ints written at their Rules offsets; map cells flat with supplied levels',
            'The startup-filled direction delta table 0x0089F6D8 is supplied as cell deltas x256 '
            '(the dumped values util::direction_tables::lepton pins)',
        ]))
