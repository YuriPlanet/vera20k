"""Original Jumpjet Infantry actions: the airborne DoTypes a JumpJet= infantry
(the Rocketeer, the Cosmonaut) takes on the real Jumpjet locomotor.

Four original bodies run on an Infantry owner (RTTI 15) with the original
Infantry Do_Action and the real Jumpjet locomotor behind owner +0x674:

- `InfantryClass::Do_Action @ 0x0051D6F0`: the request gate, the airborne
  remap of Ready (0) to Hover (0x17) at 0x0051D8BF..0x0051D8EE (vtable +0x54
  0x004DE620 -> 0x005F6B90: owner +0x74 and GetHeight at least twice the level
  height; not on a bridge +0x8C; the type's Hover record has a start frame),
  the unchanged and interruptible gates, the Doing (+0x6C4), stage (+0xF8)
  and stage-timer writes (+0x100/+0x108/+0x10C; the normalized actions through
  0x005FB2E0), and the Health-0 re-entry into Stop_Driver (vtable +0x500,
  0x0051DA96..0x0051DAA1).
- The locomotion action tail of `0x00520F40` (0x00521144..0x0052130F), run as
  the whole function with Guard and no NavCom so its earlier movement
  recovery is skipped: Is_Moving_Now (locomotor +0xA8) and a JumpJet type
  (+0xD94) on the Jumpjet locomotor (its IPersist class id against
  0x007E9AC0) take Fly (0x18) above a speed fraction (+0x578) of 0.8
  (0x007EB5C8) and Hover (0x17) at or below it, unless the firing latch
  (+0x68D) is up; otherwise Walk, Fly and Hover return to Ready.
- The firing arm of `0x005206B0` from its fire-OK branch (0x0052078F) to the
  latch store (0x00520904): a JumpJet type on the Jumpjet locomotor fires in
  FireFly (0x1A).
- `InfantryClass::DoType_Sequencer @ 0x00520AE0` at a sequence's end: the
  default arm's forced Walk/Crawl or Ready/Prone by Is_Moving and a speed
  fraction above 0.1 (0x007E3860), the AirDeathStart arm (a forced
  AirDeathFalling) and the AirDeathFinish arm (UnInit, vtable +0xF8).

The type's 42 sequence records are the retail `[RocketeerSequence]` as the
original ReadSequenceData 0x00523D00 builds them
(`infantry_sequence_rules.Fixture`).
"""
from pathlib import Path
import struct

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

from tools.native_oracle import call, finish_vectors, provenance, run_checked, STACK_BASE, STACK_SIZE
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.jumpjet_states import BASE, LOCO, OWNER, SCRATCH, States, centre
from tools.spatial_oracle.infantry_sequence_rules import Fixture as SequenceFixture, RECORDS
from tools.spatial_oracle.walk_head_occupation import TYPE, VTABLE

EXTRA = 0x21000000
SEQUENCES = EXTRA
STOP_DRIVER, UNINIT = [SCRATCH + 0xED00 + i * 0x40 for i in range(2)]
LEVEL_HEIGHT = 0xAC13C8
INFANTRY_VTABLE = 0x7EB058
DO_ACTION, MOVEMENT_ACTIONS, SEQUENCER = 0x51D6F0, 0x520F40, 0x520AE0
FIRING_ARM = (0x52078F, 0x520904)

# The retail [RocketeerSequence] (artmd.ini), shared by [ROCK] and [LUNR].
ROCKETEER_SEQUENCE = {
    'Ready': '0,1,1', 'Guard': '0,1,1', 'Prone': '86,1,6', 'Walk': '8,6,6',
    'FireUp': '164,6,6', 'Down': '260,2,2', 'Crawl': '86,6,6', 'Up': '276,2,2',
    'FireProne': '212,6,6', 'Idle1': '56,15,0,S', 'Idle2': '71,15,0,E',
    'Die1': '134,15,0', 'Die2': '149,15,0', 'Die3': '0,0,0', 'Die4': '0,0,0',
    'Die5': '0,0,0', 'Fly': '292,6,6', 'Hover': '292,6,6', 'FireFly': '370,6,6',
    'Tumble': '340,15,0', 'AirDeathStart': '340,8,0', 'AirDeathFalling': '348,1,0',
    'AirDeathFinish': '349,6,0', 'Paradrop': '418,1,0', 'Cheer': '419,8,0,E',
    'Panic': '8,6,6',
}


