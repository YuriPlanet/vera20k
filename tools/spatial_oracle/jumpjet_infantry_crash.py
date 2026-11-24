"""Original Jumpjet Infantry crash: a shot-down Rocketeer's kill, fall and end.

The owner is the `jumpjet_infantry_actions` Rocketeer (RTTI 15, the real
Jumpjet locomotor, the original Infantry Do_Action and the retail
`[RocketeerSequence]` records), with the real Infantry slots of the kill:
Set_Destination 0x0051AA40, Stop_Driver 0x0051DAF0, Assign_Target 0x0051B1F0
and FootClass::Stun 0x004D5660, and the real Infantry INoticeSink (0x007EB034)
at owner +8.

The kill runs the locomotor work of `InfantryClass::ReceiveDamage
@ 0x00517FA0` in its order, with Health 0:
- the Techno death arm's Stun (`0x00702210`);
- the Infantry arm's Stop_Driver (`0x005180FE`) and Stun (`0x00518108`);
- `FootClass::Crash @ 0x004DEBB0` (`0x0051860B`), whose Stun follows the
  crash latch +0x425.

Each Stun is Set_Destination(NULL) (the Foot null setter's Stop_Moving),
Stop_Driver (Do_Action, re-entering Stop_Driver at Health 0 when it accepts,
then Stop_Moving) and TechnoClass::Stun 0x006FCD40 (a second null
Set_Destination). Every Stop_Moving 0x0054B4D0 that finds the moving byte set
re-targets through FNPC, Move_To and the Infantry placement 0x004ACA10, whose
sub-cell pick draws on the Scenario RNG (0x0048139A).

Then per frame, in `InfantryClass::AI`'s order:
- its Health reset (`0x0051BC57..0x0051BC9D`);
- the stage tick of TechnoClass::AI_Update (`0x006FABC4..0x006FAC31`);
- Process 0x0054AEC0, whose crash latch plays AirDeathStart (Do_Action 0x22,
  `0x0054B02C`) and whose impact notice (0x117C) reaches the Infantry sink:
  SetHeight (deck or 0) and a forced AirDeathFinish (`0x00522AF7..0x00522B9B`);
- the sequencer 0x00520AE0, whose AirDeathFinish arm UnInits;
- the locomotion action tail 0x00520F40.

Rows run to the UnInit.

Mark (vtable +0x124) is the original FootClass::Mark 0x004D3780 with its
TechnoClass, ObjectClass, layer, Pick_Up and Place_Down bodies, so State 5's
Mark pair (0x0054CBE1 / 0x0054CBFE) links the faller into its cell's ground
list (+0xE4) once the Jumpjet layer 0x0054B8D0 answers Ground (IsHighFlying
0x005F6B90 below twice the level height), and the reference height's cell top
(0x00485080 -> Find_Nearest_Object 0x0047C3D0) finds it there. Each frame
records whether the owner is in that list.

Not executed:
- the rest of ReceiveDamage: the death announcement, the mission reset,
  KillPassengers, the S_BANG34 anim, experience and credit. None of them
  touches the locomotor or the Scenario RNG.
- the radio broadcasts, Detach_All and the pointer-expiry dispatch, which are
  recorded no-ops.
- CellClass::Recalc 0x0047D2B0 after each Pick_Up/Place_Down cell, and
  AddContent's discovery (0x00586360 answers visible; DiscoveredBy +0x198
  stays a no-op): neither touches the cell's object list.
- SetHeight's own Mark pair: SetHeight (+0x1CC) writes Z alone. At the impact
  State 5's last Mark(PUT) has already linked the owner at height 0, where the
  pair would unlink and relink it into the same one-object list.
- bridges.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance, run_checked, STACK_BASE, STACK_SIZE
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.jumpjet_infantry_actions import (
    Actions, EXTRA, UNINIT, rocketeer_records,
)
from tools.spatial_oracle.jumpjet_states import LOCO, OWNER, SCRATCH, cell_of
from tools.spatial_oracle.walk_head_occupation import TYPE, VTABLE, SCENARIO

MAP = 0x87F7E8
FNPC = 0x56DC20
STOP_MOVING = 0x54B4D0
NOTICE_SINK = 0x7EB034
HOUSE = EXTRA + 0x4000
RULES = EXTRA + 0x8000
(RADIO_FIRST, RADIO_ALL, DETACH_ALL, HOUSE_GET, SET_HEIGHT) = [
    SCRATCH + 0xEE00 + i * 0x20 for i in range(5)]
KILL_PASSENGERS, POINTER_EXPIRED, LAYER_REMOVE = 0x707CB0, 0x7258D0, 0x4A9770
CELL_RECALC, CRT_ATEXIT, CELL_DISCOVERY = 0x47D2B0, 0x7C978A, 0x586360
# The type's vtable: only InfantryTypeClass's occupy list (+0x90) is reached.
TYPE_VTABLE = EXTRA + 0xC000
# Seam -> stdcall argument bytes.
NO_OPS = {RADIO_FIRST: 4, RADIO_ALL: 4, DETACH_ALL: 4, KILL_PASSENGERS: 4,
          POINTER_EXPIRED: 0, LAYER_REMOVE: 4, CELL_RECALC: 4, CRT_ATEXIT: 0}


class Kill(Actions):
    def __init__(self, row, records):
        super().__init__(row, records)
        u = self.uc
        # MapClass In_Bounds reads the diamond's width +0xF4 and height +0xF8.
        u.mem_write(MAP + 0xF4, dwords(13, 20))
        for slot, fn in [(0x480, 0x51AA40), (0x500, 0x51DAF0), (0x3C8, 0x51B1F0),
                         (0x3A0, 0x4D5660), (0x274, RADIO_FIRST), (0x280, RADIO_ALL),
                         (0xDC, DETACH_ALL), (0x3C, HOUSE_GET), (0x1CC, SET_HEIGHT),
                         (0xF8, UNINIT)]:
            u.mem_write(VTABLE + slot, dwords(fn))
        u.mem_write(OWNER + 0x21C, dwords(HOUSE))
        # Foot's timer tail reads Rules +0x1768 (0x004D96D4).
        u.mem_write(RULES + 0x40C, dwords(4))
        u.mem_write(RULES + 0x1768, dwords(9))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(OWNER + 8, dwords(NOTICE_SINK))
        u.mem_write(TYPE + 0xD95, bytes([int(row.get('crashable', True))]))
        # The original Mark chain: FootClass::Mark, the Foot layer (the
        # locomotor's In_Which_Layer), the Infantry type getter, ObjectClass's
        # occupy list and redraw mark, and the dead getter ObjectClass::Mark
        # calls through +0x2C0.
        for slot, fn in [(0x124, 0x4D3780), (0x78, 0x4DB7E0), (0x88, 0x51FAF0),
                         (0x108, 0x5F5B90), (0x134, 0x5F4D10), (0x2C0, 0x708B40)]:
            u.mem_write(VTABLE + slot, dwords(fn))
        u.mem_write(TYPE, dwords(TYPE_VTABLE))
        u.mem_write(TYPE_VTABLE + 0x90, dwords(0x523C20))
        # TechnoClass's constructor sets the Techno flag Find_Nearest_Object
        # filters on (0x006F3228..0x006F3237); ObjectTypeClass's sets the +0x235
        # byte Place_Down and Pick_Up require (0x005F716E).
        u.mem_write(OWNER + 0x14, bytes([u.mem_read(OWNER + 0x14, 1)[0] | 1]))
        u.mem_write(TYPE + 0x235, b'\x01')
        self.log = []
        # `States` keeps its own merged row; the kill's keys stay here.
        self.kill_row = row

    def observe(self, u, address, size, data):
        if address == UNINIT:
            self.log.append('uninit')
            self.ret(0, 0)
        elif address == STOP_MOVING:
            self.log.append(['stop_moving', self.doing()])
            super().observe(u, address, size, data)
        elif address in NO_OPS:
            self.ret(NO_OPS[address], 0)
        elif address == CELL_DISCOVERY:
            self.ret(4, 1)
        elif address == HOUSE_GET:
            self.ret(0, HOUSE)
        elif address == SET_HEIGHT:
            # ObjectClass::SetHeight: the coordinate's Z at the given height.
            sp = u.reg_read(UC_X86_REG_ESP)
            height = struct.unpack('<i', u.mem_read(sp + 4, 4))[0]
            self.log.append(['set_height', height])
            coord = list(struct.unpack('<iii', u.mem_read(OWNER + 0x9C, 12)))
            coord[2] = height
            u.mem_write(OWNER + 0x9C, struct.pack('<iii', *coord))
            self.ret(4, 0)
        elif address == FNPC:
            # Every search answers the owner's own cell, as the real search
            # does for a Fly-zone query from a passable cell.
            sp = u.reg_read(UC_X86_REG_ESP)
            pointer = self.read32(sp + 4)
            x, y = self.owner_cell()
            u.mem_write(pointer, packed(x, y))
            self.ret(60, pointer)
        else:
            super().observe(u, address, size, data)

    def snapshot(self):
        u = self.uc
        return dict(doing=self.doing(),
                    health=struct.unpack('<i', u.mem_read(OWNER + 0x6C, 4))[0],
                    stage=struct.unpack('<i', u.mem_read(OWNER + 0xF8, 4))[0],
                    coord=list(struct.unpack('<iii', u.mem_read(OWNER + 0x9C, 12))),
                    phase=self.read32(LOCO + 0x50),
                    moving=bool(u.mem_read(LOCO + 0x4C, 1)[0]),
                    destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x40, 12))),
                    crash_latch=bool(u.mem_read(OWNER + 0x425, 1)[0]),
                    listed=self.listed(),
                    scenario_rng=[self.read32(SCENARIO + 0x21C), self.read32(SCENARIO + 0x220)])

    def listed(self):
        """The owner is in its cell's ground object list (+0xE4, linked by +0x30)."""
        cell = cell_of(*self.owner_cell())
        node = self.read32(cell + 0xE4) if cell else 0
        while node:
            if node == OWNER:
                return True
            node = self.read32(node + 0x30)
        return False

    def kill(self):
        """The kill's locomotor work in ReceiveDamage's order, at Health 0."""
        u = self.uc
        u.mem_write(OWNER + 0x6C, dwords(0))
        steps = []
        for name, entry, args in [('death_arm_stun', 0x4D5660, []),
                                  ('infantry_stop_driver', 0x51DAF0, []),
                                  ('infantry_stun', 0x4D5660, []),
                                  ('crash', 0x4DEBB0, [0])]:
            self.log = []
            self.call(entry, OWNER, args)
            step = dict(step=name, events=self.log, after=self.snapshot())
            if name == 'crash':
                step['accepted'] = bool(u.reg_read(UC_X86_REG_EAX) & 0xFF)
            steps.append(step)
        return steps

    def region(self, begin, end):
        u = self.uc
        sp = STACK_BASE + STACK_SIZE - 0x2000
        u.mem_write(sp, bytes(0x80))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ESI, OWNER)
        u.reg_write(UC_X86_REG_EBP, 0)
        run_checked(u, begin, end, count=200)

    def frame_step(self):
        self.log = []
        self.frame += 1
        self.uc.mem_write(0xA8ED84, dwords(self.frame))
        self.region(0x51BC57, 0x51BC9D)
        self.region(0x6FABC4, 0x6FAC31)
        self.call(0x54AEC0, 0, [LOCO + 4])
        self.call(0x520AE0, OWNER, [])
        self.call(0x520F40, OWNER, [])
        return dict(self.snapshot(), events=self.log)

    def execute(self):
        # The row's action, started by the original Do_Action (forced) so its
        # stage and timer are the ones it arms, not the fixture's.
        doing = self.kill_row.get('doing', -1)
        if doing != -1:
            self.uc.mem_write(OWNER + 0x6C4, dwords(-1 & 0xFFFFFFFF))
            self.call(0x51D6F0, OWNER, [doing, 1, 0])
        before = self.snapshot()
        kills = [self.kill()]
        frames = []
        second = self.kill_row.get('second_kill_frame')
        for index in range(self.kill_row.get('max_frames', 80)):
            if second is not None and index == second:
                kills.append(self.kill())
            frame = self.frame_step()
            frames.append(frame)
            if 'uninit' in frame['events']:
                break
        return dict(before=before, kills=kills, frames=frames)


