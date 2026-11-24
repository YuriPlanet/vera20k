"""Original Jumpjet Process and additive same-VM Unit/Foot callback histories.

Process is the real driver here: it gates Update_Coordinates_And_Altitude
0x0054D0F0 on Is_Moving 0x0054AE50 (moving byte +0x4C) or Is_Moving_Now
0x0054D0D0 (state not 0 and not 2), then dispatches the state at receiver +0x50
through the jump table 0x0054B19C - State0 0x0054B980, State1 0x0054BA30,
State2 0x0054BD30, State3 0x0054BFF0, State4 0x0054C550. Rows therefore cover
the gate itself, not only the handlers: an idle landed owner records that
nothing runs.

The ten exact legacy rows use a declared block of CellClass objects,
x=6..20 by y=9..11, each with
its own level, slope, LandType +0xEC, raw occupation bytes and AltObject air
slot +0xE0; every other lookup resolves to the shared dummy. Rows describe the
flight line y=10; the rows either side exist so a scatter to a north or south
neighbour lands on a declared cell rather than the dummy.
Their Move_To 0x0054B1C0 runs against a supplied FNPC result. The additive
composed controls instead run the actual FNPC and original Unit/Foot/Jumpjet,
Mark, list, occupation and air-tracker bodies in one existing reader VM.
See jumpjet_states.md for physical input binding and the two coverage profiles.

Not covered: State5 crash 0x0054CA90, bridges, building tops, cell objects in
the reference height, the radio-contact voice and multi-owner cells.
"""
from pathlib import Path
import hashlib
import json
import os
import struct
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EDI,
    UC_X86_REG_EIP, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_FPCW,
)
from tools.native_oracle import (NativeCallTrace, RET_MAGIC, changed_byte_spans,
                                finish_vectors, provenance, initialize_empty_windows_seh,
                                checked_is_bad_read_ptr_transport, reconstruct_byte_spans)
from tools.rmg_oracle.gen_rng_vectors import SEED_FN
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


# The original ten rows remain their own legacy profile. The additive profile
# below uses original Unit/Foot/Jumpjet vtables in one existing INI reader VM.
LEGACY_STATES_SHA256 = '43c492a0299ab35088312e4706f956eba2346f37a0c64ba7b54e9ed8f8b56d21'
COMPOSED_STORAGE = 0x28000000
COMPOSED_GUARD = bytes.fromhex('d3c7915b')
COMPOSED_BOUNDARIES = {
    0x54AEC0: 'process_entry', 0x54AEF5: 'post_update_alive_gate',
    0x54B050: 'dispatch_state1', 0x54B064: 'dispatch_state3',
    0x54B06E: 'dispatch_state4', 0x54B193: 'process_final_al',
    0x54B199: 'process_true_ret', 0x54BB32: 'state1_callback_return',
    0x54BB39: 'state1_save_post_callback_mark', 0x54BB42: 'state1_mark0_return',
    0x54BB49: 'state1_before_phase3', 0x54BB55: 'state1_taken_true_ret',
    0x54BB69: 'state1_free_claim_return', 0x54BB70: 'state1_free_save_mark',
    0x54BB79: 'state1_free_mark0_return', 0x54BB80: 'state1_before_phase2',
    0x54BB8C: 'state1_free_true_ret', 0x54C348: 'state3_callback_return',
    0x54C4FD: 'state3_live_destination_tail', 0x54C506: 'state3_live_land_lookup',
    0x54C53B: 'state3_before_height_write', 0x54C54B: 'state3_true_ret',
    0x487DC5: 'slot_claim_refused_ret', 0x487DE1: 'slot_claim_success_ret',
    0x487DB7: 'slot_release_ret', 0x4E0113: 'remembered_slot_changed_ret',
    0x4E011F: 'remembered_slot_initial_ret', 0x4D96C2: 'foot_timer_tail',
    0x4D9711: 'foot_setter_true_ret', 0x743170: 'unit_setter_true_ret',
    0x743184: 'unit_setter_refused_ret', 0x4138DA: 'air_update_cache_return',
    0x41399A: 'air_update_bucket_compare', 0x413A24: 'air_update_epilogue',
    0x413A29: 'air_update_true_ret',
    0x54C609: 'state4_callback_mark0_return', 0x54C61D: 'state4_taken_true_ret',
    0x54C641: 'state4_free_mark0_return', 0x54C65E: 'state4_free_true_ret',
    0x54C8EA: 'landing_before_clear_moving', 0x54C905: 'landing_null_setter_return',
}


