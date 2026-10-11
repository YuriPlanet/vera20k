"""Original Drive Force_Track, its Ship twin, and the bunker caller's separate
speed write.

Full 4B0C40 calls execute the real map lookup, no-overlay Crate pickup return,
Apply_Track_Occupation_Mode, transform and Unit raw-mark leaves. Separate rows
explicitly supply post-crate return/state at a stopped continuation boundary.
Observation hooks never change machine state. Bunker rows begin at its already-
selected call frame; Process rows stop before the owner current-speed getter.

--bunker-install additionally executes the original installation's coordinate,
facing and selector producers before its Force_Track call. This is a bounded
two-state continuation, not full bunker admission or Drive Process execution.
"""
from pathlib import Path
import hashlib
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords, packed

LOCO, FOOT, CELLS = SCRATCH + 0x100, SCRATCH + 0x1000, SCRATCH + 0x10000
SP = STACK_BASE + STACK_SIZE - 0x1000
MAP, TABLE, DUMMY = 0x87F7E8, 0xC00000, 0xABDC50
UNIT_VTABLE = 0x7F5C70
FORCE, APPLY, RAW_PUT, SPEED_SET = 0x4B0C40, 0x4B0AD0, 0x7441B0, 0x4D3710
# ShipLocomotionClass: constructor, Force_Track, Apply_Track_Occupation_Mode,
# its null coordinate, and the Force stages that match Drive's.
SHIP_CONSTRUCTOR, SHIP_FORCE, SHIP_APPLY, SHIP_NULL = 0x69EC50, 0x6A0310, 0x6A01A0, 0xB077F8
SHIP_STAGES = {
    0x6A032D: 'selector_published_before_null_guard',
    0x6A03E4: 'head_published_before_map_query',
    SHIP_APPLY: 'apply_occupation_entry',
    0x6A040F: 'occupation_return_before_destination',
}
HEAD = [10 * 256 + 96, 10 * 256 + 160, 731]
OLD_HEAD, OLD_DESTINATION = [2304, 2560, -347], [2176, 2176, 417]


