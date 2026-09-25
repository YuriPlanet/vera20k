"""Original Jumpjet crash: the kill's locomotor stops, the Process crash latch,
State5 Crash 0x0054CA90 frame by frame and its impact notice.

The fall family extends the Jumpjet state corpus (`jumpjet_states.States`): a
stock Crashable type is flown to a hover through the original Move_To and
Process, then killed the way `UnitClass::ReceiveDamage 0x00737C90` kills it.
Its Health goes to 0; the Techno death arm's Stun and Crash's own Stun each
reach the locomotor through FootClass::Stun 0x004D5660 -> Stop_Driver
0x004D55C0 -> Stop_Moving 0x0054B4D0, and between them Crash 0x004DEBB0 sets
the latch +0x425. For an owner with no NavCom the Stun's
`Assign_Destination(NULL, 1)` (UnitClass 0x00741970) returns at 0x00741A80
before touching the locomotor, so the two Stop_Moving calls are all the kill
does to it. Process 0x0054AEC0 then latches state 5 (0x0054AF2E..0x0054B02C)
and State5 runs until its impact calls the owner's INoticeSink slot 0 with
(0x117C, 0) (0x0054D090).

Besides kills in the hover, rows kill the owner on its way (State 3) and in
the descent's last step above the ground (State 4), where the kill's
Stop_Moving searches from the owner's own cell and Move_To lifts the descent
back into the climb (0x0054B455..0x0054B467) before the latch.

Not covered: the null Set_Destination a live NavCom adds to the Stun
(UnitClass 0x00741970 -> FootClass 0x004D94B0 -> Stop_Moving, idempotent for a
Unit), bridges and building tops in the reference height, and the
Magnetron-lifted arms (+0x6AD, +0x427).

A second family runs Draw_Matrix 0x0054DCC0 (ILocomotion +0x24) on the same
owner: TechnoType +0xD22 (TiltCrashJumpjet=) with either rocking angle (owner
+0x328 sideways, +0x32C forwards) at least 0.005 takes the tilt arm, which
offsets and rotates the facing matrix of LocomotionClass::Draw_Matrix
0x0055A730 by the angles and the type's voxel half sizes (+0x360, +0x368),
keyed -1; otherwise the base matrix alone.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.jumpjet_states import (
    BASE, LOCO, OWNER, SCRATCH, States, centre,
)
from tools.spatial_oracle.walk_head_occupation import TYPE

MAP = 0x87F7E8
FNPC = 0x56DC20
NOTICE_VTABLE = SCRATCH + 0xEA00
NOTICE = SCRATCH + 0xEA40
LAYER_REMOVE = 0x4A9770


class Crash(States):
    def __init__(self, row):
        super().__init__(row)
        u = self.uc
        # MapClass::In_Bounds 0x00568300 reads the diamond's width +0xF4 and
        # height +0xF8; the declared corridor lies inside a 13 x 20 map unless
        # the row narrows it.
        u.mem_write(MAP + 0xF4, dwords(*row['map_size']))
        # The owner's INoticeSink (owner +8): slot 0 records the notice.
        u.mem_write(NOTICE_VTABLE, dwords(NOTICE))
        u.mem_write(OWNER + 8, dwords(NOTICE_VTABLE))
        self.notices = []

    def observe(self, u, address, size, data):
        if address == FNPC and self.row['fnpc_owner'] and self.fnpc_calls:
            # After the order's own search, every search answers the cell the
            # owner is in, as the real search does for a Fly-zone query from a
            # passable cell (its first ring is the seed alone).
            sp = u.reg_read(UC_X86_REG_ESP)
            pointer = self.read32(sp + 4)
            x, y = self.owner_cell()
            u.mem_write(pointer, packed(x, y))
            self.fnpc_calls += 1
            self.events.append(['fnpc', [x, y]])
            self.ret(60, pointer)
        elif address == NOTICE:
            sp = u.reg_read(UC_X86_REG_ESP)
            self.notices.append([self.read32(sp + 4), self.read32(sp + 8)])
            self.ret(8, 0)
        elif address == LAYER_REMOVE:
            self.events.append('layer_remove')
            self.ret(4, 0)
        else:
            super().observe(u, address, size, data)

    def state(self):
        frame = super().state()
        # Locomotor +0x90, the landing latch State 4 raises, and the owner's
        # NavCom +0x5A4 as the cell it names.
        frame['landing_latched'] = bool(self.uc.mem_read(LOCO + 0x90, 1)[0])
        nav_com = self.read32(OWNER + 0x5A4)
        frame['nav_com'] = (list(struct.unpack('<hh', self.uc.mem_read(nav_com + 0x24, 4)))
                            if nav_com else None)
        return frame

    def process(self):
        self.fractions = []
        self.events = []
        self.slot_events = []
        self.frame += 1
        self.uc.mem_write(0xA8ED84, dwords(self.frame))
        self.call(0x54AEC0, 0, [LOCO + 4])
        frame = self.state()
        frame['events'] = self.events
        frame['slot_events'] = self.slot_events
        return frame

    def execute(self):
        u = self.uc
        row = self.row
        if row['order'] is not None:
            self.events = []
            self.call(0x54B1C0, 0, [LOCO + 4, *row['order']])
        # Fly to where the row kills the owner: held in the hover (state 2) or
        # the cruise (state 3) for hold_frames, or the descent's last step
        # above the ground.
        held = 0
        for _ in range(row['flight_frames']):
            frame = self.process()
            if row['kill_when'] == 'landing':
                if frame['phase'] == 4 and 0 < frame['coord'][2] <= row['kill_height']:
                    break
            elif frame['phase'] == KILL_PHASE[row['kill_when']]:
                held += 1
                if held >= row['hold_frames']:
                    break
            else:
                held = 0
        before = self.state()
        if row['kill_map_size'] is not None:
            u.mem_write(MAP + 0xF4, dwords(*row['kill_map_size']))
        # The kill: Health 0, the death arm's Stop_Moving, Crash's latch, then
        # Crash's own Stop_Moving.
        u.mem_write(OWNER + 0x6C, dwords(0))
        self.events = []
        self.call(0x54B4D0, 0, [LOCO + 4])
        u.mem_write(OWNER + 0x425, b'\x01')
        self.call(0x54B4D0, 0, [LOCO + 4])
        killed = dict(self.state(), events=self.events)
        frames = []
        for _ in range(row['max_frames']):
            frame = self.process()
            frame['notices'] = self.notices
            self.notices = []
            frames.append(frame)
            if frame['notices']:
                break
        return dict(before=before, killed=killed, frames=frames)


KILL_PHASE = {'hover': 2, 'cruise': 3}
FALL_BASE = dict(BASE, map_size=[13, 20], kill_map_size=None, flight_frames=0, hold_frames=4,
                 max_frames=200, tarcom=True, kill_when='hover', kill_height=None,
                 fnpc_owner=False)


def stock(turn_rate=4, speed=14, climb=5.0, crash=5.0, height=500, accel=2.0,
          wobbles=0.15, no_wobbles=False, deviation=40, balloon_hover=False):
    """A stock type block as gamemd reads it: stock spells JumpJetTurnRate= and
    JumpJetAccel=, which the case-sensitive reader never sees, so every type
    keeps the constructor's turn rate 4 and acceleration 2.0."""
    return dict(turn_rate=turn_rate, speed=speed, climb=climb, crash=crash, height=height,
                accel=accel, wobbles=wobbles, no_wobbles=no_wobbles, deviation=deviation,
                balloon_hover=balloon_hover)


