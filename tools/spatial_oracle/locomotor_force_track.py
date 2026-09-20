"""Original Drive Force_Track and the bunker caller's separate speed write.

Full 4B0C40 calls execute the real map lookup, no-overlay Crate pickup return,
Apply_Track_Occupation_Mode, transform and Unit raw-mark leaves. Separate rows
explicitly supply post-crate return/state at a stopped continuation boundary.
Observation hooks never change machine state. Bunker rows begin at its already-
selected call frame; Process rows stop before the owner current-speed getter.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW,
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
HEAD = [10 * 256 + 96, 10 * 256 + 160, 731]
OLD_HEAD, OLD_DESTINATION = [2304, 2560, -347], [2176, 2176, 417]


class OriginalForceTrack:
    def __init__(self, row):
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
        run_checked(u, 0x4AF540, RET_MAGIC, count=150,
                    required_addresses=(0x55A6C0,))
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        assert self.unsigned(self.unsigned(LOCO + 4) + 0x70) == FORCE
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
        stages = {
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
        if address == 0x4B0D20:
            self.events.append(dict(stage='post_crate_return', result_al=u.reg_read(UC_X86_REG_EAX) & 255,
                                    state=self.state()))

    def observe_write(self, u, _access, address, size, value, _data):
        if LOCO <= address < LOCO + 0x80:
            self.writes.append(dict(owner='drive', offset=address - LOCO, size=size,
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
    return dict(force_track=direct, supplied_post_crate_continuations=continuations,
                bunker_call_tail=bunker, process_speed_prefix=speed)


def metadata():
    return provenance(
        scope='Original Force_Track no-overlay calls and supplied post-crate continuations, bunker separate speed write, and Drive Process applied-fraction prefix before current-speed getter',
        assumptions=[
            'Original Drive/base constructor and unchanged Unit/Drive vtables; supplied disjoint owner/class memory, owner alive/limbo/occupation flags and finite speed fractions',
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
                          common_constructor=0x55A6C0, map_query=0x565730,
                          crate_dispatch=0x481A00, apply_occupation=APPLY,
                          transform=0x4B4780, unit_raw_put=RAW_PUT,
                          ground_height=0x578080, bunker_call_begin=0x459190,
                          bunker_call_end=0x4591C4, owner_speed_set=SPEED_SET,
                          post_crate=0x4B0D20, process_prefix=0x4B0F20,
                          before_current_speed_call=0x4B1274),
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