class OriginalForceTrack:
    def __init__(self, row):
        self.ship = row.get('family') == 'ship'
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, 0xA0000)
        u.mem_map(RET_MAGIC, 0x1000)
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        u.mem_write(0x822D80, dwords(0x0E7F))
        # Construct the actual Drive interfaces so the bunker call uses the
        # original vtable slot, rather than a synthetic dispatch table.
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, LOCO)
        run_checked(u, SHIP_CONSTRUCTOR if self.ship else 0x4AF540, RET_MAGIC, count=150,
                    required_addresses=(0x55A6C0,))
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        assert self.unsigned(self.unsigned(LOCO + 4) + 0x70) == (SHIP_FORCE if self.ship else FORCE)
        assert self.unsigned(UNIT_VTABLE + 0xF0) == RAW_PUT
        assert self.unsigned(UNIT_VTABLE + 0x1D0) == 0x5F5F30
        assert self.unsigned(UNIT_VTABLE + 0x544) == SPEED_SET

        u.mem_write(FOOT, dwords(UNIT_VTABLE))
        u.mem_write(FOOT + 0x674, dwords(LOCO + 4))
        u.mem_write(FOOT + 0x9C, dwords(9 * 256 + 128, 10 * 256 + 128, 312))
        u.mem_write(FOOT + 0x81, bytes((int(row.get('limbo', False)),)))
        u.mem_write(FOOT + 0x90, bytes((int(row.get('alive', True)),)))
        u.mem_write(FOOT + 0x578, struct.pack('<d', row.get('applied', 0.25)))
        u.mem_write(FOOT + 0x580, struct.pack('<d', 1.5))
        u.mem_write(FOOT + 0x6B6, bytes((int(row.get('occupation_enabled', False)),)))
        u.mem_write(LOCO + 0xC, dwords(FOOT))
        u.mem_write(LOCO + 0x34, dwords(*OLD_DESTINATION))
        u.mem_write(LOCO + 0x40, dwords(*row.get('old_head', OLD_HEAD)))
        u.mem_write(LOCO + 0x4C, dwords(row.get('residual', 971)))
        u.mem_write(LOCO + 0x50, struct.pack('<d', 0.375))
        u.mem_write(LOCO + 0x58, dwords(27, -7))
        u.mem_write(LOCO + 0x60, bytes((int(row.get('reversed', False)),)))
        u.mem_write(LOCO + 0x63, bytes((int(row.get('old_valid', True)),)))
        # The null coordinate and height constants are supplied runtime data.
        u.mem_write(0x8A0790, dwords(0, 0, 0))
        u.mem_write(SHIP_NULL, dwords(0, 0, 0))
        for address, value in ((0x89E7C0, 104), (0xB1D0AC, 416)):
            u.mem_write(address, dwords(value))
        u.mem_write(TABLE, bytes(0x100000))
        u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
        u.mem_write(DUMMY, bytes(0x200))
        u.mem_write(DUMMY + 0x44, dwords(-1))
        for y in range(32):
            for x in range(32):
                cell = self.cell_address(x, y)
                u.mem_write(cell + 0x24, packed(x, y))
                u.mem_write(cell + 0x44, dwords(-1))
                u.mem_write(cell + 0x11B, bytes((2, 0)))
                u.mem_write(cell + 0x124, dwords(0x400, 0x800))
                u.mem_write(cell + 0x140, dwords(0x100 if row.get('bridge', False) else 0))
                u.mem_write(TABLE + (y * 512 + x) * 4, dwords(cell))

        self.events, self.writes, self.marked_cells = [], [], set()
        self.before_owner = bytes(u.mem_read(FOOT, 0x800))
        self.before = self.state()
        u.hook_add(UC_HOOK_CODE, self.observe)
        u.hook_add(UC_HOOK_MEM_WRITE, self.observe_write)

    @staticmethod
    def cell_address(x, y):
        return CELLS + (y * 32 + x) * 0x200

    def unsigned(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def ints(self, address, count=1):
        return list(struct.unpack('<' + 'i' * count, self.uc.mem_read(address, count * 4)))

    def double_bits(self, address):
        return f'{struct.unpack("<Q", self.uc.mem_read(address, 8))[0]:016x}'

    def state(self):
        return dict(
            turn=self.ints(LOCO + 0x58)[0], cursor=self.ints(LOCO + 0x5C)[0],
            reversed=self.uc.mem_read(LOCO + 0x60, 1)[0],
            residual=self.ints(LOCO + 0x4C)[0],
            head=self.ints(LOCO + 0x40, 3), destination=self.ints(LOCO + 0x34, 3),
            track_valid=self.uc.mem_read(LOCO + 0x63, 1)[0],
            target_fraction_bits=self.double_bits(LOCO + 0x50),
            applied_fraction_bits=self.double_bits(FOOT + 0x578),
            owner_limbo=self.uc.mem_read(FOOT + 0x81, 1)[0],
            owner_alive=self.uc.mem_read(FOOT + 0x90, 1)[0],
        )

    def observe(self, u, address, _size, _data):
        stages = SHIP_STAGES if self.ship else {
            0x4B0C5D: 'selector_published_before_null_guard',
            0x4B0D14: 'head_published_before_map_query',
            APPLY: 'apply_occupation_entry',
            0x4B0D3F: 'occupation_return_before_destination',
            0x4591B2: 'bunker_after_force_before_owner_speed',
            SPEED_SET: 'owner_speed_set_entry',
        }
        if address in stages:
            self.events.append(dict(stage=stages[address], state=self.state()))
        if address == RAW_PUT:
            sp = u.reg_read(UC_X86_REG_ESP)
            self.events.append(dict(stage='unit_raw_put', coord=self.ints(self.unsigned(sp + 4), 3)))
        if address == 0x481A00:
            self.events.append(dict(stage='crate_dispatch_no_overlay'))
        if address == (0x6A03F0 if self.ship else 0x4B0D20):
            self.events.append(dict(stage='post_crate_return', result_al=u.reg_read(UC_X86_REG_EAX) & 255,
                                    state=self.state()))

    def observe_write(self, u, _access, address, size, value, _data):
        if LOCO <= address < LOCO + 0x80:
            self.writes.append(dict(owner='ship' if self.ship else 'drive',
                                    offset=address - LOCO, size=size,
                                    value=value, instruction=f'{u.reg_read(UC_X86_REG_EIP):08x}'))
        elif FOOT <= address < FOOT + 0x800:
            self.writes.append(dict(owner='foot', offset=address - FOOT, size=size,
                                    value=value, instruction=f'{u.reg_read(UC_X86_REG_EIP):08x}'))
        elif CELLS <= address < CELLS + 32 * 32 * 0x200:
            self.marked_cells.add((address - CELLS) // 0x200)

    def finish(self, row, bunker=False):
        u = self.uc
        head = row.get('head', HEAD)
        u.reg_write(UC_X86_REG_ESP, SP)
        if bunker:
            # Original459190 starts after the selected track, GetCoords, and
            # nonnull locomotor guard. The original argument construction,
            # virtual Force_Track and following owner speed setter all execute.
            u.reg_write(UC_X86_REG_EBP, FOOT)
            u.reg_write(UC_X86_REG_EDI, head[0])
            u.reg_write(UC_X86_REG_EBX, head[1])
            u.mem_write(SP + 0x14, dwords(row['turn']))
            u.mem_write(SP + 0x60, dwords(head[2]))
            run_checked(u, 0x459190, 0x4591C4, count=10000,
                        required_addresses=(0x4591AF, FORCE, 0x4591B2, 0x4591BE, SPEED_SET))
            assert u.reg_read(UC_X86_REG_ESP) == SP
        else:
            u.mem_write(SP, dwords(RET_MAGIC, LOCO + 4, row['turn'], *head))
            callback = row.get('supplied_callback')
            if callback is not None:
                # Stop at the real return boundary, retaining the native call
                # frame and locals. The supplied continuation inputs below do
                # NOT purport to be outputs of any emulated crate effect.
                run_checked(u, FORCE, 0x4B0D20, count=10000,
                            required_addresses=(0x4B0C53, 0x4B0C56, 0x481A00))
                self.events.append(dict(stage='observed_no_overlay_return_before_supplied_state',
                                        result_al=u.reg_read(UC_X86_REG_EAX) & 255,
                                        state=self.state()))
                u.reg_write(UC_X86_REG_EAX, callback['result_al'])
                u.mem_write(FOOT + 0x81, bytes((int(callback['limbo']),)))
                u.mem_write(FOOT + 0x90, bytes((int(callback['alive']),)))
                u.mem_write(LOCO + 0x40, dwords(*callback['head']))
                u.mem_write(LOCO + 0x63, bytes((int(callback['track_valid']),)))
                self.before_owner = bytes(u.mem_read(FOOT, 0x800))
                run_checked(u, 0x4B0D20, RET_MAGIC, count=10000,
                            required_addresses=(0x4B0D20,))
            elif self.ship:
                assert callback is None
                run_checked(u, SHIP_FORCE, RET_MAGIC, count=10000,
                            required_addresses=(0x6A0323, 0x6A0326, 0x6A032D))
            else:
                run_checked(u, FORCE, RET_MAGIC, count=10000,
                            required_addresses=(0x4B0C53, 0x4B0C56, 0x4B0C5D))
            assert u.reg_read(UC_X86_REG_ESP) == SP + 24

        after = self.state()
        assert after['residual'] == self.before['residual']
        assert after['reversed'] == self.before['reversed']
        owner_after = bytes(u.mem_read(FOOT, 0x800))
        if bunker:
            assert owner_after[:0x578] == self.before_owner[:0x578]
            assert owner_after[0x580:] == self.before_owner[0x580:]
        else:
            assert owner_after == self.before_owner
            assert not any(write['owner'] == 'foot' for write in self.writes)
        cells = []
        for index in sorted(self.marked_cells):
            x, y = index % 32, index // 32
            cells.append(dict(cell=[x, y], raw=self.ints(self.cell_address(x, y) + 0x124, 2)))
        return dict(input=row, before=self.before,
                    output=dict(state=after, events=self.events, writes=self.writes,
                                marked_cells=cells, owner_preserved=owner_after == self.before_owner,
                                owner_comparison_base=('supplied_post_crate_state'
                                                       if row.get('supplied_callback') is not None
                                                       else 'initial_state')))

    def process_speed_prefix(self, row):
        u = self.uc
        typ = SCRATCH + 0x3000
        u.mem_write(FOOT + 0x6C4, dwords(typ))
        u.mem_write(FOOT + 0x5E0, dwords(-1))
        u.mem_write(typ + 0xDBD, bytes((int(row['accelerates']),)))
        u.mem_write(typ + 0xE0C, bytes((int(row.get('passive', False)),)))
        u.mem_write(typ + 0x678, dwords(8))
        u.mem_write(typ + 0x2F8, dwords(0))
        u.mem_write(typ + 0x300, struct.pack('<d', 0.0625))
        u.mem_write(typ + 0x308, struct.pack('<d', 0.03125))
        u.mem_write(LOCO + 0x58, dwords(row['turn']))
        u.mem_write(LOCO + 0x50, struct.pack('<d', row['target_fraction']))
        u.mem_write(0x8A07D0, dwords(104))
        u.mem_write(0x8A07C4, dwords(416))
        self.before = self.state()
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, LOCO)
        u.mem_write(SP, dwords(RET_MAGIC, 0))
        run_checked(u, 0x4B0F20, 0x4B1274, count=10000,
                    required_addresses=(0x4B0F74, 0x4B126F))
        assert u.reg_read(UC_X86_REG_ESP) == SP - 0x100
        assert u.reg_read(UC_X86_REG_ECX) == FOOT
        getter = self.unsigned(u.reg_read(UC_X86_REG_EDX) + 0x538)
        assert getter == 0x4DB1A0
        return dict(input=row, before=self.before,
                    output=dict(state=self.state(), events=self.events, writes=self.writes,
                                next_call=f'{getter:08x}'))


class OriginalBunkerInstall(OriginalForceTrack):
    """Continue states 1 and 2 with real coordinate/facing/locomotor owners.

    The caller already found its Unit and cleared the footprint. An explicit
    global-frame advance lets the original Facing timer finish between states;
    neither a desired facing nor a Force_Track destination is computed in Python.
    """

    BUILDING, TYPE, ARGUMENT = SCRATCH + 0x5000, SCRATCH + 0x3000, SCRATCH + 0x7000
    FRAME, INITIAL_FRAME = 0xA8ED84, 100

    def __init__(self, row):
        super().__init__(row)
        u = self.uc
        self.install_trace = []
        self.phase = 'setup'
        self.original_text = bytes(u.mem_read(0x401000, 4063232))
        u.mem_write(self.BUILDING, dwords(0x7E3EBC))
        u.mem_write(self.BUILDING + 0x520, dwords(self.TYPE))
        u.mem_write(self.BUILDING + 0x9C, dwords(*row['building_location']))
        u.mem_write(self.TYPE, dwords(0x7E4570))
        u.mem_write(self.TYPE + 0xEF0, dwords(row['foundation_index']))
        u.mem_write(FOOT + 0x9C, dwords(*row['unit_location']))
        u.mem_write(self.FRAME, dwords(self.INITIAL_FRAME))
        # Actual Facing constructor, ROT setter and snap. Rates and starting
        # facing are declared inputs; duration and completion are native outputs.
        self.invoke(0x4C91C0, FOOT + 0x388)
        self.invoke(0x4C9680, FOOT + 0x388, (row['rot'],))
        u.mem_write(self.ARGUMENT, dwords(row['initial_facing']))
        self.invoke(0x4C9300, FOOT + 0x388, (self.ARGUMENT,))
        self.writes.clear()
        self.before_owner = bytes(u.mem_read(FOOT, 0x800))
        self.before = self.state()
        u.hook_add(UC_HOOK_CODE, self.observe_install)

    def invoke(self, address, receiver, args=()):
        u = self.uc
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, receiver)
        run_checked(u, address, RET_MAGIC, count=100000)
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4 + len(args) * 4
        return u.reg_read(UC_X86_REG_EAX)

    def observe_install(self, u, address, _size, _data):
        if address in (0x447AC0, 0x45EC90, 0x45ECA0, 0x5F65A0,
                       0x4CAE30, 0x7C5F00, 0x4C9220, 0x4C9480, 0x4C93D0):
            self.install_trace.append(dict(phase=self.phase, entry=f'{address:08X}'))
        if address == 0x4590EA:
            self.install_trace.append(dict(phase=self.phase, stage='set_desired',
                facing=self.unsigned(self.unsigned(u.reg_read(UC_X86_REG_ESP))) & 0xFFFF))
        if address == 0x459170:
            self.install_trace.append(dict(phase=self.phase, stage='building_center_return',
                coordinate=self.ints(u.reg_read(UC_X86_REG_EAX), 3)))
        if address == 0x459124:
            self.install_trace.append(dict(phase=self.phase, stage='current_facing_return',
                facing=u.reg_read(UC_X86_REG_EAX) & 0xFFFF))
        if address == 0x4591AF:
            sp = u.reg_read(UC_X86_REG_ESP)
            self.install_trace.append(dict(phase=self.phase, stage='force_track_arguments',
                interface=self.unsigned(sp), selector=self.ints(sp + 4)[0],
                coordinate=self.ints(sp + 8, 3)))

    def run_block(self, begin, end, required):
        u = self.uc
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, FOOT)
        u.reg_write(UC_X86_REG_ESI, self.BUILDING)
        run_checked(u, begin, end, count=100000, required_addresses=required)
        assert u.reg_read(UC_X86_REG_ESP) == SP

    def finish_install(self, row):
        u = self.uc
        self.phase = 'choose_desired'
        self.run_block(0x459069, 0x4590F9,
                       (0x5F65A0, 0x447AC0, 0x45EC90, 0x45ECA0,
                        0x4CAE30, 0x7C5F00, 0x4C9220))
        desired = self.unsigned(FOOT + 0x388) & 0xFFFF
        duration = self.ints(FOOT + 0x398)[0]
        start = self.ints(FOOT + 0x390)[0]
        state_after_facing = self.ints(self.BUILDING + 0x718)[0]
        if duration > 0:
            self.phase = 'waiting_for_turn'
            self.run_block(0x459101, 0x459374, (0x4C9480,))
            assert self.state() == self.before
        # Only clock progression is supplied. Current and IsRotating execute
        # against the retained FacingClass; no caller-produced fields change.
        completed_frame = max(self.INITIAL_FRAME, start + duration)
        u.mem_write(self.FRAME, dwords(completed_frame))
        self.phase = 'install_force_track'
        self.run_block(0x459101, 0x4591CE,
                       (0x4C9480, 0x4C93D0, 0x447AC0, 0x45EC90, 0x45ECA0,
                        0x4591AF, FORCE, 0x4591BE, SPEED_SET))
        current = next(event['facing'] for event in self.install_trace
                       if event.get('stage') == 'current_facing_return')
        center = next(event['coordinate'] for event in self.install_trace
                      if event.get('stage') == 'building_center_return')
        force = next(event for event in self.install_trace
                     if event.get('stage') == 'force_track_arguments')
        assert bytes(u.mem_read(0x401000, 4063232)) == self.original_text
        assert current == desired
        return dict(input=row, output=dict(
            desired_facing=desired, current_facing=current, turn_start=start,
            turn_duration=duration, completed_frame=completed_frame,
            waited_for_turn=duration > 0, state_after_facing=state_after_facing,
            state_after_force=self.ints(self.BUILDING + 0x718)[0],
            building_center=center, selector=force['selector'],
            force_coordinate=force['coordinate'], state=self.state(),
            trace=self.install_trace, events=self.events,
        ))


