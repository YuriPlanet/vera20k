"""Shared native collision fixture state and bounded execution.

Generators own case enumeration and reference publication. This module owns
synthetic ordinary/shared/homing layouts, supplied receivers and observed results.
Those layouts intentionally have different source addresses and FPCW cache state.
Importing this fixture does not locate retail bytes or execute native code.
"""
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import NATIVE_FPCW, load_image, run_checked

BASE, SP = 0x20000000, 0x2000E000
BULLET, TYPE, SOURCE, OBJECT = BASE, BASE + 0x1000, BASE + 0x2000, BASE + 0x3000
CELL, TARGET_CELL, BUILDING = BASE + 0x4000, BASE + 0x5000, BASE + 0x6000
RAW_COORD = BASE + 0x7800
MEM = 0x21000000
RULES, HOUSE, WALL_HOUSE, TARGET_TYPE, LOCO = MEM + 0x20000, MEM + 0x30000, MEM + 0x38000, MEM + 0x50000, MEM + 0x52000
DUMMY, STOP = 0xABDC50, 0x30000000
HOMING_TARGET, HOMING_TARGET_VTABLE = BASE + 0x2000, BASE + 0x3000
HOMING_TARGET_AIM, HOMING_TARGET_LOCATION = BASE + 0x4000, BASE + 0x4010
HOMING_SOURCE, HOMING_SOURCE_VTABLE, HOMING_SOURCE_TYPE, HOMING_SOURCE_GET_TYPE = BASE + 0x5000, BASE + 0x6000, BASE + 0x7000, BASE + 0x4020


def _machine(*, cached_control=False, return_boundary=False):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(BASE, 0x10000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    if cached_control:
        uc.mem_write(0x822D80, struct.pack('<H', NATIVE_FPCW))
    if return_boundary:
        uc.mem_map(STOP, 0x1000)
    return uc


def i32(uc, addr, value):
    uc.mem_write(addr, struct.pack('<I', value & 0xFFFFFFFF))


def read_i32(uc, addr):
    return struct.unpack('<i', uc.mem_read(addr, 4))[0]


def coords(uc, addr):
    return list(struct.unpack('<iii', uc.mem_read(addr, 12)))


def return_value(uc, value, argument_bytes):
    sp = uc.reg_read(UC_X86_REG_ESP)
    uc.reg_write(UC_X86_REG_EAX, value)
    uc.reg_write(UC_X86_REG_EIP, read_i32(uc, sp))
    uc.reg_write(UC_X86_REG_ESP, sp + 4 + argument_bytes)


def prepare_ordinary(case):
    uc = _machine()
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_EBP, BULLET)
    i32(uc, BULLET, 0x007E46E4)
    i32(uc, BULLET + 0xAC, TYPE)
    i32(uc, BULLET + 0xB0, SOURCE if case.get('source', False) else 0)
    uc.mem_write(BULLET + 0x9C, struct.pack('<iii', *case.get('old', (128, 128, case.get('old_height', 500)))))
    uc.mem_write(BULLET + 0x140, struct.pack('<iii', *case.get('target', (640, 128, 0))))
    velocity = case.get('velocity', (20, 0, -6))
    uc.mem_write(BULLET + 0xE8, struct.pack('<ddd', *velocity))
    uc.mem_write(SP + 0x90, struct.pack('<ddd', *velocity))
    uc.mem_write(TYPE + 0x2C0, bytes([case.get('vertical', False)]))
    uc.mem_write(TYPE + 0x2A2, bytes([case.get('inaccurate', False)]))
    uc.mem_write(SP + 0x24, struct.pack('<iii', *case.get('candidate', (384, 128, 0))))
    uc.mem_write(OBJECT + 0x9C, struct.pack('<iii', *case.get('object_coord', (384, 128, 0))))
    i32(uc, 0x0089DE70, 104)
    i32(uc, 0x0089DE64, 416)
    i32(uc, 0x0087F7E8 + 0xF4, case.get('map_width', 2))
    i32(uc, 0x0087F7E8 + 0xF8, case.get('map_height', 3))
    calls = []

    def hook(uc, address, size, user):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == 0x005F5F40:
            calls.append('height')
            return_value(uc, case.get('old_height', 500), 0)
        elif address in (0x005657A0, 0x00565730):
            arg = read_i32(uc, sp + 4)
            if address == 0x005657A0:
                cell = list(struct.unpack('<hh', uc.mem_read(arg, 4)))
            else:
                cell = [int(value / 256) for value in coords(uc, arg)[:2]]
            target_cell = [int(value / 256) for value in case.get('target', (640, 128, 0))[:2]]
            calls.append(['cell', *cell])
            return_value(uc, TARGET_CELL if cell == target_cell else CELL, 4)
        elif address == 0x0047C520:
            calls.append('building')
            return_value(uc, BUILDING if case.get('same_building', False) else 0, 0)
        elif address == 0x0047C3D0:
            calls.append('nearest')
            selected = case.get('selected', 'none')
            return_value(uc, SOURCE if selected == 'source' else OBJECT if selected == 'object' else 0, 12)
        elif address == 0x004F9A90:
            calls.append('alliance')
            return_value(uc, int(case.get('allied', False)), 4)

    uc.hook_add(UC_HOOK_CODE, hook)
    return uc, calls


