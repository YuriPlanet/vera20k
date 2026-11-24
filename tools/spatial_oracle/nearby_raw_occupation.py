"""Bounded original FNPC raw-plane and retained-seed queries.

Run as a module; default checks the saved outputs without writing. The complete
56DC20, rectangle56E7C0 and raw leaf4834A0 execute. Playfield=true and direct
projection are supplied read-only seams. Only zero/one-ring, single-cell queries
are covered; this does not prove the map diamond or projection implementation.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE, RET_MAGIC, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed

MAP, TABLE, DUMMY = 0x87F7E8, 0xC00000, 0xABDC50
CELL, SEED, OUTPUT = SCRATCH, SCRATCH + 0x200, SCRATCH + 0x210
OWNER, VTABLE, OWNER_GET, MARK_INPUT = [SCRATCH + i * 0x1000 for i in range(1, 5)]


def nearby_arguments(u, read32, sp):
    """Read this owner's complete15-argument FNPC boundary without executing."""
    args = [read32(sp + 4 + i * 4) for i in range(15)]
    return dict(args=args, seed=list(struct.unpack('<hh', u.mem_read(args[1], 4))),
                target=list(struct.unpack('<hh', u.mem_read(args[12], 4))))


def query(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, 0x027F)
    table = bytearray(0x40000 * 4)
    struct.pack_into('<I', table, (10 * 512 + 10) * 4, CELL)
    u.mem_write(TABLE, bytes(table))
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    u.mem_write(0x87F924, dwords(TABLE))
    u.mem_write(MAP + 0xF4, dwords(row['cap'], 0))
    u.mem_write(DUMMY, bytes(0x200))
    u.mem_write(DUMMY + 0x54, dwords(-1, -1))
    u.mem_write(DUMMY + 0x44, dwords(-1))
    u.mem_write(CELL + 0x24, packed(10, 10))
    u.mem_write(CELL + 0x44, dwords(-1))  # no overlay
    u.mem_write(CELL + 0xEC, dwords(0))  # clear land
    u.mem_write(CELL + 0x11B, bytes((row['level'] & 255, 0)))
    u.mem_write(CELL + 0x124, dwords(row['ground'], row['deck']))
    u.mem_write(CELL + 0x140, dwords(0x100 if row['structural'] else 0))
    u.mem_write(SEED, packed(*row['seed']))
    u.mem_write(0x89EA40, struct.pack('<90f', *([1.0] * 90)))
    events = []
    mark_owner = 0

    def read32(address):
        return struct.unpack('<I', u.mem_read(address, 4))[0]

    def ret(cleanup, result):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, result)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def observe(_machine, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == OWNER_GET:
            ret(0, mark_owner)
        elif address == 0x578540:
            assert read32(sp + 8) == 1
            events.append('playfield')
            ret(8, 1)
        elif address == 0x4834A0:
            events.append('raw')  # observe only; the complete body executes
        elif address == 0x6D6410:
            out, coord = read32(sp + 4), read32(sp + 8)
            x, y = struct.unpack('<ii', u.mem_read(coord, 8))
            assert x >= 0 and y >= 0
            events.append('projection')
            u.mem_write(out, packed(x // 256, y // 256))
            ret(8, out)

    u.hook_add(UC_HOOK_CODE, observe)
    if 'dummy_actions' in row:
        u.mem_write(OWNER, dwords(VTABLE))
        u.mem_write(VTABLE + 0x38, dwords(OWNER_GET))
        for address, value in [(0x89E7C0, 104), (0x89E7B4, 416), (0xA8F234, 416)]:
            u.mem_write(address, dwords(value))
        for action in row['dummy_actions']:
            mark_owner = action['owner']
            u.mem_write(MARK_INPUT, dwords(*action['coord']))
            entry = 0x5217C0 if action['put'] else 0x521850
            sp = STACK_BASE + STACK_SIZE - 0x1000
            u.mem_write(sp, dwords(RET_MAGIC, MARK_INPUT))
            u.reg_write(UC_X86_REG_ESP, sp)
            u.reg_write(UC_X86_REG_ECX, OWNER)
            run_checked(u, entry, RET_MAGIC, count=30000, required_addresses=[entry, 0x578080])
            assert u.reg_read(UC_X86_REG_ESP) == sp + 8
    args = [OUTPUT, SEED, 0, -1, 0, row['bridge_aware'], 1, 1, 0,
            row['height'], 0, 1, SEED, 0, 0]
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, *args))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, MAP)
    required = [0x56DC20]
    if row['cap']:
        required += [0x56E7C0, 0x4834A0]
    run_checked(u, 0x56DC20, RET_MAGIC, count=30000, required_addresses=required)
    assert u.reg_read(UC_X86_REG_ESP) == sp + 4 * (len(args) + 1)
    assert bytes(u.mem_read(SEED, 4)) == packed(*row['seed'])
    result = dict(cell=list(struct.unpack('<hh', u.mem_read(OUTPUT, 4))),
                  dummy=list(struct.unpack('<hh', u.mem_read(DUMMY + 0x24, 4))), events=events)
    if 'dummy_actions' in row:
        result['dummy_raw'] = [read32(DUMMY + 0x124) & 255, read32(DUMMY + 0x128) & 255]
        result['dummy_owners'] = [read32(DUMMY + 0x54), read32(DUMMY + 0x58)]
    return result