def bunker_foundation_inputs():
    """Run the existing INI-cache/type-reader owner on physical NATBNK ART.

    Foundation is ART-only. The original caller reads Image then type section;
    use identical names for the retail case and explicit synthetic controls.
    """
    from tools.spatial_oracle.engineer_bridge_cursor_caller import (
        HutTypeReader, READER_INI, READER_SP,
    )
    from tools.rules_oracle.bridge_child_sound import sections

    path = Path('ini/artmd.ini')
    raw = path.read_bytes()
    value = sections(raw)['NATBNK']['Foundation']
    rows = []
    for name, foundation in (('retail_natbnk', value), ('one_by_one', '1x1'),
                             ('three_by_two', '3x2')):
        m = HutTypeReader()
        typ = m.construct('NATBNK')
        default = m.read32(typ + 0xEF0)
        # Image binding is supplied. The selected Foundation block is the
        # original caller and reads the exact same section as this retail type.
        m.u.mem_write(typ + 0x1F8, b'NATBNK\0')
        m.make_ini({'NATBNK': {'Foundation': foundation}})
        m.phase = 'bunker_foundation'
        m.trace = []
        m.u.mem_write(READER_SP, dwords(RET_MAGIC))
        m.u.reg_write(UC_X86_REG_ESP, READER_SP)
        m.u.reg_write(UC_X86_REG_EBP, typ)
        m.u.reg_write(UC_X86_REG_EDI, typ + 0x1F8)
        m.u.reg_write(UC_X86_REG_EBX, typ + 0x24)
        m.u.reg_write(UC_X86_REG_ESI, READER_INI)
        run_checked(m.u, 0x461225, 0x46125D, count=200000,
                    required_addresses=(0x474DA0, 0x528A10))
        assert m.u.reg_read(UC_X86_REG_ESP) == READER_SP
        assert not any(event['kind'] == 'fixture_seam' for event in m.trace)
        index = m.read32(typ + 0xEF0)
        m.unchanged()
        rows.append(dict(name=name, value=foundation, constructor_default=default,
                         foundation_index=index, width=m.invoke(0x45EC90, typ),
                         height=m.invoke(0x45ECA0, typ, (0,)), reads=m.trace))
    return dict(physical=dict(file=path.as_posix(), sha256=hashlib.sha256(raw).hexdigest(),
                             section='NATBNK', key='Foundation', value=value), rows=rows)


