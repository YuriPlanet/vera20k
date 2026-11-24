"""Bounded turret sound control, with original audio functions device-disabled.

Run: python -m tools.spatial_oracle.turret_rotate_sound --check (or --write).
Select VERA20K_GAMEMD_EXE explicitly. Addresses use the historical/Steam layout
accepted by the shared runner; the sidecar records the actual executed binary.
The real sound functions execute with playback disabled and an empty handle.
The type vtable getter is supplied. No accepted playback, audio RNG, whole AI,
whole save/load or constructor equivalence is claimed.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools import native_oracle as native

ACTOR = native.SCRATCH
TYPE = ACTOR + 0x2000
VTABLE = ACTOR + 0x4000
SP = native.STACK_BASE + native.STACK_SIZE - 0x1000
COORD = (2560, 2816, 104)


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def machine(turret, latch, cue):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    native.load_image(u)
    u.mem_map(native.SCRATCH, native.SCRATCH_SIZE)
    u.mem_map(native.STACK_BASE, native.STACK_SIZE)
    u.mem_map(native.RET_MAGIC, 0x1000)
    # Supplied virtual getter returns the fixture type; native text is unchanged.
    u.mem_write(native.RET_MAGIC, b'\xb8' + dwords(TYPE) + b'\xc3')
    u.mem_write(ACTOR, dwords(VTABLE))
    u.mem_write(VTABLE + 0x84, dwords(native.RET_MAGIC))
    u.mem_write(ACTOR + 0x520, dwords(TYPE))
    u.mem_write(TYPE + 0xCA1, bytes([turret]))
    u.mem_write(TYPE + 0x560, dwords(cue))
    u.mem_write(ACTOR + 0x49C, dwords(latch))
    u.mem_write(ACTOR + 0x9C, dwords(*COORD))
    u.mem_write(0x87E2A0, dwords(0))  # No initialized audio service/retained source.
    u.mem_write(0x8464AC, b'\0')  # Native PlayAt disabled-device early return.
    return u


def read32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def region(u, start, end):
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    u.reg_write(UC_X86_REG_EBP, 0)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESP, SP)
    native.run_checked(u, start, end, count=10000, required_addresses=[start])
    if u.reg_read(UC_X86_REG_ESP) != SP:
        raise native.OracleError('Slice did not restore its call-stack depth')


def history(turret, cue, initial, samples):
    u = machine(turret, initial, cue)
    calls, rows = [], []
    entries = {0x405D40: 'stop', 0x406060: 'release',
               0x7509E0: 'play', 0x750D40: 'coordinate'}

    def observe(_u, address, _size, _context):
        if address in entries:
            event = dict(call=entries[address], latch=read32(u, ACTOR + 0x49C))
            if address in (0x405D40, 0x406060):
                event['handle_offset'] = u.reg_read(UC_X86_REG_ECX) - ACTOR
            if address == 0x7509E0:
                event['cue'] = u.reg_read(UC_X86_REG_ECX)
                event['coord'] = list(struct.unpack('<3i', u.mem_read(u.reg_read(UC_X86_REG_EDX), 12)))
                event['handle_offset'] = read32(u, u.reg_read(UC_X86_REG_ESP) + 4) - ACTOR
            if address == 0x750D40:
                event['handle_offset'] = u.reg_read(UC_X86_REG_EDX) - ACTOR
                event['coord'] = list(struct.unpack('<3i', u.mem_read(u.reg_read(UC_X86_REG_ECX), 12)))
            calls.append(event)

    u.hook_add(UC_HOOK_CODE, observe)
    for sample in samples:
        calls.clear()
        u.mem_write(ACTOR + 0x4A0, dwords(sample))
        # Include the original virtual type query before the Turret gate.
        region(u, 0x6F9F9F, 0x6FA054)
        rows.append(dict(sample=sample, latch=read32(u, ACTOR + 0x49C), calls=list(calls)))
    calls.clear()
    region(u, 0x6F6C4A, 0x6F6C70)
    return dict(turret=turret, cue=cue, initial_latch=initial, visits=rows,
                limbo=dict(latch=read32(u, ACTOR + 0x49C), sample=read32(u, ACTOR + 0x4A0), calls=calls))


def samples():
    rows = []
    for turret in (False, True):
        u = machine(turret, True, 0)
        # Supplied native FacingClass prestate: ROT=1, 64-frame arc at frame 100.
        u.mem_write(ACTOR + 0x388, dwords(0x4000, 0, 100, 0, 64, 256))
        for frame in (100, 101, 163, 164, 165):
            u.mem_write(0xA8ED84, dwords(frame))
            region(u, 0x43FCC7, 0x43FCF9)
            rows.append(dict(turret=turret, frame=frame, sample=read32(u, ACTOR + 0x4A0)))
    return rows


def generate():
    return dict(coord=COORD, samples=samples(),
                  histories=[history(True, 0, True, [0, 1, 1, 0, 1]),
                             history(True, -1, True, [1, 0]),
                             history(False, 0, True, [0, 1]),
                             history(True, 0, False, [1, 1, 0, 1])])


if __name__ == '__main__':
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: native.provenance(
            scope='10 original Building sampling rows, 13 Techno turret-control visits and 4 Limbo sound-cleanup slices on the supported historical/Steam layout. Rust compares latch transitions, ordered control requests and sampling through its existing owners.',
            assumptions=[
                'The shared runner admits the historical and Steam executable identities with matching file-backed sections/layouts; this record identifies the actual executed binary. Whole Windows startup and audible output are not compared.',
                'Supplied actor/type and FacingClass prestate, frame counter, empty handles and AI EBP=0. Constructors, whole Building/Techno AI and whole save/load do not execute.',
                'Original StopAndClear, Release, PlayAt and coordinate functions execute with the audio service absent and the playback byte disabled. Successful source retention, attack/decay timing, selected samples and audible output are outside coverage.',
                'No audio RNG path is reached under this disabled-device admission. This does not establish the number or order of draws for accepted playback. Unarmed prestate characterizes control continuation, not original Load execution.',
            ],
            substitutions=['The supplied virtual type getter returns the fixture type. No original executable instruction is modified.'],
            entry_points={'building_sample': 0x43FCC7, 'is_rotating': 0x4C9480,
                'turret_control': 0x6F9F9F, 'limbo_sound_cleanup': 0x6F6C4A,
                'stop': 0x405D40, 'release': 0x406060,
                'play_at': 0x7509E0, 'coordinate': 0x750D40}),
        source_paths={'turret_rotate_sound': Path(__file__),
                      'native_oracle': Path(native.__file__)})