def generate():
    rows = []
    for structural, ground, deck, bridge, height in [
        (False, 4, 0, False, False), (True, 4, 0, False, False),
        (True, 0, 8, False, False), (True, 0, 0, False, True),
        (True, 0, 0, True, True),
    ]:
        row = dict(structural=structural, ground=ground, deck=deck,
                   bridge_aware=bridge, height=height, level=2, cap=1, seed=[10, 10])
        rows.append(dict(input=row, output=query(row)))
    row = dict(structural=False, ground=0, deck=0, bridge_aware=False,
               height=False, level=0, cap=0, seed=[40, 41])
    rows.append(dict(input=row, output=query(row)))
    # Different absent coordinates alias the same marked Cell. The second
    # case clears through yet another coordinate before the identical query.
    for clear in [False, True]:
        actions = [dict(coord=[40 * 256 + 192, 41 * 256 + 64, 0], put=True, owner=99)]
        if clear:
            actions.append(dict(coord=[43 * 256 + 192, 44 * 256 + 64, 0], put=False, owner=99))
        row = dict(structural=False, ground=0, deck=0, bridge_aware=False,
                   height=False, level=0, cap=1, seed=[40, 42], dummy_actions=actions)
        rows.append(dict(input=row, output=query(row)))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original FNPC/rectangle/raw leaf and two missing-cell mark/query histories; not full map, projection, or Scatter parity',
        assumptions=['Sparse original fixed-stride table allocates only Cell(10,10); dummy data starts zero except overlay and infantry owners=-1',
                     'Map size sum supplies cap0 or1; Foot speed0, zone-1, movement-zone0, footprint1x1, target=seed',
                     'No overlay, reject-overlay=false, occupant-safety=false, allow-bridge=true, extra-ring=false, final-occupancy=false',
                     'All supplied land speed floats are1.0; signed level, structural flag and both raw planes vary',
                     'Two additional rows execute Infantry5217C0/521850 with supplied owner99/height constants104 and416 before querying another missing coordinate',
                     'No whole constructor, live actor reachability or multi-ring distance coverage'],
        substitutions=['578540 returns true after verifying mode1; map geometry is outside this comparison',
                       '6D6410 returns the supplied candidate cell directly; no native projection lookup side effects are claimed',
                       'Infantry+38 supplies the explicit row owner index; both raw leaves and their map/ground calls execute original code',
                       'Events observe those two seams and entry4834A0 only; map lookup, raw/height gates and collection/selection execute original bytes'],
        entry_points={'nearby': 0x56DC20, 'rectangle': 0x56E7C0, 'raw_passability': 0x4834A0,
                      'infantry_mark': 0x5217C0, 'infantry_clear': 0x521850}))
