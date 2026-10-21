"""Original PowerClass draw commands and opt-in retained producer controls.

Executes 63FB20..63FDA5 from the hash-checked gamemd image. The surface bounds
interface is replaced with a declared rectangle; 4AED70 is intercepted to record
its actual command arguments and emulate RET 56. RadarClass::Draw is not entered.
The default corpus supplies segment counts; it does not establish their producer
or clock. --state executes the original Power constructor, distribution, AI/timers,
draw and Sidebar NoInit with supplied House/raw-state inputs. Clock, inherited
Radar and non-Power NoInit members are explicit substitutions. Neither corpus
renders pixels or executes native Mouse storage/swizzling or a complete load.
"""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import itertools
import json
import struct
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *

from types import SimpleNamespace
from tools.native_oracle import STACK_BASE, STACK_SIZE, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.storage_oracle.saved_game_layout import code_guard, check_code
from tools.sidebar_oracle.geometry import machine as base_machine, put32, call

def machine():
    u=base_machine(); u.mem_map(0x21000000,0x40000); return u
n=SimpleNamespace(machine=machine,HEAP=0x21000000,STACK=STACK_BASE,STOP=RET_MAGIC,put32=put32)


def shape_command(u, read32):
    """Observe the shared CC_Draw_Shape ABI before its raster boundary."""
    sp = u.reg_read(UC_X86_REG_ESP)
    args = struct.unpack('<14I', u.mem_read(sp + 4, 56))
    return dict(caller=read32(sp), surface=u.reg_read(UC_X86_REG_ECX),
                converter=u.reg_read(UC_X86_REG_EDX), args=args,
                point=list(struct.unpack('<2i', u.mem_read(args[2], 8))),
                clip=list(struct.unpack('<4i', u.mem_read(args[3], 16))))


def presentation_return(u, read32, stack_argument_bytes):
    """Existing transport result/cleanup; volatile values are unspecified."""
    sp = u.reg_read(UC_X86_REG_ESP)
    target = read32(sp)
    u.reg_write(UC_X86_REG_EAX, 0)
    u.reg_write(UC_X86_REG_ECX, 0x13572468)
    u.reg_write(UC_X86_REG_EDX, 0x24681357)
    u.reg_write(UC_X86_REG_ESP, sp + 4 + stack_argument_bytes)
    u.reg_write(UC_X86_REG_EIP, target)


def observe_draws(u):
    """The one shared surface/shape transport for draw and producer controls."""
    surface, vtable, shape, convert = [n.HEAP + v for v in (0x20000, 0x21000, 0x22000, 0x23000)]
    callback = n.STOP + 0x100
    for address, value in ((0x887300, surface),
                           (surface, vtable), (vtable + 0x78, callback),
                           (0xac4e74, shape), (0x87f6cc, convert), (0x886f94, 158)):
        n.put32(u, address, value)
    commands = []
    bounds_calls = []

    def read32(address):
        return struct.unpack('<I', u.mem_read(address, 4))[0]

    def bounds(_u, address, _size, _data):
        assert address == callback
        sp = u.reg_read(UC_X86_REG_ESP)
        output = read32(sp + 4)
        assert u.reg_read(UC_X86_REG_ECX) == surface
        u.mem_write(output, struct.pack('<4i', 0, 0, 168, 600))
        bounds_calls.append(True)
        presentation_return(u, read32, 4)
        u.reg_write(UC_X86_REG_EAX, output)

    def draw(_u, address, _size, _data):
        assert address == 0x4aed70
        command = shape_command(u, read32)
        args = command['args']
        assert u.reg_read(UC_X86_REG_ECX) == surface
        assert u.reg_read(UC_X86_REG_EDX) == convert
        assert args[0] == shape
        assert command['clip'] == [0, 0, 168, 600]
        assert args[4:] == (0x400, 0, 0, 0, 1000, 0, 0, 0, 0, 0)
        x, y = command['point']
        commands.append([args[1], x, y])
        presentation_return(u, read32, 56)

    u.hook_add(UC_HOOK_CODE, bounds, begin=callback, end=callback)
    u.hook_add(UC_HOOK_CODE, draw, begin=0x4aed70, end=0x4aed70)
    return commands, bounds_calls