def rocketeer_records():
    """The 42 records original ReadSequenceData builds from the retail section."""
    fixture = SequenceFixture()
    # The key CRCs (0x004A1DE0) are computed up front: the fixture otherwise
    # runs a second emulator from inside its ReadString hook, which stops the
    # outer run early on some hosts.
    for key in fixture.names + [f'{name}Sounds' for name in fixture.names] + ['Sequence']:
        name = key.encode('ascii')
        fixture.crcs[key] = call(0x4A1DE0, ecx=SCRATCH, stack_args=[SCRATCH + 0x100, len(name)],
                                 writes={SCRATCH: bytes(16), SCRATCH + 0x100: name})['eax']
    fixture.execute([ROCKETEER_SEQUENCE], sequence='RocketeerSequence')
    # The four trailing dwords are the sequence's sound list, left unset by the
    # constructor loop; no retail [RocketeerSequence] key fills them.
    return [list(struct.unpack('<5i', fixture.u.mem_read(RECORDS + n * 36, 20))) + [0] * 4
            for n in range(42)]


# The retail [JUMPJET] Jumpjet block as the case-sensitive reader sees it:
# JumpJetAccel= and JumpJetTurnRate= are never read, so the constructor's 2.0
# and 4 stand.
ROCKETEER = dict(turn_rate=4, speed=30, climb=20.0, crash=25.0, height=500, accel=2.0,
                 wobbles=0.01, no_wobbles=True, deviation=1, balloon_hover=True)
ACTION_BASE = dict(BASE, **ROCKETEER, rtti=15, phase=2, moving=True,
                   start=[*centre(10), 500], target_height=500, first_frame=1000)


class Actions(States):
    def __init__(self, row, records):
        super().__init__(dict(ACTION_BASE, **row.get('owner', {})))
        u = self.uc
        u.mem_map(EXTRA, 0x10000)
        for index, record in enumerate(records):
            u.mem_write(SEQUENCES + index * 36, struct.pack('<9i', *record))
        u.mem_write(TYPE + 0xE3C, dwords(SEQUENCES))
        u.mem_write(TYPE + 0xD94, bytes([int(row.get('jumpjet', True))]))
        u.mem_write(TYPE + 0x5B4, dwords(9))
        # Original Do_Action and IsHighFlying; recorded Stop_Driver and UnInit.
        for slot, fn in [(0x558, DO_ACTION), (0x54, 0x4DE620), (0x500, STOP_DRIVER),
                         (0xF8, UNINIT)]:
            u.mem_write(VTABLE + slot, dwords(fn))
        u.mem_write(LEVEL_HEIGHT, dwords(104))
        # GameOptionsClass's stored speed index (0x00A8EB60), which the
        # normalized actions' rate reads through 0x005FB2E0.
        u.mem_write(0xA8EB60, dwords(row.get('game_speed', 1)))
        # The owner's reference on its locomotor (LocomotionClass +0x14), so
        # the class-id query's Release (0x0055A970) does not free it.
        u.mem_write(LOCO + 0x14, dwords(1))
        u.mem_write(OWNER + 0x74, b'\x01')
        u.mem_write(OWNER + 0x6C, dwords(row.get('health', 125)))
        u.mem_write(OWNER + 0x6C4, dwords(row.get('doing', -1)))
        u.mem_write(OWNER + 0xF8, dwords(row.get('stage', 0)))
        u.mem_write(OWNER + 0x100, dwords(17, 0, 91, 92, 1))
        u.mem_write(OWNER + 0x8C, bytes([int(row.get('on_bridge', False))]))
        u.mem_write(OWNER + 0x68D, bytes([int(row.get('firing', False))]))
        u.mem_write(OWNER + 0x6DB, bytes([int(row.get('prone', False))]))
        u.mem_write(OWNER + 0x578, struct.pack('<d', row.get('fraction', 0.0)))
        u.mem_write(OWNER + 0x9C, dwords(*centre(10), row.get('height', 500)))
        if 'hover_start' in row:
            u.mem_write(SEQUENCES + 0x17 * 36, dwords(row['hover_start']))
        self.recorded = []
        # The IAT slots of InterlockedIncrement/Decrement, which the
        # locomotor's AddRef/Release (0x0055A950/0x0055A970) call.
        self.interlocked = {self.read32(0x7E11C8): 1, self.read32(0x7E11CC): -1}

    def observe(self, u, address, size, data):
        if address in getattr(self, 'interlocked', ()):
            sp = u.reg_read(UC_X86_REG_ESP)
            pointer = self.read32(sp + 4)
            value = (self.read32(pointer) + self.interlocked[address]) & 0xFFFFFFFF
            u.mem_write(pointer, dwords(value))
            self.ret(4, value)
        elif address == STOP_DRIVER:
            getattr(self, 'recorded', []).append(['stop_driver', self.doing()])
            self.ret(0, 0)
        elif address == UNINIT:
            self.recorded.append('uninit')
            self.ret(0, 0)
        else:
            super().observe(u, address, size, data)

    def doing(self):
        return struct.unpack('<i', self.uc.mem_read(OWNER + 0x6C4, 4))[0]

    def result(self):
        u = self.uc
        return dict(doing=self.doing(),
                    stage=struct.unpack('<i', u.mem_read(OWNER + 0xF8, 4))[0],
                    timer=[struct.unpack('<i', u.mem_read(OWNER + x, 4))[0]
                           for x in (0x100, 0x108, 0x10C)],
                    recorded=self.recorded)

    def do_action(self, row):
        self.call(DO_ACTION, OWNER, [row['request'], int(row.get('force', False)), 0])
        accepted = bool(self.uc.reg_read(UC_X86_REG_EAX) & 0xFF)
        return dict(self.result(), accepted=accepted)

    def movement_actions(self):
        self.call(MOVEMENT_ACTIONS, OWNER, [])
        return self.result()

    def sequencer(self):
        self.call(SEQUENCER, OWNER, [])
        return self.result()

    def firing_arm(self):
        u = self.uc
        sp = STACK_BASE + STACK_SIZE - 0x2000
        u.mem_write(sp, bytes(0x40))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_EBP, OWNER)
        u.reg_write(UC_X86_REG_EDI, 0)
        u.reg_write(UC_X86_REG_ECX, TYPE)
        run_checked(u, FIRING_ARM[0], FIRING_ARM[1], count=20000,
                    required_addresses=[FIRING_ARM[0]])
        return self.result()



