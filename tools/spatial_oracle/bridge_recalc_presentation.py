"""Resident bridge TMP sources, original Recalc and independent file consumers.

Synthetic byte fixtures isolate ownership and selector arithmetic. A separate
NewUrban Wood fixture uses retail bytes, including its invalid damaged radar
pointers; this asset boundary is not claimed reachable in authored stock maps.
"""
from pathlib import Path
import hashlib
import json
import os
import struct
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import *
from tools import native_oracle as native
from tools.rmg_oracle.gen_rng_vectors import seeded_struct, draws, STRUCT_LEN
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, RET_MAGIC, OracleError, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.terrain_recalc import LAT_GLOBALS

DATA = 0x44000000
H, CELL, TABLE = DATA + 0x1000, DATA + 0x8000, DATA + 0x6000
MAP, ALLOC, ZONES, LEVELS, RULES = 0x87F7E8, DATA + 0x100000, DATA + 0x300000, DATA + 0x400000, DATA + 0x9000
PROCESS_TABLE = [(x + 3 * y) & 7 for y in range(8) for x in range(8)]
WHEELS = [1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.5, 1.0]


def synthetic_tmp(width, height, variant, damaged):
    count = width * height
    b = bytearray(16 + count * 4 + count * 68)
    struct.pack_into('<4I', b, 0, width, height, 8, 4)
    for sub in range(count):
        p = 16 + count * 4 + sub * 68
        struct.pack_into('<I', b, 16 + sub * 4, p)
        # First header's X/Y remain zero: direct OOB radar entries10/11 are
        # native null pointers, whereas selected tactical lookup wraps.
        struct.pack_into('<ii', b, p, 0 if sub == 0 else variant * 11 + sub,
                         0 if sub == 0 else -variant * 7 + sub)
        struct.pack_into('<I', b, p + 36, 4 if damaged else 0)
        b[p + 40:p + 43] = bytes((3 + variant, 11 if variant == 0 else 15, variant % 5))
        b[p + 43:p + 49] = bytes((20 + variant, 40 + sub, 60, 70 + variant, 80 + sub, 90))
        b[p + 52:p + 68] = bytes([1]) * 16
    return bytes(b)


def machine(files, coord, sub, flags, sentinel):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.mem_map(DATA, 0x600000)
    ptrs = []
    for variant, source in enumerate(files):
        head, ptr = H + variant * 0x400, DATA + 0x10000 + variant * 0x10000
        assert len(source) < 0x10000
        b = bytearray(source)
        w, h = struct.unpack_from('<II', b)
        for n in range(w * h):
            off = struct.unpack_from('<I', b, 16 + n * 4)[0]
            if off:
                struct.pack_into('<I', b, 16 + n * 4, ptr + off)
        u.mem_write(ptr, bytes(b))
        u.mem_write(head, dwords(0x7ECC48))
        u.mem_write(head + 0xA4, dwords(ptr))
        u.mem_write(head + 0x2C8, dwords(-1))
        u.mem_write(head + 0x2D4, dwords(-1))
        u.mem_write(head + 0x2BC, dwords(H + ((variant + 1) % len(files)) * 0x400))
        u.mem_write(head + 0x2F0, dwords(len(files)))
        ptrs.append(ptr)
    u.mem_write(0xA8ED2C, dwords(TABLE))
    u.mem_write(0xA8ED38, dwords(101))
    u.mem_write(TABLE + 100 * 4, dwords(H))
    u.mem_write(0x89E7C7, b'\x01')
    u.mem_write(0x89E620, dwords(*PROCESS_TABLE))
    u.mem_write(CELL + 0x24, packed(*coord))
    u.mem_write(CELL + 0x38, dwords(0xffff if sentinel else 100))
    u.mem_write(0xAA10B0, dwords(100))
    u.mem_write(CELL + 0x44, dwords(-1))
    u.mem_write(CELL + 0x11A, bytes((sub, 6, 0, 0)))
    u.mem_write(CELL + 0x140, dwords(flags))
    return u, ptrs


