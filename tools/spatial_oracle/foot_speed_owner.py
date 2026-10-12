"""Original Foot speed setter and complete Drive/Ship constructor ownership.

These are supplied CPU frames, not command/gameplay or callback admission proofs.
All executed functions use original bytes, including the common base constructor.
Setter-only rows carry the requested and stored doubles as hex bits, since the
reference JSON cannot hold NaN or infinity.
"""
from pathlib import Path
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_EIP
from tools.native_oracle import finish_vectors, provenance, run_checked, RET_MAGIC, SCRATCH
from tools.spatial_oracle.locomotor_track_cursor import OriginalCursor, LOCO, FOOT, SP
from tools.spatial_oracle.map_queries import dwords


def run_setter(family, requested):
    """The setter alone on one requested double, as bits; returns the stored bits."""
    n = OriginalCursor(family)
    n.seed(-1, -1, False, 0)
    u = n.uc
    # Existing owner memory is separate from the object being constructed.
    u.mem_write(FOOT, bytes([0xA5]) * 0x800)
    u.reg_write(UC_X86_REG_ECX, FOOT)
    u.mem_write(SP, dwords(RET_MAGIC) + struct.pack('<Q', requested))
    run_checked(u, 0x4D3710, RET_MAGIC, count=40)
    assert u.reg_read(UC_X86_REG_ESP) == SP + 12
    return n, struct.unpack('<Q', u.mem_read(FOOT + 0x578, 8))[0]


def witness(family, requested):
    n, applied = run_setter(family, struct.unpack('<Q', struct.pack('<d', requested))[0])
    applied = struct.unpack('<d', struct.pack('<Q', applied))[0]
    u = n.uc
    before = bytes(u.mem_read(FOOT, 0x800))

    ctor = 0x4AF540 if family == 'drive' else 0x69EC50
    u.mem_write(LOCO, bytes([0x5A]) * 0x80)
    u.reg_write(UC_X86_REG_ECX, LOCO)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.mem_write(SP, dwords(RET_MAGIC))
    run_checked(u, ctor, RET_MAGIC, count=100, required_addresses=(0x55A6C0,))
    assert u.reg_read(UC_X86_REG_EAX) == LOCO
    assert u.reg_read(UC_X86_REG_ESP) == SP + 4
    assert bytes(u.mem_read(FOOT, 0x800)) == before
    target = struct.unpack('<d', u.mem_read(LOCO + 0x50, 8))[0]
    result = dict(applied=applied, constructor_target=target,
                  constructor_preserves_owner=True)

    if family == 'drive':
        # Supply linked owner and a populated stash; END transfers it without
        # invoking the caller's release or executing a callback.
        interface, output, stash = LOCO + 0x18, SCRATCH + 0x7000, SCRATCH + 0x7100
        u.mem_write(LOCO + 0xC, dwords(FOOT))
        u.mem_write(LOCO + 0x68, dwords(stash))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.mem_write(SP, dwords(RET_MAGIC, interface, output))
        run_checked(u, 0x4AF930, RET_MAGIC, count=30, required_addresses=(0x4AF94D,))
        assert u.reg_read(UC_X86_REG_ESP) == SP + 12
        assert n.ints(output) == [stash] and n.ints(LOCO + 0x68) == [0]
        assert bytes(u.mem_read(FOOT, 0x800)) == before
        result['end_preserves_owner'] = True
    return dict(input=dict(family=family, requested=requested), output=result)


def setter_bits(requested):
    _, applied = run_setter('drive', requested)
    return dict(input=dict(family='setter', requested_bits=f'{requested:016x}'),
                output=dict(applied_bits=f'{applied:016x}'))


# Signed zeros, NaNs, infinities, denormals and the neighbours of 1.0.
SETTER_EDGE_BITS = (
    0x0000000000000000, 0x8000000000000000, 0x7FF8000000000000, 0xFFF8000000000000,
    0x7FF0000000000001, 0x7FF0000000000000, 0xFFF0000000000000, 0x0000000000000001,
    0x8000000000000001, 0x0010000000000000, 0x3FEFFFFFFFFFFFFF, 0x3FF0000000000000,
    0x3FF0000000000001,
)


def generate():
    return [witness(family, requested) for family in ('drive', 'ship')
            for requested in (-0.5, 0.0, 0.25, 0.5, 1.0, 1.25)] + [
        setter_bits(requested) for requested in SETTER_EDGE_BITS]


