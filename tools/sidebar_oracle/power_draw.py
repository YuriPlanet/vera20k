"""Original PowerClass draw commands, with explicitly staged presentation state.

Executes 63FB20..63FDA5 from the hash-checked gamemd image. The surface bounds
interface is replaced with a declared rectangle; 4AED70 is intercepted to record
its actual command arguments and emulate RET 56. RadarClass::Draw is not entered.
No raster, power-distribution producer, clock, or actual scene parity is claimed.
"""
from pathlib import Path
import hashlib
import importlib.util
import itertools
import json
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *

from types import SimpleNamespace
from tools.native_oracle import NATIVE_SHA256, STACK_BASE, RET_MAGIC, finish_vectors, provenance
from tools.sidebar_oracle.geometry import machine as base_machine, put32

def machine():
    u=base_machine(); u.mem_map(0x21000000,0x40000); return u
n=SimpleNamespace(machine=machine,HEAP=0x21000000,STACK=STACK_BASE,STOP=RET_MAGIC,put32=put32,EXPECTED=NATIVE_SHA256)


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


def collect():
    u = n.machine()
    owner, scenario = n.HEAP, n.HEAP + 0x10000
    surface, vtable, shape, convert = [n.HEAP + v for v in (0x20000, 0x21000, 0x22000, 0x23000)]
    callback = n.STOP + 0x100
    for address, value in ((0xa8b230, scenario), (0x887300, surface),
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
        'executable_sha256': n.EXPECTED,
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


if __name__ == '__main__':
    finish_vectors(collect,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Original63FB20 command loop:1215 staged count/flash/side/height cases plus8 dirty/visible/force cases.',
        assumptions=['Explicit staged counters; no target or timing producer equivalence claimed.',
          'Sidebar surface bounds supplied as0,0,168,600; sidebar bodyY158.'],
        substitutions=['Surface virtual+78 returns declared bounds;4AED70 records actual commands and returns with original stack cleanup, poisoning volatile outputs.'],
        entry_points={'power_draw':0x63FB20,'draw_boundary':0x4AED70}))
