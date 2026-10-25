"""Original Drive/Ship head-coordinate expressions with native direction startup.

Fresh selection adds a direction to the current Foot XYZ; the second-node and
chain expressions add a direction to their supplied previous coordinate. These
are interior coordinate-producing blocks, not complete movement admission or
final retained-field publication. No expression is reimplemented in Python.

Four controls separately execute the original Cell/Lepton compass initializers
through their true RETs, observing initialized bytes and untouched boundaries.
"""
from pathlib import Path
import hashlib
import struct

import capstone
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_ESP,
    UC_X86_REG_FPCW,
)
from tools import native_inspect
from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE,
    RET_MAGIC, IMAGE_BASE, IMAGE_SIZE, image_bytes, file_span,
    finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords


DIRECTION_INIT, DIRECTION_TABLE = 0x49F3A0, 0x89F6D8
INITIALIZER_CONTROLS = (
    ('cell', 0x49F2F0, 0x49F39B, 0x89F688, 32, '<hh'),
    ('lepton', DIRECTION_INIT, 0x49F413, DIRECTION_TABLE, 64, '<ii'),
)
FOOT, LOCO, PRIOR = SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x3000
BLOCKS = {
    ('drive', 'fresh'): (0x4B32AF, 0x4B32F0, 0x38),
    ('ship', 'fresh'): (0x6A28FF, 0x6A293F, 0x38),
    ('drive', 'second_node'): (0x4B40B0, 0x4B40D9, 0x40),
    ('ship', 'second_node'): (0x6A36E0, 0x6A3705, 0x40),
    ('drive', 'chain'): (0x4B1BC4, 0x4B1BF4, 0x20),
    ('ship', 'chain'): (0x6A120A, 0x6A123A, 0x20),
}