BASE = dict(kind='kill', owner=dict(phase=2, moving=True), fraction=0.0, max_frames=80)


def rows():
    out = []
    # The Doing at the kill decides whether the first Stop_Driver's Do_Action
    # is accepted (Ready remaps to Hover while high flying).
    for name, doing in [('hover', 0x17), ('fly', 0x18), ('fire_fly', 0x1A), ('no_action', -1),
                        ('cheer', 0x20)]:
        out.append((f'hovering_{name}', dict(BASE, doing=doing)))
    # Shot down in the cruise at speed.
    out.append(('cruising_fly', dict(BASE, doing=0x18, owner=dict(phase=3, moving=True),
                                     fraction=1.0)))
    # A second kill while it falls (splash): Health stays 0, and the forced
    # AirDeathFinish re-enters Stop_Driver.
    out.append(('second_kill_in_the_fall', dict(BASE, doing=0x17, second_kill_frame=4)))
    # A fall longer than AirDeathStart's 24 frames: its end forces
    # AirDeathFalling (0x00520BB9), whose end takes the default arm.
    out.append(('long_fall', dict(BASE, doing=0x17, height=1200)))
    # Grounded: the crash is refused (height 0; ReceiveDamage then UnInits,
    # 0x0051861D) and no Stop_Moving finds the moving byte.
    out.append(('grounded', dict(BASE, doing=0, owner=dict(phase=0, moving=False),
                                 height=0, max_frames=0)))
    return out


