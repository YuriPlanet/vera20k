"""Original kamikaze tracker (the global at 0x00ABC5F8): Push, Update, Remove and Clear.

Each row runs the tracker's static initializer 0x0054E260, then Clear 0x0054E6F0
at frame 0 (Read_Scenario zeroes the frame counter at 0x0068466B before
InitializeScenarioFromINI reaches Clear_Scene's call at 0x00685529), then the
row's script. A script step sets the frame counter and calls one entry:

- push: 0x0054E3B0 (RET 8) with the child and the target, or NULL. With
  `restart`, the step then writes the caller's {Frame, 2} timer as
  SpawnManagerClass::AI (0x006B7A3C) and ClearAllTargets (0x006B7BF0) do;
  Kill_All_Spawns (0x006B71B7) pushes without it.
- update: 0x0054E4D0, as LogicClass::AI calls it (0x0055B4F0).
- remove: 0x0054E590 (RET 4) with a child or target pointer.
- clear: 0x0054E6F0.

Native and executed: those four bodies, the static initializer and its
DynamicVectorClass constructor 0x0054EBF0 and growth through the vector vtable
0x007ECE7C, the cell-delta table initializer 0x0049F2F0, FacingClass::Current
0x004C93D0 on facings built by the constructor 0x004C91C0, Set_ROT 0x004C9680,
Snap 0x004C9300 and Set 0x004C9220, ObjectClass::GetOccupiedCell 0x005F6960
with MapClass::operator[] 0x00565730, CellClass::Adjacent_Cell 0x00481810 and
MapClass::GetCellAt 0x005657A0 over a cell array holding cells (0..127,
0..127).

Supplied seams (recorded in call order): the child's Crash (vt+0x3DC),
Assign_Target (vt+0x3C8) and Queue_Mission (vt+0x1E8); the target's GetCoords
(vt+0x48), which answers the row's coordinate; operator new 0x007C8E17 (a bump
allocator), operator delete 0x007C8B3D and the initializer's atexit 0x007C978A.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance, run_checked,
)

TRACKER = 0xABC5F8
FRAME = 0xA8ED84
MAP = 0x87F7E8
DUMMY_CELL = 0xABDC50

WORK, WORK_SIZE = 0x21000000, 0x40000
CHILD_VTABLE = WORK
TARGET_VTABLE = WORK + 0x800
MISSILE_TYPE = WORK + 0x1000
PLAIN_TYPE = WORK + 0x2000
STUBS = WORK + 0x3000
ARGUMENT = WORK + 0x3800
CHILDREN = WORK + 0x4000
CHILD_SIZE = 0x800
TARGETS = WORK + 0x20000
TARGET_SIZE = 0x100
HEAP, HEAP_SIZE = 0x23000000, 0x100000
CELL_ARRAY, CELL_ARRAY_COUNT = 0x24000000, 0x40000
CELLS, CELL_SIZE, CELL_SPAN = 0x25000000, 0x40, 128
(STUB_CRASH, STUB_ASSIGN_TARGET, STUB_QUEUE_MISSION, STUB_TARGET_COORDS) = [STUBS + i * 0x10 for i in range(4)]
SP = STACK_BASE + STACK_SIZE - 0x1000

OPERATOR_NEW = 0x7C8E17
OPERATOR_DELETE = 0x7C8B3D
ATEXIT = 0x7C978A


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def child_address(index):
    return CHILDREN + CHILD_SIZE * index


def target_address(index):
    return TARGETS + TARGET_SIZE * index


class Tracker:
    def __init__(self, row):
        self.row = row
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        u.mem_map(WORK, WORK_SIZE)
        u.mem_map(HEAP, HEAP_SIZE)
        u.mem_map(CELL_ARRAY, CELL_ARRAY_COUNT * 4)
        u.mem_map(CELLS, CELL_SPAN * CELL_SPAN * CELL_SIZE)
        u.mem_write(STUBS, b'\xC3' * 0x100)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.heap = HEAP
        self.events = []
        u.hook_add(UC_HOOK_CODE, self.observe)
        self.write_frame(0)
        self.write_map()
        self.call(0x49F2F0, 0, [])
        self.call(0x54E260, 0, [])
        self.call(0x54E6F0, TRACKER, [])
        for slot, function in [(0x3DC, STUB_CRASH), (0x3C8, STUB_ASSIGN_TARGET), (0x1E8, STUB_QUEUE_MISSION),
                               (0x1BC, 0x5F6960)]:
            u.mem_write(CHILD_VTABLE + slot, dwords(function))
        u.mem_write(TARGET_VTABLE + 0x48, dwords(STUB_TARGET_COORDS))
        u.mem_write(MISSILE_TYPE + 0xD68, b'\x01')
        u.mem_write(PLAIN_TYPE + 0xD68, b'\x00')
        for index, child in enumerate(row['children']):
            self.write_child(index, child)
        for index, coords in enumerate(row['targets']):
            address = target_address(index)
            u.mem_write(address, dwords(TARGET_VTABLE))
            u.mem_write(address + 0x9C, dwords(*coords))

    def write_frame(self, frame):
        self.frame = frame
        self.uc.mem_write(FRAME, dwords(frame))

    def write_map(self):
        u = self.uc
        u.mem_write(MAP + 0x13C, dwords(CELL_ARRAY, CELL_ARRAY_COUNT))
        for y in range(CELL_SPAN):
            for x in range(CELL_SPAN):
                cell = CELLS + CELL_SIZE * (y * CELL_SPAN + x)
                u.mem_write(cell + 0x24, struct.pack('<hh', x, y))
                u.mem_write(CELL_ARRAY + 4 * (y * 512 + x), dwords(cell))

    def write_child(self, index, child):
        u = self.uc
        address = child_address(index)
        u.mem_write(address, dwords(CHILD_VTABLE))
        u.mem_write(address + 0x6C4, dwords(MISSILE_TYPE if child['missile_spawn'] else PLAIN_TYPE))
        u.mem_write(address + 0x9C, dwords(*child['location']))
        u.mem_write(address + 0x2FC, dwords(child['ammo']))
        u.mem_write(address + 0x6CA, b'\x00')
        facing = address + 0x388
        self.write_frame(child['facing_frame'])
        self.call(0x4C91C0, facing, [])
        self.call(0x4C9680, facing, [child['rot']])
        u.mem_write(ARGUMENT, dwords(child['facing']))
        self.call(0x4C9300, facing, [ARGUMENT])
        if child['turn_to'] is not None:
            u.mem_write(ARGUMENT, dwords(child['turn_to']))
            self.call(0x4C9220, facing, [ARGUMENT])
        self.write_frame(0)

    def read32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def ret(self, cleanup, result=0):
        u = self.uc
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, result & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def call(self, entry, this, args):
        u = self.uc
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, this)
        run_checked(u, entry, RET_MAGIC, count=2_000_000, required_addresses=[entry])
        return u.reg_read(UC_X86_REG_EAX)

    def child_index(self, address):
        offset = address - CHILDREN
        assert 0 <= offset < CHILD_SIZE * len(self.row['children']) and offset % CHILD_SIZE == 0, hex(address)
        return offset // CHILD_SIZE

    def cell(self, address):
        assert address != 0 and address != DUMMY_CELL, f'cell {address:#x} outside the row cells'
        return list(struct.unpack('<hh', self.uc.mem_read(address + 0x24, 4)))

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        arg = lambda index: self.read32(sp + 4 + 4 * index)
        ecx = u.reg_read(UC_X86_REG_ECX)
        if address == STUB_CRASH:
            self.events.append(['crash', self.child_index(ecx), arg(0)])
            self.ret(4)
        elif address == STUB_ASSIGN_TARGET:
            self.events.append(['assign_target', self.child_index(ecx), self.cell(arg(0))])
            self.ret(4)
        elif address == STUB_QUEUE_MISSION:
            self.events.append(['queue_mission', self.child_index(ecx), arg(0), arg(1)])
            self.ret(8, 1)
        elif address == STUB_TARGET_COORDS:
            out = arg(0)
            u.mem_write(out, bytes(u.mem_read(ecx + 0x9C, 12)))
            self.ret(4, out)
        elif address == OPERATOR_NEW:
            size = arg(0)
            result = self.heap
            self.heap += (size + 15) & ~15
            assert self.heap <= HEAP + HEAP_SIZE
            self.ret(0, result)
        elif address == OPERATOR_DELETE:
            self.ret(0)
        elif address == ATEXIT:
            self.ret(0)

    def state(self):
        u = self.uc
        count = self.read32(TRACKER + 0x1C)
        items = self.read32(TRACKER + 0x10)
        nodes = []
        for index in range(count):
            node = self.read32(items + 4 * index)
            child, cell = struct.unpack('<II', u.mem_read(node, 8))
            nodes.append([self.child_index(child), self.cell(cell)])
        start, _, duration = struct.unpack('<iii', u.mem_read(TRACKER, 12))
        children = [[struct.unpack('<i', u.mem_read(child_address(index) + 0x2FC, 4))[0],
                     u.mem_read(child_address(index) + 0x6CA, 1)[0]]
                    for index in range(len(self.row['children']))]
        return dict(nodes=nodes, timer=[start, duration], children=children)

    def pointer(self, reference):
        if reference is None:
            return 0
        kind, index = reference
        return child_address(index) if kind == 'child' else target_address(index)

    def step(self, step):
        self.events = []
        self.write_frame(step['frame'])
        op = step['op']
        if op == 'push':
            target = None if step['target'] is None else target_address(step['target'])
            self.call(0x54E3B0, TRACKER, [child_address(step['child']), target or 0])
            if step['restart']:
                self.uc.mem_write(TRACKER, dwords(self.frame))
                self.uc.mem_write(TRACKER + 8, dwords(2))
        elif op == 'update':
            self.call(0x54E4D0, TRACKER, [])
        elif op == 'remove':
            self.call(0x54E590, TRACKER, [self.pointer(step['pointer'])])
        elif op == 'clear':
            self.call(0x54E6F0, TRACKER, [])
        else:
            raise ValueError(op)
        return dict(self.state(), events=self.events)

    def run(self):
        initial = self.state()
        return dict(start=initial, steps=[self.step(step) for step in self.row['script']])


def child(location, facing=0, missile_spawn=True, ammo=0, rot=4, turn_to=None, facing_frame=0):
    return dict(location=location, facing=facing, missile_spawn=missile_spawn, ammo=ammo, rot=rot,
                turn_to=turn_to, facing_frame=facing_frame)


def push(frame, child_index, target=None, restart=True):
    return dict(frame=frame, op='push', child=child_index, target=target, restart=restart)


def update(*frames):
    return [dict(frame=frame, op='update') for frame in frames]


def remove(frame, kind, index):
    return dict(frame=frame, op='remove', pointer=[kind, index])


def clear(frame):
    return dict(frame=frame, op='clear')


CENTER = 40 * 256 + 128
FACINGS = [0x0000, 0x0FFF, 0x1000, 0x2FFF, 0x3000, 0x5000, 0x7FFF, 0x8000, 0xA000, 0xC000, 0xDFFF, 0xF000,
           0xFFFF]

ROWS = [
    ('launches_restart_the_cadence', dict(
        children=[child([CENTER, CENTER, 500], ammo=1), child([CENTER + 300, CENTER, 500], ammo=1)],
        targets=[[CENTER + 2000, CENTER - 700, 0], [60 * 256 + 255, 30 * 256, 0]],
        script=[*update(1, 2, 3), push(10, 0, 0), *update(*range(10, 14)), push(20, 1, 1),
                *update(*range(20, 56)), remove(60, 'child', 0), *update(*range(60, 84)),
                remove(84, 'child', 1), *update(111, 112, 113)])),
    ('target_coordinates_round_toward_zero', dict(
        children=[child([CENTER, CENTER, 0]) for _ in range(5)],
        targets=[[0, 0, 0], [255, 256, 0], [40 * 256 + 255, 7 * 256, 900], [12345, 6789, 0],
                 [127 * 256 + 255, 127 * 256 + 255, 0]],
        script=[push(5, index, index) for index in range(5)] + update(6, 7))),
    ('no_target_steps_along_the_current_facing', dict(
        children=[child([CENTER + (index % 3) * 127, CENTER - (index % 2) * 128, 0], facing=facing)
                  for index, facing in enumerate(FACINGS)]
        + [child([CENTER, CENTER, 0], facing=0x0000, rot=4, turn_to=0x8000, facing_frame=3)],
        targets=[],
        script=[push(8, index) for index in range(len(FACINGS) + 1)] + update(9, 10))),
    ('a_child_without_missile_spawn_crashes', dict(
        children=[child([CENTER, CENTER, 300], missile_spawn=False, ammo=3), child([CENTER, CENTER, 300])],
        targets=[[CENTER + 512, CENTER, 0]],
        script=[push(4, 0, 0), push(4, 1, 0), *update(5, 6)])),
    ('remove_scans_from_the_back', dict(
        children=[child([CENTER + 256 * index, CENTER, 0], facing=0x4000) for index in range(4)],
        targets=[[CENTER, CENTER + 1024, 0]],
        script=[push(2, 0, 0), push(2, 1), push(2, 2, 0), push(2, 0), remove(3, 'child', 0),
                remove(3, 'child', 3), remove(3, 'target', 0), remove(3, 'child', 2), remove(3, 'child', 0),
                *update(4, 5)])),
    ('clear_drops_the_nodes_and_restarts_one_frame', dict(
        children=[child([CENTER, CENTER, 0], facing=0x6000), child([CENTER, CENTER, 0], facing=0xE000)],
        targets=[],
        script=[push(40, 0), push(41, 1), *update(42, 43), clear(100), *update(100, 101, 102, 131)])),
    ('a_push_without_restart_waits_for_the_running_cadence', dict(
        children=[child([CENTER, CENTER, 0]), child([CENTER - 600, CENTER + 600, 0], facing=0xA000)],
        targets=[[CENTER + 900, CENTER + 900, 0]],
        script=[push(10, 0, 0), *update(10, 11, 12), push(25, 1, None, restart=False),
                *update(*range(25, 44))])),
]


def generate():
    return [dict(name=name, input=row, output=Tracker(row).run()) for name, row in ROWS]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='The kamikaze tracker at 0x00ABC5F8: Push (MissileSpawn test and Crash, the target-cell and '
              'facing-step arms, Ammo and +0x6CA, node append and vector growth), Update (the {Frame, 30} '
              'cadence and the per-node Ammo, Assign_Target and Queue_Mission), Remove (child arm, scanned '
              'from the back) and Clear (the {Frame, 1} restart), with the callers\' {Frame, 2} restarts. '
              'Not the Remove cell arm, Save/Load, or the dummy cell GetCellAt answers off the cell array.',
        entry_points={'static_init': 0x54E260, 'push': 0x54E3B0, 'update': 0x54E4D0, 'remove': 0x54E590,
                      'clear': 0x54E6F0, 'facing_current': 0x4C93D0, 'occupied_cell': 0x5F6960,
                      'adjacent_cell': 0x481810, 'get_cell_at': 0x5657A0},
        assumptions=['FPCW 0E7F; the tracker static initializer and Clear run at frame 0; the cell-delta '
                     'table initializer 0x0049F2F0 runs first; MapClass+0x13C/+0x140 name a 0x40000-entry '
                     'cell array whose cells (0..127, 0..127) exist; child facings are built at the row '
                     'facing_frame; child +0x6C4 names a type whose +0xD68 MissileSpawn byte follows the '
                     'row; child Ammo +0x2FC and +0x6CA start from the row.'],
        substitutions=['Child Crash(+0x3DC), Assign_Target(+0x3C8) and Queue_Mission(+0x1E8) are recorded '
                       'seams; the target GetCoords(+0x48) answers the row coordinate; operator new is a bump '
                       'allocator; operator delete and atexit return.'],
    ))