def collect():
    u = n.machine()
    owner, scenario = n.HEAP, n.HEAP + 0x10000
    n.put32(u, 0xa8b230, scenario)
    commands, bounds_calls = observe_draws(u)

    def run(side, height, counts, flashes, force=1, dirty=0, visible=1):
        commands.clear()
        bounds_calls.clear()
        n.put32(u, scenario + 0x34b8, side)
        n.put32(u, 0xb0b504, height)
        n.put32(u, owner + 0x151c, flashes)
        for offset, value in zip((0x152c, 0x1530, 0x1534), counts):
            n.put32(u, owner + offset, value)
        u.mem_write(owner + 0x150c, bytes([dirty]))
        u.mem_write(0x884b8d, bytes([visible]))
        u.mem_write(0xb0b518, b'\x00')
        sp = n.STACK + 0x80000
        u.mem_write(sp, struct.pack('<2I', n.STOP, force))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, owner)
        u.emu_start(0x63fb20, 0x63fda5, count=20000)
        assert u.reg_read(UC_X86_REG_EIP) == 0x63fda5
        return {
            'side': side, 'strip_height': height, 'counts_surplus_output_drain': list(counts),
            'flashes': flashes, 'force': force, 'dirty_before': dirty, 'visible': visible,
            'dirty_after': int(u.mem_read(owner + 0x150c, 1)[0]),
            'redraw_flag_after': int(u.mem_read(0xb0b518, 1)[0]),
            'bounds_calls': len(bounds_calls), 'frame_x_y': commands.copy(),
        }

    cases = [run(side, height, counts, flashes)
             for side, height, counts, flashes in itertools.product(
                 (0, 1, 2), (0, 12, 300), list(itertools.product((0, 1, 3), repeat=3)),
                 (-2, 0, 1, 2, 10))]
    gates = [run(0, 12, (1, 1, 1), 2, force, dirty, visible)
             for force, dirty, visible in itertools.product((0, 1), repeat=3)]
    return {
        'executed': '0063fb20 through 0063fda5, excluding later RadarClass draw',
        'substitutions': [
            'Surface virtual +0x78 returns prepared [0,0,168,600] rectangle in caller output buffer.',
            '004aed70 records original selected SHP frame, point, clip, converter, and all other arguments; returns with original 56-byte stack cleanup. No pixels rendered.',
        ],
        'prepared': 'Owner counters, flash counter, force/dirty/visible, side, strip height, sidebar body Y=158, allocated shape/convert/surface identity sentinels. No producer reachability for every synthetic count claimed.',
        'command_constants': {'surface': 'prepared sidebar surface', 'convert_global': '0087f6cc',
                              'shape_global': '00ac4e74', 'clip': [0, 0, 168, 600],
                              'flags': 1024, 'brightness': 1000, 'other_stack_parameters': 0},
        'case_count': len(cases), 'gate_count': len(gates), 'cases': cases, 'gates': gates,
    }