TYPES = {
    'ZEP': stock(speed=5, climb=6.0, crash=12.0, height=750, no_wobbles=True,
                 balloon_hover=True),
    'SHAD': stock(speed=30, climb=10.0, crash=40.0, height=500, wobbles=0.01, deviation=1),
    'HIND': stock(speed=40, climb=50.0, crash=60.0, height=500, wobbles=0.01, deviation=1),
    'SCHP': stock(speed=30, climb=10.0, crash=40.0, height=500, wobbles=0.01, deviation=1),
    'DISK': stock(speed=16, climb=8.0, crash=15.0, height=750, wobbles=0.1, no_wobbles=True,
                  deviation=15, balloon_hover=True),
}


def fall_rows():
    rows = []
    for name, block in TYPES.items():
        # Flown two cells on to hold over its target, then shot down there.
        rows.append((f'{name}_shot_down_hovering',
                     dict(FALL_BASE, **block, order=[*centre(12), 0], flight_frames=600)))
    # The latch never engages without the moving byte: a landed-and-lifted
    # hover (state 2, +0x4C clear) is skipped by Process's gate and hangs.
    rows.append(('SHAD_hover_without_moving_byte_hangs',
                 dict(FALL_BASE, **TYPES['SHAD'], phase=2, moving=False,
                      start=[*centre(10), 500], target_height=500, max_frames=20)))
    # Outside In_Bounds State5 moves nothing; only Update descends.
    rows.append(('SHAD_shot_down_outside_the_map_bounds',
                 dict(FALL_BASE, **TYPES['SHAD'], order=[*centre(12), 0], flight_frames=600,
                      kill_map_size=[40, 20])))
    # Shot down on its way: the kill re-targets the cell under the wreck, a
    # balloon keeping the Stop's own request (0x0054B3F2..0x0054B3FA).
    for name in ('SHAD', 'ZEP'):
        rows.append((f'{name}_shot_down_in_the_cruise',
                     dict(FALL_BASE, **TYPES[name], order=[*centre(16), 0], flight_frames=600,
                          kill_when='cruise', hold_frames=12, fnpc_owner=True)))
    # Shot down in the descent's last step, one climb step above the ground:
    # Move_To lifts State 4 back into State 1, so the latch still engages.
    for name in ('SHAD', 'HIND'):
        rows.append((f'{name}_shot_down_touching_down',
                     dict(FALL_BASE, **TYPES[name], tarcom=False, order=[*centre(12), 0],
                          flight_frames=600, kill_when='landing',
                          kill_height=int(TYPES[name]['climb']), fnpc_owner=True)))
    return rows


