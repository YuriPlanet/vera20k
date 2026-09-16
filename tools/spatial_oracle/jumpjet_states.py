"""Original Jumpjet state machine driven through Process 0x0054AEC0 per frame.

Process is the real driver here: it gates Update_Coordinates_And_Altitude
0x0054D0F0 on Is_Moving 0x0054AE50 (moving byte +0x4C) or Is_Moving_Now
0x0054D0D0 (state not 0 and not 2), then dispatches the state at receiver +0x50
through the jump table 0x0054B19C - State0 0x0054B980, State1 0x0054BA30,
State2 0x0054BD30, State3 0x0054BFF0, State4 0x0054C550. Rows therefore cover
the gate itself, not only the handlers: an idle landed owner records that
nothing runs.

The map is a declared block of CellClass objects, x=6..20 by y=9..11, each with
its own level, slope, LandType +0xEC, raw occupation bytes and AltObject air
slot +0xE0; every other lookup resolves to the shared dummy. Rows describe the
flight line y=10; the rows either side exist so a scatter to a north or south
neighbour lands on a declared cell rather than the dummy.
Move_To 0x0054B1C0 runs for real against a supplied FNPC result, so the
destination is the original adjusted coordinate.

Not covered: State5 crash 0x0054CA90, bridges, building tops, cell objects in
the reference height, the radio-contact voice and multi-owner cells.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.jumpjet_coordinates import Jumpjet, KIND, TYPE_GET, HEIGHT, MARK, SET_SPEED, CELL_GET, CELL_COORD
from tools.spatial_oracle.walk_head_occupation import OWNER, VTABLE, LOCO, TYPE, OUTPUT, SCRATCH, TABLE, DUMMY

# Owner vtable seams this corpus supplies. Each no-op carries the stack cleanup
# of the slot that reaches it, so the caller's stack stays exact.
(SETLOC, ENTER, SET_DEST, LAYER_MARKED, OTHER, SET_CACHED_CELL,
 NOOP0, NOOP4, NOOP12, NOOP16, NOOP20) = [SCRATCH + 0xec00 + i * 0x40 for i in range(11)]
NOOP_CLEANUP = {NOOP0: 0, NOOP4: 4, NOOP12: 12, NOOP16: 16, NOOP20: 20}
# Corridor CellClass storage inside the mapped image, past the cell table.
CORRIDOR = 0x00D00000
# CellClass vtable; slot +0x48 is the cell's own GetCoords (0x00486840).
CELL_VTABLE = 0x007E4EEC
FRAME = 0xA8ED84
CENTER = 128
ROW_Y = 10
CORRIDOR_X = range(6, 21)
# Three rows, so a scatter to a north or south neighbour and any drift off the
# flight line still land on declared cells instead of the shared dummy.
CORRIDOR_Y = range(9, 12)


def f32(value):
    return struct.pack('<f', value)


def cell_of(x, y):
    """The corridor CellClass for a cell coordinate, or None off the corridor."""
    if y not in CORRIDOR_Y or x not in CORRIDOR_X:
        return None
    index = (y - CORRIDOR_Y.start) * len(CORRIDOR_X) + (x - CORRIDOR_X.start)
    return CORRIDOR + index * 0x200


def coord_cell(x_leptons, y_leptons):
    """The native signed lepton-to-cell step, matching 0x00565730."""
    return ((x_leptons + ((x_leptons >> 31) & 0xFF)) >> 8,
            (y_leptons + ((y_leptons >> 31) & 0xFF)) >> 8)


class States(Jumpjet):
    def __init__(self, row):
        super().__init__(dict(actions=[], request=[0, 0, 0], ground=0, level=0, slope=0))
        u = self.uc
        self.row = row
        self.fractions = []
        self.slot_events = []
        self.fnpc_calls = 0
        u.mem_write(0xABC5E8, dwords(104))
        # Two CRT static initializers, both listed in the table at 0x00812B90.
        # They fill DIFFERENT tables: 0x0049F3A0 writes the 8-byte-stride lepton
        # deltas at 0x0089F6D8 that the reference height 0x0054D820 reads, and
        # 0x0049F2F0 writes the 4-byte-stride packed adjacent-cell offsets at
        # 0x0089F688 that the neighbour step 0x00481810 reads. Without the
        # second, every scatter steps by (0,0) onto the owner's own cell.
        self.call(0x49F3A0, 0, [])
        self.call(0x49F2F0, 0, [])
        self.install_corridor()
        for slot, fn in [(0x1b4, SETLOC), (0x1b8, 0x0041BEA0), (0x1c8, 0x005F5F40),
                         (0x1d0, 0x005F5F30), (0x1ac, ENTER), (0x480, SET_DEST),
                         (0x54, LAYER_MARKED), (0x18c, NOOP4), (0x150, NOOP0),
                         (0x2f8, SET_CACHED_CELL),
                         (0xf0, NOOP4), (0xf4, NOOP4), (0x198, NOOP4), (0x3dc, NOOP4),
                         (0x558, NOOP12), (0x48c, NOOP16), (0x488, NOOP20)]:
            u.mem_write(VTABLE + slot, dwords(fn))
        u.mem_write(TYPE + 0xD70, dwords(row['turn_rate'], row['speed']))
        u.mem_write(TYPE + 0xD78, f32(row['climb']) + f32(row['crash']))
        u.mem_write(TYPE + 0xD80, dwords(row['height']))
        u.mem_write(TYPE + 0xD84, f32(row['accel']) + f32(row['wobbles']))
        u.mem_write(TYPE + 0xD8C, bytes([int(row['no_wobbles']), 0, 0, 0]))
        u.mem_write(TYPE + 0xD90, dwords(row['deviation']))
        u.mem_write(TYPE + 0xD6A, bytes([int(row['balloon_hover'])]))
        u.mem_write(TYPE + 0xECB, b'\0')
        u.mem_write(TYPE + 0x6AD, bytes([int(row['deploy_to_land'])]))
        u.mem_write(TYPE + 0xE13, bytes([int(row['simple_deployer'])]))
        u.mem_write(OWNER + 0x6C0, dwords(TYPE))
        u.mem_write(OWNER + 0x6C4, dwords(TYPE))
        u.mem_write(OWNER + 0x2B0, dwords(0, 1 if row['tarcom'] else 0))
        u.mem_write(OWNER + 0x6AD, bytes([int(row['piggyback'])]))
        # Process tail: +0x83 clear and +0x41B set skip both visibility probes;
        # +0x90 keeps the airborne layer path, +0x425 leaves the crash latch off.
        for offset, value in [(0x74, 0), (0x8C, 0), (0x8D, 0), (0x81, 0), (0x83, 0),
                              (0x41B, 1), (0x90, 1), (0x425, 0), (0x427, 0), (0x134, 0)]:
            u.mem_write(OWNER + offset, bytes([value]))
        u.mem_write(OWNER + 0x260, dwords(4))
        u.mem_write(OWNER + 0x560, dwords(0xFFFFFFFF))
        u.mem_write(OWNER + 0x9C, dwords(*row['start']))
        self.frame = row['first_frame']
        u.mem_write(FRAME, dwords(self.frame))
        self.call(0x54ad30, 0, [LOCO + 4, OWNER])
        u.mem_write(OUTPUT, dwords(row['facing']))
        self.call(0x4c9300, LOCO + 0x54, [OUTPUT])
        u.mem_write(LOCO + 0x50, dwords(row['phase']))
        u.mem_write(LOCO + 0x4C, dwords(int(row['moving'])))
        u.mem_write(LOCO + 0x70, struct.pack('<dd', 0.0, 0.0))
        u.mem_write(LOCO + 0x80, dwords(row['target_height']))

    def install_corridor(self):
        """Allocate the declared cell corridor into the original cell table."""
        u = self.uc
        table = bytearray(u.mem_read(TABLE, 0x100000))
        for y in CORRIDOR_Y:
            for x in CORRIDOR_X:
                cell = cell_of(x, y)
                u.mem_write(cell, bytes(0x200))
                struct.pack_into('<I', table, (y * 512 + x) * 4, cell)
                # Real CellClass vtable: 0x0054D6D0 and the state bodies reach
                # the cell's own GetCoords through slot +0x48.
                u.mem_write(cell, dwords(CELL_VTABLE))
                u.mem_write(cell + 0x24, packed(x, y))
                u.mem_write(cell + 0x44, dwords(0xFFFFFFFF))
                u.mem_write(cell + 0x54, dwords(0xFFFFFFFF, 0xFFFFFFFF))
                u.mem_write(cell + 0x11B, bytes((0, 0)))
        u.mem_write(TABLE, bytes(table))
        for x, level, slope in self.row['terrain']:
            u.mem_write(cell_of(x, ROW_Y) + 0x11B, bytes((level & 255, slope)))
        for x, land_type in self.row['land_types']:
            u.mem_write(cell_of(x, ROW_Y) + 0xEC, dwords(land_type))
        for x in self.row['occupied_slots']:
            u.mem_write(cell_of(x, ROW_Y) + 0xE0, dwords(OTHER))
        for x, ground in self.row['raw_occupation']:
            u.mem_write(cell_of(x, ROW_Y) + 0x124, dwords(ground))

    def owner_cell(self):
        coord = struct.unpack('<iii', self.uc.mem_read(OWNER + 0x9C, 12))
        return coord_cell(coord[0], coord[1])

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == KIND:
            self.ret(0, self.row['rtti'])
        elif address == TYPE_GET:
            self.ret(0, TYPE)
        elif address == SET_SPEED:
            self.fractions.append(struct.unpack('<Q', u.mem_read(sp + 4, 8))[0])
            self.ret(8, 0)
        elif address == MARK:
            self.ret(4, 0)
        elif address == SET_CACHED_CELL:
            # 0x0041C160 verbatim: store the packed cell at owner +0x560. Only
            # the air-bucket helpers reach it, and those are no-ops here, so the
            # cached cell stays as supplied and the cell-change probe in States
            # 1 and 3 fires every frame.
            u.mem_write(OWNER + 0x560, u.mem_read(sp + 4, 4))
            self.ret(4, 0)
        elif address == CELL_GET:
            x, y = self.owner_cell()
            self.ret(0, cell_of(x, y) or DUMMY)
        elif address == CELL_COORD:
            pointer = self.read32(sp + 4)
            x, y = self.owner_cell()
            u.mem_write(pointer, packed(x, y))
            self.ret(4, pointer)
        elif address == SETLOC:
            u.mem_write(OWNER + 0x9C, bytes(u.mem_read(self.read32(sp + 4), 12)))
            self.ret(4, 0)
        elif address == LAYER_MARKED:
            self.ret(0, 1)
        elif address == ENTER:
            cell = self.read32(sp + 4)
            x = struct.unpack('<hh', u.mem_read(cell + 0x24, 4))[0] if cell else -1
            answer = self.row['can_enter_cells'].get(x, 0)
            self.events.append(['can_enter_cell', x, answer])
            self.ret(20, answer)
        elif address == SET_DEST:
            cell = self.read32(sp + 4)
            target = (list(struct.unpack('<hh', u.mem_read(cell + 0x24, 4)))
                      if cell else None)
            self.events.append(['set_destination', target, self.read32(sp + 8)])
            self.ret(8, 0)
        elif address in NOOP_CLEANUP:
            self.ret(NOOP_CLEANUP[address], 0)
        elif address == 0x004134A0:
            self.events.append('bucket_add')
            self.ret(4, 0)
        elif address == 0x004135D0:
            self.events.append('bucket_remove')
            self.ret(4, 0)
        elif address == 0x004138C0:
            self.ret(12, 0)
        elif address == 0x0055A710:
            self.ret(8, 0)
        elif address == 0x0065AD30:
            self.ret(4, 0)
        elif address == 0x00567DA0:
            # Fog border takes four arguments: the PUSH at 0x0054C974 is the
            # first, ahead of the three pushed at 0x0054C99B..0x0054C9A1.
            self.ret(16, 0)
        elif address == 0x00481A00:
            # Crate pickup takes the owner as a stack argument: the PUSH
            # at 0x0054C9EB survives the zero-argument GetCell above it.
            self.events.append('crate_pickup')
            self.ret(4, 0)
        elif address == 0x006385C0:
            self.ret(0, 0)
        elif address == 0x004A9720:
            self.ret(4, 0)
        elif address == 0x00705D60:
            self.events.append('mission_notify')
            self.ret(0, 0)
        elif address == 0x004135A0:
            # Original body: the owner's cell +0xE0 holds a different object.
            x, y = self.owner_cell()
            cell = cell_of(x, y)
            taken = bool(cell and self.read32(cell + 0xE0) not in (0, OWNER))
            self.slot_events.append(['query', x, y, taken])
            self.ret(4, int(taken))
        elif address == 0x00487D70:
            # Original body: a zero argument releases; otherwise claim unless
            # another object already holds the slot.
            cell = u.reg_read(UC_X86_REG_ECX)
            occupant = self.read32(sp + 4)
            held = self.read32(cell + 0xE0)
            xy = list(struct.unpack('<hh', u.mem_read(cell + 0x24, 4)))
            if occupant == 0:
                u.mem_write(cell + 0xE0, dwords(0))
                self.slot_events.append(['release', *xy])
                self.ret(4, 1)
            elif held not in (0, occupant):
                self.slot_events.append(['claim_refused', *xy])
                self.ret(4, 0)
            else:
                u.mem_write(cell + 0xE0, dwords(occupant))
                self.slot_events.append(['claim', *xy])
                self.ret(4, 1)
        elif address == 0x56dc20:
            # Supplied FNPC result: each call answers the next declared cell,
            # defaulting to the ordered cell, so Stop_Moving's re-target is an
            # explicit input rather than an accident of the fixture.
            pointer = self.read32(sp + 4)
            answers = self.row['fnpc_cells']
            answer = answers[min(self.fnpc_calls, len(answers) - 1)] if answers else None
            self.fnpc_calls += 1
            x, y = answer or coord_cell(self.row['order'][0], self.row['order'][1])
            u.mem_write(pointer, packed(x, y))
            self.events.append(['fnpc', [x, y]])
            self.ret(60, pointer)
        elif address == 0x578080:
            pass
        else:
            super().observe(u, address, size, data)

    def state(self):
        u = self.uc
        self.call(0x4c93d0, LOCO + 0x54, [OUTPUT])
        x, y = self.owner_cell()
        cell = cell_of(x, y)
        return dict(
            coord=list(struct.unpack('<iii', u.mem_read(OWNER + 0x9C, 12))),
            cell=[x, y],
            current_speed=struct.unpack('<Q', u.mem_read(LOCO + 0x70, 8))[0],
            target_speed=struct.unpack('<Q', u.mem_read(LOCO + 0x78, 8))[0],
            target_height=struct.unpack('<i', u.mem_read(LOCO + 0x80, 4))[0],
            bob_phase=struct.unpack('<Q', u.mem_read(LOCO + 0x88, 8))[0],
            phase=self.read32(LOCO + 0x50),
            moving=bool(u.mem_read(LOCO + 0x4C, 1)[0]),
            destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x40, 12))),
            facing_current=self.read32(OUTPUT) & 0xFFFF,
            facing_destination=struct.unpack('<H', u.mem_read(LOCO + 0x54, 2))[0],
            body_facing=struct.unpack('<H', u.mem_read(OWNER + 0x388, 2))[0],
            slot_holder=(self.read32(cell + 0xE0) if cell else 0),
            on_bridge=bool(u.mem_read(OWNER + 0x8C, 1)[0]),
        )

    def execute(self):
        frames = []
        left_start = False
        if self.row['order'] is not None:
            self.events = []
            self.slot_events = []
            self.call(0x54b1c0, 0, [LOCO + 4, *self.row['order']])
            frames.append(dict(self.state(), speed_fractions=[], events=self.events,
                               slot_events=self.slot_events, label='move_to'))
        for _ in range(self.row['max_frames']):
            self.fractions = []
            self.events = []
            self.slot_events = []
            self.frame += 1
            self.uc.mem_write(FRAME, dwords(self.frame))
            self.call(0x54aec0, 0, [LOCO + 4])
            frame = self.state()
            frame['speed_fractions'] = self.fractions
            frame['events'] = self.events
            frame['slot_events'] = self.slot_events
            frame['label'] = 'process'
            frames.append(frame)
            if frame['phase'] != self.row['stop_phase']:
                left_start = True
            elif left_start:
                break
        return dict(frames=frames)


def centre(x, y=ROW_Y):
    return [x * 256 + CENTER, y * 256 + CENTER]


BASE = dict(rtti=1, turn_rate=4, speed=14, climb=5.0, crash=5.0, height=500, accel=2.0,
            wobbles=0.15, no_wobbles=False, deviation=40, balloon_hover=False, tarcom=False,
            start=[*centre(10), 0], facing=0x4000, target_height=0, first_frame=1000,
            max_frames=260, simple_deployer=False, deploy_to_land=False, piggyback=False,
            phase=0, moving=False, order=None, terrain=[], land_types=[], occupied_slots=[],
            raw_occupation=[], can_enter_cells={}, fnpc_cells=None, stop_phase=None)
ROWS = [
    # The gate itself: a landed, orderless owner must not move or climb.
    ('idle_landed_stays_grounded', dict(max_frames=12)),
    # A held balloon with no order is state 2 with the moving byte clear, so
    # Is_Moving_Now is false too and Update never runs.
    ('idle_hold_without_moving_is_inert',
     dict(phase=2, balloon_hover=True, start=[*centre(10), 500], target_height=500,
          max_frames=12)),
    ('takeoff_cruise_and_land', dict(order=[*centre(14), 0], stop_phase=0)),
    ('balloon_takeoff_and_hold',
     dict(order=[*centre(13), 0], balloon_hover=True, max_frames=140)),
    # Ordered to the cell it is already in, State 1 takes its in-cell branch
    # and translates instead of claiming the air slot.
    ('ascend_inside_the_destination_cell_translates',
     dict(order=[*centre(10), 0], stop_phase=0, max_frames=160)),
    # Every corridor cell's air slot is already held, so State 1 scatters.
    ('ascend_into_taken_slot_scatters',
     dict(order=[*centre(14), 0], occupied_slots=list(CORRIDOR_X), max_frames=130)),
    # A non-balloon owner ordered onto water never settles: State 4 refuses the
    # landing and claims the air slot for a hold, and State 2 immediately
    # releases it and hands back to State 4. The original alternates every frame.
    ('water_destination_alternates_hold_and_descend',
     dict(order=[*centre(13), 0], land_types=[(13, 2)], max_frames=130)),
    # The ordered cell refuses entry, so State 4 hands back to Stop_Moving,
    # whose search answers a free neighbour the owner then lands in.
    ('blocked_landing_retargets_a_free_cell',
     dict(order=[*centre(13), 0], can_enter_cells={13: 3}, fnpc_cells=[None, [12, 10]],
          stop_phase=0, max_frames=320)),
    ('hold_reorders_into_translate',
     dict(phase=2, moving=True, start=[*centre(10), 500], target_height=500,
          order=[*centre(14), 0], stop_phase=0, max_frames=200)),
    ('sloped_corridor_takeoff',
     dict(order=[*centre(13), 0], terrain=[(11, 2, 1), (12, 2, 0)], stop_phase=0,
          max_frames=320)),
]

def generate():
    return [dict(name=name, input=dict(BASE, **row), output=States(dict(BASE, **row)).execute())
            for name, row in ROWS]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Jumpjet Process 0x0054AEC0 per frame - its Is_Moving/Is_Moving_Now Update gate and the '
              'state jump table - over States 0, 1, 2, 3 and 4 on a declared flat-or-sloped cell block '
              'x=6..20 by y=9..11, entered through the original Move_To 0x0054B1C0. Covers takeoff, '
              'cruise, arrival, hold, scatter from a taken air slot, a water hold, a blocked landing and '
              'touchdown. Not State5 crash, bridges, building tops, cell objects in the reference height, '
              'the radio-contact voice or multi-owner cells.',
        entry_points={'direction_delta_init': 0x49f3a0, 'adjacent_cell_init': 0x49f2f0,
                      'cached_cell_setter': 0x41c160, 'link_to_object': 0x54ad30,
                      'move_to': 0x54b1c0, 'process': 0x54aec0, 'update': 0x54d0f0,
                      'state0': 0x54b980, 'state1': 0x54ba30, 'state2': 0x54bd30,
                      'state3': 0x54bff0, 'state4': 0x54c550,
                      'facing_current': 0x4c93d0, 'facing_snap': 0x4c9300},
        assumptions=['FPCW 0E7F, the process control word WinMain installs with _controlfp(0x300, 0x300) at '
                     '0x006BBFC1; level height 104 at 0xABC5E8 and deck 416 at 0xABC5DC; direction deltas from '
                     'the original initializer; frame counter from 1001; owner RTTI 1 (Unit); owner +0x83 clear '
                     'and +0x41B set so both Process visibility probes are skipped, +0x90 set (airborne layer), '
                     '+0x425 and +0x427 clear so the crash latch is off, +0x8C/+0x8D/+0x81 clear; per row: the '
                     'cell corridor level/slope, LandType +0xEC, raw occupation +0x124 and AltObject +0xE0, the '
                     'type block, BalloonHover +0xD6A, IsSimpleDeployer +0xE13, DeployToLand +0x6AD, the owner '
                     'piggyback byte +0x6AD and the Can_Enter_Cell answer; TarCom is a non-null marker only.'],
        substitutions=['Owner SetLocation(+0x1B4) writes the coordinate only; Mark(+0x124) records the bracket and '
                       'does NOT touch the cached cell (0x004D3780 only marks cell lists); the cached-cell setter '
                       '+0x2F8 is the original 0x0041C160 body, but its only callers are the no-op air-bucket '
                       'helpers, so +0x560 keeps its supplied value and the State 1 and 3 cell-change probes fire '
                       'every frame; '
                       'GetCell(+0x1BC) and the cell-coordinate getter (+0x2F4) answer from the owner coordinate '
                       'over the declared corridor; SetSpeedFraction(+0x544) records its argument; Can_Enter_Cell '
                       '(+0x1AC) answers per row; Set_Destination (+0x480) records its argument; the marked '
                       'predicate (+0x54) answers true; +0xF0, +0xF4, +0x150, +0x18C, +0x198, +0x3DC, +0x488, '
                       '+0x48C and +0x558 are no-ops carrying their call-site stack cleanup; air-bucket '
                       'bookkeeping 0x004134A0/0x004135D0/0x004138C0, base link 0x0055A710, radio contact '
                       '0x0065AD30, fog 0x00567DA0, crate pickup 0x00481A00, 0x006385C0 and Submit_Object '
                       '0x004A9720 are no-ops; the air-slot query 0x004135A0 and set/clear 0x00487D70 are '
                       'reimplemented over the corridor cells and their calls recorded; the FNPC search 0x0056DC20 '
                       'answers a declared cell sequence, which feeds both Move_To and the Stop_Moving re-target.'],
    ))