def metadata():
    return provenance(
        scope='Foot applied-fraction clamp, including non-finite and signed-zero requests, and preservation across complete Drive/Ship construction and successful Drive END',
        assumptions=[
            'Supplied disjoint owner/class memory, finite exactly representable fractions (setter-only rows: any double) and startup x87 state',
            'Constructor global frame/null values use mapped image state; their runtime producers are excluded',
            'END uses supplied owner-link/stash; caller release, active-slot replacement and callback admission are excluded',
        ], substitutions=['No code patches, hooks returning call results, or omitted constructor callees'],
        entry_points={'setter': 0x4D3710, 'drive_constructor': 0x4AF540,
                      'ship_constructor': 0x69EC50, 'common_constructor': 0x55A6C0,
                      'drive_end': 0x4AF930})


def walk_arrival_speed(path_head, requested):
    """Original final-head speed writer immediately before Walk PerCell(2).

    The existing setter fixture supplies mapped original code and isolated owner/
    locomotor memory. The Infantry vtable remains original. Walk path retirement,
    position/occupation, PerCell itself, boarding and complete AI are not run.
    """
    n, before = run_setter('drive', requested)
    u = n.uc
    u.mem_write(FOOT, dwords(0x7EB058))
    assert bytes(u.mem_read(0x7EB058 + 0x544, 4)) == dwords(0x4D3710)
    u.mem_write(FOOT + 0x5E0, dwords(path_head))
    u.mem_write(LOCO + 0xC, dwords(FOOT))
    u.reg_write(UC_X86_REG_EBP, LOCO)
    u.reg_write(UC_X86_REG_ECX, FOOT)
    u.reg_write(UC_X86_REG_ESP, SP)
    trace, writes = [], []
    def visit(_u, pc, _size, _data):
        trace.append(f'0x{pc:08X}')
        if pc in (0x65C780, 0x65C7E0):
            raise AssertionError('Walk arrival speed gate consumes no RNG')
    def written(_u, _access, address, size, value, _data):
        if address < FOOT + 0x580 and FOOT + 0x578 < address + size:
            writes.append(dict(pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',
                               offset=address - FOOT, size=size, value=value))
    u.hook_add(UC_HOOK_CODE, visit)
    u.hook_add(UC_HOOK_MEM_WRITE, written)
    code_before = bytes(u.mem_read(0x75BE20, 0x22))
    setter_before = bytes(u.mem_read(0x4D3710, 0x69))
    run_checked(u, 0x75BE20, 0x75BE3C, count=100,
                required_addresses=(0x75BE20, 0x75BE35, 0x75BE38))
    assert u.reg_read(UC_X86_REG_ESP) == SP - 4
    assert struct.unpack('<I', u.mem_read(SP - 4, 4))[0] == 2
    assert code_before == bytes(u.mem_read(0x75BE20, 0x22))
    assert setter_before == bytes(u.mem_read(0x4D3710, 0x69))
    after = struct.unpack('<Q', u.mem_read(FOOT + 0x578, 8))[0]
    return dict(input=dict(path_head=path_head, requested_bits=f'{requested:016x}'),
                output=dict(before_bits=f'{before:016x}', after_bits=f'{after:016x}',
                            setter_called='0x004D3710' in trace,
                            per_cell_reason=2, stop_before='0x0075BE3C',
                            original_code_unchanged=True, trace=trace, writes=writes))


def walk_arrival_cases():
    return [walk_arrival_speed(head, bits)
            for head in (-1, 0, 2, -2)
            for bits in (0x0000000000000000, 0x3FE0000000000000,
                         0x3FF0000000000000)]


def walk_arrival_metadata():
    return provenance(
        scope='Walk final-head speed fraction gate before PerCell(2), original '
              '75BE20..75BE3C and actual Infantry vtable +544 -> Foot4D3710',
        assumptions=[
            'OriginalCursor supplies mapped original image and isolated owner/locomotor '
            'memory; no Drive or Walk construction is claimed for this fragment.',
            'The original Foot setter first establishes each prior fraction. '
            'Path-head DWORD +5E0 is supplied as -1, 0, 2 or -2; no pathfinding '
            'or head-retirement producer runs.',
            'Stops immediately before the real PerCell CALL, after the reason2 '
            'argument is pushed. No PerCell, boarding, full movement or AI executes.',
        ],
        substitutions=['Prepared caller registers and isolated object state only; '
                       'original instructions/vtables and setter return are unchanged.'],
        entry_points={'walk_arrival_gate': 0x75BE20,
                      'walk_speed_call': 0x75BE2F,
                      'foot_fraction_setter': 0x4D3710,
                      'per_cell_call': 0x75BE3C})


if __name__ == '__main__':
    args = sys.argv[1:]
    if '--walk-arrival' in args:
        args.remove('--walk-arrival')
        finish_vectors(walk_arrival_cases,
                       Path(__file__).with_name('foot_speed_owner.walk_arrival.json'),
                       provenance=walk_arrival_metadata, argv=args)
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'),
                       provenance=metadata, argv=args)