DRAW_MATRIX = 0x54DCC0
FACING_CONSTRUCTOR, FACING_SET_ROT, FACING_SET = 0x4C91C0, 0x4C9680, 0x4C9300


def draw_matrix(row):
    """Draw_Matrix 0x0054DCC0 on the corpus owner: the type's TiltCrashJumpjet
    byte and voxel half sizes, the owner's rocking angles and body facing."""
    owner = States(dict(FALL_BASE, **TYPES['DISK'], order=None, phase=2, moving=False,
                        start=[*centre(10), 500], target_height=750))
    u = owner.uc
    u.mem_write(TYPE + 0xD22, bytes([row['tilt']]))
    u.mem_write(TYPE + 0x360, struct.pack('<dd', *row['half_sizes']))
    u.mem_write(OWNER + 0x328, struct.pack('<ff', *row['angles']))
    facing = OWNER + 0x388
    owner.call(FACING_CONSTRUCTOR, facing, [])
    owner.call(FACING_SET_ROT, facing, [4])
    u.mem_write(SCRATCH + 0xEB00, dwords(row['facing']))
    owner.call(FACING_SET, facing, [SCRATCH + 0xEB00])
    out, key = SCRATCH + 0xEC00 - 0x80, SCRATCH + 0xEC00 - 0x10
    u.mem_write(key, dwords(row['key']))
    owner.call(DRAW_MATRIX, 0, [LOCO + 4, out, key])
    matrix = struct.unpack('<12I', u.mem_read(out, 48))
    return dict(matrix=[f'{x:08x}' for x in matrix], key=owner.read32(key))