def call_member(u, entry, receiver, args=(), required=()):
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, *args))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, receiver)
    run_checked(u, entry, RET_MAGIC, count=30000, required_addresses=required)


def probe(files, coord, sub, flags, sentinel=False):
    u, ptrs = machine(files, coord, sub, flags, sentinel)
    call_member(u, 0x5471F0, H, (sub,))
    gate = bool(u.reg_read(UC_X86_REG_EAX) & 0xff)
    u.reg_write(UC_X86_REG_ESI, CELL)
    u.reg_write(UC_X86_REG_EDI, H)
    u.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    stop = run_checked(u, 0x47C24A, (0x47C2E5, 0x47C391), count=30000)
    p = u.reg_read(UC_X86_REG_ECX)
    selected = ptrs.index(u.reg_read(UC_X86_REG_EDI))
    radar = {'branch': 'gray' if stop == 0x47C391 else 'color'}
    if stop == 0x47C2E5:
        try:
            radar['rgb'] = list(u.mem_read(p + 0x2B, 6))
        except Exception:
            # Resume this same original state, proving the invalid pointer is
            # consumed by the original RGB read rather than only inspecting it.
            try:
                run_checked(u, stop, 0x47C2F0, count=10)
                raise AssertionError('unmapped native radar pointer did not fault')
            except OracleError:
                radar = {'branch': 'unmapped', 'pointer': p, 'fault_eip': u.reg_read(UC_X86_REG_EIP)}
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_EDI, ptrs[selected])
    u.mem_write(sp + 0x34, dwords(0 if sentinel else sub))
    run_checked(u, 0x547F9D, 0x547FB4, count=30, required_addresses=(0x547FA8, 0x547FAA))
    tactical = struct.unpack('<I', u.mem_read(sp + 0x30, 4))[0]
    offset = list(struct.unpack('<ii', u.mem_read(tactical, 8))) if tactical else None
    tactical_rgb = list(u.mem_read(tactical + 0x2B, 6)) if tactical else None
    row = dict(coord=list(coord), sub=sub, flags=flags, gate=gate,
               input_tile=0xffff if sentinel else 100,
               selected=selected, radar=radar, tactical_raw_origin=offset, tactical_rgb=tactical_rgb)
    if coord == (16, 16):
        u.mem_write(MAP + 0x13C, dwords(ALLOC, 0x40000))
        u.mem_write(ALLOC + (16 * 512 + 16) * 4, dwords(CELL))
        u.mem_write(MAP + 0xF4, dwords(16, 16, 0, 0, 16, 16))
        u.mem_write(MAP + 0x68, dwords(ZONES, 33 * 33, LEVELS))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(RULES + 0x664, b'\0')
        for land, wheel in enumerate(WHEELS):
            u.mem_write(0x89EA48 + 36 * land, struct.pack('<f', wheel))
        for address in LAT_GLOBALS:
            u.mem_write(address, dwords(-1))
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        call_member(u, 0x47D2B0, CELL, (0xffffffff,), (0x483C80,))
        b = u.mem_read(CELL, 0x144)
        row['recalc'] = dict(tile=struct.unpack_from('<i', b, 0x38)[0],
            sub=b[0x11A], level=b[0x11B], slope=b[0x11C],
            pixel_height=struct.unpack_from('<b', b, 0x11D)[0],
            land=struct.unpack_from('<i', b, 0xEC)[0], zone=struct.unpack_from('<i', b, 0x4C)[0])
    return row


LOAD_MAIN, LOAD_SCENARIO, LOAD_MAPGEN = 0x886B88, DATA + 0x5000, 0xABE890
SELECTOR_TABLE, SELECTOR_FLAG = 0x89E620, 0x89E7C7