def prepare_homing(old_height, candidate_z, airburst, inaccurate, target_present=True):
    uc = _machine()
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_EBP, BULLET)
    uc.reg_write(UC_X86_REG_EBX, BULLET + 0xE8)
    i32(uc, BULLET, 0x007E46E4)
    i32(uc, BULLET + 0xAC, TYPE)
    i32(uc, BULLET + 0x10C, HOMING_TARGET if target_present else 0)
    uc.mem_write(BULLET + 0x9C, struct.pack("<iii", 500, 128, 208 + old_height))
    uc.mem_write(BULLET + 0xE8, struct.pack("<ddd", 4.0, 0.0, 0.0))
    uc.mem_write(TYPE + 0x294, bytes([airburst]))
    uc.mem_write(TYPE + 0x2A2, bytes([inaccurate]))
    uc.mem_write(SP + 0x24, struct.pack("<iii", 504, 128, candidate_z))
    uc.mem_write(SP + 0x30, struct.pack("<iii", 640, 128, 624))
    i32(uc, HOMING_TARGET, HOMING_TARGET_VTABLE)
    i32(uc, HOMING_TARGET_VTABLE + 0x58, HOMING_TARGET_AIM)
    i32(uc, HOMING_TARGET_VTABLE + 0x48, HOMING_TARGET_LOCATION)
    floor_queries = []

    def hook(uc, address, size, user):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == 0x00578080:
            argument = read_i32(uc, sp + 4)
            floor_queries.append(coords(uc, argument))
            return_value(uc, 208, 4)
        elif address in (HOMING_TARGET_AIM, HOMING_TARGET_LOCATION):
            argument = read_i32(uc, sp + 4)
            result = (640, 128, 624 if address == HOMING_TARGET_AIM else 208)
            uc.mem_write(argument, struct.pack("<iii", *result))
            return_value(uc, argument, 4)
        elif address == HOMING_SOURCE_GET_TYPE:
            return_value(uc, HOMING_SOURCE_TYPE, 0)

    uc.hook_add(UC_HOOK_CODE, hook)
    return uc, floor_queries