def draw_matrix_rows():
    rows = []
    angle_sets = [(0.0, 0.0), (0.004, -0.0049), (0.005, 0.0), (0.0, -0.0051), (0.4, 0.0),
                  (0.0, 0.35), (0.7853982, -0.6), (-0.7, 0.7853982), (-0.3, -0.2),
                  (-0.7853982, -0.7853982)]
    for tilt in (1, 0):
        for facing in (0x0000, 0x4000, 0x6200, 0xC000):
            for angles in angle_sets:
                rows.append((f'tilt{tilt}_f{facing:04x}_{angles[0]}_{angles[1]}',
                             dict(tilt=tilt, facing=facing, angles=list(angles),
                                  half_sizes=[10.5, 10.0], key=3)))
    # Other half sizes: the offsets scale with the voxel's own extent.
    for half_sizes in ([6.0, 11.5], [20.5, 3.0]):
        for angles in ((0.4, 0.0), (0.0, 0.35), (0.7853982, -0.6)):
            rows.append((f'half{half_sizes[0]}_{half_sizes[1]}_{angles[0]}_{angles[1]}',
                         dict(tilt=1, facing=0x6200, angles=list(angles),
                              half_sizes=half_sizes, key=3)))
    return rows


def generate():
    return dict(
        fall=[dict(name=name, input=row, output=Crash(row).execute())
              for name, row in fall_rows()],
        draw_matrix=[dict(name=name, input=row, output=draw_matrix(row))
                     for name, row in draw_matrix_rows()],
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Jumpjet crash: a stock Crashable type flown to a hover through the original Move_To '
              '0x0054B1C0 and Process 0x0054AEC0, killed (Health 0, Stop_Moving 0x0054B4D0 twice '
              'around the +0x425 latch), then Process per frame through the crash latch '
              '0x0054AF2E..0x0054B02C and State5 0x0054CA90 until the 0x117C impact notice. Types: '
              'ZEP, SHAD, HIND, SCHP, DISK with their stock JumpjetSpeed/Climb/Crash/Height/Wobbles/'
              'NoWobbles/Deviation/BalloonHover; a hover without the moving byte; an out-of-bounds '
              'fall; SHAD and ZEP shot down in the cruise (State 3) and SHAD and HIND in the '
              'descent one climb step above the ground (State 4, lifted back to State 1 by the '
              'kill\'s Move_To). Not the null Set_Destination a live NavCom adds to the Stun, '
              'bridges, building tops or the lifted arms. '
              'draw_matrix: Draw_Matrix 0x0054DCC0 with TiltCrashJumpjet (+0xD22) on and off, four '
              'body facings, rocking angles around the 0.005 gate and to the balloon clamp, and '
              'three voxel half-size pairs (+0x360, +0x368).',
        entry_points={'move_to': 0x54B1C0, 'process': 0x54AEC0, 'stop_moving': 0x54B4D0,
                      'state5': 0x54CA90, 'update': 0x54D0F0, 'in_bounds': 0x568300,
                      'facing_set': 0x4C9220, 'facing_current': 0x4C93D0,
                      'draw_matrix': DRAW_MATRIX, 'base_draw_matrix': 0x55A730},
        assumptions=['Everything jumpjet_states.States assumes; owner Health +0x6C set to 0 and the '
                     'crash latch +0x425 to 1 between the two Stop_Moving calls, as the Techno death arm '
                     'and FootClass::Crash order them; the owner has no NavCom, so UnitClass '
                     'Set_Destination(NULL) returns at 0x00741A80 without reaching the locomotor; '
                     'MapClass +0xF4/+0xF8 give the In_Bounds diamond per row; +0x6AD and +0x427 clear.',
                     'draw_matrix: the owner body facing +0x388 built by the original FacingClass '
                     'constructor, SetROT(4) and Set; the half sizes written as the doubles '
                     'TechnoTypeClass::ReadINI stores from the main voxel (0x007160C7..0x0071611D); '
                     'the caller key 3.'],
        substitutions=['As jumpjet_states.States, plus: the owner INoticeSink (owner +8) slot 0 records '
                       '(notice, argument) and returns; MapClass layer remove 0x004A9770 is recorded and '
                       'otherwise a no-op; in the cruise and descent rows every FNPC 0x0056DC20 search after '
                       'the order\'s own answers the owner\'s current cell, which is what the real search '
                       'answers for a Fly-zone query from a passable cell.'],
    ))