class OriginalCoordinates:
    def __init__(self):
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, SCRATCH_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        self.sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(self.sp, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, self.sp)
        run_checked(u, DIRECTION_INIT, RET_MAGIC, count=100,
                    required_addresses=[DIRECTION_INIT, 0x49F413])
        assert u.reg_read(UC_X86_REG_ESP) == self.sp + 4

    def coord(self, address):
        return list(struct.unpack('<iii', self.uc.mem_read(address, 12)))

    def directions(self):
        return [list(struct.unpack('<ii', self.uc.mem_read(DIRECTION_TABLE + 8*i, 8)))
                for i in range(8)]

    def initializer_control(self, kind, entry, ret, table, size, encoding, fpcw):
        # The existing fixture's original Lepton startup precedes each control.
        # Only the selected table is poisoned; original code/calls are retained.
        u, sp = self.uc, self.sp
        ctor_fpcw = u.reg_read(UC_X86_REG_FPCW)
        assert ctor_fpcw == 0
        if fpcw != ctor_fpcw:
            u.reg_write(UC_X86_REG_FPCW, fpcw)
        spans = native_inspect.selected_ranges(
            image_bytes(), entry, ret - entry + 1, code_only=False)
        decoded = native_inspect.decode_ranges(
            spans, lambda ins: [native_inspect.instruction_row(ins)])
        assert len(decoded['coverage']) == 1
        assert decoded['coverage'][0]['undecoded_bytes'] == 0
        instructions = decoded['matches']
        assert instructions[0]['address'] == entry
        assert instructions[-1]['address'] == ret
        assert instructions[-1]['mnemonic'] == 'ret'
        required = [row['address'] for row in instructions]
        code = spans[0]['bytes']
        assert bytes(u.mem_read(entry, len(code))) == code
        pristine_table = bytes(u.mem_read(table, size))
        prefix = bytes(u.mem_read(table - 16, 16))
        tail = bytes(u.mem_read(table + size, 16))
        u.mem_write(table, bytes([0xA5]) * size)
        u.mem_write(sp, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, sp)
        before = bytes(u.mem_read(table, size))
        before_image = bytes(u.mem_read(IMAGE_BASE, IMAGE_SIZE))
        before_fpcw = u.reg_read(UC_X86_REG_FPCW)
        # Preserve OriginalCoordinates' own 100-instruction/10-second boundary.
        exit_pc = run_checked(u, entry, RET_MAGIC, count=100,
                              timeout_us=10_000_000, required_addresses=required)
        after = bytes(u.mem_read(table, size))
        after_image = bytes(u.mem_read(IMAGE_BASE, IMAGE_SIZE))
        after_fpcw = u.reg_read(UC_X86_REG_FPCW)
        after_sp = u.reg_read(UC_X86_REG_ESP)
        assert exit_pc == RET_MAGIC and after_sp == sp + 4
        assert before_fpcw == after_fpcw == fpcw
        assert bytes(u.mem_read(entry, len(code))) == code
        prefix_after = bytes(u.mem_read(table - 16, 16))
        tail_after = bytes(u.mem_read(table + size, 16))
        assert prefix_after == prefix and tail_after == tail
        offset = table - IMAGE_BASE
        before_rest = hashlib.sha256(
            before_image[:offset] + bytes(size) + before_image[offset + size:]).hexdigest()
        after_rest = hashlib.sha256(
            after_image[:offset] + bytes(size) + after_image[offset + size:]).hexdigest()
        assert before_rest == after_rest
        stride = struct.calcsize(encoding)
        values = [list(struct.unpack(encoding, after[i * stride:(i + 1) * stride]))
                  for i in range(8)]
        _, original_text = file_span(image_bytes(), 0x401000, 0x3E0000)
        text_before = hashlib.sha256(before_image[0x1000:0x3E1000]).hexdigest()
        text_after = hashlib.sha256(after_image[0x1000:0x3E1000]).hexdigest()
        assert text_before == text_after == hashlib.sha256(original_text).hexdigest()
        return dict(
            kind=kind, entry=entry, ret_instruction=ret, checked_exit=exit_pc,
            table_address=table, table_bytes=size, stride_bytes=stride,
            encoding=encoding, values=values, supplied_before_hex=before.hex(),
            initialized_packed_hex=after.hex(),
            original_table_before_poison_hex=pristine_table.hex(),
            constructor_fpcw=f'0x{ctor_fpcw:04X}', fpcw_before=f'0x{before_fpcw:04X}',
            fpcw_after=f'0x{after_fpcw:04X}', esp_before=sp, esp_after=after_sp,
            stack_cleanup_bytes=after_sp - sp, instruction_count_required=len(required),
            required_addresses=required, original_instructions=instructions,
            original_code_bytes=code.hex(),
            original_code_sha256=hashlib.sha256(code).hexdigest(),
            original_file_offset=spans[0]['file_offset'],
            prefix_unwritten=dict(address=table - 16, bytes=16,
                                  before_hex=prefix.hex(), after_hex=prefix_after.hex()),
            tail_unwritten=dict(address=table + size, bytes=16,
                                before_hex=tail.hex(), after_hex=tail_after.hex()),
            first_unwritten_pair=dict(
                index=8, address=table + size, bytes=stride,
                before_hex=tail[:stride].hex(), after_hex=tail_after[:stride].hex(),
                before_value=list(struct.unpack(encoding, tail[:stride])),
                after_value=list(struct.unpack(encoding, tail_after[:stride])),
                meaning='unwritten memory, not a ninth compass entry'),
            original_text_sha256=dict(before=text_before, after=text_after),
            count_budget=100, timeout_us=10_000_000,
        )

    def evaluate(self, case):
        u, sp = self.uc, self.sp
        family, operation = case['family'], case['operation']
        base, current, direction = case['base'], case['current'], case['direction']
        start, end, output = BLOCKS[family, operation]
        for register in (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
                         UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_EDI,
                         UC_X86_REG_EBP):
            u.reg_write(register, 0)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_EBP, LOCO)
        u.mem_write(sp, bytes(0x100))
        u.mem_write(LOCO + 0xC, dwords(FOOT))
        u.mem_write(FOOT + 0x9C, dwords(*current))
        u.mem_write(PRIOR, dwords(*base))
        required = [start]
        if operation == 'fresh':
            assert current == base
            u.reg_write(UC_X86_REG_EDX, FOOT + 0x9C)
            u.reg_write(UC_X86_REG_ESI, direction)
            required.append(0x41C230)  # Original XYZ copy constructor executes.
        elif operation == 'second_node':
            u.reg_write(UC_X86_REG_EBX, base[0] & 0xFFFFFFFF)
            u.mem_write(sp + 0x44, dwords(base[1], base[2]))
            if family == 'drive':
                u.reg_write(UC_X86_REG_EDX, direction)
            else:
                u.reg_write(UC_X86_REG_EAX, direction)
                u.reg_write(UC_X86_REG_EDX, base[1] & 0xFFFFFFFF)
        else:
            assert operation == 'chain'
            u.reg_write(UC_X86_REG_ESI, PRIOR)
            u.reg_write(UC_X86_REG_EDX, direction)
        run_checked(u, start, end, count=100, required_addresses=required)
        assert u.reg_read(UC_X86_REG_ESP) == sp
        assert self.coord(PRIOR) == base
        assert self.coord(FOOT + 0x9C) == current
        return self.coord(sp + output)