def generate_bunker_install():
    foundations = bunker_foundation_inputs()
    base = dict(building_location=[2688, 2688, 312], rot=5, initial_facing=0,
                foundation_index=foundations['rows'][0]['foundation_index'])
    cases = []
    # These are fixture coordinates, not expected centers or facings. Include
    # all four corners of the 2x2 footprint and the same raw-cell case that
    # used to bypass VERA's Force_Track path entirely.
    for name, coordinate in (
        ('same_raw_cell', [2688, 2688, 312]),
        ('corner_east', [2944, 2688, 312]),
        ('corner_south', [2944, 2944, 312]),
        ('corner_west', [2688, 2944, 312]),
        ('same_cell_subcell', [2707, 2771, 416]),
        ('already_at_center', [2816, 2816, 312]),
    ):
        cases.append(dict(base, name=name, unit_location=coordinate))
    for foundation in foundations['rows'][1:]:
        cases.append(dict(base, name=foundation['name'],
                          foundation_index=foundation['foundation_index'],
                          unit_location=[2688, 2688, 312]))
    # Native selector equality bands are much narrower than broad quadrants.
    # Nearby physical coordinates make the original angle producer cross them.
    for delta in (-4, -2, -1, 0, 1, 2, 4):
        for quadrant, coordinate in enumerate((
            [2688 + delta, 2688, 312], [2944 + delta, 2688, 312],
            [2944 + delta, 2944, 312], [2688 + delta, 2944, 312],
        )):
            cases.append(dict(base, name=f'corner_boundary_{quadrant}_{delta}',
                              unit_location=coordinate))
    return dict(foundation_inputs=foundations,
                rows=[OriginalBunkerInstall(row).finish_install(row) for row in cases])


