"""Original RocketLocomotionClass flights: Move_To, Process and Detonate.

Each row builds the locomotor with its constructor 0x00661EC0, links it to a
supplied owner (+0x0C), calls Move_To 0x006632E0 at the row's first frame,
then advances the frame counter and calls Process 0x006622C0 once a frame
until Detonate 0x00663030 UnInits the owner or the row's frame budget ends.
The rocket module's own CRT initializers (the fourteen at 0x008147C4) run
first, so the bridge deck height [0x00B04E6C] and the untracked cell
[0x00B04E18] hold their startup values.

Native and executed: the constructor, Move_To, Process, the impact predictor
0x006620F0, Detonate, Is_Moving 0x00661F50 and Is_Moving_Now 0x00661F90
through the ILocomotion vtable 0x007F0B1C, Sqrt_Approx, the sine, cosine and
arctangent tables, atan2, ftol, VeterancyStruct::IsElite 0x00750010,
FacingClass Set/Current on the owner's +0x388 (built by its constructor,
Set_ROT and Snap), MapClass::In_Bounds 0x00568300 on the row's Size, the
owner's GetCoords 0x005F65A0, cell-of-location 0x0041BEA0, GetHeight
0x005F5F40 and the -15 z-adjust helper 0x0048ACE0.

Supplied seams (recorded as events, in call order): the owner's Mark (+0x124),
SetLocation (+0x1B4, writes Location only), GetTechnoType (+0x84), the
retained AircraftTracker cell (+0x2F4, Foot+0x560), GetCell (+0x1BC) and that
cell's GetCoords (+0x48), UnInit (+0xF8); AircraftTracker Add 0x004134A0 /
Update 0x004138C0 / Remove 0x004135D0 (Add and Update retain the cell at
+0x560 as their +0x2F8 callback does); DisplayClass::Submit 0x004A9720;
VocClass::PlayAt 0x007509E0; operator new 0x007C8E17, AnimTypeClass::FindIndex
0x00427CB0 and the AnimClass constructor 0x00421EA0; Map::GetCellAt 0x005657A0
(the impact cell's land); SelectAnim 0x0048A4F0; the impact light 0x0048A620;
Apply_area_damage 0x00489280; and the floor height 0x00578080 that GetHeight
reads, from the row's floor (flat, or a hill east of a column).
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance, run_checked,
)

WORK, WORK_SIZE = 0x21000000, 0x20000
LOCO = WORK
INTERFACE = LOCO + 4
OWNER = WORK + 0x1000
OWNER_VTABLE = WORK + 0x2000
TYPE = WORK + 0x3000
OTHER_TYPES = WORK + 0x3800
SPAWNER = WORK + 0x4000
RULES = WORK + 0x5000
WARHEADS = WORK + 0x7000
ANIM_TYPES = WORK + 0x7400
EXPLOSION_TYPE = WORK + 0x7700
ANIM_MEMORY = WORK + 0x8000
CELL = WORK + 0x9000
CELL_VTABLE = WORK + 0x9400
LAND_CELL = WORK + 0x9800
STUBS = WORK + 0xA000
OUTPUT = WORK + 0xB000
ARGUMENT = WORK + 0xB100
(STUB_TYPE, STUB_MARK, STUB_SET_LOCATION, STUB_GET_CELL, STUB_TRACKER_CELL, STUB_UNINIT,
 STUB_CELL_COORDS) = [STUBS + i * 0x10 for i in range(7)]
SP = STACK_BASE + STACK_SIZE - 0x1000

FRAME = 0xA8ED84
RULES_POINTER = 0x8871E0
MAP = 0x87F7E8
ANIM_TYPES_ITEMS = 0x8B4154
STATIC_INITIALIZERS = 0x8147C4
ANIM_NAMES = ['V3TAKOFF', 'V3TRAIL']
ANIM_TYPE = {name: WORK + 0x7500 + 0x100 * index for index, name in enumerate(ANIM_NAMES)}

BLOCK = {'V3': 0x4B0, 'DMisl': 0x4E4, 'CMisl': 0x518}
WARHEAD_FIELD = {'V3': (0xFB0, 0xFB8), 'DMisl': (0xFB4, 0xFBC), 'CMisl': (0xFC0, 0xFC4)}
WARHEAD_NAMES = {'V3': ('V3WH', 'V3EWH'), 'DMisl': ('DMISLWH', 'DMISLEWH'),
                 'CMisl': ('CMISLWH', 'CMISLEWH')}
WARHEAD = {name: WARHEADS + 0x10 * index
           for index, name in enumerate(n for pair in WARHEAD_NAMES.values() for n in pair)}
WARHEAD_NAME = {address: name for name, address in WARHEAD.items()}


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def f32_bits(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def cell_of(x, y):
    """0x0041BEA0: each axis divides by 256 toward zero."""
    return (int(x / 256), int(y / 256))


class Rocket:
    def __init__(self, row):
        self.row = row
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        u.mem_map(WORK, WORK_SIZE)
        u.mem_write(STUBS, b'\xC3' * 0x100)
        u.reg_write(UC_X86_REG_FPCW, 0x027F)
        for index in range(14):
            self.call(self.read32(STATIC_INITIALIZERS + 4 * index), 0, [])
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.events = []
        self.frame = row['first_frame']
        u.hook_add(UC_HOOK_CODE, self.observe)
        self.write_rules()
        self.write_owner()
        u.mem_write(MAP + 0xF4, dwords(*row['map_size']))
        u.mem_write(ANIM_TYPES_ITEMS, dwords(ANIM_TYPES))
        u.mem_write(ANIM_TYPES, dwords(*(ANIM_TYPE[name] for name in ANIM_NAMES)))
        u.mem_write(CELL, dwords(CELL_VTABLE))
        u.mem_write(CELL_VTABLE + 0x48, dwords(STUB_CELL_COORDS))
        u.mem_write(LAND_CELL + 0xEC, dwords(row['impact_land']))
        u.mem_write(FRAME, dwords(self.frame))
        self.call(0x661EC0, LOCO, [])
        u.mem_write(LOCO + 0xC, dwords(OWNER))

    def write_rules(self):
        u, row = self.uc, self.row
        u.mem_write(RULES_POINTER, dwords(RULES))
        for family, offset in BLOCK.items():
            block = row['blocks'][family]
            u.mem_write(RULES + offset, dwords(
                block['pause_frames'], block['tilt_frames'], block['pitch_initial'], block['pitch_final'],
                block['turn_rate'], block['raise_rate'], block['acceleration'], block['altitude'],
                block['damage'], block['elite_damage'], block['body_length'], int(block['lazy_curve']),
                {'owner': TYPE, 'other': OTHER_TYPES + offset, 'null': 0}[block['type']]))
            for field, name in zip(WARHEAD_FIELD[family], WARHEAD_NAMES[family]):
                u.mem_write(RULES + field, dwords(WARHEAD[name]))

    def write_owner(self):
        u, row = self.uc, self.row
        u.mem_write(OWNER, dwords(OWNER_VTABLE))
        for slot, function in [(0x48, 0x5F65A0), (0x84, STUB_TYPE), (0x124, STUB_MARK),
                               (0x1B4, STUB_SET_LOCATION), (0x1B8, 0x41BEA0), (0x1BC, STUB_GET_CELL),
                               (0x1C8, 0x5F5F40), (0x2F4, STUB_TRACKER_CELL), (0xF8, STUB_UNINIT)]:
            u.mem_write(OWNER_VTABLE + slot, dwords(function))
        u.mem_write(OWNER + 0x6C, dwords(row['health']))
        u.mem_write(OWNER + 0x8C, b'\0')
        u.mem_write(OWNER + 0x90, bytes([int(row['alive'])]))
        u.mem_write(OWNER + 0x9C, dwords(*row['start']))
        u.mem_write(OWNER + 0x560, dwords(0))
        u.mem_write(OWNER + 0x6C4, dwords(TYPE))
        u.mem_write(TYPE + 0x52C, dwords(row['aux_sound']))
        u.mem_write(TYPE + 0x678, dwords(row['type_speed']))
        if row['spawner_veterancy'] is None:
            u.mem_write(OWNER + 0x2D4, dwords(0))
        else:
            u.mem_write(OWNER + 0x2D4, dwords(SPAWNER))
            u.mem_write(SPAWNER + 0x150, struct.pack('<f', row['spawner_veterancy']))
        facing = OWNER + 0x388
        self.call(0x4C91C0, facing, [])
        self.call(0x4C9680, facing, [row['rot']])
        u.mem_write(ARGUMENT, dwords(row['facing']))
        self.call(0x4C9300, facing, [ARGUMENT])

    def read32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def read_coord(self, address):
        return list(struct.unpack('<iii', self.uc.mem_read(address, 12)))

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

    def floor(self, x, y):
        hill = self.row['hill']
        if hill is not None and x >= hill['from_x']:
            return hill['floor']
        return self.row['floor']

    def owner_cell(self):
        location = self.read_coord(OWNER + 0x9C)
        return cell_of(location[0], location[1])

    def observe(self, u, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        arg = lambda index: self.read32(sp + 4 + 4 * index)
        ecx, edx = u.reg_read(UC_X86_REG_ECX), u.reg_read(UC_X86_REG_EDX)
        if address == STUB_TYPE:
            self.ret(0, self.read32(OWNER + 0x6C4))
        elif address == STUB_MARK:
            self.events.append(['mark', arg(0)])
            self.ret(4)
        elif address == STUB_SET_LOCATION:
            coord = self.read_coord(arg(0))
            u.mem_write(OWNER + 0x9C, dwords(*coord))
            self.events.append(['set_location', coord])
            self.ret(4)
        elif address == STUB_GET_CELL:
            cell = self.owner_cell()
            bridge = [list(cell)] if list(cell) in self.row['bridge_cells'] else []
            u.mem_write(CELL + 0x140, dwords(0x100 if bridge else 0))
            u.mem_write(CELL + 0x24, struct.pack('<hh', *cell))
            self.ret(0, CELL)
        elif address == STUB_CELL_COORDS:
            x, y = struct.unpack('<hh', u.mem_read(CELL + 0x24, 4))
            out = arg(0)
            u.mem_write(out, dwords(x * 256 + 128, y * 256 + 128, self.row['bridge_floor']))
            self.ret(4, out)
        elif address == STUB_TRACKER_CELL:
            out = arg(0)
            u.mem_write(out, bytes(u.mem_read(OWNER + 0x560, 4)))
            self.ret(4, out)
        elif address == STUB_UNINIT:
            self.events.append(['uninit'])
            self.uninit = True
            self.ret(0)
        elif address == 0x4134A0:
            cell = self.owner_cell()
            u.mem_write(OWNER + 0x560, struct.pack('<hh', *cell))
            self.events.append(['tracker_add', list(cell)])
            self.ret(4)
        elif address == 0x4138C0:
            new = list(struct.unpack('<hh', u.mem_read(sp + 12, 4)))
            u.mem_write(OWNER + 0x560, struct.pack('<hh', *new))
            self.events.append(['tracker_update', new])
            self.ret(12)
        elif address == 0x4135D0:
            self.events.append(['tracker_remove'])
            self.ret(4)
        elif address == 0x4A9720:
            self.events.append(['submit'])
            self.ret(4)
        elif address == 0x7509E0:
            self.events.append(['aux_sound', ecx, self.read_coord(edx)])
            self.ret(4)
        elif address == 0x7C8E17:
            self.ret(0, ANIM_MEMORY)
        elif address == 0x427CB0:
            name = bytes(u.mem_read(ecx, 16)).split(b'\0')[0].decode()
            self.ret(0, ANIM_NAMES.index(name))
        elif address == 0x421EA0:
            kind = arg(0)
            name = next((n for n, a in ANIM_TYPE.items() if a == kind), None)
            if kind == EXPLOSION_TYPE:
                name = 'explosion'
            self.events.append(['anim', name, self.read_coord(arg(1)), arg(2), arg(3), arg(4),
                                struct.unpack('<i', dwords(arg(5)))[0], arg(6)])
            self.ret(28, ecx)
        elif address == 0x5657A0:
            self.events.append(['impact_cell', list(struct.unpack('<hh', u.mem_read(arg(0), 4)))])
            self.ret(4, LAND_CELL)
        elif address == 0x48A4F0:
            self.events.append(['select_anim', ecx, WARHEAD_NAME[edx], arg(0), self.read_coord(arg(1))])
            self.ret(8, EXPLOSION_TYPE)
        elif address == 0x48A620:
            coord = list(struct.unpack('<iii', u.mem_read(sp + 4, 12)))
            self.events.append(['combat_light', ecx, WARHEAD_NAME[edx], coord, arg(3), arg(4)])
            self.ret(20)
        elif address == 0x489280:
            assert arg(0) == OWNER
            self.events.append(['area_damage', self.read_coord(ecx), struct.unpack('<i', dwords(edx))[0],
                                WARHEAD_NAME[arg(1)], arg(2), arg(3)])
            self.ret(16)
        elif address == 0x578080:
            coord = self.read_coord(arg(0))
            self.ret(4, self.floor(coord[0], coord[1]))

    def state(self):
        u = self.uc
        self.call(0x4C93D0, OWNER + 0x388, [OUTPUT])
        timer = struct.unpack('<iiii', u.mem_read(LOCO + 0x24, 16))
        trailer = struct.unpack('<iii', u.mem_read(LOCO + 0x34, 12))
        return dict(
            location=self.read_coord(OWNER + 0x9C),
            facing=struct.unpack('<H', u.mem_read(OUTPUT, 2))[0],
            destination=self.read_coord(LOCO + 0x18),
            mission_timer=[timer[0], timer[2], timer[3]],
            trailer_timer=[trailer[0], trailer[2]],
            mission_state=self.read32(LOCO + 0x40),
            current_speed=struct.unpack('<Q', u.mem_read(LOCO + 0x48, 8))[0],
            resubmit_latch=u.mem_read(LOCO + 0x50, 1)[0],
            spawner_is_elite=u.mem_read(LOCO + 0x51, 1)[0],
            current_pitch=self.read32(LOCO + 0x54),
            cruise_start_distance=struct.unpack('<i', u.mem_read(LOCO + 0x58, 4))[0],
            tracker_cell=list(struct.unpack('<hh', u.mem_read(OWNER + 0x560, 4))),
        )

    def move_to(self, destination):
        self.call(0x6632E0, 0, [INTERFACE, *destination])

    def execute(self):
        row = self.row
        self.uninit = False
        self.move_to(row['destination'])
        moves = {move['frame']: move['destination'] for move in row['later_moves']}
        frames = []
        for _ in range(row['max_frames']):
            self.events = []
            self.frame += 1
            self.uc.mem_write(FRAME, dwords(self.frame))
            if self.frame in moves:
                self.move_to(moves[self.frame])
            if self.frame in row['kill_frames']:
                self.uc.mem_write(OWNER + 0x6C, dwords(0))
            self.call(0x6622C0, 0, [INTERFACE])
            frame = self.state()
            frame['events'] = self.events
            frames.append(frame)
            if self.uninit:
                break
        return dict(start=self.initial, frames=frames)

    def run(self):
        self.initial = self.state()
        return self.execute()


V3 = dict(type='other', pause_frames=0, tilt_frames=60, pitch_initial=f32_bits(0.21),
          pitch_final=f32_bits(0.5), turn_rate=f32_bits(0.05), raise_rate=1,
          acceleration=f32_bits(0.4), altitude=768, damage=200, elite_damage=400,
          body_length=256, lazy_curve=True)
DMISL = dict(type='other', pause_frames=20, tilt_frames=60, pitch_initial=f32_bits(0.0),
             pitch_final=f32_bits(0.5), turn_rate=f32_bits(0.08), raise_rate=1,
             acceleration=f32_bits(0.8), altitude=768, damage=300, elite_damage=600,
             body_length=128, lazy_curve=False)
CMISL = dict(type='other', pause_frames=20, tilt_frames=100, pitch_initial=f32_bits(1.0),
             pitch_final=f32_bits(1.0), turn_rate=f32_bits(0.1), raise_rate=1,
             acceleration=f32_bits(1.0), altitude=768, damage=200, elite_damage=250,
             body_length=128, lazy_curve=False)
CENTER = 30 * 256 + 128


def blocks(owner, **changes):
    result = {'V3': dict(V3), 'DMisl': dict(DMISL), 'CMisl': dict(CMISL)}
    result[owner] = dict(result[owner], type='owner', **changes)
    return result


BASE = dict(first_frame=1000, max_frames=420, health=50, alive=True, start=[CENTER, CENTER, 120],
            facing=0x4000, rot=3, type_speed=38, aux_sound=7, spawner_veterancy=0.0,
            map_size=[20, 60], floor=0, hill=None, bridge_cells=[], bridge_floor=0, impact_land=0,
            later_moves=[], kill_frames=[], destination=[CENTER + 1536, CENTER, 0],
            blocks=blocks('V3'))
ROWS = [
    ('v3_east', dict()),
    ('v3_elite_turns_north', dict(spawner_veterancy=2.0, destination=[CENTER, CENTER - 1800, 0],
                                  impact_land=3)),
    ('v3_diagonal_from_west_facing', dict(facing=0xC000, destination=[CENTER + 2600, CENTER + 2100, 0])),
    ('v3_no_spawner_far', dict(spawner_veterancy=None, destination=[CENTER + 4000, CENTER - 900, 0])),
    ('v3_second_move_to_is_ignored', dict(later_moves=[dict(frame=1100, destination=[CENTER, CENTER, 0])])),
    ('v3_dead_on_the_rail', dict(kill_frames=[1001])),
    ('v3_shot_down_in_cruise', dict(kill_frames=[1150])),
    ('v3_not_alive_skips_the_tracker', dict(alive=False)),
    ('v3_leaves_the_map_bounds', dict(map_size=[20, 25], destination=[CENTER + 6000, CENTER, 0],
                                      max_frames=300)),
    ('v3_vertical_with_no_cruise_distance', dict(destination=[CENTER, CENTER, 0],
                                                 blocks=blocks('V3', pitch_final=f32_bits(1.0)))),
    ('v3_bridge_deck_impact', dict(destination=[CENTER + 1536, CENTER, 0], bridge_cells=[[36, 30]],
                                   bridge_floor=0)),
    ('dmisl_east', dict(rot=4, type_speed=46, destination=[CENTER + 2000, CENTER, 0],
                        blocks=blocks('DMisl'))),
    ('dmisl_short_dive', dict(rot=4, type_speed=46, destination=[CENTER + 500, CENTER + 200, 0],
                              spawner_veterancy=3.0, blocks=blocks('DMisl'))),
    ('dmisl_dives_into_a_hill', dict(rot=4, type_speed=46, destination=[CENTER + 2400, CENTER, 0],
                                     hill=dict(from_x=CENTER + 1700, floor=400), blocks=blocks('DMisl'))),
    ('cmisl_boomer', dict(rot=4, type_speed=51, start=[CENTER, CENTER, 0], destination=[CENTER + 3000,
                          CENTER - 1000, 0], aux_sound=9, blocks=blocks('CMisl'))),
    ('dmisl_with_null_types_takes_the_boomer_arms',
     dict(rot=4, type_speed=46, start=[CENTER, CENTER, 0], destination=[CENTER + 1500, CENTER, 0],
          blocks=dict(V3=dict(V3, type='null'), DMisl=dict(DMISL, type='null'),
                      CMisl=dict(CMISL, type='null')))),
]


def generate():
    return [dict(name=name, input=dict(BASE, **row), output=Rocket(dict(BASE, **row)).run())
            for name, row in ROWS]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='RocketLocomotionClass constructor, Move_To and one Process per frame through Detonate for the '
              'V3, DMisl and CMisl blocks: pause, tilt, climb, LazyCurve and level cruise, dive, the Boomer '
              'raise, the V3TAKOFF/V3TRAIL puffs, AircraftTracker calls, the impact predictor (destination '
              'height, structural-bridge deck, ground), the health tail, In_Bounds refusals and the Detonate '
              'explosion, light and area damage with normal and elite payloads. Not the owner Mark, '
              'SetLocation, display, tracker, sound, anim or damage bodies.',
        entry_points={'constructor': 0x661EC0, 'move_to': 0x6632E0, 'process': 0x6622C0,
                      'predictor': 0x6620F0, 'detonate': 0x663030, 'is_moving_now': 0x661F90,
                      'facing_set': 0x4C9220, 'facing_current': 0x4C93D0, 'in_bounds': 0x568300},
        assumptions=['FPCW 0E7F (WinMain _controlfp(0x300, 0x300) at 0x006BBFC1) after the rocket module '
                     'initializers at 0x008147C4 run under the CRT startup word 027F; frame counter from the '
                     'row first_frame; owner Foot+0x560 zero as the FootClass constructor 0x004D31E0 leaves '
                     'it; owner +0x8C clear; RulesClass blocks at +0x4B0/+0x4E4/+0x518 and warheads at '
                     '+0xFB0..+0xFC4 written per row; MapClass Size at 0x0087F7E8+0xF4 per row; the owner '
                     'type +0x678 speed and +0x52C AuxSound1 per row; the spawn owner veterancy at +0x150 or '
                     'no spawn owner.'],
        substitutions=['Owner Mark(+0x124), SetLocation(+0x1B4, writes Location only), GetTechnoType(+0x84), '
                       'the retained tracker cell(+0x2F4), GetCell(+0x1BC) and UnInit(+0xF8) are supplied; '
                       'AircraftTracker Add/Update/Remove are recorded and Add/Update retain +0x560; '
                       'Submit, PlayAt, operator new, FindIndex, the AnimClass constructor, GetCellAt, '
                       'SelectAnim, the impact light, Apply_area_damage and the floor height 0x00578080 are '
                       'recorded seams; the GetCell answer is one cell object whose +0x140 bridge bit and '
                       'GetCoords (cell centre, row bridge_floor) follow the row.'],
    ))