def inputs():
    poses = {
        'centered': [2176, 2176, 416],
        'noncentered_retained_z': [2133, 2201, 731],
        'cell_edges_negative_z': [2048, 2303, -347],
        'signed_xy': [-1, -257, -104],
        'signed_wrap': [2147483600, -2147483600, -2147483648],
        'null_source': [0, 0, 0],
    }
    for family, operation in BLOCKS:
        for name, base in poses.items():
            for direction in range(8):
                # The chain source is the retained head, even when the current
                # Foot is elsewhere/at another Z. This separate Foot memory is
                # supplied context, not a simulation of intervening movement.
                current = base if operation == 'fresh' else [2209, 2184, 104]
                yield dict(family=family, operation=operation, name=name,
                           direction=direction, base=base, current=current)


def generate():
    directions = OriginalCoordinates().directions()
    rows = []
    for case in inputs():
        original = OriginalCoordinates()
        rows.append(dict(input=case, output=original.evaluate(case)))
        assert original.directions() == directions
    controls = [OriginalCoordinates().initializer_control(*control, fpcw)
                for fpcw in (0, 0x0E7F) for control in INITIALIZER_CONTROLS]
    return dict(directions=directions, cases=rows, initializer_controls=controls)


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[2]
    source_files = (
        'tools/spatial_oracle/locomotor_head_coordinates.py',
        'tools/native_oracle.py',
        'tools/native_inspect.py',
        'tools/spatial_oracle/map_queries.py',
    )
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Cell/Lepton compass initializer controls and Drive/Ship fresh, second-node and chain coordinate-producing expressions',
        assumptions=[
            'All six interior blocks use supplied live register/stack frames; movement admission and final head/selector stores are excluded',
            'Original 49F3A0 initializer writes the direction table before each case; no direction deltas are supplied',
            'Original first-node expressions call XYZ copy constructor41C230; no code or calls are substituted',
            'Second-node source is supplied at the post-PUSH1/PUSH0 frame offsets; no intervening first-node admission is executed',
            'Chain source is the prior retained head, with distinct supplied current Foot XYZ; callbacks and complete chaining are excluded',
            'Signed/overflow/NullCoord inputs bound scalar behavior, not active stock-map reachability',
            '288 cases cover two active-retail families, three expressions, six supplied poses and all eight initialized directions',
            'Four controls compose fresh OriginalCoordinates fixtures; original Lepton startup precedes each selected Cell49F2F0 or Lepton49F3A0 initializer',
            'Only the selected eight-pair table is A5-poisoned; all decoded original instructions including true RET49F39B or RET49F413 are required',
            'Inherited0000 and supplied0E7F FPCW are retained; ESP advances exactly4 within the existing 100-instruction/10-second bound',
            'Initializer controls preserve original .text, the mapped image outside the selected table, and 16-byte table guards',
            'Index8 is observed as an unwritten memory boundary, not a ninth direction or an executed native accessor',
            'Initializer controls establish table bytes and boundaries; full CRT startup, consumer arithmetic and complete movement are outside their coverage',
        ], substitutions=[], entry_points={
            'direction_initializer': DIRECTION_INIT,
            'cell_initializer': 0x49F2F0,
            'cell_true_ret': 0x49F39B,
            'lepton_true_ret': 0x49F413,
            'coordinate_copy_constructor': 0x41C230,
            **{f'{family}_{operation}': row[0] for (family, operation), row in BLOCKS.items()},
        }) | {'capstone_version': capstone.__version__},
        source_paths={path: root / path for path in source_files})