def prepare_shared(case):
    uc = _machine(cached_control=True, return_boundary=True)
    uc.mem_map(MEM, 0x100000)
    for address in (0x89DE70, 0x89E7C0, 0xAC13C8):
        i32(uc, address, 104)
    i32(uc, 0xAC13BC, 416)
    i32(uc, 0x89DE64, 416)
    i32(uc, 0x87F924, MEM)
    i32(uc, 0x87F928, 512 * 8)

    def cell(address, x, y, row):
        i32(uc, address, 0x7E4EEC)
        uc.mem_write(address + 0x24, struct.pack('<hh', x, y))
        i32(uc, address + 0x38, row.get('tile', 0xFFFF))
        i32(uc, address + 0x44, row.get('overlay', 0 if row.get('wall', False) else -1))
        i32(uc, address + 0x50, 0)
        uc.mem_write(address + 0x11B, bytes([row.get('level', 0) & 255, row.get('slope', 0)]))
        i32(uc, address + 0x140, row.get('flags', 0))

    cell(DUMMY, 0, 0, case.get('dummy', {}))
    for y in range(8):
        for x in range(8):
            if [x, y] in case.get('missing', []):
                continue
            address = MEM + 0x10000 + (y * 8 + x) * 0x148
            i32(uc, MEM + (y * 512 + x) * 4, address)
            cell(address, x, y, case.get('cells', {}).get(f'{x},{y}', {}))
    i32(uc, 0x8871E0, RULES)
    uc.mem_write(RULES + 0x1850, bytes([case.get('transparency', False)]))
    i32(uc, 0xA83D84, MEM + 0x58000)
    i32(uc, MEM + 0x58000, MEM + 0x59000)
    uc.mem_write(MEM + 0x592A8, b'\x01')
    i32(uc, 0xA8022C, MEM + 0x5A000)
    i32(uc, MEM + 0x5A000, WALL_HOUSE)
    i32(uc, HOUSE + 0x30, 1)
    i32(uc, WALL_HOUSE + 0x30, 2)
    i32(uc, HOUSE + 0x5788, 4 if case.get('source_allied', False) else 0)
    i32(uc, WALL_HOUSE + 0x5788, 2 if case.get('wall_allied', False) else 0)
    i32(uc, SOURCE + 0x21C, HOUSE)
    i32(uc, BULLET, 0x7E46E4)
    i32(uc, BULLET + 0xAC, TYPE)
    uc.mem_write(BULLET + 0x8C, bytes([case.get('on_bridge', False)]))
    i32(uc, BULLET + 0xB0, SOURCE if case.get('source', True) else 0)
    candidate = case.get('candidate', [640, 640, 500])
    uc.mem_write(BULLET + 0x9C, struct.pack('<iii', *candidate))
    uc.mem_write(BULLET + 0x134, struct.pack('<iii', *case.get('origin', [128, 128, 0])))
    uc.mem_write(BULLET + 0x140, struct.pack('<iii', *case.get('launch_target', [1408, 640, 0])))
    uc.mem_write(BULLET + 0x14C, struct.pack('<hh', *case.get('previous', [1, 2])))
    for key, offset in [('cliffs', 0x296), ('walls', 0x298), ('level', 0x29D), ('flak', 0x2A3), ('aa', 0x2A4)]:
        uc.mem_write(TYPE + offset, bytes([case.get(key, False)]))
    i32(uc, 0xAA0738, case.get('water_base', 100))
    target = case.get('target')
    foundation = None
    if target:
        category = target.get('category', 'unit')
        i32(uc, BULLET + 0x10C, OBJECT)
        i32(uc, OBJECT, {'unit': 0x7F5C70, 'building': 0x7E3EBC, 'aircraft': 0x7E22A4}[category])
        uc.mem_write(OBJECT + 0x9C, struct.pack('<iii', *target.get('coord', [640, 640, 500])))
        uc.mem_write(OBJECT + 0x74, bytes([target.get('marked', True)]))
        uc.mem_write(OBJECT + 0x8C, bytes([target.get('on_bridge', False)]))
        i32(uc, OBJECT + 0x520, TARGET_TYPE)
        i32(uc, OBJECT + 0x6C4, TARGET_TYPE)
        i32(uc, TARGET_TYPE + 0xEF0, target.get('foundation', 0))
        if category == 'building':
            index = target.get('foundation', 0)
            foundation = [read_i32(uc, 0x8192B8 + index * 4), read_i32(uc, 0x819310 + index * 4)]
        if target.get('rocket', False):
            i32(uc, RULES + (0x514 if target.get('dmisl', False) else 0x4E0), TARGET_TYPE)
            i32(uc, OBJECT + 0x674, LOCO)
            i32(uc, LOCO, 0x7F0B1C)
            i32(uc, LOCO + 0x3C, target.get('phase', 0))
    trace = []
    queries = []
    receivers = {0x4CC360, 0x486840, 0x4867E0, 0x5F6B90, 0x41B920, 0x661F90, 0x410540, 0x447AC0, 0x5F6360, 0x4F9A50}

    def observe(uc, address, size, user):
        if address in receivers:
            trace.append(f'{address:08X}')
        if address in (0x565730, 0x5657A0, 0x578080):
            arg = read_i32(uc, uc.reg_read(UC_X86_REG_ESP) + 4)
            xy = list(struct.unpack('<hh' if address == 0x5657A0 else '<ii', uc.mem_read(arg, 4 if address == 0x5657A0 else 8)))
            queries.append([f'{address:08X}', *xy])

    uc.hook_add(UC_HOOK_CODE, observe)
    return uc, queries, trace, foundation