def bunker_install_metadata():
    return provenance(
        scope='Original bunker installation coordinate/facing/selector and Force_Track producers, bounded states1/2; native retail NATBNK Foundation reader',
        assumptions=[
            'Same physical supported gamemd image as this sidecar identity; original Building/BuildingType/Unit vtables, Drive/base and Facing constructors, Facing Set_ROT/Snap/Set/Current/IsRotating execute',
            'State1 begins459069 after candidate lookup, radio/contact admission, footprint clearance and scatter; ESI Building and EBP Unit are supplied. State1 writes state2; state2 begins459101 with that retained object/facing state',
            'Global frame100 is supplied, then advanced directly to original timer start+duration. Original waiting gate executes when duration is positive. Full frame scheduling, per-frame movement and intermediate turning are excluded',
            'Original Building447AC0 computes Foundation center;459069..4590F9 computes desired16 through original atan2/ftol/Set;459101..4591CE reads Current16, selects track, calls GetCoords/Drive Force_Track and writes speed/state3',
            'Physical ini/artmd.ini NATBNK Foundation string is extracted by the existing lexical owner and supplied to the existing INI cache. Original type constructor and461225..46125D Foundation caller/readers execute. NATBNK Image identity is supplied; complete type ReadINI, physical asset loading and Scenario rule layers are excluded',
            'Control foundations1x1/3x2 are explicit synthetic ART strings parsed by the same original reader. Declared raw XYZ, initial facing and ROT inputs are fixtures; expected centers, desired/current facings, timer duration, selector and Force_Track coordinates come only from native execution',
            'Reuses the existing32x32 level2/no-overlay map, Drive occupation and raw-put controls. Original instructions remain unchanged; read-only observers record input arguments and returned native values. Drive Process, final parking position, walls, reciprocal installation/deselection, screen picking and command delivery remain outside this corpus',
        ],
        substitutions=[
            'Existing HutTypeReader supplies allocator/constructor CRT seams and INI caches; no such seam may occur during the Foundation reader block',
            'Explicit initial state and the declared frame advance only; no patched instructions, custom vtables, substituted facing results or computed reference destinations',
        ],
        entry_points=dict(facing_block=0x459069, facing_end=0x4590F9,
                          install_block=0x459101, install_end=0x4591CE,
                          building_coords=0x447AC0, foundation_reader=0x461225,
                          facing_constructor=0x4C91C0, facing_rot=0x4C9680,
                          facing_snap=0x4C9300, facing_set=0x4C9220,
                          facing_current=0x4C93D0, facing_turning=0x4C9480,
                          force_track=FORCE, owner_speed_set=SPEED_SET),
    )