def collect_state():
    """Execute Power's own constructor, arithmetic, AI and draw without patches.

    House getters and the building/type-list traversal execute original bodies.
    Their supplied accounting inputs do not establish the native House producer.
    OS time, inherited Radar lifecycle/AI, non-Power NoInit members and the
    shared draw transport are substituted. The active process control word is
    PC53/chop (0x0E7F),
    including the sticky control-word write in the original Math::ftol.
    """
    owner, house = n.HEAP, n.HEAP + 0x10000
    objects = (n.HEAP + 0x17000, n.HEAP + 0x1a000)
    types = (n.HEAP + 0x18000, n.HEAP + 0x1b000)
    building_list = n.HEAP + 0x1f000
    wall_slot, clock = n.STOP + 0x300, n.STOP + 0x200

    def fixture(precision=0xe7f):
        u = n.machine()
        guard = code_guard(u)
        u.reg_write(UC_X86_REG_FPCW, precision)
        commands, bounds = observe_draws(u)
        for address, value in ((0xa83d4c, house), (house + 0x24, 0x7ea834),
                               (house + 0x6c, building_list), (house + 0x78, 2),
                               (0xa8b230, house), (house + 0x34b8, 0),
                               (0x7e1530, clock)):
            n.put32(u, address, value)
        for index, (obj, typ) in enumerate(zip(objects, types)):
            n.put32(u, building_list + index * 4, obj)
            n.put32(u, obj + 0x520, typ)
        u.mem_write(clock, b'\xa1' + struct.pack('<I', wall_slot) + b'\xc3')

        def inherited(_u, address, _size, _data):
            assert address in (0x652960, 0x652de0, 0x653850)
            presentation_return(u, lambda a: struct.unpack('<I', u.mem_read(a, 4))[0],
                                8 if address == 0x653850 else 0)

        for address in (0x652960, 0x652de0, 0x653850):
            u.hook_add(UC_HOOK_CODE, inherited, begin=address, end=address)
        call(u, 0x63f6b0, ecx=owner)
        check_code(u, guard)
        return u, commands, bounds

    def inputs(u, height, output, drain, theoretical):
        for address, value in ((0xb0b504, height), (house + 0x53a4, output),
                               (house + 0x53a8, drain), (types[0] + 0xee0, theoretical),
                               (types[0] + 0xee4, 0), (types[1] + 0xee0, 0),
                               (types[1] + 0xee4, 0)):
            n.put32(u, address, value)

    def signed(u, address, count=1):
        return list(struct.unpack('<' + 'i' * count, u.mem_read(address, 4 * count)))


    def retained(u):
        return dict(counts=signed(u, owner + 0x152c, 3),
                    flash=signed(u, owner + 0x151c)[0],
                    flash_timer=[signed(u, owner + 0x1510)[0], signed(u, owner + 0x1518)[0]],
                    slide_timer=[signed(u, owner + 0x1520)[0], signed(u, owner + 0x1528)[0]],
                    cached=signed(u, owner + 0x153c, 2),
                    animating=bool(u.mem_read(owner + 0x1538, 1)[0]),
                    dirty=int(u.mem_read(owner + 0x150c, 1)[0]))

    def visit(u, commands, fill_calls, row, clock_calls=None):
        wall_ms, height, output, drain, total, visible, reset = row
        n.put32(u, wall_slot, wall_ms)
        if reset:
            call(u, 0x63f730, ecx=owner)
        inputs(u, height, output, drain, total)
        u.mem_write(0x884b8d, bytes([visible]))
        fill_calls.clear()
        if clock_calls is not None:
            clock_calls.clear()
        call(u, 0x63fea0, (0, 0), ecx=owner)
        state = retained(u)
        commands.clear()
        # Existing observer's bounds/draw ABI; stop before inherited Radar.
        sp = n.STACK + 0x80000
        u.mem_write(sp, struct.pack('<2I', n.STOP, 1))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, owner)
        run_checked(u, 0x63fb20, 0x63fda5, count=20000)
        result = dict(input=list(row), **state, fill_calls=len(fill_calls),
                      frames=[command[0] for command in commands])
        if clock_calls is not None:
            result['clock_ms'] = clock_calls.copy()
        return result

    # Observe the original combined dword before 63F8AF converts it to x87.
    # Power's list traversal does not read the building's live state or the
    # House's dirty assessment cache. The supplied signed Power values use
    # BuildingType reader461060..46109A's EE0/EE4 split (including NEG MIN).
    rated_lists = []
    for name, powers in (
            ('empty', ()), ('stock-output', (150,)), ('stock-drain', (-10,)),
            ('stock-mixed', (150, -10)), ('signed-min', (-0x80000000,)),
            ('output-overflow', (0x7fffffff, 1)),
            ('drain-overflow', (-0x7fffffff, -1)),
            ('two-min-wrap', (-0x80000000, -0x80000000))):
        u, _, _ = fixture()
        n.put32(u, 0xb0b504, 90)
        n.put32(u, house + 0x78, len(powers))
        for typ, power in zip(types, powers):
            n.put32(u, typ + 0xee0, power if power >= 0 else 0)
            n.put32(u, typ + 0xee4, -power if power < 0 else 0)
        totals = []

        def observe_total(_u, address, _size, _data):
            assert address == 0x63f8ab
            totals.append(struct.unpack('<i', struct.pack('<I',
                u.reg_read(UC_X86_REG_EDX)))[0])

        u.hook_add(UC_HOOK_CODE, observe_total, begin=0x63f8ab, end=0x63f8ab)
        call(u, 0x63f850, ecx=owner)
        assert len(totals) == 1, (name, totals)
        filled = struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_EAX)))[0]
        rated_lists.append(dict(name=name, powers=list(powers), strip_height=90,
                                total=totals[0], filled=filled))

    numeric = []
    for precision in (0xe7f,):
        u, _, _ = fixture(precision)
        for height in (0, 12, 90, 300, 434, 653):
            # Geometry controls include the native one-segment minimum.
            output = drain = total = 0
            inputs(u, height, output, drain, total)
            call(u, 0x63f960, (owner + 0x3000, owner + 0x3004, owner + 0x3008), ecx=owner)
            numeric.append(dict(input=[height, output, drain, total], fpcw=precision,
                                counts=signed(u, owner + 0x3000, 3)))
        for height, values in itertools.product((12, 90, 300, 434), (
                (200, 0, 200), (200, 100, 300), (100, 200, 300),
                (300, 100, 400), (150, 100, 250), (100, 100, 200),
                (600, 200, 800), (10000, 0, 10000), (99, 0, 99),
                (100, 0, 100), (101, 0, 101), (200, 0, 400),
                (150, 0, 150), (150, 10, 160), (150, 60, 210))):
            output, drain, total = values
            inputs(u, height, output, drain, total)
            call(u, 0x63f960, (owner + 0x3000, owner + 0x3004, owner + 0x3008), ecx=owner)
            numeric.append(dict(input=[height, output, drain, total], fpcw=precision,
                                counts=signed(u, owner + 0x3000, 3)))

    # Small retained histories include stalls, repeated bucket inputs, interrupted
    # transitions, a theoretical-only change, resize, hide/show and reset.
    histories = []
    schedules = {
        'power-plant-settle': [(ms, 90, 200, 0, 200, 1, 0) for ms in range(0, 1601, 16)],
        'interrupt-and-reset': [
            (0, 90, 200, 100, 300, 1, 0), (0, 90, 200, 100, 300, 1, 0),
            (15, 90, 200, 100, 300, 1, 0), (16, 90, 200, 100, 300, 1, 0),
            (32, 90, 200, 100, 300, 1, 0), (1000, 90, 200, 100, 300, 1, 0),
            (1016, 90, 100, 200, 300, 1, 0), (1032, 90, 100, 200, 300, 0, 0),
            (2000, 90, 100, 200, 300, 1, 0), (2000, 90, 100, 200, 300, 1, 0),
            (2016, 90, 300, 100, 400, 1, 0), (2032, 12, 300, 100, 400, 1, 0),
            (3000, 12, 300, 100, 400, 1, 0), (3016, 12, 300, 100, 400, 1, 1),
            (3032, 12, 0, 0, 0, 1, 0), (4000, 12, 0, 0, 0, 1, 0)],
        'idle-theoretical-and-clock-wrap': [
            (0, 0, 0, 0, 0, 1, 0), (16, 0, 0, 0, 0, 1, 0),
            (32, 0, 0, 0, 0, 1, 0), (1000, 90, 0, 0, 800, 1, 0),
            (1016, 90, 0, 0, 800, 0, 0), (0xfffffff0, 90, 200, 0, 200, 1, 0),
            (0x100000000, 90, 200, 0, 200, 1, 0),
            (0x100000010, 90, 200, 0, 200, 1, 0)],
    }
    for name, schedule in schedules.items():
        u, commands, _ = fixture()
        fill_calls = []
        u.hook_add(UC_HOOK_CODE, lambda *_: fill_calls.append(True),
                   begin=0x63f850, end=0x63f850)
        visits = [visit(u, commands, fill_calls, row) for row in schedule]
        histories.append(dict(name=name, visits=visits))

    # Full Sidebar NoInit caller6A4F20..6A4FED; unrelated member constructors
    # return at declared boundaries. The Power clock/stores execute unchanged.
    no_init_boundaries = {0x6ac840: 4, 0x42f670: 8, 0x6ac8d0: 8,
                          0x4f1b00: 4, 0x405be0: 0, 0x4068f0: 16}
    loaded_histories = []
    for name, flash_timer, flash_count, load_ms, schedule, producer in (
            ('stopped-positive', (-1, 3), 4, 8000, (8000, 160000), None),
            ('stopped-zero', (-1, 0), 4, 8000, (8000, 8000, 8032, 8048), None),
            ('armed', (500, 3), 4, 8016, (8016, 8032, 8048), None),
            ('already-due', (100, 3), 4, 8000, (8000, 8000, 160000, 160000), None),
            ('wrap', (0x0fffffff, 3), 4, 0x100000000,
             (0x100000000, 0x100000030), None),
            ('zero-count', (100, 3), 0, 8000, (8000, 160000), None),
            ('retained-active', None, None, 32, (32, 32, 112, 112), (12, 32)),
            ('settled-resize-load', None, None, 8000, (8000,), (90, 1601))):
        u, commands, _ = fixture()
        guard = code_guard(u)
        inputs(u, 12, 300, 100, 400)
        u.mem_write(0x884b8d, b'\x01')
        if producer is not None:
            height, stop = producer
            inputs(u, height, 300, 100, 400)
            for wall_ms in range(0, stop, 16):
                n.put32(u, wall_slot, wall_ms)
                call(u, 0x63fea0, (0, 0), ecx=owner)
        else:
            for offset, value in ((0x152c, 0), (0x1530, 0), (0x1534, 3),
                                  (0x153c, 100), (0x1540, 300),
                                  (0x1510, flash_timer[0]), (0x1518, flash_timer[1]),
                                  (0x151c, flash_count), (0x1520, 0x12345),
                                  (0x1528, 1234)):
                n.put32(u, owner + offset, value)
            u.mem_write(owner + 0x1538, b'\x00')
        # Opaque timer words must survive NoInit, but are not saved semantics.
        n.put32(u, owner + 0x1514, 0x24681357)
        n.put32(u, owner + 0x1524, 0x13572468)
        opaque = [bytes(u.mem_read(owner + offset, 4)) for offset in (0x1514, 0x1524)]
        u.mem_write(owner + 0x150c, b'\x01')
        prior = retained(u)
        clock_calls, writes, fill_calls, excluded_calls = [], [], [], []

        def excluded(_u, address, _size, _data):
            excluded_calls.append(address)
            presentation_return(u, lambda at: struct.unpack('<I', u.mem_read(at, 4))[0],
                                no_init_boundaries[address])

        for address in no_init_boundaries:
            u.hook_add(UC_HOOK_CODE, excluded, begin=address, end=address)
        u.hook_add(UC_HOOK_CODE,
                   lambda *_: clock_calls.append(struct.unpack('<I', u.mem_read(wall_slot, 4))[0]),
                   begin=clock, end=clock)
        u.hook_add(UC_HOOK_CODE, lambda *_: fill_calls.append(True),
                   begin=0x63f850, end=0x63f850)

        def observe_write(_u, _access, address, size, value, _data):
            writes.append([address - owner, size, value])

        write_hook = u.hook_add(UC_HOOK_MEM_WRITE, observe_write,
                                begin=owner + 0x150c, end=owner + 0x1543)
        input_row = [load_ms, 12, 300, 100, 400, 1, 0]
        inputs(u, *input_row[1:5])
        n.put32(u, wall_slot, load_ms)
        call(u, 0x6a4f20, (owner + 0xf000,), ecx=owner)
        u.hook_del(write_hook)
        assert u.reg_read(UC_X86_REG_ESP) == n.STACK + STACK_SIZE - 4096 + 8
        assert {address: excluded_calls.count(address) for address in no_init_boundaries} == {
            0x6ac840: 1, 0x42f670: 1, 0x6ac8d0: 23,
            0x4f1b00: 1, 0x405be0: 1, 0x4068f0: 4}
        no_init = retained(u)
        for key, value in prior.items():
            if key != 'slide_timer':
                assert no_init[key] == value, (name, key)
        assert writes == [[0x1520, 4, (load_ms & 0xffffffff) >> 4], [0x1528, 4, 0]]
        assert clock_calls == [load_ms & 0xffffffff]
        assert opaque == [bytes(u.mem_read(owner + offset, 4)) for offset in (0x1514, 0x1524)]
        no_init.update(clock_ms=clock_calls.copy(), writes=writes)
        visits = [visit(u, commands, fill_calls,
                        [wall_ms, *input_row[1:]], clock_calls) for wall_ms in schedule]
        check_code(u, guard)
        loaded_histories.append(dict(name=name, input=input_row, prior=prior,
                                     no_init=no_init, visits=visits))
    return dict(rated_lists=rated_lists, numeric=numeric, histories=histories,
                loaded_histories=loaded_histories)