def shared_probe(case):
    uc, queries, trace, foundation = prepare_shared(case)
    uc.mem_write(SP, struct.pack('<II', STOP, RAW_COORD))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, BULLET)
    run_checked(uc, 0x468BB0, STOP, count=100000)
    return dict(**case, admitted=bool(uc.reg_read(UC_X86_REG_EAX) & 255), result=coords(uc, RAW_COORD),
        dummy_coord=list(struct.unpack('<hh', uc.mem_read(DUMMY + 0x24, 4))),
        foundation_dimensions=foundation, queries=queries, receivers=trace)


def line_walk(case):
    """Original 4CC100 and reached 4CC360 bodies; hooks only observe samples."""
    uc, queries, trace, _ = prepare_shared(case)
    source, target = RAW_COORD, RAW_COORD + 0x10
    uc.mem_write(source, struct.pack('<iii', *case['source_coord']))
    uc.mem_write(target, struct.pack('<iii', *case['target_coord']))
    uc.mem_write(SP, struct.pack('<III', STOP, TYPE, HOUSE))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, source)
    uc.reg_write(UC_X86_REG_EDX, target)
    samples = []

    def observe_sample(uc, address, size, user):
        if address == 0x4CC360:
            samples.append(coords(uc, uc.reg_read(UC_X86_REG_ESP) + 8))

    uc.hook_add(UC_HOOK_CODE, observe_sample)
    run_checked(uc, 0x4CC100, STOP, count=100000)
    result = uc.reg_read(UC_X86_REG_EAX)
    blocking_cell = list(struct.unpack('<hh', uc.mem_read(result + 0x24, 4))) if result else None
    return dict(**case, samples=samples, blocking_cell=blocking_cell,
        dummy_coord=list(struct.unpack('<hh', uc.mem_read(DUMMY + 0x24, 4))),
        queries=queries, receivers=trace)


def admission(case):
    uc, calls = prepare_ordinary(case)
    run_checked(uc, 0x004677D3, 0x00467B7A, count=100000)
    return dict(**case, impact=bool(uc.mem_read(SP + 0x18, 1)[0]),
        reason=read_i32(uc, SP + 0x20), near_target=bool(uc.mem_read(SP + 0x1F, 1)[0]),
        result=coords(uc, SP + 0x24), calls=calls)