def generate():
    direct = []
    # All retail Drive TurnTrack selectors with both retained short choices.
    # Includes native table-zero and handoff cases as well as special64..71.
    for turn in range(72):
        for reverse in (False, True):
            row = dict(kind='admitted', turn=turn, reversed=reverse)
            direct.append(OriginalForceTrack(row).finish(row))
    # Null XYZ returns after publishing arbitrary signed selectors/cursor0.
    for turn in (-2147483648, -17, -1, 0, 63, 64, 67, 68, 69, 70, 71, 2147483647):
        for old_head in (OLD_HEAD, [0, 0, 0]):
            row = dict(kind='null_coordinate', turn=turn, head=[0, 0, 0],
                       old_head=old_head, reversed=True, residual=-23)
            direct.append(OriginalForceTrack(row).finish(row))
    # Nonnull is a full XYZ comparison, including head Z with zero X/Y.
    for head in ([0, 0, 731], [0, HEAD[1], 0], [HEAD[0], 0, 0], HEAD):
        for alive in (False, True):
            for limbo in (False, True):
                row = dict(kind='owner_gate', turn=71, head=head, alive=alive,
                           limbo=limbo, residual=0, old_head=[0, 0, 0], old_valid=False)
                direct.append(OriginalForceTrack(row).finish(row))
    for turn in (-1, 0, 67, 71):
        for bridge in (False, True):
            row = dict(kind='occupation_plane', turn=turn, head=HEAD,
                       bridge=bridge, occupation_enabled=True, applied=0.75)
            direct.append(OriginalForceTrack(row).finish(row))
    continuations = []
    for result in (0, 1):
        for alive in (False, True):
            for limbo in (False, True):
                for head in ([0, 0, 0], [HEAD[0] + 256, HEAD[1], -347]):
                    row = dict(kind='supplied_post_crate_continuation', turn=71,
                               supplied_callback=dict(result_al=result, alive=alive,
                                                      limbo=limbo, head=head,
                                                      track_valid=head != [0, 0, 0]))
                    continuations.append(OriginalForceTrack(row).finish(row))
    # Apply4B0B80 calls Transform4B4780, which reloads retained head XY at
    # 4B47E2/E5; final raw mark still receives Force's original supplied XYZ.
    # Selector1 has a handoff. Two displaced heads, both reversed values and
    # both raw planes distinguish its anchor, suppression and owner-height Z.
    for head in ([HEAD[0] + 256, HEAD[1], -347],
                 [HEAD[0] - 256, HEAD[1] - 256, 941]):
        for reverse in (False, True):
            for bridge in (False, True):
                row = dict(kind='supplied_post_crate_handoff_continuation', turn=1,
                           reversed=reverse, bridge=bridge,
                           supplied_callback=dict(result_al=1, alive=True, limbo=False,
                                                  head=head, track_valid=True))
                continuations.append(OriginalForceTrack(row).finish(row))
    bunker = []
    for turn in range(67, 71):
        for applied in (0.0, 0.25, 0.75):
            row = dict(kind='bunker_call_tail', turn=turn, applied=applied)
            bunker.append(OriginalForceTrack(row).finish(row, bunker=True))
    speed = []
    for turn in (63, 64, 67, 71):
        for accelerates in (False, True):
            for target in (0.375, 1.0, 1.25):
                row = dict(kind='process_speed_prefix', turn=turn, accelerates=accelerates,
                           target_fraction=target, applied=0.25)
                speed.append(OriginalForceTrack(row).process_speed_prefix(row))
    for turn in (63, 64):
        row = dict(kind='process_speed_prefix', turn=turn, accelerates=True,
                   passive=True, target_fraction=1.0, applied=0.25)
        speed.append(OriginalForceTrack(row).process_speed_prefix(row))
    # The Ship twin: the warp's selector -1 and a sample of TurnTrack
    # selectors (Ship has no special 64..71), both reversed values, the null
    # coordinate, the owner gates and both raw planes.
    ship = []
    for turn in (-1, 0, 1, 18, 63):
        for reverse in (False, True):
            row = dict(family='ship', kind='admitted', turn=turn, reversed=reverse)
            ship.append(OriginalForceTrack(row).finish(row))
    for old_head in (OLD_HEAD, [0, 0, 0]):
        row = dict(family='ship', kind='null_coordinate', turn=-1, head=[0, 0, 0],
                   old_head=old_head, reversed=True, residual=-23)
        ship.append(OriginalForceTrack(row).finish(row))
    for alive in (False, True):
        for limbo in (False, True):
            row = dict(family='ship', kind='owner_gate', turn=-1, head=HEAD, alive=alive,
                       limbo=limbo, residual=0, old_head=[0, 0, 0], old_valid=False)
            ship.append(OriginalForceTrack(row).finish(row))
    for bridge in (False, True):
        row = dict(family='ship', kind='occupation_plane', turn=-1, head=HEAD,
                   bridge=bridge, occupation_enabled=True, applied=0.75)
        ship.append(OriginalForceTrack(row).finish(row))
    return dict(force_track=direct, supplied_post_crate_continuations=continuations,
                bunker_call_tail=bunker, process_speed_prefix=speed,
                ship_force_track=ship)