def do_action_rows():
    rows = []
    currents = [-1, 0, 3, 4, 0x17, 0x18, 0x1A, 0x20, 0x22, 0x23, 0x24]
    for request in (0, 2, 3, 0x17, 0x18, 0x1A, 0x22, 0x23, 0x24):
        for current in currents:
            for force in (False, True):
                rows.append(dict(kind='do_action', request=request, doing=current, force=force))
    # IsHighFlying's boundary, the bridge and the missing Hover record.
    for height in (0, 207, 208, 500):
        for current in (-1, 3, 0x17, 0x18):
            rows.append(dict(kind='do_action', request=0, doing=current, height=height))
    for speed in (0, 4, 6):
        rows.append(dict(kind='do_action', request=0, doing=0x18, game_speed=speed))
    for current in (-1, 0x18):
        rows.append(dict(kind='do_action', request=0, doing=current, on_bridge=True))
        rows.append(dict(kind='do_action', request=0, doing=current, hover_start=0))
    # Health 0: an accepted request re-enters Stop_Driver; a refused one does not.
    for request, current in ((0, 0x18), (0, 0x17), (0x22, 0x17), (0x24, 0x22), (0x23, 0x22)):
        for force in (False, True):
            rows.append(dict(kind='do_action', request=request, doing=current, force=force,
                             health=0))
    return rows


def movement_rows():
    rows = []
    for phase, moving in ((0, False), (0, True), (1, True), (2, False), (2, True), (3, True),
                          (4, True), (5, True), (6, False)):
        for fraction in (0.0, 0.8, 0.8000001, 1.0):
            for current in (-1, 0, 3, 0x17, 0x18, 0x1A, 0x22):
                rows.append(dict(kind='movement', owner=dict(phase=phase, moving=moving),
                                 fraction=fraction, doing=current))
    for firing in (False, True):
        rows.append(dict(kind='movement', owner=dict(phase=3, moving=True), fraction=1.0,
                         doing=0x1A, firing=firing))
    # Low over the ground Ready stays Ready; a non-JumpJet type walks.
    rows.append(dict(kind='movement', owner=dict(phase=2, moving=False), doing=0x17,
                     height=100))
    rows.append(dict(kind='movement', owner=dict(phase=3, moving=True), fraction=1.0,
                     doing=0, jumpjet=False))
    return rows