def u32(u, at):
    return struct.unpack('<I', u.mem_read(at, 4))[0]


def load_rng_states(u):
    return {name: bytes(u.mem_read(at, STRUCT_LEN)).hex()
            for name, at in [('main', LOAD_MAIN), ('scenario', LOAD_SCENARIO), ('mapgen', LOAD_MAPGEN)]}


def load_selector_case(spec):
    files = [synthetic_tmp(1, 1, i, spec['damaged_header']) for i in range(spec['files'])]
    u, file_pointers = machine(files, (1, 2), 0, spec['cell_flags'], spec['sentinel'])
    before_main = draws(seeded_struct(spec['seed']), spec['advance'])[1]
    u.mem_write(LOAD_MAIN, before_main)
    u.mem_write(LOAD_SCENARIO, seeded_struct(1234))
    u.mem_write(LOAD_MAPGEN, seeded_struct(0x10203))
    u.mem_write(SELECTOR_FLAG, bytes([int(spec['warm'])]))
    # With flag0 these supplied bytes must be overwritten by actual original
    # initialization. With flag1 the existing process table must be retained.
    supplied_table = PROCESS_TABLE if spec['warm'] else [0xA5A5A5A5] * 64
    u.mem_write(SELECTOR_TABLE, dwords(*supplied_table))
    for i in range(spec['files']):
        head = H + i * 0x400
        # The native loader's DL=1 file-management facts are supplied here.
        u.mem_write(head + 0x2F4, b'\x01')
        u.mem_write(head + 0x2F5, f'fixture{i}.tmp\0'.encode())
        if i == spec['files'] - 1:
            u.mem_write(head + 0x2BC, dwords(0))
    # Actual native iterator starts (1,width), then(2,1) for supplied width2.
    # A following empty pointer bounds this synthetic loaded-cell plane.
    cells = ALLOC
    u.mem_write(MAP + 0xF4, dwords(2))
    u.mem_write(MAP + 0x13C, dwords(cells))
    u.mem_write(cells + (2 * 512 + 1) * 4, dwords(CELL))
    fixture_cells = [CELL]
    if spec.get('two_cells'):
        second = CELL + 0x400
        u.mem_write(second, bytes(u.mem_read(CELL, 0x200)))
        u.mem_write(second + 0x24, packed(2, 1))
        u.mem_write(cells + (1 * 512 + 2) * 4, dwords(second))
        fixture_cells.append(second)
    cell_before = [bytes(u.mem_read(at, 0x200)).hex() for at in fixture_cells]
    entries, raw_draws, visited_cells, selected_files, freed_files = [], [], [], [], []
    code = [(native.IMAGE_BASE + rva, bytes(u.mem_read(native.IMAGE_BASE + rva, size)))
            for rva, _, size, _, flags in native._sections(native.image_bytes())
            if flags & 0x20000000 and size]

    def observe(_u, pc, _size, _data):
        if pc in (0x6C8C40, 0x4068E0, 0x7C8B3D):
            if pc == 0x7C8B3D:
                pointer = u32(u, u.reg_read(UC_X86_REG_ESP) + 4)
                freed_files.append(file_pointers.index(pointer))
            sp = u.reg_read(UC_X86_REG_ESP)
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_ESP, sp + 4)
            u.reg_write(UC_X86_REG_EIP, u32(u, sp))
        elif pc in (0x546DA0, 0x578350, 0x578290, 0x4814F0):
            entries.append(hex(pc))
        elif pc == 0x546DEC:
            cell = u.reg_read(UC_X86_REG_EDI)
            visited_cells.append(list(struct.unpack('<hh', u.mem_read(cell + 0x24, 4))))
        elif pc == 0x546EA0:
            selected_files.append((u.reg_read(UC_X86_REG_ESI) - H) // 0x400)
        elif pc == 0x65C780:
            assert u.reg_read(UC_X86_REG_ECX) == LOAD_MAIN
        elif pc == 0x48152B:
            raw_draws.append(u.reg_read(UC_X86_REG_EAX))

    u.hook_add(UC_HOOK_CODE, observe)
    before = load_rng_states(u)
    sp = native.STACK_BASE + native.STACK_SIZE - 0x1000
    u.reg_write(UC_X86_REG_ESP, sp)
    stop = native.run_checked(u, 0x5BE653, 0x5BE65C, count=300000,
                              required_addresses=(0x5BE657, 0x546DA0, 0x578350, 0x578290, 0x546EC1, 0x54701C),
                              context=spec)
    after = load_rng_states(u)
    continuation_draws, replay = draws(before_main, len(raw_draws))
    assert continuation_draws == raw_draws
    assert replay.hex() == after['main']
    assert before['scenario'] == after['scenario'] and before['mapgen'] == after['mapgen']
    assert all(bytes(u.mem_read(at, len(data))) == data for at, data in code)
    table = list(struct.unpack('<64I', u.mem_read(SELECTOR_TABLE, 256)))
    usage = [u32(u, H + i * 0x400 + 0x308) for i in range(spec['files'])]
    assert sum(usage) == len(fixture_cells)
    cell_after = [bytes(u.mem_read(at, 0x200)).hex() for at in fixture_cells]
    assert cell_before == cell_after
    file_present = [u32(u, H + i * 0x400 + 0xA4) != 0 for i in range(spec['files'])]
    assert file_present == [count != 0 for count in usage]
    assert u.reg_read(UC_X86_REG_ESP) == sp
    return dict(input=spec, entries=entries, raw_draws=raw_draws, raw_draw_count=len(raw_draws),
                before=before, after=after, flag_after=u.mem_read(SELECTOR_FLAG, 1)[0],
                table_before=supplied_table, table_after=table, usage_counts=usage,
                selected=selected_files[0], selected_files=selected_files,
                visited_cells=visited_cells, freed_files=freed_files, file_present=file_present,
                cell_before=cell_before, cell_after=cell_after,
                stop=hex(stop), original_executable_bytes_unchanged=True)


def load_selector_cases():
    cases = []
    for name, files, warm, damaged, flags, sentinel, seed, advance in (
        ('cold_fixed_count2_seed0', 2, False, False, 0, False, 0, 0),
        ('cold_table_count8_seed0', 8, False, False, 0, False, 0, 0),
        ('cold_table_count8_advanced', 8, False, False, 0, False, 1234, 7),
        ('warm_table_retains_other_main', 8, True, False, 0, False, 1234, 7),
        ('cold_single_valid_skips', 1, False, False, 0, False, 0, 0),
        ('cold_damaged_header_skips', 8, False, True, 0x2000, False, 0, 0),
        ('cold_clear_sentinel_count1_initializes', 1, False, False, 0, True, 0, 0),
    ):
        cases.append(load_selector_case(dict(name=name, files=files, warm=warm, damaged_header=damaged,
                                  cell_flags=flags, sentinel=sentinel, seed=seed, advance=advance)))
    cases.append(load_selector_case(dict(name='cold_two_cell_order', files=8, warm=False,
                              damaged_header=False, cell_flags=0, sentinel=False,
                              seed=0, advance=0, two_cells=True)))
    return cases



def cases():
    groups = []
    for name, count, damaged in [('ordinary2', 2, False), ('damaged2', 2, True),
                                  ('ordinary8', 8, False), ('single_damaged', 1, True)]:
        files = [synthetic_tmp(5 if n == 1 else 3, 2 if n == 1 else 5, n, damaged if n == 0 else False)
                 for n in range(count)]
        rows = [probe(files, (16, 16), sub, flags) for sub in range(15) for flags in (0, 0x2000)]
        rows += [probe(files, coord, sub, 0) for coord in [(-32768, -32768), (-32768, 0), (511, 511)] for sub in (1, 4)]
        if name == 'damaged2':
            rows += [probe(files, (4, 4), 1, flags, sentinel=True) for flags in (0, 0x2000)]
        groups.append(dict(name=name, files_hex=[b.hex() for b in files], cases=rows))
    directory = Path(os.environ.get('VERA20K_BRIDGE_REPAIR_ASSETS', str(ROOT / '.local/bridge-repair-assets/extract')))
    names = ['Ovrpsb03.ubn', 'Ovrpsb03a.ubn']
    files = [(directory / name).read_bytes() for name in names]
    groups.append(dict(name='retail_ubn_wood03',
        files=[dict(name=name, sha256=hashlib.sha256(b).hexdigest()) for name, b in zip(names, files)],
        cases=[probe(files, (16, 16), sub, flags) for sub in range(15) for flags in (0, 0x2000)]))
    return dict(process_table=PROCESS_TABLE, wheel_values=WHEELS, groups=groups,
                load_selector_cases=load_selector_cases())


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: dict(provenance(
        scope='Original pristine Recalc plus independent radar file/pointer and tactical selected-entry lookup; synthetic sources and NewUrban Wood03 retail bytes; eight original saved-MouseLoad tile scan/cull controls with cold/warm process selector',
        assumptions=[
            'Registered tile100, supplied relocated TMP heads and variant chain; original TMP loader excluded',
            'Historical presentation groups supply initialized process selector table; added load_selector_cases separately execute cold/warm initialization and Main continuation',
            'Recalc cases use real16,16 in16x16 map, level6, no overlay/objects, CliffBack0 and supplied Wheel/LAT policies',
            'Radar runs original47C24A prefix,5471F0,4814F0,544E00; no brightness/minimap composition claim',
            'Tactical runs original547F9D selected-template division/table fetch; raw RGB identifies selected entry, stored origin is not decoded canvas offset; no full drawing/GPU claim',
            'Two sentinelFFFF cases retain Cell subtile1 for radar/coordinate selection and use tactical subtile0; supplied ClearTile100 has damaged gate true',
            'Retail NewUrban Wood damaged OOB pointers fault in original RGB read; no authored retail-map reachability claim',
            'Load controls reuse resident TMP fixture and one/two supplied restored cells; original5BE653 clearsCL/DL and calls full546DA0, returning5BE65C after real iterator, variant selection, use counts and file culling',
            'Load controls supply type+2F4 and filenames with all TMPs initially resident; used files need no547020 reload. Cell fixture0x200 bytes are bracketed unchanged; no native SAV/complete theater registry or full resource IO claim',
            'Incoming Main is native Seed/Next; actual draw order is independently replayed. Scenario/MapGen are separately bracketed. Cold flag0 uses poisoned table; warm flag1 uses declared retained process table',
        ], substitutions=[
            'Load controls only:6C8C40 clock0,4068E0 diagnostic return and7C8B3D free transport recording supplied file pointers; native cull clears type+A4/+A8 itself',
        ], entry_points={'recalc':0x47D2B0, 'radar_prefix':0x47C24A,
            'damaged_gate':0x5471F0, 'selector':0x4814F0, 'file_chain':0x544E00, 'tactical_entry':0x547F9D,
            'mouse_load_call':0x5BE653, 'tile_scan_cull':0x546DA0,
            'iterator_reset':0x578350, 'iterator_next':0x578290,
            'main_next':0x65C780, 'return_to_mouse_load':0x5BE65C}), command='python -m tools.spatial_oracle.bridge_recalc_presentation --check'),
        source_paths={'producer':Path(__file__), 'native':Path(native.__file__),
                      'rng_owner':Path(__file__).parents[1]/'rmg_oracle/gen_rng_vectors.py',
                      'map_fixture':Path(__file__).with_name('map_queries.py'),
                      'lat_fixture':Path(__file__).with_name('terrain_recalc.py')})
