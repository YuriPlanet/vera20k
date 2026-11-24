"""Original Jumpjet cruise: Update_Coordinates_And_Altitude then State3_Translate per frame.

Each frame advances the frame counter, runs Update 0x0054D0F0 and, while the
locomotor is still in state 3, State3_Translate 0x0054BFF0, matching Process
0x0054AEC0's order for a moving locomotor. Link_To_Object 0x0054AD30 copies the
type block and builds the locomotor facing at the type's JumpjetTurnRate.
The CRT static initializer 0x0049F3A0 (listed at 0x00812BB0) runs first, so the
eight-way direction deltas at 0x0089F6D8 that the reference height 0x0054D820
reads hold their startup values rather than zero.
Rows toggle the Unit simple-deployer byte (UnitType+0xE13), DeployToLand
(TechnoType+0x6AD), the owner piggyback byte (+0x6AD) and the LandType of cell
(10,10) (+0xEC) to reach State3's hold, tail and piggyback arrival branches.
The map is the shared fixture: cell (10,10) is level 0 and flat, cell (9,10)
keeps the base harness's level 2 and slope 1, and every other lookup resolves to
the dummy cell. The slow_fast_turn row flies over (9,10), so the corpus covers
the look-ahead and current-cell terrain reference over a sloped cell; no bridge,
building or cell object is present, so those branches are outside this corpus.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.jumpjet_coordinates import Jumpjet, KIND, SET_SPEED
from tools.spatial_oracle.walk_head_occupation import CELL, OWNER, VTABLE, LOCO, TYPE, OUTPUT, SCRATCH

SETLOC, NOOP_F4, NOOP_480, UNIT_EXT = [SCRATCH + 0xe800 + i * 0x100 for i in range(4)]
FRAME = 0xA8ED84
CENTER = 2560 + 128


def f32(value):
    return struct.pack('<f', value)


class Flight(Jumpjet):
    def __init__(self, row):
        super().__init__(dict(actions=[], request=[0, 0, 0], ground=0, level=0, slope=0))
        u = self.uc
        self.row = row
        self.fractions = []
        u.mem_write(0xABC5E8, dwords(104))
        # CRT static initializer: fills g_DirectionDelta (0x0089F6D8) for 0x0054D820.
        self.call(0x49F3A0, 0, [])
        for slot, fn in [(0x1b4, SETLOC), (0x1b8, 0x0041BEA0), (0x1c8, 0x005F5F40),
                         (0x1d0, 0x005F5F30), (0xf4, NOOP_F4), (0x480, NOOP_480)]:
            u.mem_write(VTABLE + slot, dwords(fn))
        u.mem_write(TYPE + 0xD70, dwords(row['turn_rate'], row['speed']))
        u.mem_write(TYPE + 0xD78, f32(row['climb']) + f32(row['crash']))
        u.mem_write(TYPE + 0xD80, dwords(row['height']))
        u.mem_write(TYPE + 0xD84, f32(row['accel']) + f32(row['wobbles']))
        u.mem_write(TYPE + 0xD8C, bytes([int(row['no_wobbles']), 0, 0, 0]))
        u.mem_write(TYPE + 0xD90, dwords(row['deviation']))
        u.mem_write(TYPE + 0xD6A, bytes([int(row['balloon_hover'])]))
        u.mem_write(TYPE + 0xECB, b'\0')
        u.mem_write(OWNER + 0x6C0, dwords(TYPE))
        u.mem_write(OWNER + 0x6C4, dwords(UNIT_EXT))
        u.mem_write(OWNER + 0x2B0, dwords(0, 1 if row['tarcom'] else 0))
        u.mem_write(OWNER + 0x6AD, bytes([int(row['piggyback'])]))
        u.mem_write(UNIT_EXT + 0xE13, bytes([int(row['simple_deployer'])]))
        u.mem_write(TYPE + 0x6AD, bytes([int(row['deploy_to_land'])]))
        u.mem_write(CELL + 0xEC, dwords(row['destination_land_type']))
        u.mem_write(OWNER + 0x74, b'\0')
        u.mem_write(OWNER + 0x8C, b'\0')
        u.mem_write(OWNER + 0x560, dwords(0xFFFFFFFF))
        self.frame = row['first_frame']
        u.mem_write(FRAME, dwords(self.frame))
        self.call(0x54ad30, 0, [LOCO + 4, OWNER])
        u.mem_write(OWNER + 0x9C, dwords(*row['start']))
        u.mem_write(OUTPUT, dwords(row['facing']))
        self.call(0x4c9300, LOCO + 0x54, [OUTPUT])
        u.mem_write(LOCO + 0x40, dwords(*row['destination'], 0))
        u.mem_write(LOCO + 0x4C, dwords(1, 3))
        u.mem_write(LOCO + 0x70, struct.pack('<dd', 0.0, 0.0))
        u.mem_write(LOCO + 0x80, dwords(row['target_height']))

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == KIND:
            self.ret(0, self.row['rtti'])
        elif address == SET_SPEED:
            self.fractions.append(struct.unpack('<Q', u.mem_read(sp + 4, 8))[0])
            self.ret(8, 0)
        elif address == SETLOC:
            coord = bytes(u.mem_read(self.read32(sp + 4), 12))
            u.mem_write(OWNER + 0x9C, coord)
            self.ret(4, 0)
        elif address == NOOP_F4:
            self.ret(4, 0)
        elif address == NOOP_480:
            self.events.append('set_destination')
            self.ret(8, 0)
        elif address == 0x004138C0:
            self.ret(12, 0)
        elif address == 0x0055A710:
            self.ret(8, 0)
        elif address == 0x00705D60:
            self.events.append('mission_notify')
            self.ret(0, 0)
        elif address == 0x004135A0:
            self.events.append('slot_query')
            self.ret(4, 0)
        elif address == 0x00487D70:
            self.events.append('slot_claim')
            self.ret(4, 1)
        elif address == 0x578080:
            pass
        else:
            super().observe(u, address, size, data)

    def state(self):
        u = self.uc
        self.call(0x4c93d0, LOCO + 0x54, [OUTPUT])
        return dict(
            coord=list(struct.unpack('<iii', u.mem_read(OWNER + 0x9C, 12))),
            current_speed=struct.unpack('<Q', u.mem_read(LOCO + 0x70, 8))[0],
            target_speed=struct.unpack('<Q', u.mem_read(LOCO + 0x78, 8))[0],
            target_height=struct.unpack('<i', u.mem_read(LOCO + 0x80, 4))[0],
            bob_phase=struct.unpack('<Q', u.mem_read(LOCO + 0x88, 8))[0],
            phase=self.read32(LOCO + 0x50),
            facing_current=self.read32(OUTPUT) & 0xFFFF,
            facing_destination=struct.unpack('<H', u.mem_read(LOCO + 0x54, 2))[0],
            body_facing=struct.unpack('<H', u.mem_read(OWNER + 0x388, 2))[0],
        )

    def execute(self):
        frames = []
        for _ in range(self.row['max_frames']):
            self.fractions = []
            self.events = []
            self.frame += 1
            self.uc.mem_write(FRAME, dwords(self.frame))
            self.call(0x54d0f0, LOCO, [])
            if self.read32(LOCO + 0x50) == 3:
                self.call(0x54bff0, LOCO, [])
            frame = self.state()
            frame['speed_fractions'] = self.fractions
            frame['events'] = self.events
            frames.append(frame)
            if frame['phase'] != 3:
                break
        return dict(frames=frames)


BASE = dict(rtti=1, turn_rate=4, speed=14, climb=5.0, crash=5.0, height=500, accel=2.0,
            wobbles=0.15, no_wobbles=False, deviation=40, balloon_hover=False, tarcom=False,
            start=[CENTER, CENTER, 500], facing=0x4000, target_height=500, first_frame=1000,
            max_frames=400, simple_deployer=False, deploy_to_land=False, piggyback=False,
            destination_land_type=0)
ROWS = [
    ('east_cruise', dict(destination=[CENTER + 1536, CENTER])),
    ('west_reverse', dict(destination=[CENTER - 1536, CENTER])),
    ('north_left_turn', dict(destination=[CENTER, CENTER - 1536])),
    ('south_right_turn', dict(destination=[CENTER, CENTER + 1536])),
    ('low_start', dict(destination=[CENTER + 1536, CENTER], start=[CENTER, CENTER, 100])),
    ('balloon_infantry', dict(destination=[CENTER + 700, CENTER + 300], rtti=15, balloon_hover=True)),
    ('diagonal_no_wobble', dict(destination=[CENTER + 900, CENTER + 900], facing=0x2000, no_wobbles=True)),
    ('slow_fast_turn', dict(destination=[CENTER - 600, CENTER + 50], speed=4, turn_rate=20)),
    ('tarcom_cruise', dict(destination=[CENTER + 1000, CENTER - 700], tarcom=True, accel=0.7, wobbles=0.33)),
    ('east_from_west_facing', dict(destination=[CENTER + 1536, CENTER], facing=0xC000)),
    ('siege_chopper_arrival_hold', dict(destination=[CENTER + 900, CENTER], simple_deployer=True,
                                        deploy_to_land=True)),
    ('water_destination_keeps_height', dict(start=[CENTER + 1536, CENTER, 500], destination=[CENTER, CENTER],
                                            destination_land_type=2)),
    ('piggyback_arrival', dict(destination=[CENTER + 700, CENTER], piggyback=True)),
    ('low_height_floor', dict(destination=[CENTER + 1200, CENTER], height=100, target_height=208)),
]


def generate():
    return [dict(name=name, input=dict(BASE, **row), output=Flight(dict(BASE, **row)).execute())
            for name, row in ROWS]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Jumpjet Link_To_Object type copy and facing, then per-frame Update_Coordinates_And_Altitude and '
              'State3_Translate on the two-cell fixture (10,10 level 0 flat; 9,10 level 2 slope 1; dummy elsewhere) '
              'until the locomotor leaves state 3. Not ascend, hold, descend, crash, bridges, building tops, '
              'cell objects or the air-slot contents.',
        entry_points={'direction_delta_init': 0x49f3a0, 'link_to_object': 0x54ad30,
                      'update': 0x54d0f0, 'state3': 0x54bff0,
                      'facing_current': 0x4c93d0, 'facing_snap': 0x4c9300},
        assumptions=['FPCW 0E7F, the process control word WinMain installs with _controlfp(0x300, 0x300) at '
                     '0x006BBFC1; level height 104 at 0xABC5E8 and deck 416 at 0xABC5DC; two-cell map fixture as '
                     'scoped; direction deltas from the original initializer; frame counter from 1001; owner RTTI 1 '
                     '(Unit) or 15 (Infantry); owner +0x74 and +0x8C clear; per row: owner piggyback byte +0x6AD, '
                     'UnitType +0xE13 IsSimpleDeployer, TechnoType +0x6AD DeployToLand and cell (10,10) LandType +0xEC; '
                     'TarCom is a non-null marker only.'],
        substitutions=['Owner SetLocation(+0x1B4) writes the coordinate only; Mark(+0x124), SetSpeedFraction'
                       '(+0x544, argument recorded), +0xF4 and Set_Destination(+0x480) are supplied callbacks; '
                       'air-bucket bookkeeping 0x004138C0, base link 0x0055A710 and mission notify 0x00705D60 are '
                       'no-ops; air-slot query 0x004135A0 answers free and claim 0x00487D70 succeeds.'],
    ))
