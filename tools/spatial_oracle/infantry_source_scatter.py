"""Original source-aware Infantry Scatter through destination dispatch.

Runs the real entry, house/mission/ability/Walk readers, heading math, Scenario
RNG, Foot navigation coordinate, map lookup/playfield, height and projection.
Default Can_Enter_Cell answers and QueueMission/SetDestination effects are
observable seams. The companion infantry_scatter_entry corpus selects the real
+1AC body instead. The infantry_scatter_destination corpus additionally executes QueueMission,
Infantry/Foot SetDestination and Walk MoveTo, observing first Process separately.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP,
    UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    load_image, run_checked, finish_vectors, provenance,
    SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC,
)
from tools.spatial_oracle.map_queries import dwords, packed

ACTOR, TYPE, HOUSE, LOCO, RULES, SOURCE, VT, SCENARIO = [
    SCRATCH + i * 0x2000 for i in range(8)]
CELLS = SCRATCH + 0x10000
ENTRY, QUEUE, SET = [SCRATCH + 0xF000 + i * 0x100 for i in range(3)]
MAP, TABLE, DUMMY = 0x87F7E8, 0xC00000, 0xABDC50
BOUNDS = (16, -16, -16, 64, 64)


def query(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0xA0000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    table = bytearray(0x100000)
    overrides = {(x, y): (level, flags) for x, y, level, flags in case.get('cells', [])}
    for y in range(32):
        for x in range(32):
            ptr = CELLS + (y * 32 + x) * 0x200
            struct.pack_into('<I', table, (y * 512 + x) * 4, ptr)
            u.mem_write(ptr, dwords(0x7E4EEC))
            u.mem_write(ptr + 0x24, packed(x, y))
            level, flags = overrides.get((x, y), (0, 0))
            u.mem_write(ptr + 0x11B, bytes([level & 255, 0]))
            u.mem_write(ptr + 0x140, dwords(flags))
    if case.get('live_entry'):
        # Declared empty lists/overlays/raw owners. The real +1AC body reads
        # Cell land rows and raw occupation independently of object lists.
        for y in range(32):
            for x in range(32):
                ptr = CELLS + (y * 32 + x) * 0x200
                u.mem_write(ptr + 0x44, dwords(-1))
                u.mem_write(ptr + 0x54, dwords(-1, -1))
        for x, y, ground, deck in case.get('raw', []):
            ptr = CELLS + (y * 32 + x) * 0x200
            u.mem_write(ptr + 0x124, dwords(ground, deck))
        for x, y in case.get('blocked_terrain', []):
            ptr = CELLS + (y * 32 + x) * 0x200
            u.mem_write(ptr + 0xEC, dwords(1))
        for x, y, slope in case.get('slopes', []):
            ptr = CELLS + (y * 32 + x) * 0x200
            u.mem_write(ptr + 0x11C, bytes([slope]))
        u.mem_write(0x89EA40, struct.pack('<18f', *([1.0] * 9 + [0.0] * 9)))
    u.mem_write(TABLE, bytes(table))
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    bounds = case.get('bounds', BOUNDS)
    u.mem_write(MAP + 0xF4, dwords(bounds[0]))
    u.mem_write(MAP + 0xFC, dwords(*bounds[1:]))
    u.mem_write(DUMMY + 0x11B, b'\0\0')
    u.mem_write(DUMMY + 0x140, dwords(0))
    u.mem_write(DUMMY + 0x24, packed(123, -234))
    u.mem_write(VT, bytes(u.mem_read(0x7EB058, 0x600)))
    assert struct.unpack('<I', u.mem_read(VT + 0x1AC, 4))[0] == 0x51BF90
    for offset, pointer in [(0x1AC, ENTRY), (0x1E8, QUEUE), (0x480, SET)]:
        if (offset == 0x1AC and not case.get('live_entry')
                or offset != 0x1AC and not case.get('live_setter')):
            u.mem_write(VT + offset, dwords(pointer))
    u.mem_write(ACTOR, dwords(VT))
    u.mem_write(ACTOR + 0x6C0, dwords(TYPE))
    u.mem_write(ACTOR + 0x6C4, dwords(case.get('doing', -1)))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(ACTOR + 0x674, dwords(LOCO))
    u.mem_write(ACTOR + 0x684, b'\xff')
    u.mem_write(ACTOR + 0xAC, dwords(case.get('mission', 5)))
    u.mem_write(ACTOR + 0xB4, dwords(-1))
    actor = case.get('actor', [2688, 2688, 0])
    u.mem_write(ACTOR + 0x9C, dwords(*actor))
    u.mem_write(ACTOR + 0x8C, bytes([case.get('on_bridge', False)]))
    u.mem_write(TYPE + 0xEBF, b'\x01')
    u.mem_write(LOCO, dwords(0x7F69F8))
    u.mem_write(LOCO + 8, dwords(ACTOR))
    u.mem_write(LOCO + 0x24, dwords(*case.get('head', [0, 0, 0])))
    u.mem_write(0xA8E3A8 + 5 * 32 + 9, b'\x01')
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(0xA8B230, dwords(SCENARIO))
    u.mem_write(0xA8F1E0, packed(0, 0))
    for address in [0x89C848, 0xA8F200, 0x8B3DA8]:
        u.mem_write(address, dwords(0, 0, 0))
    u.mem_write(SOURCE, dwords(*case.get('source', [1000, 2688, 0])))

    def read32(address):
        return struct.unpack('<I', u.mem_read(address, 4))[0]

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    events = []
    checks = []
    destination = None
    start_direction = None
    pending_entry = None
    entry_return = None

    def observe(_u, address, _size, _data):
        nonlocal destination, start_direction, pending_entry, entry_return
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x51D487:
            start_direction = read32(sp + 0x1C) & 7
        elif address == 0x65C7E0:
            events.append('random')
        elif address == entry_return and pending_entry is not None:
            checks.append([*pending_entry, u.reg_read(UC_X86_REG_EAX)])
            pending_entry = None
        elif address == ENTRY or (case.get('live_entry') and address == 0x51BF90):
            args = [read32(sp + 4 + i * 4) for i in range(5)]
            coord = list(struct.unpack('<hh', u.mem_read(args[0] + 0x24, 4)))
            assert args[3:] == [0, 1]
            if case.get('live_entry'):
                pending_entry = [coord, args[1], struct.unpack('<i', dwords(args[2]))[0]]
                entry_return = read32(sp)
                events.append('entry')
                return
            code = case.get('answers', [0] * 8)[args[1]]
            checks.append([coord, args[1], struct.unpack('<i', dwords(args[2]))[0], code])
            events.append('entry')
            ret(20, code)
        elif address == QUEUE or (case.get('live_setter') and address == read32(0x7EB058 + 0x1E8)):
            assert [read32(sp + 4), read32(sp + 8)] == [2, 0]
            events.append('queue_move')
            if not case.get('live_setter'):
                ret(8)
        elif address == SET or (case.get('live_setter') and address == 0x51AA40):
            assert read32(sp + 8) == 1
            destination = list(struct.unpack('<hh', u.mem_read(read32(sp + 4) + 0x24, 4)))
            events.append('destination')
            if not case.get('live_setter'):
                ret(8)
        elif case.get('live_setter'):
            if address in [read32(0x7E11C8), read32(0x7E11CC)]:
                pointer = read32(sp + 4)
                value = read32(pointer) + (1 if address == read32(0x7E11C8) else -1)
                u.mem_write(pointer, dwords(value))
                ret(4, value)
            elif address in [0x4D94B0, 0x75ACB0, 0x75ADA0, 0x51DAF0, 0x4834A0, 0x75AEC0]:
                events.append(hex(address))

    u.hook_add(UC_HOOK_CODE, observe)

    def call(entry, this, args):
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ECX, this)
        u.reg_write(UC_X86_REG_ESP, sp)
        run_checked(u, entry, RET_MAGIC, count=300000, required_addresses=[entry])
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 * (1 + len(args))

    call(0x49F2F0, 0, [])  # native startup populates the neighbour table
    call(0x65C6D0, SCENARIO + 0x218, [case.get('seed', 1)])
    if case.get('live_setter'):
        call(0x6D1830, 0, [])
        call(0x6D18C0, 0, [])
        call(0x6D1BF0, 0, [])
        call(0x75AA90, LOCO, [])
        u.mem_write(LOCO + 0xC, dwords(ACTOR))
        u.mem_write(LOCO + 0x14, dwords(1))
        u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
        u.mem_write(LOCO + 0x28, dwords(*case.get('head', [0, 0, 0])))
        u.mem_write(LOCO + 0x10, bytes([not case.get('power_off', False)]))
        u.mem_write(LOCO + 0x34, bytes([case.get('moving', False)]))
        if case.get('moving'):
            u.mem_write(LOCO + 0x1C, dwords(7808, 2688, 0))
        u.mem_write(TYPE + 0xE3C, dwords(SCRATCH + 0x90000))
        u.mem_write(ACTOR + 0x5A0, dwords(123))
        u.mem_write(ACTOR + 0x5E0, dwords(2, 3, 4, 5))
        u.mem_write(ACTOR + 0x558, packed(9, 8))
        u.mem_write(ACTOR + 0x6B7, b'\x01')
        u.mem_write(ACTOR + 0x6DC, b'\x01')
        u.mem_write(ACTOR + 0x640, dwords(50, 0, 5))
        u.mem_write(ACTOR + 0x668, dwords(40, 0, 6))
        u.mem_write(ACTOR + 0x64C, dwords(7))
        u.mem_write(ACTOR + 0x388, struct.pack('<HH', 0x4000, 0x4000))
        u.mem_write(ACTOR + 0x6AD, bytes([case.get('swap_active', False)]))
        u.mem_write(ACTOR + 0x82, bytes([case.get('open_transport', False)]))
        u.mem_write(ACTOR + 0x2E4, dwords(TYPE if case.get('bunker', False) else 0))
        u.mem_write(ACTOR + 0x270, bytes([case.get('warp_out', False), case.get('warp_in', False)]))
        u.mem_write(0xA8ED84, dwords(100))
        u.mem_write(RULES + 0x1768, dwords(22))
        u.mem_write(0xA8E3A8 + case.get('mission', 5) * 32 + 9, b'\x01')
    call(0x51D0D0, ACTOR, [SOURCE, case.get('force', False), False])
    assert pending_entry is None
    if case.get('live_entry'):
        assert checks, 'live-entry witnesses must reach the original body'
    result = dict(input=case, destination=destination, checks=checks, events=events,
                  start_direction=start_direction,
                  random_indices=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)])
    if case.get('live_setter'):
        result['setter'] = dict(
            nav=list(struct.unpack('<hh', u.mem_read(read32(ACTOR + 0x5A4) + 0x24, 4))) if read32(ACTOR + 0x5A4) else None,
            aux=read32(ACTOR + 0x5A0),
            destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x1C, 12))),
            head=list(struct.unpack('<iii', u.mem_read(LOCO + 0x28, 12))),
            queue=list(struct.unpack('<iiii', u.mem_read(ACTOR + 0x5E0, 16))),
            reference=list(struct.unpack('<hh', u.mem_read(ACTOR + 0x558, 4))),
            queued_mission=struct.unpack('<i', u.mem_read(ACTOR + 0xB4, 4))[0],
            facing=read32(ACTOR + 0x388),
            moving=u.mem_read(LOCO + 0x34, 1)[0],
            powered=u.mem_read(LOCO + 0x10, 1)[0],
            blocked=u.mem_read(ACTOR + 0x6B7, 1)[0],
            movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
            blocked_timer=[read32(ACTOR + 0x668), read32(ACTOR + 0x670)],
            retries=read32(ACTOR + 0x64C),
            entry_blocked=u.mem_read(ACTOR + 0x6DC, 1)[0],
        )
        assert '0x75aec0' not in events, 'source-aware Scatter must not Process immediately'
        if case.get('process_probe'):
            before_events = len(events)
            endpoint = 0x75BD29 if any(result['setter']['head']) else 0x4D3920
            sp = STACK_BASE + STACK_SIZE - 0x1000
            u.mem_write(sp, dwords(RET_MAGIC, 0))
            u.reg_write(UC_X86_REG_ESP, sp)
            u.reg_write(UC_X86_REG_ECX, LOCO)
            run_checked(u, 0x75AEC0, endpoint, count=100000, required_addresses=[0x75AEC0])
            result['process'] = dict(endpoint=hex(endpoint), events=events[before_events:])
            del events[before_events:]

    return result


def generate():
    cases = [dict(seed=seed, source=source) for seed in (1, 31, 42)
             for source in ([1000, 2688, 0], [4000, 2688, 0], [2688, 1000, 0],
                            [2688, 4000, 0], [1000, 1000, 0], [4000, 4000, 0],
                            [1000, 4000, 0], [4000, 1000, 0], [2688, 2688, 1])]
    cases += [dict(answers=[code] * 8) for code in range(1, 8)]
    cases += [dict(answers=[0 if i == direction else 7 for i in range(8)])
              for direction in range(8)]
    cases += [dict(cells=[[11, 9, 0, 0x100]]),
              dict(cells=[[x, y, 0, 0x100] for x, y in
                          [(10, 9), (11, 9), (11, 10), (11, 11),
                           (10, 11), (9, 11), (9, 10), (9, 9)]]),
              dict(cells=[[12, 10, 8, 0]]),
              dict(cells=[[11, 9, 0, 0x1000], [12, 10, 0, 0x100]]),
              dict(cells=[[10, 10, -1, 0]], on_bridge=True),
              dict(head=[15 * 256 + 128, 15 * 256 + 128, 0]),
              dict(bounds=[16, 0, 0, 1, 1]),
              dict(actor=[128, 128, 0], source=[-256, 128, 0]),
              dict(doing=7), dict(force=True, doing=31),
              dict(source=[-2147483648, 2688, 0]),
              dict(source=[2688, -2147483648, 0]),
              dict(actor=[384, 384, 0], source=[384, 1000, 0]),
              dict(actor=[384, 384, 0], source=[1000, 1000, 0],
                   cells=[[0, 1, 0, 0x100]])]
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Source-aware Infantry Scatter selection, native RNG and dispatch order; supplied Can_Enter_Cell answers and observed destination receiver, not full movement parity.',
        entry_points={'scatter': 0x51D0D0, 'navigation_coord': 0x4DBDF0,
                      'projection': 0x6D6410, 'height': 0x5F5F00,
                      'direction_startup': 0x49F2F0,
                      'playfield': 0x578460, 'random': 0x65C7E0},
        assumptions=['Valid Fraidycat Infantry with native Walk interface, AI house, Guard Scatter enabled; zero NullCoord/NullCell.',
                     '32x32 allocated cells and supplied raw playfield fields widened for boundary selection; not a map-loader or retail boundary-reachability fixture. Original heading arithmetic under chop53 control word.',
                     'Original49F2F0 initializes the runtime direction table before Scatter; never use its cold image zeros.',
                     'No NULL-source FNPC or true/true DoAction31 cases.'],
        substitutions=['Infantry+1AC returns per-direction supplied numeric answers.',
                       'QueueMission and SetDestination are argument-checking observers.']))