def reflection(matrix, slope, velocity, elasticity):
    uc, _ = prepare_ordinary(dict(velocity=velocity))
    uc.mem_write(0x00B45188 + slope * 48, struct.pack('<12I', *matrix))
    uc.mem_write(CELL + 0x11C, bytes([slope]))
    uc.mem_write(SP + 0x44, struct.pack('<iii', 384, 128, -1))
    uc.mem_write(SP + 0x50, struct.pack('<d', elasticity))
    run_checked(uc, 0x00467666, 0x00467778, count=100000)
    result = list(struct.unpack('<ddd', uc.mem_read(SP + 0x90, 24)))
    return dict(slope=slope, velocity=velocity, elasticity=elasticity,
        result_f32_bits=[struct.unpack('<I', struct.pack('<f', value))[0] for value in result],
        quantized_result=[int(value) for value in result])


def nearest(objects):
    # Original E4 walk, eligibility, virtual +48 (including Building center),
    # low-byte distance and strict tie handling. No hooks.
    uc = _machine(cached_control=True, return_boundary=True)
    addresses = [BASE + 0x1000 + i * 0x2000 for i in range(len(objects))]
    i32(uc, CELL + 0xE4, addresses[0] if addresses else 0)
    for index, (address, obj) in enumerate(zip(addresses, objects)):
        building = obj.get('building', False)
        i32(uc, address, 0x7E3EBC if building else 0x7F522C if obj.get('terrain', False) else 0x7F5C70)
        # Abstract410170 clears bits0..2; Object5F3900 adds2; only
        # Techno6F2B40 adds1. Terrain71BB90 never adds Techno identity.
        flags = 2 if obj.get('terrain', False) else 3
        if not obj.get('eligible', True):
            flags &= ~1
        uc.mem_write(address + 0x14, bytes([flags]))
        i32(uc, address + 0x30, addresses[index + 1] if index + 1 < len(addresses) else 0)
        uc.mem_write(address + 0x9C, struct.pack('<iii', *obj['coord']))
        if building:
            kind = address + 0x1000
            i32(uc, address + 0x520, kind)
            i32(uc, kind + 0xEF0, obj.get('foundation', 0))
            foundation = obj.get('foundation', 0)
            obj['dimensions'] = [read_i32(uc, 0x8192B8 + foundation * 4), read_i32(uc, 0x819310 + foundation * 4)]
    uc.mem_write(SP, struct.pack('<IIII', 0x30000000, RAW_COORD, 0, 0))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, CELL)
    run_checked(uc, 0x47C3D0, 0x30000000, count=100000)
    result = uc.reg_read(UC_X86_REG_EAX)
    return dict(objects=objects, selected=addresses.index(result) if result else None)


def final_handoff(case):
    uc, _ = prepare_homing(1, case['candidate'][2], case['airburst'], case['inaccurate'], case['target_present'])
    uc.mem_write(SP + 0x24, struct.pack('<iii', *case['candidate']))
    uc.mem_write(BULLET + 0xE8, struct.pack('<ddd', *case['velocity']))
    uc.mem_write(SP + 0x1F, bytes([case['near_target']]))
    i32(uc, SP + 0x60, case['mode'])
    run_checked(uc, 0x467CA9, 0x467E53, count=100000)
    return dict(**case, result=coords(uc, BULLET + 0x9C), target_aim=[640,128,624], target_location=[640,128,208])