if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--state', action='store_true')
    options, rest = parser.parse_known_args()
    if options.state:
        finish_vectors(collect_state, Path(__file__).with_name('power_state.json'), argv=rest,
                       provenance=lambda: provenance(
            scope='Power typed-House-list sums/fill returns, numeric and retained AI/draw histories plus eight loaded-state Sidebar NoInit controls; supplied House accounting, geometry and raw state.',
            assumptions=['House power interface vtable7EA834 and its real getters; supplied House output/drain and owned typed-building list.',
                         'Native PC53/chop control word0x0E7F. No House producer/native scenario equivalence.',
                         'Constructor/reset inherited Radar body excluded. Power initial fields and clocks execute original instructions.',
                         'Six loaded priors are supplied raw controls; native ctor/AI produces the active and settled-resize priors. No Mouse storage/swizzle or cross-process load equivalence.'],
            substitutions=['timeGetTime returns explicit input;6C8C40 executes original SHR4.',
                           'Inherited Radar constructor/reset/AI returns at its interface. No Radar-state equivalence.',
                           'Sidebar NoInit6A4F20 executes; inherited6AC840 returns RET4, member42F670/6AC8D0 RET8,4F1B00 RET4,405BE0 RET0 and Strip iterator4068F0 RET16. Their bodies are excluded.',
                           'Existing surface-bounds and CC_Draw_Shape command transport; no native pixels.'],
            entry_points={'constructor':0x63F6B0,'reset':0x63F730,'fill':0x63F850,
                          'rated_sum_observer':0x63F8AB,
                          'distribution':0x63F960,'animation':0x63FEA0,'draw':0x63FB20,
                          'no_init':0x6A4F20,'no_init_slide_start':0x6A4F8E,
                          'no_init_slide_duration':0x6A4F94,'wall_clock':0x6C8C40}))
    else:
        finish_vectors(collect,Path(__file__).with_suffix('.json'),argv=rest,provenance=lambda:provenance(
        scope='Original63FB20 command loop:1215 staged count/flash/side/height cases plus8 dirty/visible/force cases.',
        assumptions=['Explicit staged counters; no target or timing producer equivalence claimed.',
          'Sidebar surface bounds supplied as0,0,168,600; sidebar bodyY158.'],
        substitutions=['Surface virtual+78 returns declared bounds;4AED70 records actual commands and returns with original stack cleanup, poisoning volatile outputs.'],
        entry_points={'power_draw':0x63FB20,'draw_boundary':0x4AED70}))