def generate():
    records = rocketeer_records()
    return [dict(name=name, input=row, output=Kill(row, records).execute())
            for name, row in rows()]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Jumpjet Infantry crash on the jumpjet_infantry_actions Rocketeer: the kill\'s '
              'locomotor work in InfantryClass::ReceiveDamage order at Health 0 (Techno death arm '
              'Stun 0x702210, Infantry Stop_Driver 0x5180FE, Stun 0x518108, FootClass::Crash '
              '0x4DEBB0), then per frame the Infantry AI Health reset 0x51BC57, the stage tick '
              '0x6FABC4, Process 0x54AEC0 (crash latch Do_Action 0x22, State5, impact notice to '
              'the Infantry sink 0x522A60), the sequencer 0x520AE0 and the tail 0x520F40, to the '
              'UnInit. Kills in the hover with Doing Hover/Fly/FireFly/none/Cheer, in the cruise, '
              'a second kill in the fall, a fall from 1200 leptons (AirDeathFalling), and a '
              'grounded kill, with the original FootClass::Mark chain behind vtable +0x124; each '
              'snapshot records whether the owner is in its cell\'s ground list. Not bridges.',
        entry_points={'foot_stun': 0x4D5660, 'infantry_stop_driver': 0x51DAF0,
                      'infantry_set_destination': 0x51AA40, 'infantry_assign_target': 0x51B1F0,
                      'techno_stun': 0x6FCD40, 'foot_crash': 0x4DEBB0, 'stop_moving': STOP_MOVING,
                      'process': 0x54AEC0, 'infantry_notice': 0x522A60,
                      'sequencer': 0x520AE0, 'movement_actions': 0x520F40,
                      'health_reset': 0x51BC57, 'stage_tick': 0x6FABC4},
        assumptions=['Everything jumpjet_infantry_actions.Actions assumes, with the row\'s action '
                     'started by the original Do_Action (forced) before the kill; the owner type '
                     'Crashable +0xD95 set; an AI house (+0x21C, zeroed); Rules +0x1768 = 9; '
                     'MapClass diamond 13 x 20; the owner holds no target and no NavCom; the '
                     'owner carries the TechnoClass constructor\'s Techno flag (+0x14 bit 0) and '
                     'its type the ObjectTypeClass constructor\'s +0x235 = 1.'],
        substitutions=['As jumpjet_infantry_actions, plus: every FNPC 0x56DC20 search answers the '
                       'owner\'s cell; the radio (vtable +0x274, +0x280), Detach_All (+0xDC), '
                       'KillPassengers 0x707CB0, the pointer-expiry dispatch 0x7258D0 and the '
                       'layer remove 0x4A9770 are no-ops; SetHeight (+0x1CC) writes the '
                       'coordinate\'s Z without its Mark pair; UnInit (+0xF8) is recorded. Mark '
                       '(+0x124) is the original FootClass::Mark 0x4D3780 chain, with CellClass::'
                       'Recalc 0x47D2B0 and the CRT atexit 0x7C978A no-ops and AddContent\'s '
                       'discovery query 0x586360 answering visible.'],
    ))