def sequencer_rows():
    rows = []
    # Doing -1 skips the stage test (0x00520AEF) and takes the default arm
    # every frame; Cheer (0x20) at its end takes it too.
    for current, stage in ((0x17, 6), (0x18, 6), (0x1A, 6), (0x1A, 5), (0x22, 8), (0x22, 7),
                           (0x23, 1), (0x24, 6), (0x24, 5), (0, 1), (-1, 0), (0x20, 8)):
        for moving, fraction in ((False, 0.0), (True, 0.05), (True, 0.5)):
            rows.append(dict(kind='sequencer', doing=current, stage=stage, fraction=fraction,
                             owner=dict(phase=3 if moving else 2, moving=moving)))
    return rows


def firing_rows():
    return [dict(kind='firing', doing=current, jumpjet=jumpjet)
            for current in (-1, 0x17, 0x18, 0x1A) for jumpjet in (True, False)]


def sequencer_arms(uc):
    """The arm `DoType_Sequencer` 0x00520AE0 dispatches each Doing to at its
    end (0x00520B13..0x00520B27): Doing - 0xB through the byte table at
    0x00520F1C into the arm table at 0x00520EFC; -1 and anything outside
    0..0x1B take the default arm 0x00520CE6."""
    index = bytes(uc.mem_read(0x520F1C, 0x1C))
    arms = struct.unpack('<8I', uc.mem_read(0x520EFC, 32))
    out = []
    for doing in range(-1, 42):
        offset = doing - 0xB
        arm = arms[index[offset]] if 0 <= offset <= 0x1B else 0x520CE6
        out.append([doing, f'{arm:08X}'])
    return out


def generate():
    records = rocketeer_records()
    out = []
    for row in do_action_rows() + movement_rows() + sequencer_rows() + firing_rows():
        fixture = Actions(row, records)
        kind = row['kind']
        if kind == 'do_action':
            output = fixture.do_action(row)
        elif kind == 'movement':
            output = fixture.movement_actions()
        elif kind == 'sequencer':
            output = fixture.sequencer()
        else:
            output = fixture.firing_arm()
        out.append(dict(input=row, output=output))
    arms = sequencer_arms(Actions(dict(kind='table'), records).uc)
    return dict(records=records, sequencer_arms=arms, rows=out)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Jumpjet Infantry DoTypes on the original Infantry Do_Action 0x0051D6F0 and the real '
              'Jumpjet locomotor: the Ready->Hover airborne remap, gates and stage-timer writes and '
              'the Health-0 Stop_Driver re-entry; the locomotion action tail of 0x00520F40 '
              '(Fly/Hover by speed fraction, the firing latch, the return to Ready); the firing arm '
              '0x0052078F..0x00520904 (FireFly); the DoType sequencer 0x00520AE0 default, '
              'AirDeathStart and AirDeathFinish arms. Not the stage tick, the Stun chain, the crash '
              'or anything drawn.',
        entry_points={'do_action': DO_ACTION, 'is_high_flying': 0x4DE620,
                      'movement_actions': MOVEMENT_ACTIONS, 'firing_arm': FIRING_ARM[0],
                      'sequencer': SEQUENCER, 'speed_normalize': 0x5FB2E0,
                      'read_sequence_data': 0x523D00},
        assumptions=['Everything jumpjet_states.States assumes, with owner RTTI 15 and the retail '
                     '[JUMPJET] Jumpjet block; type JumpJet +0xD94 per row, MovementZone +0x5B4 9 '
                     '(Fly); level height 104 at 0x00AC13C8; stored game speed 1 at 0x00A8EB60 '
                     'unless the row says otherwise; owner +0x74 set, Health 125 unless the '
                     'row says 0, stage timer (17, 91, 92) with increment 1, the owner at the height '
                     'the row gives over a flat corridor; Guard with no NavCom in the movement rows; '
                     'the retail [RocketeerSequence] records built by the original ReadSequenceData.'],
        substitutions=['As jumpjet_states.States, plus: Stop_Driver (vtable +0x500) and UnInit '
                       '(+0xF8) record their call and return; the OS InterlockedIncrement/Decrement '
                       'imports the locomotor AddRef/Release call emulate their integer operation '
                       'and stdcall cleanup; the firing arm runs as a region with EBP the owner, ECX '
                       'the type and weapon index 0.'],
    ))
