"""Replay original FreeRadar parser/reset and House decision against recorded cases.

Run from the repository root: python -m tools.free_radar_oracle.oracle.
The retail executable is supplied through VERA20K_GAMEMD_EXE or RA2_DIR.
No native calls are patched or substituted. The fixture keeps all blackout
cases, although Rust acceptance currently covers only inactive radar blackout.
"""
from pathlib import Path
import hashlib
import json
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import *
from tools import native_oracle as native

HEAP, STACK, STOP = 0x1000000, 0x2000000, 0x2100000

def new_machine():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    native.load_image(u)
    u.mem_map(HEAP, 0x100000)
    u.mem_map(STACK, 0x100000)
    u.mem_map(STOP, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, native.NATIVE_FPCW)
    return u

def put32(u, address, value):
    u.mem_write(address, struct.pack('<I', value & 0xffffffff))

def call(u, address, args, ecx=0):
    sp = STACK + 0x80000
    u.mem_write(sp, struct.pack('<' + 'I' * (len(args)+1), STOP, *[a & 0xffffffff for a in args]))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, ecx)
    native.run_checked(u, address, STOP, count=15_000_000)

def block(machine, begin, end):
    machine.reg_write(UC_X86_REG_ESP, STACK + 0x80000)
    native.run_checked(machine, begin, end, count=1_000_000)


def ini_machine(token):
    machine = new_machine()
    obj, section, cached_entry, entry, text, crc = [HEAP + v for v in (0, 0x1000, 0x2000, 0x3000, 0x4000, 0x5000)]
    # Original CRC of the literal key; no hand-supplied hash value.
    call(machine, 0x4a1de0, (0x83dff8, 9), crc)
    key_hash = machine.reg_read(UC_X86_REG_EAX)
    put32(machine, obj + 4, 0x82bf9c)  # original Basic string cache identity
    put32(machine, obj + 8, section)
    put32(machine, section + 0x30, int(token is not None))
    put32(machine, section + 0x3c, cached_entry)
    put32(machine, cached_entry, key_hash)
    put32(machine, cached_entry + 4, entry)
    put32(machine, entry + 0x10, text)
    if token is not None:
        machine.mem_write(text, token.encode('ascii') + b'\0')
    return machine, obj



def collect():
    parser_cases = []
    tokens = [None, '', 'yes', 'YES', 'true', '1', 'no', 'NO', 'false', '0', 'invalid', 'yep', 'nah', '2']
    for prior in (0, 1):
        for token in tokens:
            machine, obj = ini_machine(token)
            scenario = HEAP + 0x10000
            machine.mem_write(scenario + 0x34a4, bytes([prior]))
            machine.reg_write(UC_X86_REG_ESI, scenario)
            machine.reg_write(UC_X86_REG_EDI, obj)
            block(machine, 0x68a5e3, 0x68a61a)
            actual = machine.mem_read(scenario + 0x34a4, 1)[0]
            parser_cases.append({'prior': prior, 'token': token, 'stored': actual})

    reset_machine = new_machine()
    reset_machine.reg_write(UC_X86_REG_EBP, HEAP)
    reset_machine.reg_write(UC_X86_REG_EBX, 0)  # Set_Defaults establishes EBX=0
    reset_machine.mem_write(HEAP + 0x34a4, b'\xff')
    block(reset_machine, 0x68383c, 0x683842)
    assert reset_machine.mem_read(HEAP + 0x34a4, 1) == b'\0'

    availability_cases = []
    timers = [(-1, 0, 100), (-1, 1, 100), (100, 10, 100), (100, 10, 109), (100, 10, 110), (100, 10, 111), (100, 0, 100)]
    for free_radar in (0, 1):
        for power in ((0, 0), (0, 100), (50, 100), (100, 100), (200, 100)):
            for start, duration, frame in timers:
                machine = new_machine()
                house, scenario = HEAP, HEAP + 0x20000
                put32(machine, 0xa83d4c, house)
                put32(machine, 0xa8b230, scenario)
                put32(machine, 0xa8ed84, frame)
                machine.mem_write(scenario + 0x34a4, bytes([free_radar]))
                machine.mem_write(house + 0x5779, b'\x01')
                put32(machine, house + 0x2b0, start)
                put32(machine, house + 0x2b8, duration)
                put32(machine, house + 0x53a4, power[0])
                put32(machine, house + 0x53a8, power[1])
                put32(machine, house + 0x78, 0)  # no buildings in bounded scene
                machine.reg_write(UC_X86_REG_ECX, house)
                block(machine, 0x508df0, 0x508f2f)
                actual = machine.mem_read(machine.reg_read(UC_X86_REG_ESP) + 0x10, 1)[0]
                assert machine.mem_read(house + 0x5779, 1) == b'\0'
                availability_cases.append({'free_radar': free_radar, 'power_output_drain': power,
                                           'timer_start_duration_frame': [start, duration, frame], 'available': actual})

    # Nonlocal branch completes the original body and clears only its dirty flag.
    nonlocal_machine = new_machine()
    put32(nonlocal_machine, 0xa83d4c, HEAP + 0x10000)
    nonlocal_machine.mem_write(HEAP + 0x5779, b'\x01')
    nonlocal_machine.mem_write(0x87f7e8 + 0x14d8, b'\x01')
    call(nonlocal_machine, 0x508df0, (), HEAP)
    assert nonlocal_machine.mem_read(HEAP + 0x5779, 1) == b'\0'
    assert nonlocal_machine.mem_read(0x87f7e8 + 0x14d8, 1) == b'\x01'


    return {'parser_cases': parser_cases, 'reset_stored': 0,
            'availability_cases': availability_cases,
            'nonlocal_full_body': {'recheck_cleared': True, 'radar_availability_unchanged': True}}

if __name__ == '__main__':
    path = Path(__file__).parent / 'fixtures/native-free-radar.json'
    raw = path.read_bytes()
    assert hashlib.sha256(raw).hexdigest() == 'f40f4e4befe6ba2bbcb1511ab11470cbc18323fed537bd8014d2c87039b62cb8'
    expected = json.loads(raw)
    actual = json.loads(json.dumps(collect()))
    for key, value in actual.items():
        assert value == expected[key], key
    print('PASS original FreeRadar: 28 parser cases, reset, 70 empty-provider decisions, nonlocal body; no fixture files written')
