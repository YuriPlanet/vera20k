"""Original stock-radius LightSource enable/disable dispatch, before return.

The Cell recompute and Map redraw sinks are recorded/substituted. This proves
dispatch timing/area and source gates, not pixel math or building ownership.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance,
    run_checked,
)
from tools.spatial_oracle.map_queries import dwords, packed

SOURCE, TABLE, CELLS = 0xB00000, 0xC00000, 0x3000000
WIDTH = HEIGHT = 128


def publication_observer(u, read32, valid_cell, calls, redraws, *, substitute_cell=True):
    """Observe this owner's native Cell ABI and retain its Map redraw seam.

    A composed runtime can execute real Cell light gathering by opting out of
    the historical Cell sink. It must validate each actual receiver itself.
    This helper creates no VM, source, Cell, result or expected light value.
    """
    def observe(uc, address, _size, _data):
        if address not in (0x483E30, 0x4F42F0):
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        receiver = uc.reg_read(UC_X86_REG_ECX)
        if address == 0x483E30:
            if not valid_cell(receiver):
                raise RuntimeError(f'non-fixture Cell pointer: {receiver:#x}')
            args = struct.unpack('<6I', uc.mem_read(sp + 4, 24))
            if args != (0, 65536, 0, 1000, 1000, 1000):
                raise RuntimeError(f'non-recompute Cell arguments: {args}')
            calls.append(list(struct.unpack('<HH', uc.mem_read(receiver + 0x24, 4))))
            if not substitute_cell:
                return
            cleanup = 24
        else:
            redraws.append(struct.unpack('<I', uc.mem_read(sp + 4, 4))[0])
            cleanup = 4
        destination = read32(sp)
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
        uc.reg_write(UC_X86_REG_EIP, destination)
    return observe


def stock_lamp_sequence():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 4096)
    u.mem_map(CELLS, WIDTH * HEIGHT * 0x200)
    table = bytearray(512 * 512 * 4)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            ptr = CELLS + (y * WIDTH + x) * 0x200
            struct.pack_into('<I', table, (y * 512 + x) * 4, ptr)
            u.mem_write(ptr + 0x24, packed(x, y))
    u.mem_write(TABLE, bytes(table))
    u.mem_write(0x87F924, dwords(TABLE))
    u.mem_write(SOURCE, bytes(0x80))
    u.mem_write(SOURCE + 0x34, dwords(2, 24 * 256 + 128, 24 * 256 + 128, 0, 5000))
    u.mem_write(0x829AE4, b'\x01')
    u.mem_write(0xA8EB78, dwords(2))
    # No queued work exists in this admitted standard mode-zero fixture.
    u.mem_write(0xABCA50, dwords(0))
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    calls, redraws = [], []

    observe = publication_observer(u, lambda p: struct.unpack('<I', u.mem_read(p, 4))[0],
        lambda p: CELLS <= p < CELLS + WIDTH * HEIGHT * 0x200, calls, redraws)
    u.hook_add(UC_HOOK_CODE, observe)
    events, area = [], None
    for name, entry, detail, enabled in [
        ('enable', 0x554A60, 2, 1),
        ('enable_again', 0x554A60, 2, 1),
        ('disable', 0x554A80, 2, 1),
        ('disable_again', 0x554A80, 2, 1),
        ('low_detail_enable', 0x554A60, 1, 1),
        ('low_detail_disable', 0x554A80, 1, 1),
        ('load_gate_enable', 0x554A60, 2, 0),
    ]:
        calls.clear()
        redraws.clear()
        u.mem_write(0xA8EB78, dwords(detail))
        u.mem_write(0x829AE4, bytes([enabled]))
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(sp, dwords(RET_MAGIC, 0))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, SOURCE)
        run_checked(u, entry, RET_MAGIC, count=300000)
        if area is None:
            area = list(calls)
        if calls and calls != area:
            raise RuntimeError('enable/disable affected areas differ')
        events.append(dict(name=name, active=bytes(u.mem_read(SOURCE + 0x48, 1))[0],
                           recomputes_before_return=len(calls), redraws=list(redraws),
                           queued=struct.unpack('<I', u.mem_read(0xABCA50, 4))[0]))
    return dict(width=WIDTH, height=HEIGHT, center=[24, 24], radius=5000,
                area=area, events=events)


if __name__ == '__main__':
    finish_vectors(stock_lamp_sequence, Path(__file__).with_suffix('.json'),
                   provenance=lambda: provenance(
        scope='Original mode-zero LightSource wrapper and complete area dispatch',
        assumptions=[
            'Supplied dense 128x128 Cell table; ordinary GALITE radius5000, detail2 and center24,24',
            'Source48 starts disabled; each original wrapper retains its native state for the next event',
            'Original sqrt/ftol runs with runner FPCW0x0E7F; only positive stock-radius coordinates are covered',
            'Building/radiation callsite mode-zero reachability is separately established in LIGHTSOURCE_QUEUED_MODE_CALLER_CENSUS_GHIDRA_REPORT.md',
            'No rendering, source intensity/color sampling, global profile update, allocation, power admission or persistence is emulated',
        ],
        substitutions=[
            'Cell483E30 records actual receiver and six recompute arguments, then returns without computing colors',
            'Map4F42F0 records redraw argument and returns without display traversal',
        ],
        entry_points={'enable': 0x554A60, 'disable': 0x554A80,
                      'area': 0x554AF0, 'slot_admission': 0x5657E0,
                      'cell_lookup': 0x5657A0, 'sqrt': 0x4CAC40, 'ftol': 0x7C5F00},
    ))
