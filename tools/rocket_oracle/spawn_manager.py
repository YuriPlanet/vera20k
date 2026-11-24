"""Original SpawnManagerClass: AI, SetTarget, PointerExpired, ClearAllTargets and Kill_All_Spawns.

Each row runs a script over one supplied manager (SpawnManagerClass, 0x74 bytes; its nodes are
SpawnControl, 0x18 bytes) and its owner. A script step sets the frame counter (0x00A8ED84) and
calls one entry, thiscall on the manager:

- ai: SpawnManagerClass::AI 0x006B7230 (RET), once per frame over the step's frame range;
- set_target: SetTarget 0x006B7B90 (RET 4);
- pointer_expired: PointerExpired 0x006B7C60 (RET 4) with a target, child or the owner;
- clear: ClearAllTargets 0x006B7BB0 (RET);
- kill: Kill_All_Spawns 0x006B7100 (RET);
- world: changes the row's world (the owner's and children's answers and fields), no call.

Native and executed: those five bodies (each calls the others as the original does),
HouseClass::IonSensitivesShouldBeOffline 0x0053A130 (constant false), FacingClass::Current
0x004C93D0 on the owner's PrimaryFacing (+0x388, built by the constructor 0x004C91C0, Set_ROT
0x004C9680 and Snap 0x004C9300), MapClass::operator[] 0x005657A0 and CellClass::Adjacent_Cell
0x00481810 (after the cell-delta table initializer 0x0049F2F0) over a cell array holding cells
(0..127, 0..127), and the CMislType offsets 0x0084009C/0x008400A0 from the retail bytes.

Supplied seams, each a scratch INT3 stub or an entry hook that records its arguments in call
order and answers from the row's world with the native callee's stack cleanup:
- the owner (vtable stubs): GetTechnoType vt+0x84 (the owner type: MissileSpawn +0xD68 and the
  burst base +0xDB0), GetWeapon vt+0x3F8 (weapon 0: Burst +0x9C, the +0x131 byte), GetFLH
  vt+0xB0 (records weapon index, base and the burst index +0x3B8 at the call; answers the row
  coordinate), GetMapCoords vt+0x1B8, CanFireAtTarget vt+0x3AC, Owner vt+0x3C; its locomotor
  interface at +0x674 (Is_Moving +0x10, Is_Moving_Now +0x80);
- each child (vtable stubs): Unlimbo vt+0xD8, Assign_Destination vt+0x480, Queue_Mission
  vt+0x1E8, Assign_Target vt+0x3C8, Limbo vt+0xD4, GetMapCoords vt+0x1B8, UnInit vt+0xF8; the
  spawn type's CreateObject vt+0x8C hands out the row's next spare child;
- entry hooks: the kamikaze tracker's Push 0x0054E3B0, answered after its child stores (a
  MissileSpawn= child gets +0x6CA = 1 and Ammo +0x2FC = 1; any other child Crashes, recorded),
  and Remove 0x0054E590 (the tracker's node bookkeeping is kamikaze.py's evidence), operator
  new 0x007C8E17 (bump allocator),
  AnimTypeClass::FindIndexByName 0x00427CB0 (answers 0 for V3TAKOFF) and the AnimClass
  constructor 0x00421EA0.
Every other vtable entry of the owner, its locomotor, the children and the spawn type fails the
row, as do _com_issue_error 0x007DC720 and a write outside the stack, the scratch objects, the
bump heap and the kamikaze timer (0x00ABC5F8..0x00ABC603).

Each step records the normalized events (the vocabulary the Rust replay produces), the native
detail (GetFLH arguments, the Unlimbo coordinate and direction, the anim), and afterwards the
manager (status +0x70, targets +0x68/+0x6C, UpdateTimer +0x50/+0x58, SpawnTimer +0x5C/+0x64),
its nodes (unit, status, timer, missile flag), the kamikaze timer, the owner's burst index and
the children's Health, EstimatedHealth, Ammo and SpawnOwner.

Usage: python -m tools.rocket_oracle.spawn_manager [--check|--write]
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, STACK_BASE, STACK_SIZE, OracleError, finish_vectors, load_image, provenance,
    run_checked,
)

AI, SET_TARGET, CLEAR_ALL, POINTER_EXPIRED, KILL_ALL = 0x6B7230, 0x6B7B90, 0x6B7BB0, 0x6B7C60, 0x6B7100
FRAME, RULES_POINTER, KAMIKAZE, MAP = 0xA8ED84, 0x8871E0, 0xABC5F8, 0x87F7E8
ANIM_TYPES_POINTER, V3TAKOFF_NAME = 0x8B4154, 0x840098
PUSH, KAMIKAZE_REMOVE = 0x54E3B0, 0x54E590
OPERATOR_NEW, ANIM_FIND, ANIM_CTOR, COM_ERROR = 0x7C8E17, 0x427CB0, 0x421EA0, 0x7DC720
FACING_CTOR, FACING_ROT, FACING_SNAP = 0x4C91C0, 0x4C9680, 0x4C9300
CELL_DELTA_INIT = 0x49F2F0
# Case 0's store of the burst parity into the owner's CurrentBurstIndex (+0x3B8).
BURST_PARITY_STORE = 0x6B73F6
HOOKS = {PUSH: ('push', 8), KAMIKAZE_REMOVE: ('kamikaze_remove', 4), OPERATOR_NEW: ('operator_new', 0),
         ANIM_FIND: ('anim_find', 0), ANIM_CTOR: ('anim', 0x1C), COM_ERROR: ('com_error', None)}

WORK, WORK_SIZE = 0x21000000, 0x40000
MANAGER = WORK
ITEMS = WORK + 0x100
NODES = WORK + 0x200            # SpawnControl nodes, 0x20 apart
OWNER = WORK + 0x1000
OWNER_VT = WORK + 0x1800
OWNER_TYPE = WORK + 0x2000
WEAPON_STRUCT = WORK + 0x3000
WEAPON_TYPE = WORK + 0x3100
LOCO = WORK + 0x3400
LOCO_VT = WORK + 0x3500
HOUSE = WORK + 0x3800
RULES = WORK + 0x4000
SPAWN_TYPE = WORK + 0x5000
SPAWN_TYPE_VT = WORK + 0x6000
OTHER_TYPE = WORK + 0x7000      # a child type that is not the pool's
CHILD_VT = WORK + 0x8000
STUBS = WORK + 0x8800
ANIM_TYPES = WORK + 0x8C00
ANIM_TYPE = WORK + 0x8D00
DUMMY_TYPES = WORK + 0x8E00     # the Rules type pointers the pool's type is not
TARGETS = WORK + 0x9000         # 0x100 apart
CHILDREN, CHILD_SIZE, MAX_CHILDREN = WORK + 0xA000, 0x800, 16
FAILS = WORK + 0x1A000          # INT3 entries that fail the row, one per unexpected vtable slot
HEAP, HEAP_SIZE = 0x23000000, 0x10000
CELL_ARRAY, CELL_ARRAY_COUNT = 0x24000000, 0x40000
CELLS, CELL_SIZE, CELL_SPAN = 0x25000000, 0x40, 128
SP = STACK_BASE + STACK_SIZE - 0x1000

# Vtable stubs: name -> (vtable, slot, stack cleanup).
STUB_SLOTS = {
    'owner_type': ('owner', 0x84, 0), 'get_weapon': ('owner', 0x3F8, 4), 'get_flh': ('owner', 0xB0, 0x14),
    'owner_cell': ('owner', 0x1B8, 4), 'can_fire_at': ('owner', 0x3AC, 4), 'owner_house': ('owner', 0x3C, 0),
    'is_moving': ('loco', 0x10, 4), 'is_moving_now': ('loco', 0x80, 4),
    'unlimbo': ('child', 0xD8, 8), 'set_destination': ('child', 0x480, 8), 'queue_mission': ('child', 0x1E8, 8),
    'assign_target': ('child', 0x3C8, 4), 'limbo': ('child', 0xD4, 0), 'child_cell': ('child', 0x1B8, 4),
    'uninit': ('child', 0xF8, 0), 'create_object': ('type', 0x8C, 4),
}
STUB_AT = {name: STUBS + 0x10 * index for index, name in enumerate(STUB_SLOTS)}
STUB_NAME = {address: name for name, address in STUB_AT.items()}
VTABLES = {'owner': (OWNER_VT, 0x600), 'loco': (LOCO_VT, 0x100), 'child': (CHILD_VT, 0x600),
           'type': (SPAWN_TYPE_VT, 0x100)}
FAIL_BASE = {'owner': FAILS, 'loco': FAILS + 0x800, 'child': FAILS + 0x1000, 'type': FAILS + 0x1800}


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def s32(value):
    return struct.unpack('<i', dwords(value))[0]


def node_address(index):
    return NODES + 0x20 * index


def child_address(index):
    return CHILDREN + CHILD_SIZE * index


def target_address(index):
    return TARGETS + 0x100 * index


class Manager:
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
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.mem_write(STUBS, b'\xCC' * 0x400)
        u.mem_write(FAILS, b'\xCC' * 0x2000)
        self.heap = HEAP
        self.events, self.detail, self.violations = [], [], []
        self.pending_parity = None
        self.world = {'owner': dict(row['owner']), 'children': [dict(c) for c in row['children']]}
        self.spares = list(range(len(row['children']) - row['spares'], len(row['children'])))
        self.write_frame(0)
        self.write_map()
        self.call(CELL_DELTA_INIT, 0, [])
        self.write_static()
        self.write_owner()
        for index in range(len(row['children'])):
            self.write_child(index)
        self.write_manager()
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUBS, end=STUBS + 0x3FF)
        u.hook_add(UC_HOOK_CODE, self.on_fail, begin=FAILS, end=FAILS + 0x1FFF)
        for address in HOOKS:
            u.hook_add(UC_HOOK_CODE, self.on_hook, begin=address, end=address)
        u.hook_add(UC_HOOK_CODE, self.on_parity, begin=BURST_PARITY_STORE, end=BURST_PARITY_STORE)
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)
        self.auditing = True

    # -- fixture
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

    def write_static(self):
        u, row = self.uc, self.row
        for kind, (vtable, size) in VTABLES.items():
            u.mem_write(vtable, b''.join(dwords(FAIL_BASE[kind] + offset) for offset in range(0, size, 4)))
        for name, (kind, slot, _) in STUB_SLOTS.items():
            u.mem_write(VTABLES[kind][0] + slot, dwords(STUB_AT[name]))
        u.mem_write(LOCO, dwords(LOCO_VT))
        u.mem_write(SPAWN_TYPE, dwords(SPAWN_TYPE_VT))
        spawn = row['spawn_type']
        u.mem_write(SPAWN_TYPE + 0xD68, bytes([spawn['missile_spawn']]))
        u.mem_write(SPAWN_TYPE + 0x684, dwords(spawn['ammo']))
        u.mem_write(SPAWN_TYPE + 0xA0, dwords(spawn['strength']))
        u.mem_write(OTHER_TYPE + 0xD68, bytes([row.get('other_type_missile_spawn', 0)]))
        u.mem_write(OTHER_TYPE + 0x684, dwords(1))
        u.mem_write(OTHER_TYPE + 0xA0, dwords(1))
        # Rules: the V3 and DMisl frame pairs and the three type pointers.
        u.mem_write(RULES_POINTER, dwords(RULES))
        rules = row['rules']
        u.mem_write(RULES + 0x4B0, dwords(*rules['v3_frames']))
        u.mem_write(RULES + 0x4E4, dwords(*rules['dmisl_frames']))
        for index, (slot, family) in enumerate([(0x4E0, 'v3'), (0x514, 'dmisl'), (0x548, 'cmisl')]):
            u.mem_write(RULES + slot, dwords(SPAWN_TYPE if row['family'] == family else DUMMY_TYPES + 0x10 * index))
        u.mem_write(ANIM_TYPES_POINTER, dwords(ANIM_TYPES))
        u.mem_write(ANIM_TYPES, dwords(ANIM_TYPE))
        for index in range(len(row.get('targets', []))):
            u.mem_write(target_address(index), dwords(0x7E0000 + index))

    def write_owner(self):
        u, owner = self.uc, self.world['owner']
        u.mem_write(OWNER, dwords(OWNER_VT))
        u.mem_write(OWNER + 0x14, bytes([4 if owner['foot'] else 0]))
        u.mem_write(OWNER + 0x6C, dwords(owner['health']))
        u.mem_write(OWNER + 0x90, bytes([owner['alive_byte']]))
        u.mem_write(OWNER + 0x9C, dwords(*owner['location']))
        u.mem_write(OWNER + 0x21C, dwords(HOUSE))
        u.mem_write(OWNER + 0x3B8, dwords(owner['burst_index']))
        u.mem_write(OWNER + 0x674, dwords(LOCO))
        u.mem_write(OWNER + 0x6AD, bytes([owner['swap_latch']]))
        u.mem_write(OWNER_TYPE + 0xD68, bytes([owner['missile_spawn']]))
        u.mem_write(OWNER_TYPE + 0xDB0, dwords(*owner['second_spawn_offset']))
        weapon = owner['weapon']
        u.mem_write(WEAPON_STRUCT, dwords(WEAPON_TYPE if weapon is not None else 0))
        if weapon is not None:
            u.mem_write(WEAPON_TYPE + 0x9C, dwords(weapon['burst']))
            u.mem_write(WEAPON_TYPE + 0x131, bytes([weapon['spawner']]))
        facing = OWNER + 0x388
        self.call(FACING_CTOR, facing, [])
        self.call(FACING_ROT, facing, [owner.get('rot', 8)])
        u.mem_write(SP + 0x100, dwords(owner['facing']))
        self.call(FACING_SNAP, facing, [SP + 0x100])

    def write_child(self, index):
        u, child = self.uc, self.world['children'][index]
        address = child_address(index)
        u.mem_write(address, dwords(CHILD_VT))
        u.mem_write(address + 0x6C, dwords(child['health'], child['health']))
        u.mem_write(address + 0x9C, dwords(*child['location']))
        u.mem_write(address + 0x2D4, dwords(0))
        u.mem_write(address + 0x2FC, dwords(child['ammo']))
        u.mem_write(address + 0x6C4, dwords(OTHER_TYPE if child.get('type') == 'other' else SPAWN_TYPE))
        u.mem_write(address + 0x6CA, bytes([child.get('tracked', 0)]))

    def write_manager(self):
        u, manager = self.uc, self.row['manager']
        u.mem_write(MANAGER, bytes(0x74))
        u.mem_write(MANAGER + 0x24, dwords(OWNER, SPAWN_TYPE, len(manager['nodes']), manager['regen'],
                                            manager['reload']))
        u.mem_write(MANAGER + 0x3C, dwords(ITEMS, len(manager['nodes'])))
        u.mem_write(MANAGER + 0x48, dwords(len(manager['nodes'])))
        u.mem_write(MANAGER + 0x50, dwords(manager['update_timer'][0], 0, manager['update_timer'][1]))
        u.mem_write(MANAGER + 0x5C, dwords(manager['spawn_timer'][0], 0, manager['spawn_timer'][1]))
        u.mem_write(MANAGER + 0x68, dwords(self.pointer(manager['target']), self.pointer(manager['new_target']),
                                            manager['status']))
        for index, node in enumerate(manager['nodes']):
            address = node_address(index)
            u.mem_write(ITEMS + 4 * index, dwords(address))
            u.mem_write(address, dwords(self.pointer(node['unit']), node['status'], node['timer'][0], 0,
                                        node['timer'][1], node['missile']))

    # -- calls
    def read32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def call(self, entry, this, args):
        u = self.uc
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, this)
        run_checked(u, entry, RET_MAGIC, count=2_000_000, required_addresses=[entry])
        if u.reg_read(UC_X86_REG_ESP) != SP + 4 + 4 * len(args):
            raise OracleError(f'0x{entry:08X} returned with the wrong stack cleanup')
        if self.violations:
            raise OracleError('; '.join(self.violations))
        return u.reg_read(UC_X86_REG_EAX)

    def ret(self, cleanup, result=0):
        u = self.uc
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, result & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def arg(self, index):
        return self.read32(self.uc.reg_read(UC_X86_REG_ESP) + 4 + 4 * index)

    def fail(self, message):
        self.violations.append(message)
        self.uc.emu_stop()

    # -- naming
    def pointer(self, reference):
        if reference is None:
            return 0
        if reference == 'owner':
            return OWNER
        kind, index = reference
        return child_address(index) if kind == 'child' else target_address(index)

    def name(self, pointer):
        if pointer == 0:
            return None
        if pointer == OWNER:
            return 'owner'
        if pointer == HOUSE:
            return 'house'
        if CHILDREN <= pointer < CHILDREN + CHILD_SIZE * MAX_CHILDREN and (pointer - CHILDREN) % CHILD_SIZE == 0:
            return ['child', (pointer - CHILDREN) // CHILD_SIZE]
        if TARGETS <= pointer < TARGETS + 0x1000 and (pointer - TARGETS) % 0x100 == 0:
            return ['target', (pointer - TARGETS) // 0x100]
        if CELLS <= pointer < CELLS + CELL_SPAN * CELL_SPAN * CELL_SIZE:
            return ['cell', *struct.unpack('<hh', self.uc.mem_read(pointer + 0x24, 4))]
        raise OracleError(f'unexpected pointer 0x{pointer:08X}')

    def child_of(self, this):
        name = self.name(this)
        if not isinstance(name, list) or name[0] != 'child':
            raise OracleError(f'child call on 0x{this:08X}')
        return name[1]

    # -- hooks
    def on_stub(self, u, address, _size, _data):
        name = STUB_NAME.get(address)
        if name is None:
            return self.fail(f'unknown stub 0x{address:08X}')
        kind, _, pops = STUB_SLOTS[name]
        this = u.reg_read(UC_X86_REG_ECX)
        owner = self.world['owner']
        value = 0
        if kind == 'owner' and this != OWNER:
            return self.fail(f'{name} on 0x{this:08X}')
        if kind == 'loco':
            if self.arg(0) != LOCO:
                return self.fail(f'{name} on 0x{self.arg(0):08X}')
            self.events.append([self.frame, name])
            value = owner['moving'] if name == 'is_moving' else owner['moving_now']
        elif name == 'owner_type':
            value = OWNER_TYPE
        elif name == 'get_weapon':
            if self.arg(0) != 0:
                return self.fail(f'GetWeapon({self.arg(0)})')
            value = WEAPON_STRUCT
        elif name == 'get_flh':
            out, index = self.arg(0), self.arg(1)
            base = [s32(self.arg(2)), s32(self.arg(3)), s32(self.arg(4))]
            self.detail.append([self.frame, 'get_flh', s32(index), base, s32(self.read32(OWNER + 0x3B8))])
            u.mem_write(out, dwords(*owner['flh']))
            value = out
        elif name == 'owner_cell':
            out = self.arg(0)
            u.mem_write(out, struct.pack('<hh', *owner['cell']))
            value = out
        elif name == 'can_fire_at':
            self.events.append([self.frame, 'can_fire_at', self.name(self.arg(0))])
            value = owner['can_fire']
        elif name == 'owner_house':
            value = HOUSE
        elif name == 'create_object':
            if this != SPAWN_TYPE or self.arg(0) != HOUSE:
                return self.fail('CreateObject on another type or house')
            if not self.spares:
                return self.fail('no spare child left')
            index = self.spares.pop(0)
            self.events.append([self.frame, 'create_object', ['child', index]])
            value = child_address(index)
        else:
            child = self.child_of(this)
            if name == 'unlimbo':
                coord = [s32(v) for v in struct.unpack('<III', u.mem_read(self.arg(0), 12))]
                self.detail.append([self.frame, 'unlimbo', ['child', child], coord, s32(self.arg(1))])
                self.events.append([self.frame, 'launch', ['child', child], self.pending_parity])
                self.pending_parity = None
                value = 1
            elif name == 'set_destination':
                self.events.append([self.frame, 'set_destination', ['child', child], self.name(self.arg(0)),
                                    s32(self.arg(1))])
                value = 1
            elif name == 'queue_mission':
                self.events.append([self.frame, 'queue_mission', ['child', child], s32(self.arg(0)),
                                    s32(self.arg(1))])
                value = 1
            elif name == 'assign_target':
                self.events.append([self.frame, 'assign_target', ['child', child], self.name(self.arg(0))])
            elif name == 'limbo':
                self.events.append([self.frame, 'limbo', ['child', child]])
                value = 1
            elif name == 'uninit':
                self.events.append([self.frame, 'uninit', ['child', child]])
            elif name == 'child_cell':
                out = self.arg(0)
                u.mem_write(out, struct.pack('<hh', *self.world['children'][child]['cell']))
                value = out
        return self.ret(pops, value)

    def on_fail(self, u, address, _size, _data):
        for kind, base in FAIL_BASE.items():
            if base <= address < base + 0x800:
                return self.fail(f'{kind} vtable slot +0x{address - base:X} reached')
        return self.fail(f'fail stub 0x{address:08X}')

    def on_hook(self, u, address, _size, _data):
        name, pops = HOOKS[address]
        if pops is None:
            return self.fail(f'{name} reached')
        value = 0
        if name == 'push':
            if u.reg_read(UC_X86_REG_ECX) != KAMIKAZE:
                return self.fail('Push on another tracker')
            pushed = self.arg(0)
            self.events.append([self.frame, 'push', self.name(pushed), self.name(self.arg(1))])
            if u.mem_read(self.read32(pushed + 0x6C4) + 0xD68, 1)[0]:
                u.mem_write(pushed + 0x6CA, b'\x01')
                u.mem_write(pushed + 0x2FC, dwords(1))
            else:
                self.detail.append([self.frame, 'crash', self.name(pushed)])
        elif name == 'kamikaze_remove':
            if u.reg_read(UC_X86_REG_ECX) != KAMIKAZE:
                return self.fail('Remove on another tracker')
            self.events.append([self.frame, 'kamikaze_remove', self.name(self.arg(0))])
        elif name == 'operator_new':
            size = self.arg(0)
            value = self.heap
            self.heap += (size + 15) & ~15
            if self.heap > HEAP + HEAP_SIZE:
                return self.fail('heap exhausted')
        elif name == 'anim_find':
            text = bytes(u.mem_read(u.reg_read(UC_X86_REG_ECX), 9))
            if text != b'V3TAKOFF\x00':
                return self.fail(f'FindIndexByName({text!r})')
        elif name == 'anim':
            if self.arg(0) != ANIM_TYPE:
                return self.fail('AnimClass of another type')
            coord = [s32(v) for v in struct.unpack('<III', u.mem_read(self.arg(1), 12))]
            self.detail.append([self.frame, 'anim', 'V3TAKOFF', coord, *[s32(self.arg(i)) for i in range(2, 7)]])
            self.events.append([self.frame, 'takeoff_anim'])
            value = u.reg_read(UC_X86_REG_ECX)
        return self.ret(pops, value)

    def on_parity(self, u, _address, _size, _data):
        self.pending_parity = u.reg_read(UC_X86_REG_ECX)

    def on_write(self, u, _access, address, size, value, _data):
        if not getattr(self, 'auditing', False):
            return
        end = address + size
        if STACK_BASE <= address and end <= STACK_BASE + STACK_SIZE:
            return
        if HEAP <= address and end <= HEAP + HEAP_SIZE:
            return
        if KAMIKAZE <= address and end <= KAMIKAZE + 0xC:
            return
        if address == OWNER + 0x3B8 and size == 4:
            return
        if MANAGER <= address and end <= NODES + 0x20 * 16:
            return
        if CHILDREN <= address and end <= CHILDREN + CHILD_SIZE * MAX_CHILDREN:
            return
        self.fail(f'write at 0x{address:08X}+{size} from 0x{u.reg_read(UC_X86_REG_EIP):08X}')

    # -- state
    def state(self):
        u = self.uc
        start, _, duration = struct.unpack('<iii', u.mem_read(MANAGER + 0x50, 12))
        spawn_start, _, spawn_duration = struct.unpack('<iii', u.mem_read(MANAGER + 0x5C, 12))
        nodes = []
        for index in range(self.read32(MANAGER + 0x48)):
            address = self.read32(self.read32(MANAGER + 0x3C) + 4 * index)
            unit, status, timer_start, _, timer_duration, missile = struct.unpack('<IiiIiI', u.mem_read(address, 24))
            nodes.append(dict(unit=self.name(unit), status=status, timer=[timer_start, timer_duration],
                              missile=missile))
        kamikaze_start, _, kamikaze_duration = struct.unpack('<iii', u.mem_read(KAMIKAZE, 12))
        children = []
        for index in range(len(self.world['children'])):
            address = child_address(index)
            health, estimated = struct.unpack('<ii', u.mem_read(address + 0x6C, 8))
            children.append(dict(health=health, estimated=estimated,
                                 ammo=s32(self.read32(address + 0x2FC)),
                                 spawn_owner=self.name(self.read32(address + 0x2D4))))
        return dict(status=s32(self.read32(MANAGER + 0x70)), target=self.name(self.read32(MANAGER + 0x68)),
                    new_target=self.name(self.read32(MANAGER + 0x6C)), update_timer=[start, duration],
                    spawn_timer=[spawn_start, spawn_duration], nodes=nodes,
                    kamikaze_timer=[kamikaze_start, kamikaze_duration],
                    owner_burst=s32(self.read32(OWNER + 0x3B8)), children=children)

    def change_world(self, step):
        self.auditing = False
        for key, value in step.get('owner', {}).items():
            self.world['owner'][key] = value
        if step.get('owner'):
            owner = self.world['owner']
            self.uc.mem_write(OWNER + 0x14, bytes([4 if owner['foot'] else 0]))
            self.uc.mem_write(OWNER + 0x6C, dwords(owner['health']))
            self.uc.mem_write(OWNER + 0x90, bytes([owner['alive_byte']]))
            self.uc.mem_write(OWNER + 0x9C, dwords(*owner['location']))
            self.uc.mem_write(OWNER + 0x6AD, bytes([owner['swap_latch']]))
            self.uc.mem_write(OWNER_TYPE + 0xD68, bytes([owner['missile_spawn']]))
        for index, fields in step.get('children', {}).items():
            child = self.world['children'][int(index)]
            child.update(fields)
            address = child_address(int(index))
            if 'health' in fields:
                self.uc.mem_write(address + 0x6C, dwords(child['health']))
            if 'ammo' in fields:
                self.uc.mem_write(address + 0x2FC, dwords(child['ammo']))
            if 'location' in fields:
                self.uc.mem_write(address + 0x9C, dwords(*child['location']))
            if 'tracked' in fields:
                self.uc.mem_write(address + 0x6CA, bytes([child['tracked']]))
        self.auditing = True

    def step(self, step):
        self.events, self.detail = [], []
        op = step['op']
        if op == 'ai':
            first, last = step['frames']
            for frame in range(first, last + 1):
                self.write_frame(frame)
                self.call(AI, MANAGER, [])
        else:
            if op != 'world':
                self.write_frame(step['frame'])
            if op == 'set_target':
                self.call(SET_TARGET, MANAGER, [self.pointer(step['target'])])
            elif op == 'pointer_expired':
                self.call(POINTER_EXPIRED, MANAGER, [self.pointer(step['pointer'])])
            elif op == 'clear':
                self.call(CLEAR_ALL, MANAGER, [])
            elif op == 'kill':
                self.call(KILL_ALL, MANAGER, [])
            elif op == 'world':
                self.change_world(step)
            else:
                raise ValueError(op)
        return dict(self.state(), events=self.events, detail=self.detail)

    def run(self):
        initial = self.state()
        return dict(start=initial, steps=[self.step(step) for step in self.row['script']])


# -- rows
CENTER = 40 * 256 + 128


def owner(**fields):
    base = dict(foot=True, health=600, alive_byte=1, location=[CENTER, CENTER, 0], burst_index=0, swap_latch=0,
                missile_spawn=0, second_spawn_offset=[0, 0, 0], weapon=dict(burst=1, spawner=1), facing=0x4000,
                rot=8, moving=0, moving_now=0, flh=[CENTER + 100, CENTER - 40, 120], cell=[40, 40], can_fire=1)
    base.update(fields)
    return base


def child(**fields):
    base = dict(health=100, ammo=1, location=[CENTER, CENTER, 0], cell=[40, 40], tracked=0)
    base.update(fields)
    return base


def node(unit, status=0, timer=(-1, 0), missile=1):
    return dict(unit=unit, status=status, timer=list(timer), missile=missile)


def manager(nodes, regen=400, reload=0, status=0, target=None, new_target=None, update_timer=(0, 20),
            spawn_timer=(-1, 0)):
    return dict(nodes=nodes, regen=regen, reload=reload, status=status, target=target, new_target=new_target,
                update_timer=list(update_timer), spawn_timer=list(spawn_timer))


def row(family, nodes, children, script, spares=0, spawn_type=None, owner_fields=None, targets=2, **extra):
    return dict(family=family, rules=dict(v3_frames=[0, 60], dmisl_frames=[9, 18]),
                spawn_type=spawn_type or dict(missile_spawn=1, ammo=1, strength=100),
                owner=owner(**(owner_fields or {})), children=children, spares=spares,
                manager=manager(nodes, **extra.pop('manager', {})), targets=list(range(targets)),
                script=script, **extra)


def ai(first, last=None):
    return dict(op='ai', frames=[first, first if last is None else last])


def set_target(frame, index):
    return dict(op='set_target', frame=frame, target=None if index is None else ['target', index])


def expire(frame, reference):
    return dict(op='pointer_expired', frame=frame, pointer=reference)


def world(owner=None, children=None):
    return dict(op='world', owner=owner or {}, children={str(k): v for k, v in (children or {}).items()})


C = lambda index: ['child', index]  # noqa: E731
T = lambda index: ['target', index]  # noqa: E731

ROWS = [
    # A V3 fires, its missile leaves the slot after Tilt + Pause, the slot regenerates and the
    # launcher fires again; the second missile dies on the rail and frees its slot at once.
    ('v3_cycle', row('v3', [node(C(0))], [child(), child(), child()], spares=2, script=[
        ai(0, 9), set_target(5, 0), ai(10, 45), ai(46, 95), ai(96, 500), set_target(501, 1), ai(502, 530),
        world(children={1: dict(health=0)}), expire(531, C(1)), ai(532, 545), ai(546, 940)])),
    # The V3 is ordered while its manager pass falls on the same frame, then retargeted twice
    # before the launch: SetTarget queues only a target that differs from the current one.
    ('v3_retarget', row('v3', [node(C(0))], [child()], script=[
        ai(0, 19), set_target(20, 0), ai(20, 29), set_target(30, 0), set_target(30, 1), ai(30, 40),
        set_target(41, None), ai(41, 60)])),
    # Owner gates on a missile node: Is_Moving, Is_Moving_Now, the Foot locomotor-swap latch and a
    # latch on a non-Foot owner, which the original ignores.
    ('v3_owner_gates', row('v3', [node(C(0))], [child()], manager=dict(status=1, target=T(0)), script=[
        world(owner=dict(moving=1)), ai(0, 20), world(owner=dict(moving=0, moving_now=1)), ai(21, 30),
        world(owner=dict(moving_now=0, swap_latch=1)), ai(31, 40), world(owner=dict(foot=False)), ai(41, 50)])),
    # A Dreadnought: two DMisl nodes, Burst=2 sets the burst parity per slot, the manager's
    # SpawnTimer spaces the launches 20 frames apart, and the Launching pass waits for both.
    ('dread_volley', row('dmisl', [node(C(0)), node(C(1))], [child(), child(), child(), child()], spares=2,
                         owner_fields=dict(weapon=dict(burst=2, spawner=1), burst_index=1), script=[
        ai(0, 2), set_target(3, 0), ai(3, 80), ai(81, 500)])),
    # A MissileSpawn= owner spaces its launches 9 frames apart; a Boomer pool (CMislType) adds the
    # V3TAKOFF anim and the CMisl offsets, and takes the DMisl frames for its wait.
    ('cmisl_missile_parent', row('cmisl', [node(C(0)), node(C(1))], [child(), child()],
                                 owner_fields=dict(missile_spawn=1, weapon=dict(burst=1, spawner=0)),
                                 manager=dict(status=1, target=T(0), update_timer=(-1, 0)), script=[
        ai(0, 60)])),
    # Mode 0 asks CanFireAtTarget; a refusal clears every target.
    ('cannot_fire', row('v3', [node(C(0))], [child()], owner_fields=dict(can_fire=0), script=[
        set_target(1, 0), ai(1, 30), world(owner=dict(can_fire=1)), set_target(31, 1), ai(31, 40)])),
    # The current target expires while a queued one waits: Launching finds no current target,
    # clears both and drops the queued order; the next order launches.
    ('launching_target_lost', row('v3', [node(C(0))], [child(), child()], spares=1,
                                  manager=dict(status=1, target=T(0), new_target=T(1)), script=[
        ai(0, 4), expire(5, T(0)), ai(5, 30), set_target(31, 1), ai(31, 120)])),
    # PointerExpired arms: the queued target, the current target with and without a queued one,
    # an unrelated object, then the slot child.
    ('pointer_expired_targets', row('v3', [node(C(0))], [child()], manager=dict(target=T(0), new_target=T(1)),
                                    script=[expire(1, T(1)), expire(2, T(0)), set_target(3, 1),
                                            set_target(3, 0), expire(4, T(0)), expire(5, T(1)),
                                            expire(6, C(0))])),
    # A Carrier's wing: the aircraft arms of every node status (2 holds over the owner or comes
    # home, 3 attacks or comes home, 4 docks within 20 leptons of the owner's height or comes
    # back, 6 rearms).
    ('carrier_wing', row(None, [node(C(0), missile=0), node(C(1), missile=0), node(C(2), missile=0)],
                         [child(cell=[41, 40], location=[CENTER + 256, CENTER, 300]), child(), child(), child()],
                         spares=1, spawn_type=dict(missile_spawn=0, ammo=3, strength=150),
                         owner_fields=dict(weapon=dict(burst=1, spawner=1)),
                         manager=dict(regen=300, reload=120), script=[
        ai(0, 0), set_target(1, 0), ai(1, 60),
        world(children={0: dict(ammo=0)}), ai(61, 80),
        world(children={0: dict(cell=[40, 40], location=[CENTER, CENTER, 25])}), ai(81, 90),
        world(children={0: dict(location=[CENTER, CENTER, 19])}), ai(91, 100),
        world(children={1: dict(ammo=0, cell=[40, 40], location=[CENTER, CENTER, -5]),
                         2: dict(ammo=0, cell=[40, 40], location=[CENTER, CENTER, 0])}),
        ai(101, 120), ai(121, 260),
        world(children={1: dict(health=0)}), expire(261, C(1)), ai(262, 600)])),
    # A Carrier whose target dies between its two launches: the airborne child comes home from
    # status 2 and docks; the docked one stays.
    ('carrier_target_lost', row(None, [node(C(0), missile=0), node(C(1), missile=0)], [child(), child()],
                                spawn_type=dict(missile_spawn=0, ammo=3, strength=150),
                                manager=dict(regen=300, reload=120), script=[
        ai(0, 0), set_target(1, 0), ai(1, 34), expire(35, T(0)), ai(36, 80)])),
    # PointerExpired on a slot child: an aircraft child alive and off the tracker keeps its slot,
    # one on the tracker or dead frees it; a missile node always frees it.
    ('pointer_expired_slots', row(None, [node(C(0), status=2, missile=0), node(C(1), status=2, missile=0),
                                         node(C(2), status=2, missile=0), node(C(3), status=2, missile=1)],
                                  [child(), child(tracked=1), child(health=0), child()],
                                  spawn_type=dict(missile_spawn=0, ammo=3, strength=150), script=[
        expire(7, C(0)), expire(8, C(1)), expire(9, C(2)), expire(10, C(3))])),
    # ClearAllTargets hands a launched MissileSpawn child (status 2) to the tracker and frees its
    # slot; an aircraft child in status 2 stays.
    ('clear_all_targets', row('v3', [node(C(0), status=2), node(C(1), status=2, missile=0), node(C(2))],
                              [child(), child(type='other'), child()],
                              manager=dict(status=1, target=T(0), new_target=T(1)), script=[
        dict(op='clear', frame=12)])),
    # A child type with MissileSpawn= on a node the hardcoded families do not flag: the Launching
    # pass pushes it and expires it at once.
    ('missile_child_unflagged', row(None, [node(C(0), missile=0)], [child(), child()], spares=1,
                                    spawn_type=dict(missile_spawn=1, ammo=1, strength=100),
                                    manager=dict(status=1, target=T(0)), script=[ai(0, 30), ai(31, 340)])),
    # Kill_All_Spawns from the owner's PointerExpired on a live owner (zero regen) and a dead one.
    ('kill_all_spawns_alive', row('v3', [node(C(0)), node(C(1), status=1, timer=(0, 60)), node(C(2), status=2),
                                         node(None, status=7, timer=(3, 400)), node(C(3), status=6, timer=(0, 9))],
                                  [child(), child(), child(), child()], manager=dict(target=T(0)), script=[
        expire(20, 'owner')])),
    ('kill_all_spawns_dead', row('dmisl', [node(C(0)), node(C(1), status=2), node(C(2), status=4, missile=0)],
                                 [child(), child(), child()], owner_fields=dict(health=0),
                                 manager=dict(target=T(0)), script=[expire(30, 'owner'), ai(30, 60)])),
    # The update timer: stopped with time left never runs; stopped at zero runs at once.
    ('update_timer_paused', row('v3', [node(C(0))], [child()], manager=dict(update_timer=(-1, 5)), script=[
        set_target(0, 0), ai(0, 40)])),
    ('update_timer_zero', row('v3', [node(None, status=7, timer=(-1, 0))], [child()], spares=1,
                              manager=dict(update_timer=(-1, 0)), script=[ai(7, 7), ai(8, 30)])),
    # Regeneration flags each new child by the pool type against the three Rules types.
    ('regen_flags', row('cmisl', [node(None, status=7, timer=(0, 5), missile=0)], [child()], spares=1,
                        manager=dict(update_timer=(-1, 0)), script=[ai(10, 10)])),
    ('regen_flags_other', row(None, [node(None, status=7, timer=(0, 5), missile=1)], [child()], spares=1,
                              spawn_type=dict(missile_spawn=0, ammo=2, strength=90),
                              manager=dict(update_timer=(-1, 0)), script=[ai(10, 10)])),
]


def generate():
    return [dict(name=name, input=data, output=Manager(data).run()) for name, data in ROWS]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='SpawnManagerClass::AI 0x006B7230 (the UpdateTimer gate, every node status 0..7 with its owner '
              'and target gates, launch, regeneration and rearm, and the manager status 0..2), SetTarget '
              '0x006B7B90, PointerExpired 0x006B7C60 (all four arms), ClearAllTargets 0x006B7BB0 and '
              'Kill_All_Spawns 0x006B7100, over V3, DMisl, CMisl and aircraft pools. Not the constructor, '
              'CountAliveSpawns/CountDockedSpawns, Save/Load, or the seams\' bodies (Unlimbo, Limbo, '
              'Assign_Destination, the tracker, AnimClass).',
        entry_points={'ai': AI, 'set_target': SET_TARGET, 'pointer_expired': POINTER_EXPIRED,
                      'clear_all_targets': CLEAR_ALL, 'kill_all_spawns': KILL_ALL,
                      'ion_offline': 0x53A130, 'facing_current': 0x4C93D0, 'map_cell': 0x5657A0,
                      'adjacent_cell': 0x481810},
        assumptions=['FPCW 0E7F; the cell-delta table initializer 0x0049F2F0 runs first; MapClass+0x13C/+0x140 '
                     'name a 0x40000-entry cell array whose cells (0..127, 0..127) exist; the owner\'s '
                     'PrimaryFacing is built at frame 0 (ROT 8) and snapped to the row facing; the manager, its '
                     'nodes, the owner and children fields are the row\'s; Rules +0x4B0/+0x4B4 and '
                     '+0x4E4/+0x4E8 hold the V3 and DMisl frame pairs, +0x4E0/+0x514/+0x548 the V3, DMisl and '
                     'CMisl types (the pool type in the row\'s family slot).'],
        substitutions=['Owner GetTechnoType/GetWeapon/GetFLH/GetMapCoords/CanFireAtTarget/Owner, its '
                       'locomotor\'s Is_Moving/Is_Moving_Now, the children\'s Unlimbo/Assign_Destination/'
                       'Queue_Mission/Assign_Target/Limbo/GetMapCoords/UnInit and the type\'s CreateObject are '
                       'recorded INT3 stubs answering the row; Push/Remove of the kamikaze tracker, operator '
                       'new, FindIndexByName and the AnimClass constructor are recorded entry hooks.'],
    ))