def metadata():
    return provenance(
        scope='Original Drive Force_Track no-overlay calls and supplied post-crate continuations, Ship Force_Track no-overlay calls, bunker separate speed write, and Drive Process applied-fraction prefix before current-speed getter',
        assumptions=[
            'Original Drive/base constructor and unchanged Unit/Drive vtables; supplied disjoint owner/class memory, owner alive/limbo/occupation flags and finite speed fractions',
            'Ship rows construct the original ShipLocomotionClass (0x69EC50) and call its own Force_Track 0x6A0310 and Apply_Track_Occupation_Mode 0x6A01A0 from slot +0x70: selectors -1, 0, 1, 18 and 63 with both reversed values, the null coordinate, the owner gates and both raw planes; Ship NullCoord 0xB077F8 is supplied XYZ0',
            'All72 Drive selectors and both retained reversed values; null-coordinate cases include arbitrary signed selectors, retained null/non-null heads and negative residual',
            'Map uses supplied32x32 allocated flat cells at level2, no overlays, raw ground0x400/deck0x800, optional structural bridge flag; remaining fixed-stride slots are null and shared dummy has overlay-1',
            'NullCoord XYZ0, ground-level104, Unit raw bridge offset416 and ambient x87 control0E7F are supplied runtime state, not full startup/map/type construction',
            'Full-call rows execute Crate pickup original no-overlay early return. Separate continuation rows explicitly supply AL, alive/limbo, head and valid after that return; crate effect bodies and producers of those mutations are outside coverage',
            'Eight supplied selector1 handoff continuations displace retained head from original supplied destination, with both reversed values and structural planes; handoff Transform reloads retained XY and owner-height Z, final mark retains supplied XYZ',
            'Bunker rows start after original facing selection/GetCoords/non-null-interface admission, with supplied selected67..70 and full XYZ; original virtual calls and SetSpeedFraction execute, but complete bunker mission and subsequent Process do not',
            'Read-only instruction/memory-write observers record publication order, actual raw-put XYZ, all Drive/Foot writes and changed raw cells; Force calls/continuations assert entire Foot memory unchanged relative to initial or explicitly supplied post-crate state respectively',
            'Process4B0F20 stops before original owner GetCurrentSpeed call4B1274. Supplied UnitType Accelerates and Passive bytes, raw speed8, slowdown0, deceleration1/16, acceleration1/32, no linked members/transport/slowdown flags; current-speed getter and later movement excluded',
        ],
        substitutions=[
            'No patched instructions, custom virtual tables or control-flow-changing hooks; all reached callees execute original bytes',
            'Only separately labeled post-crate continuation rows replace AL and declared owner/head state at the stopped4B0D20 boundary; retained CPU frame/locals and subsequent native instructions execute unchanged',
        ],
        entry_points=dict(force_track=FORCE, drive_constructor=0x4AF540,
                          ship_force_track=SHIP_FORCE, ship_constructor=SHIP_CONSTRUCTOR,
                          ship_apply_occupation=SHIP_APPLY, ship_transform=0x6A3DB0,
                          common_constructor=0x55A6C0, map_query=0x565730,
                          crate_dispatch=0x481A00, apply_occupation=APPLY,
                          transform=0x4B4780, unit_raw_put=RAW_PUT,
                          ground_height=0x578080, bunker_call_begin=0x459190,
                          bunker_call_end=0x4591C4, owner_speed_set=SPEED_SET,
                          post_crate=0x4B0D20, process_prefix=0x4B0F20,
                          before_current_speed_call=0x4B1274),
    )


if __name__ == '__main__':
    if '--bunker-install' in sys.argv:
        args = [arg for arg in sys.argv[1:] if arg != '--bunker-install']
        finish_vectors(generate_bunker_install,
                       Path(__file__).with_suffix('.bunker_install.json'),
                       provenance=bunker_install_metadata, argv=args,
                       source_paths={'harness': Path(__file__)})
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
