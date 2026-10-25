"""Original wait-bar timing and paint controls for the shared LAN oracle.

Run with ``python -m tools.storage_oracle.lan_lobby --wait-bars --write``.
Whole64A7B0 supplies lag, unsigned scaling, color branches and paint geometry;
there is no Python timing/color/width model. Only window geometry, raw clock
returns and terminal surface operations are supplied through existing owners.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_EDX, UC_X86_REG_ESP

from tools.native_oracle import OracleError, STACK_BASE, STACK_SIZE, provenance
from tools.storage_oracle.shell_relayout import Machine, Image, PARENT
from tools.storage_oracle.saved_game_layout import code_guard, check_code


SOURCE_PATHS = {
    "wait_bars": Path(__file__),
    "machine": Path(__file__).with_name("shell_relayout.py"),
    "guards": Path(__file__).with_name("saved_game_layout.py"),
    "native_oracle": Path(__file__).parent.parent / "native_oracle.py",
}


class WaitBarMachine(Machine):
    def __init__(self):
        super().__init__(0xEA, 800, 600)
        self.row = 0
        self.numeric = {}
        self.paints = []
        self.clock_calls = []
        self.now_ms = 0
        # Preserve actual GetDlgItem and manager lookup, with two source nodes
        # and one admitted private connection (local node has no connection).
        nodes = self.alloc(8)
        for i, house in enumerate((0, 7)):
            node = self.alloc(0x85)
            self.w32(node + 0x6F, house)
            self.w32(nodes + i * 4, node)
        self.w32(0xA8DA78, nodes)
        self.w32(0xA8DA84, 2)
        self.w32(0xA8B238, 3)
        peer = self.alloc(0xB8)
        self.w32(peer + 0x64, 7)
        self.w32(0xA8E9C0 + 0x28, peer)
        self.w32(0xA8E9C0 + 0x44, 1)
        for control in (0x648, 0x649):
            window = next(h for h in self.children if self.windows[h]['id'] == control)
            self.windows[window]['rect'] = [10, 20, 210, 40]
        # Explicit RGB565 presentation format, unrelated to timing arithmetic.
        for address, value in ((0x8A0DD0, 11), (0x8A0DD4, 3), (0x8A0DD8, 0),
                               (0x8A0DDC, 3), (0x8A0DE0, 5), (0x8A0DE4, 2)):
            self.w32(address, value)
        surface = self.alloc(0x40)
        vtable = self.alloc(0x80)
        self.w32(surface, vtable)
        self.w32(vtable + 0x78, self.stub(self.surface_rect))
        self.w32(vtable + 0x54, self.stub(self.draw_outline))
        self.w32(vtable + 0x10, self.stub(self.fill_rect))
        self.w32(0x887310, surface)
        if Image(self.uc).imports().get(0x7E1530) != 'timeGetTime':
            raise OracleError('unexpected native time import')
        self.w32(0x7E1530, self.stub(self.time_get_time))
        self.uc.hook_add(UC_HOOK_CODE, self.observe)
        self.guard = code_guard(self.uc)

    def rect(self, address):
        return list(struct.unpack('<4i', self.uc.mem_read(address, 16)))

    def surface_rect(self):
        out = self.arg(0)
        self.uc.mem_write(out, struct.pack('<4i', 0, 0, 800, 600))
        self.ret(4, out)

    def draw_outline(self):
        self.paints.append(dict(row=self.row, operation='outline', clip=self.rect(self.arg(0)),
                                rect=self.rect(self.arg(1)), color=self.arg(2)))
        self.ret(12, 1)

    def fill_rect(self):
        self.paints.append(dict(row=self.row, operation='fill', clip=self.rect(self.arg(0)),
                                rect=self.rect(self.arg(1)), color=self.arg(2)))
        self.ret(12, 1)

    def time_get_time(self):
        self.clock_calls.append(self.now_ms)
        self.ret(0, self.now_ms)

    def observe(self, uc, pc, _size, _data):
        if pc == 0x64A7CE:
            self.row = uc.reg_read(UC_X86_REG_EBX)
            self.numeric[self.row] = {'branch_color': 'green'}
        row = self.numeric.get(self.row)
        if row is None:
            return
        if pc == 0x64A871:
            row['lag'] = uc.reg_read(UC_X86_REG_EBP)
        elif pc == 0x64A8BA:
            row['branch_color'] = 'yellow'
        elif pc == 0x64A8DF:
            row['branch_color'] = 'red'
        elif pc == 0x64A922:
            row['scaled_millis_wrapped'] = uc.reg_read(UC_X86_REG_EDX)
        elif pc == 0x64A927:
            row['age_ticks'] = uc.reg_read(UC_X86_REG_EDX)
        elif pc == 0x64A92F:
            row['branch_color'] = 'blue'
        elif pc == 0x64A972:
            row['effective_lag'] = uc.reg_read(UC_X86_REG_EBP)
        elif pc == 0x64A993:
            row['percent_before_signed_clamp'] = uc.reg_read(UC_X86_REG_EAX)
        elif pc == 0x64A9B8:
            row['outer_fill_width'] = uc.reg_read(UC_X86_REG_EBP)

    def run_case(self, spec):
        self.numeric, self.paints, self.clock_calls = {}, [], []
        self.now_ms = spec['now_ms']
        self.w32(0xAFA418, spec['wait_elapsed'])
        self.w32(0xAFA278 + 0x0C, spec['peer_elapsed'])
        self.w32(0xAFA278 + 0x14, spec['reference_ms'])
        before = bytes(self.uc.mem_read(0xAFA278, 24))
        self.uc.mem_write(STACK_BASE + STACK_SIZE - 0x4000, b'\xA5' * 0x4000)
        Machine.call(self, 0x64A7B0, ecx=PARENT, count=100_000)
        if self.uc.reg_read(UC_X86_REG_ESP) != STACK_BASE + STACK_SIZE - 0x2000:
            raise OracleError('full bar function did not return with correct stack')
        if set(self.numeric) != {0, 1} or len(self.paints) != 4:
            raise OracleError('full two-row paint did not finish')
        if bytes(self.uc.mem_read(0xAFA278, 24)) != before:
            raise OracleError('presentation changed peer timing inputs')
        check_code(self.uc, self.guard)
        return dict(input=spec, rows=self.numeric, paint=self.paints,
                    clock_returns=self.clock_calls, original_code_unchanged=True)


def generate():
    def case(name, wait_elapsed=481, peer_elapsed=0, now_ms=7000, reference_ms=0):
        return dict(name=name, wait_elapsed=wait_elapsed, peer_elapsed=peer_elapsed,
                    now_ms=now_ms, reference_ms=reference_ms)
    specs = [
        case('green_upper', 240), case('yellow_lower', 241), case('yellow_upper', 480),
        case('blue_just_below_threshold', now_ms=6999),
        case('red_at_threshold', now_ms=7000), case('red_above_threshold', now_ms=7017),
        case('lag_wrap_to_small', 5, 0xFFFFFFFD),
        case('lag_wrap_to_max', 0, 1), case('lag_high_bit_width_wrap', 0x80000000),
        case('raw_clock_wrap_small_delta', now_ms=32, reference_ms=0xFFFFFFF0),
        case('raw_clock_high_bit', now_ms=0x80000000),
        case('raw_reference_high_bit', now_ms=0, reference_ms=0x80000000),
        case('raw_clock_max', now_ms=0xFFFFFFFF),
        case('raw_negative_delta', now_ms=16, reference_ms=32),
        case('raw_delta_before_times60_wrap', now_ms=71582788),
        case('raw_delta_after_times60_wrap', now_ms=71582789),
        case('raw_delta_after_wrap_blue_edge', now_ms=71589788),
        case('raw_delta_after_wrap_red_edge', now_ms=71589789),
        case('high_reference_blue_edge', now_ms=0x80001B57, reference_ms=0x80000000),
        case('high_reference_red_edge', now_ms=0x80001B58, reference_ms=0x80000000),
    ]
    m = WaitBarMachine()
    return dict(source='unicorn/gamemd.exe', cases=[m.run_case(s) for s in specs])


def metadata():
    m = provenance(scope='Whole64A7B0 numeric projection and terminal paint arguments, including unsigned millisecond scaling and signed width clamp',
        assumptions=[
            'Native648881..6488AE writes one raw timeGetTime value to all peer+14 fields at wait entry. Valid FrameSync64A606..64A60D writes wall-bucket-now minus wait-entry to peer+C.648C06..648C36 snapshots waitelapsed and peer rows for the dialog.',
            'Each control supplies waitelapsedAFA418, peer+C and peer+14 at AFA278, plus raw timeGetTime return. These are explicit timing inputs; original64A7B0 computes every lag, scale, division, branch and width.',
            'Two source rows (local0/remote7) map through original541040 to one supplied admitted connection. Local row lag is natively0. Rectangles are supplied200x20 pixels, RGB565 format explicitly selected; rasterization is excluded.',
            'Original executable sections must remain unchanged. Full64A7B0 must return with correct stack, produce both rows, and leave timing input storage unchanged.',
        ], substitutions=[
            'Shared shell_relayout.Machine supplies native resource children, allocation and Win32 geometry imports. timeGetTime returns the declared raw milliseconds. Terminal surface GetRect/DrawRect/FillRect capture output; no bar arithmetic is substituted.',
        ], entry_points={'bar':0x64A7B0,'peer_lookup':0x541040,'lag_subtract':0x64A86A,
                         'scaled_subtract':0x64A920,'unsigned_divide':0x64A922,'threshold':0x64A927,
                         'blue_override':0x64A92F,'percent':0x64A972,'width':0x64A99D})
    return m