class ComposedStates:
    """Additive complete-call histories; this contains no flight implementation.

    Original constructors/readers supply stock type and Rules values. Existing
    Unit fixture, Mark/discovery, FNPC plane and tracker/display owners supply
    their declared world storage. Every reached gameplay body executes.
    """

    def __init__(self, inputs, stock_report, case):
        from tools.rules_oracle.bridge_anim_inputs import Reader
        from tools.spatial_oracle.building_body_rules import INI
        from tools.projectile_oracle.bridge_render_inputs import lexical
        from tools.spatial_oracle.track_destination import make_destination_fixture
        from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE as UNIT_TYPE, LOCO as UNIT_LOCO, SP
        from tools.spatial_oracle.unit_source_scatter import CELLS, SCENARIO as UNIT_SCENARIO, MAP
        from tools.spatial_oracle.unit_entry import HOUSE
        from tools.spatial_oracle.jumpjet_coordinates import initialize_jumpjet_levels
        from tools.spatial_oracle.object_flight_height import initialize_object_scalars
        from tools.spatial_oracle.jumpjet_entry_discovery import initialize_mark_projection, initialize_discovery_prefix
        from tools.spatial_oracle.fly_landing_phase import initialize_air_tracker, initialize_display_layers
        from tools.spatial_oracle.house_base_projection import initialize_nearby_zone_plane

        self.case = case
        self.stage = 'inputs'
        self.record = False
        self.boundaries, self.steps, self.events, self.writes, self.draws = [], [], [], [], []
        self.actor, self.typ, self.loco, self.sp = ACTOR, UNIT_TYPE, UNIT_LOCO, SP
        self.cells, self.map = CELLS, MAP
        self.other = COMPOSED_STORAGE + 0x60000
        self.reader = Reader(Path(inputs), {})
        self.uc = u = self.reader.u
        initialize_empty_windows_seh(u)
        self.read32 = self.reader.read32
        self.native_calls = NativeCallTrace(u, self.read32)
        u.hook_add(UC_HOOK_CODE, self.observe)
        u.hook_add(UC_HOOK_MEM_WRITE, self.written)
        self.original_text = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
        # Original Rules constructor, including BlockagePathDelay60 and the
        # initial Facing ROT. Spare registries/storage are the existing owner.
        rules = self.reader.alloc(0x2000)
        u.mem_write(0x8871E0, dwords(rules))
        self.reader.invoke(0x665650, rules)
        self.rules = rules
        u.mem_write(0xA83CE0, dwords(0x7EB6D4, self.reader.alloc(4096), 1024, 1, 0, 10))
        self.reader.invoke(0x7470D0, UNIT_TYPE, (self.reader.cstring('SHAD'),))
        self.input_constructor = self.type_state()
        # Reuse the existing Mission/ENGINEER Country ctor and selected native
        # reader sequence for the supplied human House's landing fog callback.
        self.country = self.reader.alloc(0x400)
        u.mem_write(0xA83C98, dwords(0x7EB6D4, self.reader.alloc(128 * 4), 128, 1, 0, 10))
        self.reader.invoke(0x5113F0, self.country, (self.reader.cstring('Americans'),))
        self.input_country_constructor = bytes(u.mem_read(self.country, 0x400)).hex()
        self.input_layers = []
        report = json.loads(Path(stock_report).read_text())
        assert report['requested_type'] == 'SHAD' and report['mode']['id'] == 1
        names = ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'XMP03T4.MAP')
        land_names = {self.reader.string(self.read32(0x839D68 + i * 4)) for i in range(12)}
        for name, source in zip(names, report['source_layers']):
            path = Path(inputs) / name
            if not path.exists():
                assert name == 'LANGRULE.INI' and not source['source_present']
                self.input_layers.append(dict(file=name, absent=True, before=self.type_state(),
                                              after=self.type_state()))
                continue
            raw = path.read_bytes()
            digest = hashlib.sha256(raw).hexdigest()
            assert digest == source['source']['source_sha256'], (name, 'physical layer identity')
            sections, lines = lexical(raw, {'SHAD', 'General', 'AI', 'Americans'} | land_names)
            self.reader.make_ini(sections)
            before = self.type_state()
            #526810 returns the found section pointer, not an AL boolean.
            admitted = bool(self.reader.invoke(0x526810, INI, (UNIT_TYPE + 0x24,)))
            reads = []
            if admitted:
                # Exact reached reader regions; defaults are retained native
                # constructor fields. Parent/ART/full-type discovery is outside
                # this bounded input witness, never a supplied scalar result.
                for begin, end in ((0x7121D1, 0x7121EB), (0x7123ED, 0x71243A),
                                   (0x712553, 0x71256D), (0x713377, 0x713397),
                                   (0x714802, 0x71481C), (0x714B14, 0x714B35),
                                   (0x714D8E, 0x714DAF), (0x7150A8, 0x715227)):
                    self.block(begin, end, {UC_X86_REG_EBP: UNIT_TYPE,
                               UC_X86_REG_EBX: UNIT_TYPE + 0x24,
                               UC_X86_REG_ESI: INI, UC_X86_REG_EDI: INI})
                    reads.append([hex(begin), hex(end)])
                # DeployToFire immediately precedes and supplies the AL stored
                # by the interleaved IsSimpleDeployer block at74768E.
                self.block(0x74766B, 0x74769F, {UC_X86_REG_EDI: UNIT_TYPE,
                           UC_X86_REG_EBX: INI, UC_X86_REG_EBP: UNIT_TYPE + 0x24})
                reads.append(['0x74766b', '0x74769f'])
                u.mem_write(SP + 0x380, dwords(INI))
                self.block(0x71605E, 0x716090, {UC_X86_REG_EBP: UNIT_TYPE,
                           UC_X86_REG_EBX: UNIT_TYPE + 0x24})
                reads.append(['0x71605e', '0x716090'])
            self.reader.invoke(0x674000, 0, (INI,))
            if 'AI' in sections:
                self.block(0x673A0C, 0x673A37, {UC_X86_REG_ESI: rules, UC_X86_REG_EDI: INI})
            if self.reader.invoke(0x526810, INI, (self.country + 0x24,)):
                # Start at the preceding Multiplay read: its AL is stored in
                # the interleaved passive block, preserving both native flags.
                self.block(0x511A5B, 0x511A95, {UC_X86_REG_EBX: self.country,
                           UC_X86_REG_ESI: INI, UC_X86_REG_EDI: self.country + 0x24})
            self.input_layers.append(dict(file=name, sha256=digest, section_admitted=admitted,
                source_lines=lines, before=before, reads=reads, after=self.type_state(),
                blockage_path_delay=self.read32(rules + 0x1768),
                country_multiplay=list(u.mem_read(self.country + 0x1A5, 2))))
        self.input_final = self.type_state()
        self.input_type_raw = bytes(u.mem_read(UNIT_TYPE, 0xF00)).hex()
        self.input_rules_raw = bytes(u.mem_read(rules, 0x2000)).hex()
        expected = report['final_type']
        for flag in ('balloon_hover', 'is_simple_deployer', 'deploy_to_land', 'jumpjet',
                     'hover_attack', 'crashable', 'tilt_crash_jumpjet'):
            assert self.input_final[flag] == expected[flag], ('native/type report disagreement', flag)
        params = expected['jumpjet_params']
        for field in ('turn_rate', 'height', 'deviation', 'no_wobbles'):
            assert self.input_final[field] == params[field], ('native/type report disagreement', field)
        assert self.input_final['speed'] == params['speed']['value']
        assert bytes(u.mem_read(UNIT_TYPE + 0x34C, 16)) == bytes(u.mem_read(0x7E9AC0, 16))
        for field in ('climb', 'crash', 'accel', 'wobbles'):
            assert self.input_final[field + '_bits'] == params[field]['binary32_bits']
        self.stock_comparison = dict(report_sha256=hashlib.sha256(Path(stock_report).read_bytes()).hexdigest(),
                                     checked_native_fields=list(self.input_final))
        # The existing Unit owner reuses this same VM and original heap/type.
        u.mem_write(UNIT_LOCO, b'\xA5' * 0x98)
        self.uc, self.call, self.read32 = make_destination_fixture(dict(case, family='jumpjet',
            rot=self.input_final['unit_rot'],
            retained_type=True, rules_pointer=rules), uc=u)
        u.mem_map(COMPOSED_STORAGE, 0x80000)
        initialize_mark_projection(u, projection=COMPOSED_STORAGE,
                                   secondary=COMPOSED_STORAGE + 0x8000)
        initialize_air_tracker(u, SP, buffers=COMPOSED_STORAGE + 0x10000, extent=(40, 40))
        initialize_display_layers(u, SP, buffers=COMPOSED_STORAGE + 0x20000)
        initialize_nearby_zone_plane(u, size=(16, 16), plane=COMPOSED_STORAGE + 0x48000,
                                    rows=COMPOSED_STORAGE + 0x4D000)
        u.mem_write(MAP + 0xF4, dwords(16, 16, 0, 0, 32, 32))
        u.mem_write(0x87F924, dwords(0xC00000))
        u.mem_write(0x887324, dwords(COMPOSED_STORAGE + 0x30000))
        # make_source_fixture's historical speed table is overwritten by the
        # same physical land reader after its map binding, not a supplied cost.
        for name in names:
            path = Path(inputs) / name
            if path.exists():
                sections, _ = lexical(path.read_bytes(), land_names)
                self.reader.make_ini(sections)
                self.reader.invoke(0x674000, 0, (INI,))
        u.mem_write(ACTOR + 0x6C, dwords(100))
        u.mem_write(ACTOR + 0x90, b'\x01')
        u.mem_write(ACTOR + 0x94, dwords(-1))
        u.mem_write(ACTOR + 0x260, dwords(4))
        for x, y, land in case.get('land_types', []):
            u.mem_write(self.cell(x, y) + 0xEC, dwords(land))
        # Real Foot constructor's tube-index store. Zero is a live tube index
        # and later landing/change-cell code dereferences its House table.
        self.block(0x4D31F1, 0x4D31FB, {UC_X86_REG_ESI: ACTOR})
        u.mem_write(HOUSE + 0x1EC, b'\x01\x01')
        u.mem_write(HOUSE + 0x34, dwords(self.country))
        u.mem_write(0xA83D4C, dwords(HOUSE))
        u.mem_write(0xA8B238, dwords(5))
        initialize_discovery_prefix(u, self.block, owner=ACTOR)
        initialize_jumpjet_levels(self.call)
        # Object's distinct startup scalars are consumed by the actual owner
        # IsHighFlying5F6B90 inside Jumpjet Layer54B8D0. A cold AC13C8=0
        # falsely makes marked low takeoff Z report Air and skips CellPut.
        object_scalars = (0xAC13C8, 0xAC13BC)
        self.input_object_startup = dict(table='0x008141D8',
            before={f'0x{address:08X}': self.read32(address) for address in object_scalars})
        entries = initialize_object_scalars(u, self.call)
        self.input_object_startup.update(initializers=[f'0x{entry:08X}' for entry in entries],
            after={f'0x{address:08X}': self.read32(address) for address in object_scalars})
        assert [self.read32(address) for address in object_scalars] == [104, 416]
        # Map's original CRT startup supplies the projection divisor consumed
        # by actual IsShrouded586360 inside CellPut. This same four-entry
        # startup is established by the existing AnyTown/repair fixtures.
        for entry in (0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
            self.call(entry, 0, [])
        assert self.read32(0xABDE88) == 104
        # This sparse fixture has no registered IsoTile/TMP, so Recalc(-1)
        # invalidates Cell+38=0 toFFFF and uses Clear fallback. Supplemental
        # LandType writes above supply cached Cell+EC only, not base terrain.
        self.input_tile_registry = dict(registered_tile_count=self.read32(0xA8ED38),
                                       registered_tile_array_pointer=f'0x{self.read32(0xA8ED2C):08X}')
        assert self.input_tile_registry == dict(registered_tile_count=0,
                                               registered_tile_array_pointer='0x00000000')
        for entry in (0x49F2F0, 0x49F3A0):
            self.call(entry, 0, [])
        self.call(0x54AD30, 0, [UNIT_LOCO + 4, ACTOR])
        self.input_link = bytes(u.mem_read(UNIT_LOCO + 0x1C, 0x24)).hex()
        # Distinct native NullCell prestates: neither field is derived from
        # grid membership or from the other's cached value.
        u.mem_write(ACTOR + 0x560, bytes(u.mem_read(0x8B3C58, 4)))
        u.mem_write(ACTOR + 0x564, bytes(u.mem_read(0x8B3D88, 4)))
        self.rngs = {'main': 0x886B88, 'scenario': UNIT_SCENARIO + 0x218, 'mapgen': 0xABE890}
        for ptr in self.rngs.values():
            self.call(SEED_FN, ptr, [case.get('seed', 31)])
        # Existing real Unit footprint getter/lazy storage, with only CRT
        # atexit transport supplied. No Infantry one-cell vtable override.
        self.call(0x5F5B90, ACTOR, [0, COMPOSED_STORAGE + 0x50000])
        self.call(0x4D3780, ACTOR, [1])
        if not case.get('marked', True):
            self.call(0x4D3780, ACTOR, [0])
        self.call(0x4A9720, MAP, [ACTOR])
        if case.get('profile') == 'uninit':
            # Bounded active-game retirement input. The ordinary selected
            # Process histories do not claim a whole-match loader. Here the
            # real count ctor/add and deferred-vector startup bind the fields
            # whole UnInit consumes; no Limbo/expiry result is substituted.
            u.mem_write(0xA8E9A0, b'\x01')
            self.call(0x49F9B0, HOUSE + 0x5564, [])
            self.call(self.read32(self.read32(UNIT_TYPE) + 0x40), UNIT_TYPE, [])
            self.call(0x49FA00, HOUSE + 0x5564, [u.reg_read(UC_X86_REG_EAX)])
            self.block(0x7353F2, 0x7353F8, {UC_X86_REG_ESI: ACTOR, UC_X86_REG_EDI: 0xFFFFFFFF})
            self.block(0x725850, 0x725886, {})
        if case.get('taken', True):
            sx, sy = case.get('foreign_slot', [12, 10])
            u.mem_write(self.other, dwords(0x7F5C70))
            u.mem_write(self.other + 0x14, dwords(5))
            u.mem_write(self.other + 0x6C, dwords(100))
            u.mem_write(self.other + 0x90, b'\x01')
            u.mem_write(self.other + 0x6C0, dwords(UNIT_TYPE))
            u.mem_write(self.other + 0x6C4, dwords(UNIT_TYPE))
            u.mem_write(self.other + 0x9C,
                        dwords(*case.get('foreign_xyz', [sx * 256 + 128, sy * 256 + 128, 500])))
            u.mem_write(self.other + 0x564, bytes(u.mem_read(0x8B3D88, 4)))
            self.call(0x487D70, self.cell(sx, sy), [self.other])
            if case.get('foreign_air_registered', True):
                self.call(0x4134A0, 0x887888, [self.other])
        for base, size in ((ACTOR, 0x700), (UNIT_LOCO, 0x98), (self.other, 0x700)):
            u.mem_write(base + size, COMPOSED_GUARD)
        self.cell_baseline = bytes(u.mem_read(CELLS, 0x90000))
        self.memory_regions = dict(cells=(CELLS, 0x90000), world=(COMPOSED_STORAGE, 0x80000),
                                   map=(MAP, 0x200), dummy=(0xABDC50, 0x200),
                                   air_headers=(0x887888, 400 * 24), display_headers=(0x8A0360, 5 * 24),
                                   house=(HOUSE, 0x6000), deferred_header=(0xB0F698, 24))
        self.memory_baselines = {name: bytes(u.mem_read(ptr, size))
                                 for name, (ptr, size) in self.memory_regions.items()}
        self.vtables = {hex(ptr): hashlib.sha256(bytes(u.mem_read(ptr, size))).hexdigest()
                       for ptr, size in ((0x7F5C70, 0x600), (0x7F6218, 0x100), (0x7ECD44, 0x110))}
        self.record = True
        self.stage = 'ready'

    def cell(self, x, y):
        assert 0 <= x < 32 and 0 <= y < 32, ('outside mapped composed cells', x, y)
        return self.cells + (y * 32 + x) * 0x200

    def block(self, begin, end, registers, required=None):
        # Reuse Entry's established original interior-block execution owner.
        from tools.spatial_oracle.jumpjet_entry_discovery import Entry
        Entry.block(self, begin, end, registers, required)

    def type_state(self):
        u, p = self.uc, self.typ
        flag = lambda off: bool(u.mem_read(p + off, 1)[0])
        signed = lambda off: struct.unpack('<i', u.mem_read(p + off, 4))[0]
        return dict(balloon_hover=flag(0xD6A), is_simple_deployer=flag(0xE13),
                    deploy_to_land=flag(0x6AD), jumpjet=flag(0xD94),
                    hover_attack=flag(0x390), crashable=flag(0xD95),
                    tilt_crash_jumpjet=flag(0xD22), unit_rot=signed(0x71C),
                    locomotor_clsid_hex=bytes(u.mem_read(p + 0x34C, 16)).hex(),
                    speed_type=signed(0x67C), movement_zone=signed(0x5B4),
                    turn_rate=signed(0xD70), speed=signed(0xD74), height=signed(0xD80),
                    deviation=signed(0xD90), no_wobbles=flag(0xD8C),
                    **{name + '_bits': f'0x{self.read32(p + off):08x}' for name, off in
                       (('climb', 0xD78), ('crash', 0xD7C), ('accel', 0xD84), ('wobbles', 0xD88))})

    def snapshot(self, label):
        from tools.spatial_oracle.fly_landing_phase import AIR_TRACKER
        from tools.spatial_oracle.crate_ground_membership import LAYERS
        u, read = self.uc, self.read32
        pc, sp = u.reg_read(UC_X86_REG_EIP), u.reg_read(UC_X86_REG_ESP)
        self.native_calls.returned(pc, sp)
        for base, size in ((self.actor, 0x700), (self.loco, 0x98), (self.other, 0x700)):
            assert bytes(u.mem_read(base + size, 4)) == COMPOSED_GUARD
        assert u.reg_read(UC_X86_REG_FPCW) == 0x0E7F, ('ambient FPCW', label)
        cells = []
        for y in range(32):
            for x in range(32):
                pointer = self.cell(x, y)
                raw = bytes(u.mem_read(pointer, 0x200))
                baseline = self.cell_baseline[((y * 32 + x) * 0x200):((y * 32 + x + 1) * 0x200)]
                if raw != baseline or read(pointer + 0xE0) or read(pointer + 0xE4) or read(pointer + 0xE8):
                    cells.append(dict(cell=[x, y], raw_hex=raw.hex(), slot=hex(read(pointer + 0xE0)),
                                      ground_head=hex(read(pointer + 0xE4)), bridge_head=hex(read(pointer + 0xE8))))
        def vectors(base, count, capacity):
            out = []
            for index in range(count):
                header = base + index * 24
                length = read(header + 0x10)
                assert length <= capacity, ('vector capacity exceeded', hex(header), length)
                if length:
                    out.append(dict(index=index, raw_header=bytes(u.mem_read(header, 24)).hex(),
                                    items=[hex(read(read(header + 4) + i * 4)) for i in range(length)]))
            # All headers, including empty ones, are losslessly retained by
            # state.memory against memory_baselines. Avoid repeating the same
            # 9,600-byte block at every nested boundary. Active order/headers
            # stay explicit for direct production-reader comparisons.
            return dict(members=out)
        xyz = lambda pointer: list(struct.unpack('<iii', u.mem_read(pointer, 12)))
        packed_cell = lambda pointer: list(struct.unpack('<hh', u.mem_read(pointer, 4)))
        signed = lambda pointer: struct.unpack('<i', u.mem_read(pointer, 4))[0]
        def cell_identity(pointer):
            if not pointer:
                return None
            assert self.cells <= pointer < self.cells + 32 * 32 * 0x200
            assert (pointer - self.cells) % 0x200 == 0
            return packed_cell(pointer + 0x24)
        def facing(pointer):
            # Stored FacingClass words, not an extra Get_Current invocation
            # while an original composed callback is paused. The middle timer
            # word remains opaque in foot_raw_hex/jumpjet_raw_hex.
            return dict(destination_word=struct.unpack('<H', u.mem_read(pointer, 2))[0],
                        current_word=struct.unpack('<H', u.mem_read(pointer + 4, 2))[0],
                        timer=[signed(pointer + 8), signed(pointer + 0x10)],
                        rate_word=struct.unpack('<H', u.mem_read(pointer + 0x14, 2))[0])
        nav_count = read(self.actor + 0x598)
        assert nav_count <= 10
        state = dict(foot_raw_hex=bytes(u.mem_read(self.actor, 0x700)).hex(),
                     jumpjet_raw_hex=bytes(u.mem_read(self.loco, 0x98)).hex(),
                     # The other whole Foot block is inside the retained world
                     # baseline/deltas, including its guard and untouched tail.
                     position=xyz(self.actor + 0x9C), destination=xyz(self.loco + 0x40),
                     phase=read(self.loco + 0x50), moving=u.mem_read(self.loco + 0x4C, 1)[0],
                     landing_latched=bool(u.mem_read(self.loco + 0x90, 1)[0]),
                     marked=u.mem_read(self.actor + 0x74, 1)[0], alive=u.mem_read(self.actor + 0x90, 1)[0],
                     health=signed(self.actor + 0x6C), on_bridge=bool(u.mem_read(self.actor + 0x8C, 1)[0]),
                     mission=signed(self.actor + 0xAC), queued_mission=signed(self.actor + 0xB4),
                     mission_status=signed(self.actor + 0xBC),
                     path=list(struct.unpack('<4i', u.mem_read(self.actor + 0x5E0, 16))),
                     reference_cell=packed_cell(self.actor + 0x558),
                     tracker_cell=packed_cell(self.actor + 0x560), slot_cell=packed_cell(self.actor + 0x564),
                     other_tracker_cell=packed_cell(self.other + 0x560),
                     other_slot_cell=packed_cell(self.other + 0x564),
                     nav=hex(read(self.actor + 0x5A4)), aux=hex(read(self.actor + 0x5A0)),
                     nav_cell=cell_identity(read(self.actor + 0x5A4)),
                     aux_cell=cell_identity(read(self.actor + 0x5A0)),
                     movement_timer=[signed(self.actor + 0x640), signed(self.actor + 0x648)],
                     blocked_timer=[signed(self.actor + 0x668), signed(self.actor + 0x670)],
                     blocked=bool(u.mem_read(self.actor + 0x6B7, 1)[0]), retries=signed(self.actor + 0x64C),
                     body_facing=facing(self.actor + 0x388), loco_facing=facing(self.loco + 0x54),
                     current_speed_bits=bytes(u.mem_read(self.loco + 0x70, 8))[::-1].hex(),
                     target_speed_bits=bytes(u.mem_read(self.loco + 0x78, 8))[::-1].hex(),
                     target_height=signed(self.loco + 0x80),
                     bob_phase_bits=bytes(u.mem_read(self.loco + 0x88, 8))[::-1].hex(),
                     nav_queue=[hex(read(read(self.actor + 0x58C) + i * 4)) for i in range(nav_count)],
                     cells=cells, air=vectors(AIR_TRACKER, 400, 16), display=vectors(LAYERS, 5, 16),
                     rng={name: bytes(u.mem_read(ptr, 0x3F4)).hex() for name, ptr in self.rngs.items()},
                     memory={name: dict(sha256=hashlib.sha256(raw).hexdigest(),
                                       changed_spans=changed_byte_spans(self.memory_baselines[name], raw))
                             for name, (ptr, size) in self.memory_regions.items()
                             for raw in (bytes(u.mem_read(ptr, size)),)})
        state['nav_queues'] = []
        for off in (0x588, 0x5AC):
            ptr, capacity, count = read(self.actor + off + 4), read(self.actor + off + 8), read(self.actor + off + 0x10)
            assert 0 <= count <= capacity <= 16
            assert ptr or not capacity
            state['nav_queues'].append(dict(offset=hex(off), raw_header=bytes(u.mem_read(self.actor + off, 24)).hex(),
                                           backing_hex=bytes(u.mem_read(ptr, capacity * 4)).hex() if capacity else ''))
        capacity = read(0xB0F6A0)
        assert capacity <= 16
        state['deferred_queue'] = dict(raw_header=bytes(u.mem_read(0xB0F698, 24)).hex(),
                                      backing_hex=bytes(u.mem_read(read(0xB0F69C), capacity * 4)).hex() if capacity else '')
        self.boundaries.append(dict(label=label, stage=self.stage, pc=hex(pc), esp=hex(sp),
                                    eax=u.reg_read(UC_X86_REG_EAX), frame=read(FRAME),
                                    fpcw=u.reg_read(UC_X86_REG_FPCW), state=state))
        return len(self.boundaries) - 1

    def observe(self, u, pc, _size, _data):
        from tools.spatial_oracle.nearby_raw_occupation import nearby_arguments
        sp = u.reg_read(UC_X86_REG_ESP)
        if self.reader.guid_transport(u, pc, sp, self.events):
            return
        if checked_is_bad_read_ptr_transport(u, pc, sp, self.events):
            return
        if pc == 0x7C978A:
            # CRT registration transport; no gameplay return is supplied.
            self.reader.ret(0)
            return
        if not self.record:
            return
        specs = {
            0x54AEC0: ('jumpjet_process', 1, 4), 0x741970: ('unit_set_destination', 2, 8),
            0x4D94B0: ('foot_set_destination', 2, 8), 0x54B1C0: ('jumpjet_move_to', 4, 16),
            0x54B4D0: ('jumpjet_stop', 1, 4), 0x54BA30: ('state1', 0, 0),
            0x54BD30: ('state2', 0, 0),
            0x54BFF0: ('state3', 0, 0), 0x54C550: ('state4', 0, 0),
            0x4135A0: ('slot_query', 1, 4), 0x487D70: ('cell_slot', 1, 4),
            0x4E00B0: ('remembered_slot', 1, 4), 0x4D3780: ('foot_mark', 1, 4),
            0x5683C0: ('map_put', 2, 8), 0x5687F0: ('map_remove', 2, 8),
            0x47E8A0: ('cell_put', 2, 8), 0x47EA90: ('cell_remove', 2, 8),
            0x47D2B0: ('cell_recalculate', 1, 4), 0x4134A0: ('air_add', 1, 4),
            0x4135D0: ('air_remove', 1, 4), 0x4138C0: ('air_update', 3, 12),
            0x41C160: ('tracker_cell_setter', 1, 4),
            0x7441B0: ('unit_occupation_set', 1, 4),
            0x744210: ('unit_occupation_clear', 1, 4),
            0x5F6060: ('object_set_z', 1, 4),
            0x4DE5D0: ('foot_uninit', 0, 0), 0x5F65F0: ('object_uninit', 0, 0),
            0x7440B0: ('unit_limbo', 0, 0), 0x4DB260: ('foot_limbo', 0, 0),
            0x6F6AC0: ('techno_limbo', 0, 0), 0x5F4D30: ('object_limbo', 0, 0),
            0x7258D0: ('pointer_expired', 0, 0),
            0x4A9720: ('display_submit', 1, 4), 0x4A9770: ('display_remove', 1, 4),
            0x56DC20: ('fnpc', 15, 60), 0x56E7C0: ('fnpc_rectangle', 9, 36),
            SEED_FN: ('rng_seed', 1, 4),
            0x65C780: ('rng_raw', 0, 0), 0x65C7E0: ('rng_ranged', 2, 8),
        }
        returned = self.native_calls.returned(pc, sp)
        for index in returned:
            row = self.native_calls.calls[index]
            if row['name'] in ('unit_set_destination', 'foot_set_destination', 'jumpjet_move_to'):
                self.snapshot(row['name'] + '_composed_return')
        if pc in specs:
            index = self.native_calls.entered(pc, sp, specs[pc])
            row = self.native_calls.calls[index]
            row['stage'] = self.stage
            if pc == 0x56DC20:
                row['request'] = nearby_arguments(u, self.read32, sp)
            if pc in (SEED_FN, 0x65C780, 0x65C7E0):
                receiver = u.reg_read(UC_X86_REG_ECX)
                row['stream'] = next((name for name, ptr in self.rngs.items() if ptr == receiver), hex(receiver))
        if pc in (0x65C84B, 0x65C79D):
            receiver = u.reg_read(UC_X86_REG_EDX if pc == 0x65C84B else UC_X86_REG_ECX)
            self.draws.append(dict(stage=self.stage, pc=hex(pc), raw=u.reg_read(UC_X86_REG_ESI),
                                   receiver=hex(receiver),
                                   stream=next((name for name, ptr in self.rngs.items() if ptr == receiver), hex(receiver)),
                                   indices_before=[self.read32(receiver + 4), self.read32(receiver + 8)]))
        if pc in COMPOSED_BOUNDARIES:
            self.snapshot(COMPOSED_BOUNDARIES[pc])

    def written(self, u, _access, address, size, value, _data):
        assert address + size <= 0x401000 or address >= 0x7E1000, ('native text write', hex(address))
        if self.record and (self.actor <= address < self.actor + 0x700 or
                            self.loco <= address < self.loco + 0x98):
            self.writes.append(dict(stage=self.stage, pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                    address=hex(address), size=size, raw=value))

    def invoke_step(self, label, entry, receiver, args):
        self.stage = label
        step = dict(label=label, before=self.snapshot(label + '_before'),
                    call_start=len(self.native_calls.calls), draw_start=len(self.draws))
        self.call(entry, receiver, args)
        assert self.uc.reg_read(UC_X86_REG_EIP) == RET_MAGIC
        assert self.uc.reg_read(UC_X86_REG_ESP) == self.sp + 4 + len(args) * 4
        step.update(after=self.snapshot(label + '_after'), call_end=len(self.native_calls.calls),
                    draw_end=len(self.draws), eax=self.uc.reg_read(UC_X86_REG_EAX))
        assert not self.native_calls.pending, ('unmatched composed native returns', self.native_calls.pending)
        if entry == 0x54AEC0:
            assert any(row['label'] == 'process_true_ret' and row['stage'] == label for row in self.boundaries)
        self.steps.append(step)

    def supplied(self, label, writes):
        """Declare supplemental retained inputs between complete native calls."""
        self.stage = label
        before = self.snapshot(label + '_before_inputs')
        for pointer, raw in writes:
            self.uc.mem_write(pointer, raw)
        self.steps.append(dict(label=label, supplied=[dict(address=hex(ptr), raw_hex=raw.hex())
                                                      for ptr, raw in writes],
                               before=before, after=self.snapshot(label + '_after_inputs')))

    def process_visit(self, visit):
        self.uc.mem_write(FRAME, dwords(self.case.get('frame', 100) + visit + 1))
        self.invoke_step(f'process_{visit}', 0x54AEC0, 0, [self.loco + 4])

    def execute(self):
        profile = self.case.get('profile', 'ordinary')
        continuation = self.case.get('continuation', 'translate')
        if profile == 'ordinary':
            self.invoke_step('cell_order', 0x741970, self.actor,
                             [self.cell(*self.case.get('order', [14, 10])), 1])
            transitioned = False
            for visit in range(self.case.get('max_visits', 220)):
                self.process_visit(visit)
                if visit == self.case.get('scenario_seed0_after_visit'):
                    # Declared load handoff only: Scenario Load689470 calls
                    # post-read683560, whose683564..683571 executes this Seed(0).
                    # Reuse the original RNG owner in this live VM; no whole
                    # Save/Load, raw state injection or supplied Seed result.
                    assert continuation == 'arrival'
                    self.invoke_step('scenario_load_seed0', SEED_FN,
                                     self.rngs['scenario'], [0])
                transitioned |= any(row['label'] in ('state1_taken_true_ret', 'state1_free_true_ret')
                                    for row in self.boundaries)
                if not transitioned:
                    continue
                if continuation == 'arrival':
                    if self.read32(self.loco + 0x50) == 0 and not self.uc.mem_read(self.loco + 0x4C, 1)[0]:
                        break
                elif any(row['label'] == 'dispatch_state3' and row['stage'] == self.stage
                         for row in self.boundaries):
                    break
            else:
                raise AssertionError('ordinary composed transition/continuation was not reached')
            assert transitioned
            if continuation == 'stop':
                self.invoke_step('unit_stop', 0x741970, self.actor, [0, 1])
                for stop_visit in range(visit + 1, visit + 161):
                    self.process_visit(stop_visit)
                    if self.read32(self.loco + 0x50) == 0 and not self.uc.mem_read(self.loco + 0x4C, 1)[0]:
                        break
                else:
                    raise AssertionError('ordinary Stop continuation did not land')
        elif profile == 'slot_lifetime':
            self.invoke_step('register_air', 0x4134A0, 0x887888, [self.actor])
            for label, xy, owner in (
                ('refused_foreign_claim', [12, 10], self.actor),
                ('first_claim', [10, 10], self.actor),
                ('replacement_claim', [11, 10], self.actor),
                ('release_occupied_owner_slot', [11, 10], 0),
                ('claim_before_same_owner', [11, 10], self.actor),
                ('same_owner_claim', [11, 10], self.actor),
                ('release_owner_slot', [11, 10], 0),
                ('release_empty_slot', [11, 10], 0),
                ('release_foreign_slot', [12, 10], 0),
                ('claim_released_slot', [12, 10], self.actor),
                ('final_release', [12, 10], 0)):
                self.invoke_step(label, 0x487D70, self.cell(*xy), [owner])
        elif profile == 'uninit':
            self.invoke_step('register_air', 0x4134A0, 0x887888, [self.actor])
            self.invoke_step('claim_before_uninit', 0x487D70, self.cell(10, 10), [self.actor])
            self.invoke_step('unit_uninit', 0x4DE5D0, self.actor, [])
            # Whole UnInit queues the object; it has not destroyed its Foot
            # storage or cleared the remembered slot. Execute only the
            # original later destructor's cache block, with EBX obtained
            # from that destructor's own XOR. Its unrelated team/global
            # vector/sound/locomotor teardown is outside this control.
            self.stage = 'foot_destructor_slot_block'
            step = dict(label=self.stage, before=self.snapshot(self.stage + '_before'),
                        call_start=len(self.native_calls.calls), draw_start=len(self.draws),
                        original_blocks=[dict(begin='0x4d3595', end='0x4d3597'),
                                         dict(begin='0x4d3632', end='0x4d366e')],
                        supplied_registers=dict(esi=hex(self.actor)),
                        whole_destructor=False)
            self.block(0x4D3595, 0x4D3597, {UC_X86_REG_ESI: self.actor}, [0x4D3595])
            assert self.uc.reg_read(UC_X86_REG_EBX) == 0
            self.block(0x4D3632, 0x4D366E, {UC_X86_REG_ESI: self.actor},
                       [0x4D3632, 0x4D365B, 0x4D3668])
            assert self.uc.reg_read(UC_X86_REG_EIP) == 0x4D366E
            assert not self.native_calls.pending
            step.update(after=self.snapshot(self.stage + '_after'),
                        call_end=len(self.native_calls.calls), draw_end=len(self.draws))
            self.steps.append(step)
        else:
            assert profile in ('state1', 'state3', 'state4')
            phase = int(profile[-1])
            destination = self.case.get('destination', [3712, 2688, 0] if phase == 1 else [2688, 2688, 0])
            self.invoke_step('retained_request_move_to', 0x54B1C0, 0, [self.loco + 4, *destination])
            nav = self.case.get('prior_nav', [14, 10] if phase == 1 else [10, 10])
            self.supplied('retained_handler_inputs', [
                (self.loco + 0x50, dwords(phase)),
                (self.loco + 0x80, dwords(self.case.get('target_height', self.input_final['height'] if phase == 1 else 0))),
                (self.actor + 0x5A4, dwords(self.cell(*nav) if nav else 0)),
                (self.actor + 0x2B4, dwords(self.other if self.case.get('tarcom', False) else 0)),
            ])
            self.invoke_step('register_air', 0x4134A0, 0x887888, [self.actor])
            self.invoke_step(profile + '_decision', {1: 0x54BA30, 3: 0x54BFF0, 4: 0x54C550}[phase],
                             self.loco, [])
            for visit in range(self.case.get('following_visits', 2)):
                self.process_visit(visit)
        assert self.original_text == hashlib.sha256(bytes(self.uc.mem_read(0x401000, 0x3E0000))).hexdigest()
        for ptr, expected in self.vtables.items():
            size = {'0x7f5c70': 0x600, '0x7f6218': 0x100, '0x7ecd44': 0x110}[ptr]
            assert hashlib.sha256(bytes(self.uc.mem_read(int(ptr, 16), size))).hexdigest() == expected
        return dict(input_constructor=self.input_constructor, input_layers=self.input_layers,
                    input_world=dict(cell_allocation=[32, 32], cell_stride=0x200,
                                     map_f4=list(struct.unpack('<6i', self.uc.mem_read(self.map + 0xF4, 24))),
                                     tracker_extent=[40, 40], tracker_capacity=16,
                                     default_land_type=0, default_level=0, default_slope=0,
                                     default_tile_index=0, default_overlay_index=-1,
                                     **self.input_tile_registry,
                                     land_type_override_source='cached_cell_ec',
                                     object_startup=self.input_object_startup,
                                     initial_cell_order=self.case.get('order', [14, 10]) if profile == 'ordinary' else None,
                                     land_type_overrides=self.case.get('land_types', []),
                                     supplied_unit_fixture=True, full_scenario_load=False),
                    input_final=self.input_final, input_link_raw=self.input_link,
                    input_type_raw_hex=self.input_type_raw, input_rules_raw_hex=self.input_rules_raw,
                    input_country_constructor_raw_hex=self.input_country_constructor,
                    input_country_final_raw_hex=bytes(self.uc.mem_read(self.country, 0x400)).hex(),
                    stock_comparison=self.stock_comparison, steps=self.steps, boundaries=self.boundaries,
                    native_calls=self.native_calls.calls, rng_draws=self.draws, writes=self.writes,
                    platform_transports=self.events,
                    memory_baselines={name: dict(address=hex(self.memory_regions[name][0]), raw_hex=raw.hex(),
                                                sha256=hashlib.sha256(raw).hexdigest())
                                      for name, raw in self.memory_baselines.items()},
                    text_sha256=self.original_text, original_vtables=self.vtables,
                    code_unchanged=True, guards_intact=True)


def composed_control(inputs, stock_report, case=None):
    case = dict(dict(name='stock_shad_occupied_state1_composed', actor=[2688, 2688, 0],
                     frame=100, seed=31, taken=True), **(case or {}))
    return dict(name=case['name'], input=case,
                output=ComposedStates(inputs, stock_report, case).execute())


def composed_cases():
    ordinary = dict(actor=[2688, 2688, 0], frame=100, seed=31,
                    foreign_slot=[12, 10], foreign_xyz=[3200, 2688, 500])
    direct = dict(actor=[2688, 2688, 500], frame=100, seed=31,
                  foreign_slot=[10, 10], foreign_xyz=[2688, 2688, 500])
    return [
        dict(ordinary, name='stock_taken_state1_arrival', continuation='arrival'),
        dict(ordinary, name='stock_free_state1_translate', taken=False),
        dict(ordinary, name='stock_taken_state1_stop', continuation='stop'),
        dict(direct, name='state1_taken_unmarked', profile='state1', marked=False),
        dict(direct, name='state1_free_unmarked', profile='state1', marked=False, taken=False),
        dict(direct, name='state1_taken_same_nav', profile='state1', prior_nav=[11, 11]),
        dict(direct, name='state1_taken_deployed_refusal', profile='state1', deployed=True),
        dict(direct, name='state1_taken_power_refusal', profile='state1', power_off=True),
        dict(direct, name='state1_taken_skip_move', profile='state1', skip_move=True),
        dict(direct, name='state3_live_new_water', profile='state3', tarcom=True,
             land_types=[[11, 11, 2]]),
        dict(direct, name='state3_live_new_clear', profile='state3', tarcom=True,
             land_types=[[10, 10, 2]]),
        dict(direct, name='state4_water_taken', profile='state4', land_types=[[10, 10, 2]]),
        dict(direct, name='state4_water_free', profile='state4', taken=False,
             land_types=[[10, 10, 2]]),
        dict(direct, name='slot_claim_replacement_release', profile='slot_lifetime',
             foreign_slot=[12, 10], foreign_xyz=[3200, 2688, 500]),
        dict(direct, name='unit_uninit_live_slot', profile='uninit',
             foreign_slot=[12, 10], foreign_xyz=[3200, 2688, 500]),
        dict(ordinary, name='stock_taken_state1_arrival_scenario_seed0',
             continuation='arrival', scenario_seed0_after_visit=50),
    ]


def legacy_rows():
    return [dict(name=name, input=dict(BASE, **row), output=States(dict(BASE, **row)).execute())
            for name, row in ROWS]


def generate():
    legacy = legacy_rows()
    canonical = json.dumps(legacy, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()
    assert hashlib.sha256(canonical).hexdigest() == LEGACY_STATES_SHA256, 'ten legacy Jumpjet rows changed'
    inputs = Path(os.environ.get('VERA20K_JUMPJET_INPUTS', 'target/asset/jumpjet-shad/extract'))
    report = Path(os.environ.get('VERA20K_JUMPJET_STOCK_REPORT', 'target/asset/jumpjet-shad/final-type.json'))
    assert inputs.is_dir() and report.is_file(), 'Set VERA20K_JUMPJET_INPUTS and VERA20K_JUMPJET_STOCK_REPORT; see jumpjet_states.md'
    return dict(schema_version=2, legacy_rows=legacy,
                composed_controls=[composed_control(inputs, report, case) for case in composed_cases()])


def compact_projection(raw):
    """Project saved native memory; no invocation or gameplay result is added.

    Every step's before/after scalar fields and full RNG states stay intact.
    Step boundary indices refer to the retained array; raw_boundary_index
    identifies each row in the full archive. Call/write receipts keep their
    original ordering. The publication owner archives the entire input
    byte-for-byte before this projection, including every opaque raw span.
    """
    from tools.spatial_oracle.unit_scatter_state import ACTOR, LOCO as UNIT_LOCO
    projected = dict(schema_version=raw['schema_version'], legacy_rows=raw['legacy_rows'],
                     composed_controls=[])
    for control in raw['composed_controls']:
        output = control['output']
        identities = {ACTOR: 'actor', COMPOSED_STORAGE + 0x60000: 'foreign'}
        def owner(pointer):
            pointer = int(pointer, 16) if isinstance(pointer, str) else pointer
            assert pointer == 0 or pointer in identities, ('unrepresented native owner', hex(pointer))
            return identities.get(pointer)
        baseline = output['memory_baselines']
        world_base = int(baseline['world']['address'], 16)
        world_bytes = bytes.fromhex(baseline['world']['raw_hex'])
        cell_base = int(baseline['cells']['address'], 16)
        cell_bytes = bytes.fromhex(baseline['cells']['raw_hex'])
        stride = output['input_world']['cell_stride']
        allocation = output['input_world']['cell_allocation']
        def cell_identity(pointer):
            if pointer == 0:
                return None
            offset = pointer - cell_base
            assert 0 <= offset < allocation[0] * allocation[1] * stride and offset % stride == 0
            return list(struct.unpack_from('<hh', cell_bytes, offset + 0x24))
        retained = {0}
        for step in output['steps']:
            retained.update((step['before'], step['after']))
        boundary_indices = {raw_index: index for index, raw_index in enumerate(sorted(retained))}
        assert all(0 <= index < len(output['boundaries']) for index in retained)
        boundaries = []
        for raw_index in sorted(retained):
            boundary = output['boundaries'][raw_index]
            state = boundary['state']
            actor_raw = bytes.fromhex(state['foot_raw_hex'])
            world = reconstruct_byte_spans(world_bytes, state['memory']['world']['changed_spans'])
            assert hashlib.sha256(world).hexdigest() == state['memory']['world']['sha256']
            foreign_offset = COMPOSED_STORAGE + 0x60000 - world_base
            foreign_raw = world[foreign_offset:foreign_offset + 0x700]
            object_bytes = {ACTOR: actor_raw, COMPOSED_STORAGE + 0x60000: foreign_raw}
            def members(head):
                # CellPut47E8DB/47E8EB and CellRemove47EA90 use the original
                # Object NextObject+30 chain. These supplied controls contain
                # only the actual actor/foreign pointer identities above.
                result, seen = [], set()
                pointer = int(head, 16)
                while pointer:
                    assert pointer not in seen and pointer in object_bytes
                    seen.add(pointer)
                    result.append(owner(pointer))
                    pointer = struct.unpack_from('<I', object_bytes[pointer], 0x30)[0]
                return result
            state = {key: value for key, value in state.items()
                     if key not in ('foot_raw_hex', 'jumpjet_raw_hex', 'memory', 'nav_queues')}
            cells = []
            for cell in state['cells']:
                raw_cell = bytes.fromhex(cell['raw_hex'])
                cells.append(dict(cell=cell['cell'], slot=cell['slot'], slot_owner=owner(cell['slot']),
                                  ground_head=cell['ground_head'], bridge_head=cell['bridge_head'],
                                  ground_raw=struct.unpack_from('<I', raw_cell, 0x124)[0],
                                  bridge_raw=struct.unpack_from('<I', raw_cell, 0x128)[0],
                                  ground_owners=members(cell['ground_head']),
                                  bridge_owners=members(cell['bridge_head'])))
            state['cells'] = cells
            for name in ('air', 'display'):
                state[name] = dict(members=[dict(index=vector['index'], items=vector['items'],
                                                owners=[owner(pointer) for pointer in vector['items']])
                                            for vector in state[name]['members']])
            state['other'] = dict(position=list(struct.unpack_from('<iii', foreign_raw, 0x9C)),
                                  health=struct.unpack_from('<i', foreign_raw, 0x6C)[0],
                                  marked=foreign_raw[0x74], alive=foreign_raw[0x90],
                                  on_bridge=bool(foreign_raw[0x8C]))
            state['tarcom_owner'] = owner(struct.unpack_from('<I', actor_raw, 0x2B4)[0])
            boundaries.append(dict(boundary, state=state, raw_boundary_index=raw_index))
        steps = []
        for step in output['steps']:
            step = dict(step)
            step.update(before=boundary_indices[step['before']], after=boundary_indices[step['after']])
            if step.get('call_start', 0) < step.get('call_end', 0):
                invocation = output['native_calls'][step['call_start']]
                step.update(entry=invocation['entry'], ecx=invocation['ecx'], args=invocation['args'])
            if 'supplied' in step:
                inputs = {}
                for write in step['supplied']:
                    address = int(write['address'], 16)
                    value = struct.unpack('<I', bytes.fromhex(write['raw_hex']))[0]
                    if address == UNIT_LOCO + 0x50:
                        inputs['phase'] = value
                    elif address == UNIT_LOCO + 0x80:
                        inputs['target_height'] = struct.unpack('<i', bytes.fromhex(write['raw_hex']))[0]
                    elif address == ACTOR + 0x5A4:
                        inputs['nav_cell'] = cell_identity(value)
                    elif address == ACTOR + 0x2B4:
                        inputs['tarcom_owner'] = owner(value)
                    else:
                        raise AssertionError(('unrepresented supplied input', write))
                step['supplied_inputs'] = inputs
            steps.append(step)
        output = {key: value for key, value in output.items()
                  if key not in ('memory_baselines', 'input_type_raw_hex', 'input_rules_raw_hex',
                                 'input_country_constructor_raw_hex', 'input_country_final_raw_hex')}
        output = dict(output, boundaries=boundaries, steps=steps,
                      raw_boundary_count=len(control['output']['boundaries']),
                      input_world=dict(output['input_world'],
                                       object_identities={name: hex(pointer) for pointer, name in identities.items()}))
        projected['composed_controls'].append(dict(control, output=output))
    return projected


if __name__ == '__main__':
    # Load the existing setup/observation owners before finish_vectors captures
    # source identity. This pins every local Python module loaded by the two
    # profiles without binding concurrently prepared Rust test source.
    import importlib
    import sys
    for module in (
        'tools.rules_oracle.bridge_anim_inputs', 'tools.spatial_oracle.building_body_rules',
        'tools.projectile_oracle.bridge_render_inputs', 'tools.spatial_oracle.track_destination',
        'tools.spatial_oracle.unit_scatter_state', 'tools.spatial_oracle.unit_source_scatter',
        'tools.spatial_oracle.unit_entry', 'tools.spatial_oracle.jumpjet_entry_discovery',
        'tools.spatial_oracle.fly_landing_phase', 'tools.spatial_oracle.house_base_projection',
        'tools.spatial_oracle.nearby_raw_occupation', 'tools.spatial_oracle.crate_ground_membership',
        'tools.spatial_oracle.object_flight_height',
    ):
        importlib.import_module(module)
    root = Path(__file__).resolve().parents[2]
    local_sources = sorted({Path(module.__file__).resolve() for module in sys.modules.values()
                            if getattr(module, '__file__', None)
                            and Path(module.__file__).suffix == '.py'
                            and Path(module.__file__).resolve().is_relative_to(root / 'tools')})
    source_paths = {path.relative_to(root).as_posix(): path for path in local_sources}
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Schema2 retains the ten exact legacy rows and adds sixteen same-VM composed controls. '
              'Composed stock SHAD Cell order runs actual Unit741970->Foot4D94B0->Jumpjet54B1C0, '
              'FNPC56DC20/56E7C0, Process54AEC0 through trueRET54B199 and following Translate; '
              'occupied/free State1 callback/claim, post-operation Mark0/+74 restore/phase suffix, '
              'State3 live post-setter LandType tail, State4, Stop/arrival, distinct Foot560/564 caches, '
              'slot refusal/replacement/release and active-game UnInit. One arrival control executes '
              'original Scenario Random Seed(0) at a declared post-process50 load handoff, then continues '
              'to touchdown; this is not whole Scenario Save/Load. The later destructor slot clear '
              'is an original interior block, not whole destructor/deferred drain. Legacy scope: '
              'Jumpjet Process 0x0054AEC0 per frame - its Is_Moving/Is_Moving_Now Update gate and the '
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
                      'facing_current': 0x4c93d0, 'facing_snap': 0x4c9300,
                      'unit_type_constructor': 0x7470d0, 'rules_constructor': 0x665650,
                      'jumpjet_constructor': 0x54ac40, 'country_constructor': 0x5113f0,
                      'unit_set_destination': 0x741970, 'foot_set_destination': 0x4d94b0,
                      'stop': 0x54b4d0, 'fnpc': 0x56dc20, 'fnpc_rectangle': 0x56e7c0,
                      'foot_mark': 0x4d3780, 'cell_recalculate': 0x47d2b0,
                      'cell_air_slot': 0x487d70, 'foot_remember_slot': 0x4e00b0,
                      'air_add': 0x4134a0, 'air_remove': 0x4135d0, 'air_update': 0x4138c0,
                      'unit_occupation_set': 0x7441b0, 'unit_occupation_clear': 0x744210,
                      'object_set_z': 0x5f6060, 'foot_uninit': 0x4de5d0,
                      'object_crt_table': 0x8141d8, 'object_level_height_init': 0x5f37c0,
                      'object_bridge_height_init': 0x5f3860,
                      'scenario_seed': SEED_FN,
                      'foot_destructor_slot_begin': 0x4d3632, 'foot_destructor_slot_end': 0x4d366e},
        assumptions=['Legacy rows only: FPCW 0E7F, the process control word WinMain installs with _controlfp(0x300, 0x300) at '
                     '0x006BBFC1; level height 104 at 0xABC5E8 and deck 416 at 0xABC5DC; direction deltas from '
                     'the original initializer; frame counter from 1001; owner RTTI 1 (Unit); owner +0x83 clear '
                     'and +0x41B set so both Process visibility probes are skipped, +0x90 set (airborne layer), '
                     '+0x425 and +0x427 clear so the crash latch is off, +0x8C/+0x8D/+0x81 clear; per row: the '
                     'cell corridor level/slope, LandType +0xEC, raw occupation +0x124 and AltObject +0xE0, the '
                     'type block, BalloonHover +0xD6A, IsSimpleDeployer +0xE13, DeployToLand +0x6AD, the owner '
                     'piggyback byte +0x6AD and the Can_Enter_Cell answer; TarCom is a non-null marker only.',
                     'Composed controls: FPCW0E7F; original Rules/UnitType/Country/Jumpjet constructors, '
                     'selected original physical RULESMD/LANGRULE(absent)/MPBattleMD/AnyTown reader blocks, '
                     'original Land and BlockagePathDelay readers and Link. Recorded stock production report '
                     'is a comparison receipt, never a native initializer. Full type/ART/scenario loading is '
                     'outside coverage; physical INI bytes and admitted reader regions are recorded.',
                     'Composed controls: existing supplied Unit raw fixture, House human/current/Country, '
                     '32x32 flat Cell storage; BSS IsoTileArrayA8ED2C/countA8ED38 remain0, Cell+38=0 '
                     'and overlay+44=-1. Per-control LandType overrides write cached Cell+EC only; '
                     'original Recalc47D58D invalidates the unregistered tile toFFFF, then47DB48 '
                     'writes Clear and47DB52 slope0. Original Object14CRT table8141D8 executes '
                     'before any Mark, producing distinct AC13C8=104/AC13BC=416 rather than '
                     'borrowing Map/Jumpjet values or supplying a layer. Map metadata, empty nearby '
                     'zone plane, 40x40 tracker extent with 400 original vector headers, display headers and '
                     'live foreign Unit/air-slot premises are recorded. All three complete RNG states start seed31; '
                     'stock_taken_state1_arrival_scenario_seed0 executes original65C6D0 on Scenario+218 '
                     'with argument0 after process50. This bounded handoff follows original683564..683571 '
                     'reached by Load689470 via6894C5, but excludes whole native Save/Load and its other work. '
                     'ordinary histories start frame100. GameActive remains supplied0 for ordinary Process; '
                     'the UnInit control supplies1 with original House counter/deferred-vector startup. '
                     'Supplemental handler/gate controls declare all retained writes. Native +520=-1 and '
                     'distinct +560/+564 NullCell inputs come from original constructor/data owners.',
                     'The full raw archive retains every composed Foot700/Jumpjet98 boundary, all three RNG states, timers, '
                     'navigation queues and lossless deltas over whole cell/world/map/House/vector storage. '
                     'Whole .text/vtables, padding guards, FPCW and actual composed PC/ESP/cleanup returns '
                     'are checked. Destructor control executes only XOR4D3595 and cache4D3632..4D366E '
                     'with ESI=retired Foot, not whole destructor or deferred drain.'],
        substitutions=['Legacy rows only: Owner SetLocation(+0x1B4) writes the coordinate only; Mark(+0x124) records the bracket and '
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
                       'answers a declared cell sequence, which feeds both Move_To and the Stop_Moving re-target.',
                       'Composed controls: existing physical-text lexical transport into original INI caches; '
                       'Windows ASCII-to-UTF16/CLSIDFromString transport, empty FS SEH chain and checked '
                       'IsBadReadPtr transport; CRT atexit registration. Reached gameplay bodies, including '
                       'FNPC, SetDestination, Mark, cell lists/Recalc, occupation, air tracker and fog, execute '
                       'original instructions with no supplied gameplay result.'],
    ), source_paths=source_paths, projection=compact_projection)