def geometry(case, matrices):
    uc, queries, _, _ = prepare_shared(dict(case,candidate=[int(v) for v in case['candidate']]))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_EBP, BULLET)
    old = case.get('old', [640,640,500])
    candidate = case['candidate']
    uc.mem_write(BULLET+0x9C, struct.pack('<iii',*old))
    uc.mem_write(SP+0xA8, struct.pack('<iii',*old))
    uc.mem_write(SP+0x44, struct.pack('<iii',*[int(v) for v in candidate]))
    uc.mem_write(SP+0x68, struct.pack('<ddd',*candidate))
    uc.mem_write(SP+0x90, struct.pack('<ddd',*case.get('velocity',[20,3,-6])))
    uc.mem_write(SP+0x50, struct.pack('<d',case.get('elasticity',0.75)))
    source=SOURCE if case.get('source',True) else 0
    building=case.get('building')
    dimensions=None
    if building:
        address=read_i32(uc,MEM+(2*512+2)*4)
        i32(uc,address+0xE4,OBJECT)
        uc.mem_write(0xA8E9A0,b'\x01')
        i32(uc,OBJECT,0x7E3EBC)
        uc.mem_write(OBJECT+0x14,b'\x03')
        i32(uc,OBJECT+0x520,TARGET_TYPE)
        i32(uc,OBJECT+0x21C,WALL_HOUSE)
        index=building.get('foundation',0)
        i32(uc,TARGET_TYPE+0xEF0,index)
        i32(uc,TARGET_TYPE+0x408,TARGET_TYPE+0x2000 if building.get('undeploy',False) else 0)
        dimensions=[read_i32(uc,0x8192B8+index*4),read_i32(uc,0x819310+index*4)]
        if building.get('source_identity',False):
            source=OBJECT
    i32(uc,BULLET+0xB0,source)
    i32(uc,SP+0x64,source)
    for slope,matrix in enumerate(matrices):
        uc.mem_write(0xB45188+slope*48,struct.pack('<12I',*matrix))
    run_checked(uc, 0x467494,0x4677D3,count=100000)
    return dict(**case,impact=bool(uc.mem_read(SP+0x18,1)[0]),result=coords(uc,SP+0x24),
        result_candidate_bits=list(struct.unpack('<3Q',uc.mem_read(SP+0x68,24))),
        result_velocity_bits=list(struct.unpack('<3Q',uc.mem_read(SP+0x90,24))),foundation_dimensions=dimensions,queries=queries)


def homing_admission(height, distance, velocity, airburst, empty_target):
    uc, queries = prepare_homing(height, 207, airburst, False)
    uc.mem_write(BULLET + 0xE8, struct.pack('<ddd', *velocity))
    i32(uc, SP + 0x10, distance)
    if empty_target:
        uc.mem_write(SP + 0x30, bytes(12))
    run_checked(uc, 0x00466DB1, 0x00466E6B, count=10000)
    return dict(height=height, distance=distance, velocity=velocity,
                airburst=airburst, empty_target=empty_target,
                impact=bool(uc.mem_read(SP + 0x18, 1)[0]),
                candidate=coords(uc, SP + 0x24), floor_queries=queries)


def homing_handoff(height, mode, airburst, inaccurate, present):
    uc, queries = prepare_homing(height, 208 + height, airburst, inaccurate, present)
    uc.mem_write(BULLET + 0x9C, struct.pack('<iii', 504, 128, 208 + height))
    run_checked(uc, 0x00467BF0, 0x00467C0C, count=10000)
    i32(uc, SP + 0x60, mode)
    run_checked(uc, 0x00467CA9, 0x00467E53, count=10000)
    return dict(height=height, fuse_mode=mode, airburst=airburst,
                inaccurate=inaccurate, target_present=present,
                impact=coords(uc, BULLET + 0x9C), floor_queries=queries)


def homing_source_mode(present, jumpjet, mode):
    uc, _ = prepare_homing(1, 209, False, False)
    i32(uc, BULLET + 0xB0, HOMING_SOURCE if present else 0)
    i32(uc, HOMING_SOURCE, HOMING_SOURCE_VTABLE)
    i32(uc, HOMING_SOURCE_VTABLE + 0x84, HOMING_SOURCE_GET_TYPE)
    uc.mem_write(HOMING_SOURCE_TYPE + 0xD94, bytes([jumpjet]))
    uc.reg_write(UC_X86_REG_ESI, mode)
    run_checked(uc, 0x00467C3C, 0x00467C6A, count=10000)
    return dict(source_present=present, source_jumpjet=jumpjet,
                detector_mode=mode, admitted_mode=uc.reg_read(UC_X86_REG_ESI))
