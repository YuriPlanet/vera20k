"""Original selected ground Unit action-line chains and shared pixel leaves.

Prepared actor/terrain/camera inputs compose existing native fixture owners.
Original class setters, geometry, timers, projection, clipping and pixel writes
execute without patching gameplay instructions. See action_lines.md for bounds.
"""
from pathlib import Path
import gc
import gzip
import hashlib
import itertools
import json
import struct
import sys

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                              UC_X86_REG_ESP)
from tools import native_inspect, native_oracle as native
from tools.sidebar_oracle import stock
from tools.projectile_oracle import bridge_render_inputs_palette as palette
from tools.procedural_drawing_oracle import house_color, rally
from tools.rules_oracle.bridge_child_sound import sections
from tools.spatial_oracle import track_destination
from tools.spatial_oracle.unit_scatter_state import ACTOR, LOCO
from tools.spatial_oracle.unit_entry import CELL
from tools.spatial_oracle.bridge_damage_admission import words, call, MEM, SP, read32

MAP = 0x87F7E8
CONVERT, LOOKUP, QUEUE, EXTRA_CELL = (MEM + n for n in (0x65000, 0x66000, 0x67000, 0x68000))
TIMER = 0xB0EA80
FRAME = 0xA8ED84
BRIDGE_INITIALIZERS = (0x4D2F50, 0x4D2F80, 0x4D2FA0, 0x4D2FC0,
                       0x4D2FE0, 0x4D3000, 0x4D3020, 0x4D30C0)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def signed_words(u, pointer, count):
    return list(struct.unpack('<' + 'i' * count, u.mem_read(pointer, count * 4)))


def physical_palette():
    outer = (native.configured_gamemd().parent / 'ra2.mix').read_bytes()
    inner = stock.mix(outer)[stock.mix_hash('cache.mix')]
    cache = stock.mix(inner)
    m = palette.PaletteReader({})
    m.assets = {name.upper(): cache[stock.mix_hash(name)] for name in ('anim.pal', 'palette.pal')}
    palette.initialize(m)
    convert = m.read32(0x87F6C4)
    lookup = m.read32(convert + 0x174)
    row = bytes(m.u.mem_read(lookup, 512))
    evidence = dict(
        physical_inputs=dict(
            outer_archive=dict(path='ra2.mix', bytes=len(outer), sha256=sha(outer)),
            inner_archive=dict(path='ra2.mix/cache.mix', bytes=len(inner), sha256=sha(inner)),
            palettes={name: dict(bytes=len(raw), sha256=sha(raw), hex=raw.hex()) for name, raw in m.assets.items()},
            original_loads=m.asset_loaded),
        native_convert=dict(global_address='0x87F6C4', bytes_per_pixel=m.read32(convert + 4),
            shade_count=m.read32(convert + 0x16C), middle_row=(lookup-m.read32(convert + 0x170))//512,
            middle_row_sha256=sha(row), middle_row_hex=row.hex(), selected_index=3,
            selected_word=struct.unpack_from('<H', row, 6)[0]))
    return row, evidence


def baseline_cases():
    rows = []
    def add(name, target, source_cell=(20, 20), camera=(-80, 555), clip=(0, 0, 160, 120), start=100, frame=100, family='geometry'):
        rows.append(dict(name=name, family=family, source=[source_cell[0]*256+128, source_cell[1]*256+128, 0], target_cell=list(target), camera=list(camera), clip=list(clip), timer_start=start, frame=frame, level=0, slope=0, flags=0, alpha='clear'))
    for name, delta in (
        ('same_cell', (0,0)), ('right', (1,-1)), ('left', (-1,1)),
        ('down', (1,1)), ('up', (-1,-1)),
        ('octant_down_right_shallow', (1,0)), ('octant_down_right_steep', (2,1)),
        ('octant_down_left_steep', (1,2)), ('octant_down_left_shallow', (0,1)),
        ('octant_up_left_shallow', (-1,0)), ('octant_up_left_steep', (-2,-1)),
        ('octant_up_right_steep', (-1,-2)), ('octant_up_right_shallow', (0,-1)),
        ('clip_right', (2,-2)), ('clip_left', (-2,2)),
        ('clip_bottom', (4,4)), ('clip_top', (-4,-4))):
        add(name, (20+delta[0],20+delta[1]))
    for name, source, target in (
        ('cross_horizontal', (18,22), (22,18)), ('cross_horizontal_reverse', (22,18), (18,22)),
        ('cross_vertical', (16,16), (24,24)), ('cross_vertical_reverse', (24,24), (16,16)),
        ('cross_diagonal', (16,20), (24,20)), ('cross_diagonal_reverse', (24,20), (16,20)),
        ('reject_left', (18,22), (17,23)), ('reject_right', (22,18), (23,17)),
        ('reject_top', (16,16), (15,15)), ('reject_bottom', (24,24), (25,25))):
        add(name,target,source_cell=source)
    add('top_left_border',(21,19),camera=(0,615))
    add('bottom_right_border',(19,21),camera=(-159,496))
    add('interior_tactical_clip_horizontal',(22,18),source_cell=(18,22),clip=(20,15,100,80))
    add('interior_tactical_clip_diagonal',(24,20),source_cell=(16,20),clip=(20,15,100,80))
    add('original_probe_control',(14,20),source_cell=(10,20),camera=(-335,440),family='original_control')
    timer_pairs = [
        (100,99),(100,100),(100,124),(100,125),(100,126),
        (0,0),(0,24),(0,25),
        (0x7FFFFFF0,0x7FFFFFFF),(0x7FFFFFF0,0x80000008),(0x7FFFFFF0,0x80000009),
        (0xFFFFFFF0,0xFFFFFFFF),(0xFFFFFFF0,8),(0xFFFFFFF0,9),
        (0xFFFFFFFF,0xFFFFFFFF),(0xFFFFFFFF,0),(0xFFFFFFFF,24),(0xFFFFFFFF,100),(0xFFFFFFFF,0x80000000),
        (0,0x7FFFFFFF),(0,0x80000000),(0,0x80000018),(0,0x80000019),(0,0xFFFFFFFF),
        (100,0x80000064)]
    for start,frame in timer_pairs:
        add(f'timer_{start:08X}_{frame:08X}',(21,20),start=start,frame=frame,family='timer')
    return rows



def producer_cases():
    rows = baseline_cases()
    base = dict(source=[5248, 5248, 0], target_cell=[21, 20], camera=[-80, 555],
                clip=[0, 0, 160, 120], timer_start=100, frame=100,
                level=0, slope=0, flags=0, alpha='clear')
    for level in (0, 2, 6):
        for bridge in (False, True):
            z = level * 104
            rows.append(dict(base, name=f'height_{level}_bridge_{int(bridge)}', family='height',
                level=level, flags=0x100 if bridge else 0,
                source=[5248, 5248, z], camera=[-80, 555-z//2]))
    for slope in range(1, 17):
        for bridge in (False, True):
            rows.append(dict(base, name=f'slope_{slope}_bridge_{int(bridge)}', family='slope',
                level=2, slope=slope, flags=0x100 if bridge else 0,
                source=[5248, 5248, 208], camera=[-80, 451]))
    rows += [dict(base, name='source_location_subcell_height', family='source',
                  source=[5311, 5207, 111], camera=[-80, 505]),
             dict(base, name='source_on_bridge_raw_location', family='source',
                  source=[5248, 5248, 416], on_bridge=True, camera=[-80, 347]),
             dict(base, name='outside_map_skips_bridge', family='height', flags=0x100,
                  map_size=[0, 0]),
             dict(base, name='nav_null_no_queue', family='nav', nav_null=True),
             dict(base, name='nav_null_with_queue', family='nav', nav_null=True,
                  queue_cells=[[22, 20], [20, 21]]),
             dict(base, name='queue_one_overrides_nav', family='nav', queue_cells=[[20, 21]]),
             dict(base, name='queue_last_overrides_nav', family='nav', queue_cells=[[22, 20], [20, 21]]),
             dict(base, name='queue_last_bridge_slope', family='nav', level=2, slope=5,
                  flags=0x100, source=[5248, 5248, 208], camera=[-80, 451],
                  queue_cells=[[22, 20], [20, 21]])]
    return rows


class ActionFixture:
    """One adapter composes existing surface/map, palette and destination owners."""
    def __init__(self, case, palette_row):
        self.case = case
        self.surface = rally.Rally(case, size=case.get('surface_size', rally.SIZE))
        self.surface.setup_building()
        self.u = u = self.surface.u
        self.actor = rally.BUILDING
        u.mem_write(0x88731C, words(rally.SURFACE))
        u.mem_write(self.actor, words(0x7F5C70))
        u.mem_write(self.actor + 0x2B4, words(0))
        u.mem_write(self.actor + 0x5A4, words(0 if case.get('nav_null') else rally.TARGET))
        u.mem_write(self.actor + 0x598, words(0))
        u.mem_write(self.actor + 0x8C, bytes([case.get('on_bridge', False)]))
        u.mem_write(MAP + 0xF4, words(*case.get('map_size', [32, 32])))
        for index, cell in enumerate(case.get('queue_cells', [])):
            pointer = EXTRA_CELL + index * 0x200
            u.mem_write(pointer, bytes(u.mem_read(rally.TARGET, 0x148)))
            u.mem_write(pointer + 0x24, struct.pack('<hh', *cell))
            u.mem_write(rally.TABLE + (cell[1] * 512 + cell[0]) * 4, words(pointer))
            u.mem_write(QUEUE + index * 4, words(pointer))
        if case.get('queue_cells'):
            u.mem_write(self.actor + 0x58C, words(QUEUE))
            u.mem_write(self.actor + 0x598, words(len(case['queue_cells'])))
        for pc in BRIDGE_INITIALIZERS:
            call(u, pc)
        self.bridge_height = read32(u, 0x8B3DF4)
        assert self.bridge_height == 416 and read32(u, 0x8B3E00) == 104
        u.mem_write(0x87F6C4, words(CONVERT))
        u.mem_write(CONVERT + 4, words(2))
        u.mem_write(CONVERT + 0x174, words(LOOKUP))
        u.mem_write(LOOKUP, palette_row)
        self.palette_row = palette_row
        self.end_pixels = rally.PIXELS + self.surface.pixel_bytes
        u.mem_write(rally.PIXELS - 32, bytes([0xA5]) * 32)
        u.mem_write(self.end_pixels, bytes([0x5A]) * 32)
        self.trace = native.NativeCallTrace(u, lambda p: read32(u, p))
        self.rng = []
        self.hook = u.hook_add(UC_HOOK_CODE, self.observe)

    def returned(self, pc, sp):
        for index in self.trace.returned(pc, sp):
            row = self.trace.calls[index]
            if row['name'] == 'clip_line':
                row['points_after'] = [signed_words(self.u, p, 2) for p in row['point_pointers']]
                row['accepted_al'] = row['return_eax'] & 255
            elif row['name'] == 'intersect_rect':
                row['rectangle_after'] = signed_words(self.u, row['output_pointer'], 4)
            elif row['name'] == 'cell_get_coords':
                row['coordinate_after'] = signed_words(self.u, row['args'][0], 3)
            elif row['name'] in ('attack_pivot', 'attack_aim', 'object_coordinate',
                                 'target_coordinate', 'object_location'):
                row['coordinate_after'] = signed_words(self.u, row['args'][0], 3)
            elif row['name'] == 'facing_current':
                row['facing_after'] = read32(self.u, row['args'][0]) & 0xFFFF

    def observe(self, _u, pc, _size, _data):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        self.returned(pc, sp)
        if pc in (0x65C780, 0x65C7E0):
            self.rng.append(hex(pc))
        specs = {0x70D150: ('restart_timer', 0, 0), 0x4DC060: ('foot_action_lines', 2, 8),
                 0x7049C0: ('action_line', 9, 36), 0x7BB020: ('fill_endpoint', 2, 8),
                 0x7BA5E0: ('solid_wrapper', 3, 12), 0x7BA610: ('solid_line', 4, 16),
                 0x7BC2B0: ('clip_line', 1, 4), 0x421B60: ('intersect_rect', 3, 12),
                 0x486840: ('cell_get_coords', 1, 4), 0x568300: ('map_bounds', 1, 4),
                 0x565730: ('world_cell', 1, 4), 0x578080: ('ground_height', 1, 4),
                 0x741970: ('unit_destination', 2, 8), 0x4D94B0: ('foot_destination', 2, 8),
                 0x4AFD40: ('drive_move', 4, 16)}
        if self.case.get('attack'):
            specs.update({0x6F3D60: ('attack_pivot', 2, 8),
                0x70BCB0: ('attack_aim', 1, 4), 0x4AFF60: ('drive_draw_matrix', 3, 12),
                0x55A730: ('locomotor_draw_matrix', 3, 12),
                0x410540: ('target_coordinate', 1, 4),
                0x5F65A0: ('object_coordinate', 1, 4),
                0x41BE00: ('object_location', 1, 4),
                0x4AFB80: ('drive_is_moving', 1, 4),
                0x4DB1A0: ('live_foot_speed', 0, 0),
                0x50C050: ('house_speed', 1, 4), 0x70D0D0: ('ability', 1, 4),
                0x70E1A0: ('current_weapon', 0, 0), 0x70E140: ('get_weapon', 1, 4),
                0x7177C0: ('type_weapon', 1, 4), 0x773070: ('weapon_speed', 1, 4),
                0x4C93D0: ('facing_current', 1, 4)})
        if pc not in specs:
            return
        row = self.trace.calls[self.trace.entered(pc, sp, specs[pc])]
        args = row['args']
        if pc == 0x7049C0:
            row['source_coordinate'] = signed_words(u, sp + 4, 3)
            row['target_coordinate'] = signed_words(u, sp + 16, 3)
        elif pc == 0x7BB020:
            row['rectangle'] = signed_words(u, args[0], 4)
        elif pc == 0x7BA5E0:
            row['points'] = [signed_words(u, p, 2) for p in args[:2]]
        elif pc == 0x7BA610:
            row['clip_rectangle'] = signed_words(u, args[0], 4)
            row['points'] = [signed_words(u, p, 2) for p in args[1:3]]
        elif pc == 0x7BC2B0:
            ptrs = [u.reg_read(UC_X86_REG_ECX), u.reg_read(UC_X86_REG_EDX)]
            row['point_pointers'] = ptrs
            row['points_before'] = [signed_words(u, p, 2) for p in ptrs]
            row['clip_rectangle'] = signed_words(u, args[0], 4)
        elif pc == 0x421B60:
            row['output_pointer'] = u.reg_read(UC_X86_REG_ECX)
            row['clip_rectangle'] = signed_words(u, u.reg_read(UC_X86_REG_EDX), 4)
            row['rectangle_before'] = signed_words(u, args[0], 4)
        elif pc in (0x4AFF60, 0x4AFB80):
            row['com_this'] = args[0]

    def setup_attack(self):
        """Supply actor inputs using the existing live-speed and weapon owners.

        Drive/Facing constructors and setters execute. No target getter, speed,
        locomotor or weapon virtual is replaced. This bounded adapter does not
        claim whole Unit construction; the prerequisite mode composes that.
        """
        from tools.projectile_oracle import fireat_speed
        from tools.spatial_oracle import track_speed_native
        case, u = self.case, self.u
        target = MEM + 0x71000
        source_loco, target_loco, heading = MEM + 0x72000, MEM + 0x72100, MEM + 0x72200
        house, country, rules = MEM + 0x74000, MEM + 0x7B000, MEM + 0x7C000
        u.mem_map(native.SCRATCH, native.SCRATCH_SIZE)
        weapon = case['weapon_inputs']
        fireat_speed.weapon(u, True, weapon['rot'], int(weapon['floater']), weapon['speed'])
        track_speed_native.seed_getter_state(u, case['target_speed_inputs'],
                                            target, rally.TYPE, house, country, rules)
        if 'source_speed_inputs' in case:
            track_speed_native.seed_getter_state(u, case['source_speed_inputs'],
                                                self.actor, rally.TYPE, rally.HOUSE, country, rules)
        u.mem_write(rules + 0x16B8, words(weapon['gravity']))
        u.mem_write(rally.TYPE + 0x898, words(fireat_speed.WEAPON if weapon['present'] else 0))
        u.mem_write(rally.TYPE + 0x720, words(case['turret_offset']))
        u.mem_write(rally.TYPE + 0xCA1, b'\1')
        for actor, loco, coord, hull, turret in (
            (self.actor, source_loco, case['source'], case['body_facing'], case['turret_facing']),
            (target, target_loco, case['target_xyz'], case['target_facing'], 0),
        ):
            u.mem_write(actor, words(0x7F5C70))
            u.mem_write(actor + 0x14, words(4))
            u.mem_write(actor + 0x6C4, words(rally.TYPE))
            # Original Unit constructor's idle deploy sentinel. The existing
            # track_destination fixture supplies this same prerequisite; zero
            # would make Unit746C90 reject the original Drive Move_To.
            u.mem_write(actor + 0x6D8, words(case['unit_deploy_state']))
            u.mem_write(actor + 0x9C, words(*coord))
            self.invoke(0x4AF540, loco)
            u.mem_write(loco + 0xC, words(actor))
            u.mem_write(actor + 0x674, words(loco + 4))
            for field, angle in ((0x388, hull), (0x3A0, turret)):
                self.invoke(0x4C91E0, actor + field, (0,))
                u.mem_write(heading, words(angle))
                self.invoke(0x4C9300, actor + field, (heading,))
        if case.get('source_moving'):
            self.invoke(0x4AFD40, 0, (source_loco + 4, *case['source']))
        if case['target_moving']:
            self.invoke(0x4AFD40, 0, (target_loco + 4, *case['target_xyz']))
        self.attack_target_motion = bool(self.invoke(0x4AFB80, 0, (target_loco + 4,)) & 255)
        assert self.attack_target_motion == case['target_moving']
        u.mem_write(self.actor + 0x2B4, words(target if case['target_present'] else 0))
        self.attack_state_ranges = dict(source=(self.actor, 0x700), target=(target, 0x700),
            source_locomotor=(source_loco, 0x70), target_locomotor=(target_loco, 0x70),
            timer=(TIMER, 12))
        if 'source_speed_inputs' in case:
            self.captured_getter_checks = dict(
                source_moving=bool(self.invoke(0x4AFB80, 0, (source_loco + 4,)) & 255),
                target_moving=self.attack_target_motion,
                source_current_speed=self.invoke(0x4DB1A0, self.actor),
                target_current_speed=self.invoke(0x4DB1A0, target))
            assert self.captured_getter_checks == case['captured_getter_expectations']

    def invoke(self, pc, this=0, args=()):
        answer = call(self.u, pc, this, args)
        self.returned(native.RET_MAGIC, self.u.reg_read(UC_X86_REG_ESP))
        assert not self.trace.pending, self.trace.pending
        return answer

    def start(self, frame):
        self.u.mem_write(FRAME, words(frame))
        self.invoke(0x70D150)
        timer = list(struct.unpack('<3I', self.u.mem_read(TIMER, 12)))
        assert timer[0] == frame and timer[2] == 25
        return timer

    def read_only_draw(self, draw):
        """One guard for direct Foot and production Tactical Attack drawing."""
        before = {name: bytes(self.u.mem_read(pointer, size))
                  for name, (pointer, size) in self.attack_state_ranges.items()}
        forbidden = []
        def observe(_u, pc, _size, _data):
            if pc in (0x65C640, 0x65C660, 0x65C780, 0x65C7E0,
                      0x70D150, 0x70D4A0, 0x7258D0, 0x5F65F0):
                forbidden.append(f'0x{pc:08X}')
        hook = self.u.hook_add(UC_HOOK_CODE, observe)
        try:
            draw()
        finally:
            self.u.hook_del(hook)
        unchanged = {name: raw == bytes(self.u.mem_read(*self.attack_state_ranges[name]))
                     for name, raw in before.items()}
        assert all(unchanged.values()), unchanged
        assert not forbidden, forbidden
        return dict(state_unchanged=unchanged, rng_timer_restart_detach_entries=forbidden)

    def result(self):
        u = self.u
        assert not self.trace.pending, self.trace.pending
        assert not self.rng, self.rng
        assert bytes(u.mem_read(rally.PIXELS - 32, 32)) == bytes([0xA5]) * 32
        assert bytes(u.mem_read(self.end_pixels, 32)) == bytes([0x5A]) * 32
        assert bytes(u.mem_read(LOOKUP, 512)) == self.palette_row
        pixels = self.surface.pixels()
        result = dict(input=self.case, calls=self.trace.calls, rng_entries=self.rng,
            surface_guards_unchanged=True, palette_row_unchanged=True,
            bridge_height=self.bridge_height, pixel_count=len(pixels),
            words=sorted({p[2] for p in pixels}), pixels=pixels,
            full_surface_sha256=sha(bytes(u.mem_read(rally.PIXELS, self.surface.pixel_bytes))))
        u.hook_del(self.hook)
        return result


def producer(case, row):
    f = ActionFixture(case, row)
    timer = f.start(case['timer_start'])
    f.u.mem_write(FRAME, words(case['frame']))
    f.invoke(0x4DC060, f.actor, (0, 0))
    assert timer == list(struct.unpack('<3I', f.u.mem_read(TIMER, 12)))
    return dict(f.result(), native_timer=timer)


def attack_cases():
    """Finite prepared Unit/Drive inputs, separate from retail command history."""
    speed = dict(speed_percent=7, raw=17, house=1.0,
        veteran=float(struct.unpack('<f', struct.pack('<f', 1.2))[0]),
        rank=0.0, crate=1.0, applied=1.0, faster=True,
        elite_faster=False, flag_owner=-1)
    base = dict(attack=True, family='attack', source=[5248, 5248, 416], target_xyz=[5504, 5248, 416],
        target_cell=[19, 20], camera=[-80, 495], clip=[0, 0, 160, 120],
        timer_start=100, frame=100, level=4, slope=0, flags=0, alpha='clear',
        target_present=True, target_moving=False, target_facing=0,
        body_facing=0, turret_facing=0x4000, turret_offset=0, unit_deploy_state=-1,
        target_speed_inputs=speed, current_weapon_index=0,
        weapon_inputs=dict(speed=95, speed_percent=40, range_cells=5,
                           rot=0, floater=False, gravity=6, present=True))
    rows = []
    for index, (dx, dy) in enumerate(((256, 0), (256, 256), (0, 256), (-256, 256),
                                    (-256, 0), (-256, -256), (0, -256), (256, -256))):
        for moving in (False, True):
            rows.append(dict(base, name=f'attack_direction_{index}_moving_{int(moving)}',
                target_xyz=[5248 + dx, 5248 + dy, 416], target_moving=moving,
                target_facing=index * 8192))
    for name, extra in (
        ('same_coordinate', dict(target_xyz=base['source'])),
        ('turret_other', dict(turret_facing=0xC000)),
        ('hull_other', dict(body_facing=0x4000)),
        ('pivot_80', dict(turret_offset=80)),
        ('pivot_80_hull_other', dict(turret_offset=80, body_facing=0x4000)),
        ('pivot_80_turret_other', dict(turret_offset=80, turret_facing=0xC000)),
        ('queue_priority', dict(queue_cells=[[22, 20], [20, 21]])),
        ('no_nav', dict(nav_null=True)),
        ('timer_last', dict(frame=124)), ('timer_expired', dict(frame=125)),
        ('tarcom_null_nav', dict(target_present=False)),
        ('tarcom_null_no_nav', dict(target_present=False, nav_null=True)),
        ('moving_fraction_half', dict(target_moving=True,
            target_speed_inputs=dict(speed, applied=0.5))),
        ('moving_veteran', dict(target_moving=True,
            target_speed_inputs=dict(speed, rank=1.0))),
        ('moving_crate', dict(target_moving=True,
            target_speed_inputs=dict(speed, crate=1.25))),
    ):
        rows.append(dict(base, name='attack_' + name, **extra))
    return rows


def attack(case, row):
    f = ActionFixture(case, row)
    f.setup_attack()
    timer = f.start(case['timer_start'])
    f.u.mem_write(FRAME, words(case['frame']))
    guard = f.read_only_draw(lambda: f.invoke(0x4DC060, f.actor, (0, 0)))
    return dict(f.result(), native_timer=timer, target_motion_admitted=f.attack_target_motion,
                **guard)


def batch_cases():
    cases = []
    for count, multiple_targets, reverse in ((32, False, False), (33, False, False),
            (64, False, False), (64, True, False), (64, True, True)):
        targets = [[21, 20], [19, 20], [20, 21], [20, 19]]
        actors = [dict(source=[4864 + (i % 8) * 88, 4864 + (i // 8) * 88, 0],
            target_cell=targets[i % 4 if multiple_targets else 0], selected=True,
            level=0, slope=0, flags=0) for i in range(count)]
        order = list(range(count))
        if reverse:
            order.reverse()
        cases.append(dict(name=f'batch_{count:03}_' +
            ('crossed' if multiple_targets else 'shared') + ('_reverse' if reverse else ''),
            frame=100, timer_start=100, camera=[-80, 555], clip=[0, 0, 160, 120],
            actors=actors, techno_order=order))
    return cases


def batch(case, row):
    """Execute the real local Techno-array loop, including every Foot call."""
    f = ActionFixture(case, row)
    if case.get('attack'):
        # Production Attack reuses the established actor/Drive pair itself.
        # Copying that actor would retain a Drive back-pointer to the old one.
        assert 'actors' not in case
        f.setup_attack()
    timer = f.start(case['timer_start'])
    u = f.u
    u.mem_write(FRAME, words(case['frame']))
    actor_template = bytes(u.mem_read(f.actor, 0x700))
    cell_template = bytes(u.mem_read(rally.TARGET, 0x148))
    actors = case.get('actors', [case])
    order = case.get('techno_order', list(range(len(actors))))
    actor_pointers, cell_pointers = [], {}
    for index, actor in enumerate(actors):
        pointer = f.actor if case.get('attack') else MEM + 0x70000 + index * 0x800
        actor_pointers.append(pointer)
        u.mem_write(pointer, actor_template)
        u.mem_write(pointer + 0x9C, words(*actor['source']))
        u.mem_write(pointer + 0x6C, words(actor.get('health', 100)))
        u.mem_write(pointer + 0x83, bytes([actor['selected']]))
        cell_key = tuple(actor['target_cell'])
        state = (actor['level'], actor['slope'], actor['flags'])
        if cell_key in cell_pointers:
            cell_pointer, previous = cell_pointers[cell_key]
            assert previous == state, 'shared physical Cell cannot have two terrain states'
        else:
            cell_pointer = MEM + 0xA0000 + len(cell_pointers) * 0x200
            cell_pointers[cell_key] = (cell_pointer, state)
            u.mem_write(cell_pointer, cell_template)
            u.mem_write(cell_pointer + 0x24, struct.pack('<hh', *cell_key))
            u.mem_write(cell_pointer + 0x11B, bytes(state[:2]))
            u.mem_write(cell_pointer + 0x140, words(state[2]))
            u.mem_write(rally.TABLE + (cell_key[1] * 512 + cell_key[0]) * 4,
                        words(cell_pointer))
        u.mem_write(pointer + 0x5A4, words(0 if actor.get('nav_null') else cell_pointer))
        # Ordinary MTNK has neither capture-manager nor temporal consumers.
        assert all(read32(u, pointer + offset) == 0 for offset in (0x294, 0x2BC, 0x2C0))
    array = MEM + 0xB0000
    u.mem_write(array, words(*(actor_pointers[i] for i in order)))
    u.mem_write(0xA8EC7C, words(array))
    u.mem_write(0xA8EC88, words(len(actor_pointers)))
    u.mem_write(0xA8B238, words(1))
    u.mem_write(0xA83D4C, words(rally.HOUSE))
    u.mem_write(0x843108, b'\x01')
    u.mem_write(0xAC4CF4, b'\x00')
    u.mem_write(SP, bytes(0x110))
    u.reg_write(UC_X86_REG_ESP, SP)
    def draw():
        native.run_checked(u, 0x6D46DD, 0x6D4912, count=5000000,
            required_addresses=[0x637AA0, 0x50B6F0])
    extra = {}
    if case.get('attack'):
        extra = dict(f.read_only_draw(draw), target_motion_admitted=f.attack_target_motion,
                     captured_getter_checks=f.captured_getter_checks)
    else:
        draw()
    assert u.reg_read(UC_X86_REG_ESP) == SP
    f.returned(0x6D4912, SP)
    pointer_to_index = {p: i for i, p in enumerate(actor_pointers)}
    call_order = [pointer_to_index[int(c['ecx'], 16)] for c in f.trace.calls
                  if c['name'] == 'foot_action_lines']
    assert call_order == [i for i in order if actors[i]['selected']]
    action_calls = [dict(actor_index=pointer_to_index[int(c['ecx'], 16)],
        source_coordinate=c['source_coordinate'], target_coordinate=c['target_coordinate'],
        args=c['args']) for c in f.trace.calls if c['name'] == 'action_line']
    if 'actors' in case:
        assert len(action_calls) == len(call_order)
    assert timer == list(struct.unpack('<3I', u.mem_read(TIMER, 12)))
    return dict(f.result(), native_timer=timer, actor_call_order=call_order,
        action_calls=action_calls, actor_pointers=[f'0x{p:08X}' for p in actor_pointers],
        executed_slice=['0x006D46DD', '0x006D4912'], stack_delta=0, **extra)


def compositing_cases():
    directions = [(192, 0), (192, 96), (0, 192), (-96, 192),
                  (-192, 0), (-192, -96), (0, -192)]
    cases = []
    for count in (2, 33, 64):
        primitives = []
        for i in range(count):
            dx, dy = directions[i % len(directions)]
            primitives.append(dict(source=[5248 - dx, 5248 - dy, 0],
                target=[5248 + dx, 5248 + dy, 0], color_index=3 if i % 2 == 0 else 8))
        for reverse in (False, True):
            order = list(range(count))
            if reverse:
                order.reverse()
            cases.append(dict(name=f'compositing_{count:03}' + ('_reverse' if reverse else ''),
                camera=[-80, 555], clip=[0, 0, 160, 120], primitives=primitives,
                primitive_order=order, action_flags=[0, 0]))
    return cases


def compositing(case, row, colors):
    """Ordered original7049C0 calls; supplied XYZ, no Attack producer claim."""
    f = ActionFixture(case, row)
    for index in case['primitive_order']:
        primitive = case['primitives'][index]
        rgb = bytes(colors[str(primitive['color_index'])]['rgb'])
        rgb_argument = int.from_bytes(rgb + b'\0', 'little')
        f.invoke(0x7049C0, f.actor,
            (*primitive['source'], *primitive['target'], rgb_argument, *case['action_flags']))
    action_calls = [dict(primitive_index=index, source_coordinate=c['source_coordinate'],
        target_coordinate=c['target_coordinate'], args=c['args'])
        for index, c in zip(case['primitive_order'],
            (c for c in f.trace.calls if c['name'] == 'action_line'), strict=True)]
    return dict(f.result(), primitive_call_order=case['primitive_order'], action_calls=action_calls)


def production_inputs():
    """Prepared state and identities copied from the twelve valid release receipts.

    These literals keep native regeneration independent of ignored capture paths;
    the production archive/comparator independently binds them to each receipt.
    """
    records = [{'run': 'unit-move-active-v1',
      'frame': 6,
      'timer_start': 5,
      'source': [7296, 23680, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': False,
      'crop_origin': [176, 336],
      'capture_sha256': 'eeea2c5c791ebe5ba7042b149ffb66518c40a8f75cdf068ada43bfc0228688bc',
      'profile_sha256': '40867e4e969354b30a2c9236f80b948947f0defda39d87fda46394a3c8a9e5f1',
      'frame_sha256': '455b8456235da85a13ff00ccfee4201a5479fcb64114f0a2e3fb00a4149c24b6'},
     {'run': 'unit-selected-only-v1',
      'frame': 6,
      'timer_start': 4,
      'source': [7296, 23680, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': True,
      'crop_origin': [176, 336],
      'capture_sha256': '4e23ad1e73664a15cf376660d7c792d90e95576e9a53bd514959b396f15844c0',
      'profile_sha256': '48da7a40346d761afe1cde0a828f6d00c02e9729118f13ad0f8952ab1ab00567',
      'frame_sha256': '02a759640488fd6bc51185b5d368b78dedc26913a040e35b31e89fb8b851a5f0'},
     {'run': 'unit-move-before-stop-v1',
      'frame': 7,
      'timer_start': 5,
      'source': [7296, 23680, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': False,
      'crop_origin': [176, 336],
      'capture_sha256': '48707e591e9468e1999c6387f49857bf951ecdc427d7c7cc55477d73fa1a2226',
      'profile_sha256': '25d6bd225c3ab37e380f2d6b0e2f6417eebab31796fe3ffa4b561bc96488fe2e',
      'frame_sha256': '5746e7a050b5304124dc29f6c02f31b1c71fa03510ccffd8c7c2a8c0e3aa7a1a'},
     {'run': 'unit-move-stopped-v1',
      'frame': 7,
      'timer_start': 5,
      'source': [7296, 23680, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': True,
      'crop_origin': [176, 336],
      'capture_sha256': '4c90dcdfad51b53fb496f22ce882435011bb3eb8fee4d3ae168c2799dd291bc9',
      'profile_sha256': 'c6f19895938cdc6e7358b2cfd5e1f59fed97f99696293b1048b846ad1a049ff7',
      'frame_sha256': 'd80670eaf9b1754f5c73816c3d626082ebc699da5c8f4f98c291355248aa1977'},
     {'run': 'unit-move-last-active-v1',
      'frame': 29,
      'timer_start': 5,
      'source': [7617, 23926, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': False,
      'crop_origin': [176, 336],
      'capture_sha256': '9781d6094b079673915cdc564e23755211d95835a931918cddc67e0aa19f45f6',
      'profile_sha256': 'be74112bd0f7cbb5fa4a7239282a6c076fef727e942dad7755cddd85dc79db3f',
      'frame_sha256': 'd015dd29b61bb500f83811712cf6162d0b642a7e6e51e77667c255c7f2c762ee'},
     {'run': 'unit-move-expired-v1',
      'frame': 30,
      'timer_start': 5,
      'source': [7642, 23931, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': False,
      'crop_origin': [176, 336],
      'capture_sha256': 'ef0e2b87a16dd055fcc755024220c0e9abfa47362edf5b880eed762cec25b14a',
      'profile_sha256': 'd139967b26f806cf9499b946a227b82e94d28a67c2f7b9e778b20770d579071b',
      'frame_sha256': '91ef1e5759ed2dffc55d0dfcab83a3da55654ec4b67557398b187de8ae77e54c'},
     {'run': 'unit-move-expired-next-v1',
      'frame': 31,
      'timer_start': 5,
      'source': [7668, 23935, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': False,
      'crop_origin': [176, 336],
      'capture_sha256': '14afdd1c63a4adba9ad190084a1c02228546a965c9b2ae45995aa700b14284fc',
      'profile_sha256': '31fbc11c173fc5f13dbce593c63df2e80aac16a04a106b8622ec1e1393eeada2',
      'frame_sha256': '2772b4f632589bf81fef500170156faa0cec9765f264d48e5b1e4f36b4cf696d'},
     {'run': 'unit-move-reselect-v1',
      'frame': 31,
      'timer_start': 30,
      'source': [7668, 23935, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': False,
      'crop_origin': [176, 336],
      'capture_sha256': '3e74ca085d6b6d938c711c39d3b74fd2ff9b5f2a04441a3b7676d15838d7f96e',
      'profile_sha256': '8626b70f5a9d908d75e6586ded5e8aea047618c7a0e5aeb8bff566a8f0488141',
      'frame_sha256': '2af20da1da22fa9a9dce3c321e46180430ff75e64502e37fec692420c135bcfb'},
     {'run': 'empty-band-unselected-v1',
      'frame': 5,
      'timer_start': 4,
      'source': [7296, 23680, 416],
      'target_cell': [35, 94],
      'selected': False,
      'nav_null': True,
      'crop_origin': [176, 336],
      'capture_sha256': 'bbd841d1fca7828eb5b1892602c4d0513cf026ac9a146867f0ca4c69406c6678',
      'profile_sha256': '3ac2aeac211bb79a06b70b70d3e54840c864a3956a3cf4a55e924d6beb4b3b65',
      'frame_sha256': '85c471ebcd7c0405a0681f28d01de812577f6719096318cd6593fc08bcc95e03'},
     {'run': 'unit-move-empty-band-restart-v1',
      'frame': 31,
      'timer_start': 30,
      'source': [7668, 23935, 416],
      'target_cell': [29, 96],
      'selected': True,
      'nav_null': False,
      'crop_origin': [80, 336],
      'capture_sha256': 'f458a8c4f34d08e4c4ba3b89be8314ab524acfd0d5b98a9c20e261df914d3e71',
      'profile_sha256': '80db2aca17fbaab27e99ed807349b18863830e8489bc2b70d3fd72b1f68587cc',
      'frame_sha256': '1dd6906e848ff04eb94b2e0dba5995187302090645471bd22bdb03a6a5b94a8c'},
     {'run': 'unit-move-arrived-control-v1',
      'frame': 95,
      'timer_start': 5,
      'source': [9088, 24192, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': True,
      'crop_origin': [176, 336],
      'capture_sha256': '9bbcc82e4146285cb234f61225b315552b9afb32206a98a60cc4b31deb676028',
      'profile_sha256': '0175a0d69739b46662405f2165f1c76674e3beb2e79f7f71f1242269f3d6e0a4',
      'frame_sha256': '7e3978dd5a2080a5e5a0649b7bf83dcdd2fe7c1c317acd91c6c1987011a92913'},
     {'run': 'unit-move-arrived-reselect-v1',
      'frame': 95,
      'timer_start': 94,
      'source': [9088, 24192, 416],
      'target_cell': [35, 94],
      'selected': True,
      'nav_null': True,
      'crop_origin': [176, 336],
      'capture_sha256': '1262353de7d469ebe07474271821d61582d94756e071e66bb771b0915e7e172f',
      'profile_sha256': '83572ac0b01abd5c75aa521600859949e0e9180d41fe2f86fc25bbee07e8c9da',
      'frame_sha256': '1ba8eb4e6e9dca6ee61b1467e3a34be0e399619a91af2d5b1e928ac813c30eb0'}]
    result = []
    for record in records:
        origin = record["crop_origin"]
        case = dict(name=record["run"], surface_size=[160, 160], clip=[0, 0, 160, 160],
            camera=[-2100 + origin[0], 1426 + origin[1] - 15], map_size=[80, 85],
            frame=record["frame"], timer_start=record["timer_start"],
            source=record["source"], target_cell=record["target_cell"],
            selected=record["selected"], nav_null=record["nav_null"],
            health=300, on_bridge=False, level=4, slope=0, flags=0, queue_cells=[], alpha="clear")
        production = {key: record[key] for key in ("run", "crop_origin", "capture_sha256",
            "profile_sha256", "frame_sha256")}
        production.update(actor_id=1374, executable_sha256=
            "dc06728fb081e18054a28e9e9aa61e5ca6e7e3e82a20acc9052ac2471e6653dd")
        result.append(dict(input=case, production=production))
    return result


def attack_production_inputs():
    """Four immutable captured states; regeneration never reads ignored receipts."""
    records = [{'run': 'stationary-attack-inputs-v1',
      'frame': 1286,
      'timer_start': 1282,
      'source': [7349, 23680, 416],
      'target_xyz': [8832, 23680, 416],
      'source_cell': [28, 92],
      'target_actor_cell': [34, 92],
      'nav_cell': [34, 92],
      'selected': True,
      'source_action_line_inputs': {'applied_speed_fraction_fixed_bits': 65536,
                                    'body_facing': 16384,
                                    'crate_speed_multiplier_f64_bits': 4607182418800017408,
                                    'current_speed': 17,
                                    'current_weapon': '105mm',
                                    'house_speed_bonus_f32_bits': 1065353216,
                                    'is_moving': True,
                                    'locomotor': 'Drive',
                                    'rocking_angles_fixed_bits': None,
                                    'turret_facing': 16383,
                                    'turret_offset': 0,
                                    'veterancy': 0},
      'target_action_line_inputs': {'applied_speed_fraction_fixed_bits': 0,
                                    'body_facing': 49152,
                                    'crate_speed_multiplier_f64_bits': 4607182418800017408,
                                    'current_speed': 0,
                                    'current_weapon': '105mm',
                                    'house_speed_bonus_f32_bits': 1065353216,
                                    'is_moving': False,
                                    'locomotor': 'Drive',
                                    'rocking_angles_fixed_bits': None,
                                    'turret_facing': 49152,
                                    'turret_offset': 0,
                                    'veterancy': 0},
      'terrain_inputs': [{'cell': [28, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0},
                         {'cell': [34, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0},
                         {'cell': [32, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0}],
      'capture_sha256': '83aa60796fe19650da9e42fe91027b80b36aed50b904b1f7f5b390691c9cec17',
      'profile_sha256': 'b585b2bc4de66d665b8d7581c1f669cef19c99f1fcfbc29d344a2c62ed4209a7',
      'frame_sha256': 'd80af5698ada3413aeb0b190487ed4dc8d0d79ca6cf9bb00a05f94d53aa047ea'},
     {'run': 'stationary-enemy-band-control-inputs-v1',
      'frame': 1286,
      'timer_start': 1285,
      'source': [7349, 23680, 416],
      'target_xyz': [8832, 23680, 416],
      'source_cell': [28, 92],
      'target_actor_cell': [34, 92],
      'nav_cell': [34, 92],
      'selected': False,
      'source_action_line_inputs': {'applied_speed_fraction_fixed_bits': 65536,
                                    'body_facing': 16384,
                                    'crate_speed_multiplier_f64_bits': 4607182418800017408,
                                    'current_speed': 17,
                                    'current_weapon': '105mm',
                                    'house_speed_bonus_f32_bits': 1065353216,
                                    'is_moving': True,
                                    'locomotor': 'Drive',
                                    'rocking_angles_fixed_bits': None,
                                    'turret_facing': 16383,
                                    'turret_offset': 0,
                                    'veterancy': 0},
      'target_action_line_inputs': {'applied_speed_fraction_fixed_bits': 0,
                                    'body_facing': 49152,
                                    'crate_speed_multiplier_f64_bits': 4607182418800017408,
                                    'current_speed': 0,
                                    'current_weapon': '105mm',
                                    'house_speed_bonus_f32_bits': 1065353216,
                                    'is_moving': False,
                                    'locomotor': 'Drive',
                                    'rocking_angles_fixed_bits': None,
                                    'turret_facing': 49152,
                                    'turret_offset': 0,
                                    'veterancy': 0},
      'terrain_inputs': [{'cell': [28, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0},
                         {'cell': [34, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0},
                         {'cell': [32, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0}],
      'capture_sha256': 'f3f576b47e3fc0b0e5e8352561ec5e3f1ef671c734d9f30726dd3bd7bb644813',
      'profile_sha256': 'e49b4bb1da3af5121543abe74f549cf48d4317d7385f7393fa0e3cb80f5557d7',
      'frame_sha256': '1255163e82d3b292fd004f6ee4442a6ceb0b285739576b5d3e04d4e57e7c2c7a'},
     {'run': 'moving-attack-inputs-v1',
      'frame': 1298,
      'timer_start': 1282,
      'source': [7500, 23680, 416],
      'target_xyz': [8451, 23680, 416],
      'source_cell': [29, 92],
      'target_actor_cell': [33, 92],
      'nav_cell': None,
      'selected': True,
      'source_action_line_inputs': {'applied_speed_fraction_fixed_bits': 19661,
                                    'body_facing': 16384,
                                    'crate_speed_multiplier_f64_bits': 4607182418800017408,
                                    'current_speed': 5,
                                    'current_weapon': '105mm',
                                    'house_speed_bonus_f32_bits': 1065353216,
                                    'is_moving': True,
                                    'locomotor': 'Drive',
                                    'rocking_angles_fixed_bits': None,
                                    'turret_facing': 16383,
                                    'turret_offset': 0,
                                    'veterancy': 0},
      'target_action_line_inputs': {'applied_speed_fraction_fixed_bits': 65536,
                                    'body_facing': 49152,
                                    'crate_speed_multiplier_f64_bits': 4607182418800017408,
                                    'current_speed': 17,
                                    'current_weapon': '105mm',
                                    'house_speed_bonus_f32_bits': 1065353216,
                                    'is_moving': True,
                                    'locomotor': 'Drive',
                                    'rocking_angles_fixed_bits': None,
                                    'turret_facing': 49153,
                                    'turret_offset': 0,
                                    'veterancy': 0},
      'terrain_inputs': [{'cell': [28, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0},
                         {'cell': [34, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0},
                         {'cell': [32, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0}],
      'capture_sha256': 'f313660a560a2e3ee588f96b91c1d179f37b5654ac72a6bad9daa9b0f1229bb4',
      'profile_sha256': '3d04cff8ad569b21761aa736b36ebfbc615bf79b120a0a68365c5d98e3b200ff',
      'frame_sha256': '483db1a8c34f9dfb8560319fdf092f39cd07aae9ca6dde4160603f53a268293f'},
     {'run': 'moving-enemy-band-control-inputs-v1',
      'frame': 1298,
      'timer_start': 1297,
      'source': [7500, 23680, 416],
      'target_xyz': [8451, 23680, 416],
      'source_cell': [29, 92],
      'target_actor_cell': [33, 92],
      'nav_cell': None,
      'selected': False,
      'source_action_line_inputs': {'applied_speed_fraction_fixed_bits': 19661,
                                    'body_facing': 16384,
                                    'crate_speed_multiplier_f64_bits': 4607182418800017408,
                                    'current_speed': 5,
                                    'current_weapon': '105mm',
                                    'house_speed_bonus_f32_bits': 1065353216,
                                    'is_moving': True,
                                    'locomotor': 'Drive',
                                    'rocking_angles_fixed_bits': None,
                                    'turret_facing': 16383,
                                    'turret_offset': 0,
                                    'veterancy': 0},
      'target_action_line_inputs': {'applied_speed_fraction_fixed_bits': 65536,
                                    'body_facing': 49152,
                                    'crate_speed_multiplier_f64_bits': 4607182418800017408,
                                    'current_speed': 17,
                                    'current_weapon': '105mm',
                                    'house_speed_bonus_f32_bits': 1065353216,
                                    'is_moving': True,
                                    'locomotor': 'Drive',
                                    'rocking_angles_fixed_bits': None,
                                    'turret_facing': 49153,
                                    'turret_offset': 0,
                                    'veterancy': 0},
      'terrain_inputs': [{'cell': [28, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0},
                         {'cell': [34, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0},
                         {'cell': [32, 92],
                          'allocated': True,
                          'level': 4,
                          'slope': 0,
                          'raw_bridge_flags': 0}],
      'capture_sha256': 'b600f0f044abaa9861700b1660a3400320bb885943a97a30dd5e89a58abe68e6',
      'profile_sha256': '42b84286db242211625f342a7ba015f1a1b1f5396cbfd47f14ec7b280cbc6249',
      'frame_sha256': '40254ac9b2438abadd0e38e76ac40e94bb53b1d8e4842d4c02bb10bd511197b5'}]
    result = []
    for record in records:
        src = record['source_action_line_inputs']
        dst = record['target_action_line_inputs']
        speed = {}
        for label, observed in (('source', src), ('target', dst)):
            assert observed['veterancy'] == 0 and observed['current_weapon'] == '105mm'
            assert observed['turret_offset'] == 0 and observed['rocking_angles_fixed_bits'] is None
            assert observed['locomotor'] == 'Drive'
            speed[label] = dict(speed_percent=7, raw=17,
                house=struct.unpack('<f', struct.pack('<I', observed['house_speed_bonus_f32_bits']))[0],
                crate=struct.unpack('<d', struct.pack('<Q', observed['crate_speed_multiplier_f64_bits']))[0],
                applied=observed['applied_speed_fraction_fixed_bits'] / 65536,
                veteran=float(struct.unpack('<f', struct.pack('<f', 1.2))[0]),
                rank=0.0, faster=True, elite_faster=False, flag_owner=-1)
        origin = [176, 336]
        case = dict(name=record['run'], attack=True, family='attack',
            surface_size=[192, 160], clip=[0, 0, 192, 160],
            camera=[-2100 + origin[0], 1426 + origin[1] - 15], map_size=[80, 85],
            source=record['source'], target_xyz=record['target_xyz'],
            target_cell=record['nav_cell'] or [34, 92], nav_null=record['nav_cell'] is None,
            selected=record['selected'], timer_start=record['timer_start'], frame=record['frame'],
            health=300, on_bridge=False, level=4, slope=0, flags=0, queue_cells=[], alpha='clear',
            target_present=True, source_moving=src['is_moving'], target_moving=dst['is_moving'],
            body_facing=src['body_facing'], turret_facing=src['turret_facing'],
            target_facing=dst['body_facing'], turret_offset=0, unit_deploy_state=-1,
            source_speed_inputs=speed['source'], target_speed_inputs=speed['target'],
            current_weapon_index=0,
            weapon_inputs=dict(speed=95, speed_percent=40, range_cells=5,
                               rot=0, floater=False, gravity=6, present=True),
            captured_getter_expectations=dict(source_moving=src['is_moving'], target_moving=dst['is_moving'],
                source_current_speed=src['current_speed'], target_current_speed=dst['current_speed']))
        production = {key: record[key] for key in ('run', 'capture_sha256', 'profile_sha256',
            'frame_sha256', 'source_cell', 'target_actor_cell', 'source_action_line_inputs',
            'target_action_line_inputs', 'terrain_inputs')}
        production.update(actor_id=1374, target_actor_id=1386, crop_origin=origin,
            executable_sha256='4b4a83ceed0f074bab7dd0461c2bfef33f6e26a55c5842e8f43601eaad06e18c')
        result.append(dict(input=case, production=production))
    return result


def solid_cases():
    cases = []
    for dx, dy in ((0, 0), (48, 0), (0, 48), (48, 48), (48, 17), (17, 48),
                   (32, 16), (16, 32), (7, 3), (3, 7)):
        for sx, sy in ((1, 1), (-1, 1), (-1, -1), (1, -1)):
            for reverse in (False, True):
                a, b = [80, 60], [80 + dx * sx, 60 + dy * sy]
                if reverse:
                    a, b = b, a
                cases.append(dict(from_point=a, to_point=b, clip=[0, 0, 160, 120]))
    cases += [dict(from_point=a, to_point=b, clip=clip)
        for a, b in (([-40, 60], [200, 60]), ([80, -40], [80, 160]),
                     ([-40, 0], [200, 120]), ([200, 120], [-40, 0]),
                     ([-20, 10], [-1, 80]), ([20, 140], [80, 121]))
        for clip in ([0, 0, 160, 120], [20, 15, 100, 80])]
    # Identical directions arising from the axis sign combinations need one row.
    return [dict(name=f'solid_{i:03}', **c) for i, c in enumerate(
        {repr(c): c for c in cases}.values())]


def solid(case, row):
    f = ActionFixture(case, row)
    a, b, clip = MEM + 0x31000, MEM + 0x31010, MEM + 0x31020
    f.u.mem_write(a, words(*case['from_point']))
    f.u.mem_write(b, words(*case['to_point']))
    f.u.mem_write(clip, words(*case['clip']))
    color = struct.unpack_from('<H', row, 6)[0]
    result = f.invoke(0x7BA610, rally.SURFACE, (clip, a, b, color)) & 255
    return dict(f.result(), returned_al=result)


def clip_rect_cases():
    source = [[x-2, y-2, 3, 3] for x, y in ((0, 0), (1, 1), (2, 2), (159, 119),
        (160, 120), (161, 121), (-1, 20), (20, -1), (80, 60), (20, 15), (120, 95))]
    source += [[-20, -20, 200, 160], [3, 4, 0, 3], [3, 4, 3, 0], [3, 4, -1, 3]]
    return [dict(name=f'rect_{i:03}', rectangle=r, clip=c, offset=[7, 11])
        for i, (r, c) in enumerate(itertools.product(source,
            ([0, 0, 160, 120], [20, 15, 100, 80], [0, 0, 0, 120])))]


def intersect(case, row):
    f = ActionFixture(case, row)
    out, clip, source, x, y = [MEM + 0x31000 + i * 0x20 for i in range(5)]
    f.u.mem_write(clip, words(*case['clip']))
    f.u.mem_write(source, words(*case['rectangle']))
    f.u.mem_write(x, words(case['offset'][0]))
    f.u.mem_write(y, words(case['offset'][1]))
    f.u.reg_write(UC_X86_REG_EDX, clip)
    answer = f.invoke(0x421B60, out, (source, x, y))
    assert answer == out
    result = dict(input=case, rectangle=signed_words(f.u, out, 4),
        offset=[*signed_words(f.u, x, 1), *signed_words(f.u, y, 1)], calls=f.trace.calls)
    assert not f.result()['pixels']
    return result


def timer_prerequisites(row):
    result = []
    for frame in (0, 100, 0xFFFFFFFF):
        case = dict(name=f'static_default_{frame:08X}', source=[5248, 5248, 0],
                    target_cell=[21, 20], camera=[-80, 555], frame=frame)
        f = ActionFixture(case, row)
        f.u.mem_write(FRAME, words(frame))
        f.u.mem_write(SP, words(native.RET_MAGIC))
        f.u.reg_write(UC_X86_REG_ESP, SP)
        native.run_checked(f.u, 0x6F2AB0, 0x6F2AC9, count=100)
        assert f.u.reg_read(UC_X86_REG_ESP) == SP - 4
        timer = list(struct.unpack('<3I', f.u.mem_read(TIMER, 12)))
        f.invoke(0x4DC060, f.actor, (0, 0))
        result.append(dict(f.result(), native_timer=timer, executed_slice=['0x006F2AB0', '0x006F2AC9'],
                           stack_delta=-4))
    for start, frame, duration in ((100, 100, 25), (100, 124, 25), (100, 125, 25),
        (100, 90, 25), (0xFFFFFFF0, 8, 25), (0xFFFFFFFF, 100, 25),
        (100, 100, 0), (0, 0x80000000, 25)):
        case = dict(name=f'load_{start:08X}_{frame:08X}_{duration}',
                    source=[5248, 5248, 0], target_cell=[21, 20], camera=[-80, 555],
                    timer_before=[start, 0x12345678, duration], frame=frame)
        f = ActionFixture(case, row)
        f.u.mem_write(TIMER, words(*case['timer_before']))
        f.u.mem_write(FRAME, words(frame))
        f.u.mem_write(SP, words(native.RET_MAGIC))
        f.u.reg_write(UC_X86_REG_ESP, SP)
        native.run_checked(f.u, 0x685167, 0x6851A7, count=100)
        assert f.u.reg_read(UC_X86_REG_ESP) == SP - 4
        timer = list(struct.unpack('<3I', f.u.mem_read(TIMER, 12)))
        f.invoke(0x4DC060, f.actor, (0, 0))
        result.append(dict(f.result(), native_timer=timer, executed_slice=['0x00685167', '0x006851A7'],
                           stack_delta=-4))
    return result


def tactical_gates(row):
    result = []
    states = [dict(mode=1, local=local, selected=selected, option=option, planning=planning,
                   campaign_flags=[False, False])
              for local, selected, option, planning in itertools.product((False, True), repeat=4)]
    states += [dict(mode=0, local=False, selected=True, option=True, planning=False,
                    campaign_flags=list(flags)) for flags in itertools.product((False, True), repeat=2)]
    for i, state in enumerate(states):
        case = dict(name=f'gate_{i:02}', source=[5248, 5248, 0], target_cell=[21, 20],
                    camera=[-80, 555], **state)
        f = ActionFixture(case, row)
        f.start(100)
        u = f.u
        u.mem_write(0xA8B238, words(case['mode']))
        u.mem_write(0xA83D4C, words(rally.HOUSE if case['local'] else rally.HOUSE + 4))
        u.mem_write(rally.HOUSE + 0x1EC, bytes(case['campaign_flags']))
        u.mem_write(f.actor + 0x83, bytes([case['selected']]))
        u.mem_write(0x843108, bytes([case['option']]))
        u.mem_write(0xAC4CF4, bytes([case['planning']]))
        planning = f.invoke(0x637AA0) & 255
        house = f.invoke(0x50B6F0, rally.HOUSE) & 255
        u.mem_write(0xA8EC7C, words(rally.SELECTED))
        u.mem_write(SP + 0x13, bytes([planning]))
        u.mem_write(SP + 0x14, words(0))
        u.reg_write(UC_X86_REG_ESP, SP)
        stop = native.run_checked(u, 0x6D470D, (0x6D4756, 0x6D4764, 0x6D479F, 0x6D48FA),
                                  count=300000, required_addresses=[0x50B6F0])
        assert u.reg_read(UC_X86_REG_ESP) == SP
        f.returned(stop, SP)
        result.append(dict(f.result(), stop=f'0x{stop:08X}', human_player=house,
                           planning_result=planning, selected_virtual_reached=stop == 0x6D4756))
    return result


def destination_cases(row):
    result = []
    for name, extra in (
        ('fresh', {}), ('same_nav', dict(same_nav=True)),
        ('same_nav_forced', dict(same_nav=True, force_reassign=True)),
        ('same_nav_queue', dict(same_nav=True, nav_queue=2)),
        ('same_nav_queue_forced', dict(same_nav=True, nav_queue=2, force_reassign=True)),
        ('queue_clear', dict(nav_queue=2)), ('queue_preserve', dict(nav_queue=2, flag=0)),
        ('clear_destination', dict(nav_queue=2, same_nav=True, null=True)),
        ('bridge', dict(bridge=True)), ('skip_move', dict(skip_move=True))):
        case = dict(name='destination_' + name, family='drive', entry='unit',
                    camera=[-80, 255], source=[2688, 2688, 0], target_cell=[11, 10], **extra)
        f = ActionFixture(case, row)
        f.u.mem_map(native.SCRATCH, 0x10000)
        u, invoke, get32 = track_destination.make_destination_fixture(case, uc=f.u)
        f.actor = ACTOR
        # The existing setter fixture supplies every32x32 cell; its F8 is not
        # required by setter. Foot's separate diamond admission does read it.
        u.mem_write(MAP + 0xF4, words(16, 32))
        for pc in BRIDGE_INITIALIZERS:
            f.invoke(pc)
        f.start(100)
        f.invoke(0x741970, ACTOR, (0 if case.get('null') else CELL, case.get('flag', 1)))
        state = dict(source=signed_words(u, ACTOR + 0x9C, 3),
            nav=[11, 10] if get32(ACTOR + 0x5A4) == CELL else None,
            nav_queue=get32(ACTOR + 0x598), destination=signed_words(u, LOCO + 0x34, 3),
            head=signed_words(u, LOCO + 0x40, 3),
            movement_timer=[get32(ACTOR + 0x640), get32(ACTOR + 0x648)],
            blocked_timer=[get32(ACTOR + 0x668), get32(ACTOR + 0x670)])
        f.invoke(0x4DC060, ACTOR, (0, 0))
        result.append(dict(f.result(), setter_result=state))
    return result


def stock_context():
    inputs = {}
    for filename, section, keys in (
        ('rulesmd.ini', 'MTNK', ('Image', 'Locomotor', 'SpeedType', 'MovementZone', 'Turret', 'Primary')),
        ('artmd.ini', 'GTNK', ('TurretOffset', 'PrimaryFireFLH'))):
        path = Path('ini') / filename
        raw = path.read_bytes()
        values = sections(raw)[section]
        inputs[filename] = dict(bytes=len(raw), sha256=sha(raw), section=section,
                                literal_strings={key: values.get(key) for key in keys})
    return inputs


def instruction_evidence():
    image = native.image_bytes()
    ranges = {
        'tactical_after_rally_and_gate': (0x6D46CF, 0x6D4756),
        'tactical_forward_array_tail': (0x6D48FA, 0x6D4910),
        'tactical_object_effect_overlay_order': (0x6D463F, 0x6D46DD),
        'tactical_complete_prepared_local_loop': (0x6D46DD, 0x6D4912),
        'object_draw_and_extras_sweep': (0x6D8DB0, 0x6D97C8),
        'unit_extras_rank_selection_and_health': (0x6F5190, 0x6F5EEF),
        'unit_health_brackets_and_pips': (0x6F64A0, 0x6F6AC0),
        'gscreen_sidebar_tactical_gadgets_and_cursor': (0x4F4480, 0x4F45A9),
        'mouseclass_draw_sidebar_tail': (0x6D0E2F, 0x6D0E47),
        'wwmouse_constructor': (0x7B8730, 0x7B88C8),
        'wwmouse_capture_background_and_draw_cursor': (0x7B90C0, 0x7B92C1),
        'wwmouse_restore_background': (0x7B92D0, 0x7B93E8),
        'foot_entry_and_timer': (0x4DC060, 0x4DC0B3),
        'foot_move': (0x4DC1AA, 0x4DC339),
        'solid_leaf': (0x7BA610, 0x7BA8BD),
        'clip_rect': (0x421B60, 0x421C81),
        'timer_static_initializer': (0x6F2AB0, 0x6F2AD0),
        'timer_restart': (0x70D150, 0x70D174),
        'successful_load_timer_reanchor': (0x685167, 0x6851AC),
        'save_globals_caller': (0x67D40F, 0x67D434),
        'load_globals_caller': (0x67E8A0, 0x67E8C8),
        'load_then_postload_caller': (0x67E64E, 0x67E68A),
        'ordinary_globals_save': (0x67F7E0, 0x67F9BC),
        'ordinary_globals_load': (0x67F9C0, 0x67FD17),
        'delegated_globals_save': (0x539890, 0x539AD9),
        'delegated_globals_load': (0x539AE0, 0x539E9C),
        'display_click_dispatch_and_restart': (0x4ABC94, 0x4ABFB3),
        'planning_move_gate_1': (0x639040, 0x639054),
        'planning_move_gate_2': (0x639130, 0x639147),
        'planning_move_gate_2_true': (0x639203, 0x639207),
        'move_event_target_and_destination': (0x4C7467, 0x4C7482),
        'techno_array_append': (0x6F3183, 0x6F31D5),
        'techno_array_retain_or_append': (0x6F442E, 0x6F44E9),
        'techno_array_remove': (0x6F460C, 0x6F4640),
        'array_remove_preserves_order': (0x63F000, 0x63F031),
        'unit_same_destination_admission': (0x741970, 0x741ADE),
        'unit_queue_clear_gate': (0x7422E8, 0x7422F4),
        'unit_action_line_option_read': (0x5FA802, 0x5FA823),
    }
    rows = {}
    for name, (start, end) in ranges.items():
        report = native_inspect.inspect(image, native_inspect.parser().parse_args(
            ['disasm', hex(start), '--bytes', str(end-start)]))
        _, raw = native.file_span(image, start, end-start)
        rows[name] = dict(begin=f'0x{start:08X}', end_exclusive=f'0x{end:08X}',
                         bytes_hex=raw.hex(), sha256=sha(raw), **report)
    return rows


def ordering_vtables():
    image = native.image_bytes()
    return {name: dict(vtable=f'0x{table:08X}', entries={f'0x{offset:X}':
        dict(address=f'0x{table + offset:08X}',
             bytes_hex=native.file_span(image, table + offset, 4)[1].hex(),
             function=f'0x{struct.unpack("<I", native.file_span(image, table + offset, 4)[1])[0]:08X}')
        for offset in offsets}) for name, table, offsets in (
            ('Unit', 0x7F5C70, (0x104, 0x10C, 0x110, 0x438, 0x44C, 0x450, 0x454)),
            ('WWMouse', 0x7F7B2C, (0x3C, 0x40)))}


def production_reference():
    """Reuse the physical archive and prior native map-dimension evidence."""
    archive = (native.configured_gamemd().parent / 'multimd.mix').read_bytes()
    entry = stock.mix_hash('XMP03T4.MAP')
    raw = stock.mix(archive)[entry]
    physical_fields = sections(raw)['Map']
    previous_path = Path('tools/spatial_oracle/anytown_damage/next_family_native.json.gz')
    previous_raw = previous_path.read_bytes()
    previous = json.loads(gzip.decompress(previous_raw))['map']
    assert previous['sha256'] == sha(raw) and previous['bytes'] == len(raw)
    assert previous['bounds_normalization']['size'] == [80, 85]
    assert physical_fields == dict(Theater='TEMPERATE', Size='0,0,80,85', LocalSize='5,5,70,73')
    return dict(
        map=dict(archive='multimd.mix', archive_bytes=len(archive), archive_sha256=sha(archive),
            entry_id_hex=f'0x{entry:08X}', file='XMP03T4.MAP', bytes=len(raw), sha256=sha(raw),
            physical_fields=physical_fields, size=previous['bounds_normalization']['size'],
            prior_native_packet=dict(path=str(previous_path), sha256=sha(previous_raw), map=previous)),
        capture_root='logs/procedural-drawing', world_y_bias=15, production_camera=[-2100, 1426],
        production_zoom=1, production_extent=[800, 600], tactical_extent=[632, 568],
        crop_extent=[160, 160], owner='VERA-OBSERVER', actor_type='MTNK',
        input_boundaries=[
            'Each row pins the valid final release receipt, physical profile, full BGRA frame and executable. Source Location, selected state, NavCom, timer restart/current frames and target Cell fields are prepared from those receipts; the native producer does not execute the gesture, native Scenario, placement or movement history.',
            'The receipt physical_leptons getter and renderer object_location share exact XY/Z for the moved ground Unit. The initial grounded flat-level4 fallback also gives416. No equivalence is asserted for slope, air or another legacy nonexact-Z position by these production rows.',
            'Nav queue is explicitly empty: default navigation plus the sealed nonshift ordinary Move clear_queue1 route. There is no shift/planning queue producer in these profiles; later path/host consumers pop or clear. The receipt does not independently serialize that queue. target_cell35,94 is an unused fixture fallback whenever nav_null istrue.',
            'Stock initialized UnitActionLines defaults are used; startup capture ignores operator RA2MD.INI. Planning is inactive and the captured owner is the local noncampaign human. The real Tactical loop still executes its House, selected, option and planning gates before Foot.',
            'Native camera equals captured camera plus crop origin minus[0,15] at zoom1. All actual endpoint boxes and segments remain inside the crop, checked against original rectangle/line clipping receipts. Background is a write-mask sentinel, not expected production terrain or object pixels.',
            'Map Size80,85 comes from matching physical Map strings and the saved anytown native normalization packet. Target level4/slope0/flags0 and source movement are capture-supplied inputs, not a replay of native map loading, whole terrain resolution or locomotor history.',
            'Final production comparison must bind these inputs to saved receipts and compare every original opaque store against actual pixels. The arrived reselect/control pair has identical tactical pixels; its full-frame sidebar tooltip differs after reselect. This native corpus does not generate or compare that tooltip.',
        ])


def timer_references():
    image = native.image_bytes()
    return {f'0x{address:08X}': native_inspect.inspect(image,
        native_inspect.parser().parse_args(['find-bytes', address.to_bytes(4, 'little').hex()]))
        for address in (TIMER, TIMER + 4, TIMER + 8)}


def attack_production_reference(palette_row):
    """Pin input provenance and the zero-offset source-basis boundary."""
    from tools.native_slope import slope_matrices
    matrices = slope_matrices()
    assert len(matrices) == 21 and all(row[i] == 0 for row in matrices for i in (3, 7, 11))
    previous_path = Path('tools/projectile_oracle/ordinary_collision_vectors.json')
    previous_raw = previous_path.read_bytes()
    previous = json.loads(previous_raw)
    assert previous['sha256'] == native.NATIVE_SHA256 and previous['slope_matrices'] == matrices

    case = attack_production_inputs()[0]['input']
    f = ActionFixture(case, palette_row)
    f.setup_attack()
    u = f.u
    u.mem_write(FRAME, words(case['frame']))
    u.mem_write(0xB45188, words(*(word for row in matrices for word in row)))
    # Supplied VXL half sizes exercise the zero-angle translation arithmetic.
    # This does not run the VXL asset reader or claim a captured slope history.
    u.mem_write(rally.TYPE + 0x360, struct.pack('<dd', 15.5, 20.5))
    loco = read32(u, f.actor + 0x674)
    output = MEM + 0x7E000
    controls = []
    for slope, active in [(slope, False) for slope in range(21)] + [(0, True)]:
        u.mem_write(loco + 0x18, words(slope, slope))
        timer = [case['frame'] - 4, 0, 8, 8] if active else [0xFFFFFFFF, 0, 0, 0]
        u.mem_write(loco + 0x20, words(*timer))
        entries = []
        def observe(_u, pc, _size, _data):
            if pc in (0x4AFFEE, 0x4B02B3, 0x7559B0, 0x755A40):
                entries.append(f'0x{pc:08X}')
        hook = u.hook_add(UC_HOOK_CODE, observe)
        try:
            guard = f.read_only_draw(lambda: f.invoke(0x6F3D60, f.actor, (output, 0)))
        finally:
            u.hook_del(hook)
        coordinate = signed_words(u, output, 3)
        assert coordinate == case['source']
        assert ('0x004AFFEE' in entries) == active
        if active:
            assert '0x00755A40' in entries
        controls.append(dict(slope=slope, previous_slope=slope, slope_timer_words=timer,
            binary_frame=case['frame'], active_transition=active, coordinate=coordinate,
            entries=entries, **guard))

    image = native.image_bytes()
    instructions = {name: native_inspect.inspect(image, native_inspect.parser().parse_args(
        ['disasm', hex(start), '--bytes', str(end-start)]))
        for name, start, end in (
            ('drive_draw_matrix', 0x4AFF60, 0x4B0405),
            ('base_facing_matrix', 0x55A730, 0x55A7CF),
            ('quaternion_matrix_zero_translation', 0x646980, 0x646A75),
            ('matrix_translate', 0x5AE890, 0x5AE8EF))}
    prerequisite_path = Path(__file__).with_name('action_lines_attack_prerequisites.json')
    return dict(capture_root='logs/procedural-drawing/attack-production',
        release_label='procedural-unit-attack-production-v1',
        executable_sha256='4b4a83ceed0f074bab7dd0461c2bfef33f6e26a55c5842e8f43601eaad06e18c',
        production_camera=[-2100, 1426], production_zoom=1, world_y_bias=15,
        production_extent=[800, 600], tactical_extent=[632, 568], crop_extent=[192, 160],
        crop_origin=[176, 336], map_reference='production_reference.map',
        prerequisite_packet=dict(path=prerequisite_path.relative_to(Path(__file__).resolve().parents[2]).as_posix(), sha256=sha(prerequisite_path.read_bytes()),
            fields='type_inputs, type_readers.turret_offset, type_readers.warhead_ap, aim_cases'),
        no_rocking_origin=dict(startup_matrix_owner='tools/native_slope.py:slope_matrices',
            prior_packet=dict(path=str(previous_path), sha256=sha(previous_raw)),
            translation_words=[[row[i] for i in (3, 7, 11)] for row in matrices],
            source=case['source'], body_facing=case['body_facing'], turret_facing=case['turret_facing'],
            turret_offset=0, rocking_angles=[0, 0], supplied_half_sizes=[15.5, 20.5],
            controls=controls, instructions=instructions),
        input_boundaries=[
            'Four valid receipts pin final state, raw opt-in action_line_inputs, profile, full BGRA frame and release executable. The native loop begins from those prepared states; it does not replay Scenario loading, input gestures, event scheduling, pursuit or movement history.',
            'Source and target original Drive constructors, Move_To when captured IsMoving istrue, original IsMoving, live Foot speed and Facing setters execute. Captured fixed applied-fraction bits are divided by65536 exactly; captured Housef32 and cratef64 bits decode directly. Original speed reads must equal both captured current_speed values. Captured rookie rank maps to native0.0; no promotion accumulation is inferred.',
            'Both current weapons are physical105mm, type MTNK/GTNK offset0, with physical full-reader Speed7/raw17, weaponSpeed40/Range5/postpass95, CannonROT0/Floaterfalse and Gravity6 from the pinned prerequisite packet. Physical AP defaults Rocker/DirectRockerfalse. No rocking owner is captured; zero native angles are supplied. Queue state remains the ordinary nonshift Attack/Move empty-queue route, not an observed queue serialization.',
            'The stationary pair captures sourceCell28,92 and targetCell34,92. Moving final actors occupy29,92 and33,92; those two terrain cells were not observed. Exact actor physicalZ416 is supplied. The supplied level4/slope0/flags0 Cell34,92 is the observed competing NavCom or unused null-Nav fallback, not a claimed terrain observation for the moving actors.',
            'The source basis uses the original flat Drive setup, retaining captured source motion. At TurretOffset0 and zero rocking the origin is invariant under all21 original startup slope matrices: every translation column iszero. The22 pivot controls execute all stable slots plus the active slope-timer zero-angle arithmetic with supplied half sizes15.5/20.5. The latter enters4AFFEE and original755A40 with equal source/destination slopes; different-slope interpolation is instruction evidence because646980 explicitly clears every translation column. These controls do not establish arbitrary rocking, malformed matrices or a captured slope transition history.',
            'Stock UnitActionLines defaults, local human owner, active selected source and inactive planning feed the unchanged Tactical loop. Unselected matched controls preserve Target and motion but make no Foot call or pixel store. Setup getter checks precede the guarded drawing interval; drawing leaves actor/Drive/timer bytes unchanged and enters no observed RNG, restart or detach call.',
            'Native camera equals captured camera plus crop origin minus[0,15] at zoom1. Both complete endpoint boxes and every original line remain unclipped in192x160, inside the unchanged guarded64KiB surface reservation. Background is a write-mask sentinel. Production comparison must bind these raw inputs to receipts and compare every original opaque store; selection brackets/health differences in the unselected control are a distinct earlier pass.',
        ])


def attack_instruction_evidence():
    image = native.image_bytes()
    ranges = dict(source_pivot=(0x6F3D60, 0x6F3F34), target_aim=(0x70BCB0, 0x70BE45),
        foot_attack_branch=(0x4DC0B3, 0x4DC1AA), current_weapon=(0x70E1A0, 0x70E1D9),
        object_high_flight=(0x5F6B90, 0x5F6BC5), cannon_air_rejection=(0x6FC705, 0x6FC73C),
        elevation_range_division=(0x6F703D, 0x6F706E),
        object_action_attack_admission=(0x700536, 0x70056C),
        attack_event_mission=(0x4C739B, 0x4C73C0),
        event_target_destination=(0x4C7467, 0x4C7482),
        fireat_argument_speed=(0x6FE4F2, 0x6FE545),
        fireat_tarcom_aim=(0x6FE625, 0x6FE661),
        pointer_expiry_scan_target=(0x7079B7, 0x707A43),
        turret_offset_art_read=(0x715876, 0x71589A),
        warhead_rocker_read=(0x75D5AA, 0x75D5DE))
    rows = {}
    for name, (start, end) in ranges.items():
        report = native_inspect.inspect(image, native_inspect.parser().parse_args(
            ['disasm', hex(start), '--bytes', str(end-start)]))
        _, raw = native.file_span(image, start, end-start)
        rows[name] = dict(begin=f'0x{start:08X}', end_exclusive=f'0x{end:08X}',
                         bytes_hex=raw.hex(), sha256=sha(raw), **report)
    return rows


def attack_type_readers():
    """Execute the existing retail cache/reader owners for pivot and no-rocking."""
    from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ESI
    from tools.spatial_oracle.anytown_damage import mtnk_attack as base
    from tools.spatial_oracle.engineer_bridge_cursor_caller import HutTypeReader
    from tools.spatial_oracle.building_body_rules import SP as READER_SP
    from tools.rules_oracle.weapon_speed_order import fresh

    m = HutTypeReader()
    u = m.u
    typ = m.alloc(0x1900)
    m.phase = 'MTNK_constructor'
    m.invoke(0x7470D0, typ, (m.cstring('MTNK'),))
    constructor = {name: signed_words(u, typ + offset, 1)[0]
                   for name, offset in (('turret_offset', 0x720), ('turret_count', 0x808),
                                        ('weapon_count', 0x80C))}
    art_path = base.layers()[0][1].parent / 'ARTMD.INI'
    art_raw = art_path.read_bytes()
    physical, physical_lines = base.lexical(art_raw, {'GTNK'})
    u.mem_write(typ + 0x1F8, b'GTNK\0')
    reads, rows = [], []
    def observe(_u, pc, _size, _data):
        if pc == 0x5276D0:
            sp = u.reg_read(UC_X86_REG_ESP)
            reads.append(dict(section=m.string(m.read32(sp + 4)),
                key=m.string(m.read32(sp + 8)), default=signed_words(u, sp + 12, 1)[0]))
    hook = u.hook_add(UC_HOOK_CODE, observe)
    for name, values in (
        ('physical_GTNK', physical['GTNK']), ('positive_control', {'TurretOffset': '80'}),
        ('missing_retains_prior', {}), ('wrong_case_retains_prior', {'turretoffset': '999'}),
        ('negative_control', {'TurretOffset': '-80'}),
        ('decimal_wrapping', {'TurretOffset': '4294967297'}),
    ):
        m.phase = name
        m.make_ini({'GTNK': values})
        before = signed_words(u, typ + 0x720, 1)[0]
        u.reg_write(UC_X86_REG_ESP, READER_SP)
        u.reg_write(UC_X86_REG_EBP, typ)
        u.reg_write(UC_X86_REG_ESI, typ + 0x1F8)
        native.run_checked(u, 0x715876, 0x71589A, required_addresses=[0x5276D0])
        rows.append(dict(name=name, raw=values.get('TurretOffset'), before=before,
            after=signed_words(u, typ + 0x720, 1)[0], read=reads[-1],
            stack_delta=u.reg_read(UC_X86_REG_ESP) - READER_SP))
    u.hook_del(hook)
    m.unchanged()
    result = dict(turret_offset=dict(constructor=constructor, rows=rows,
        art=dict(file='ARTMD.INI', bytes=len(art_raw), sha256=sha(art_raw),
                 sections=physical, source_lines=physical_lines)))

    m, _, _ = fresh()
    u = m.u
    u.mem_write(0x8874C0, words(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    warhead = m.invoke(0x75E3B0, m.cstring('AP'))
    original = bytes(u.mem_read(warhead + 0x14E, 2)).hex()
    reads, rows = [], []
    def observe_bool(_u, pc, _size, _data):
        if pc == 0x5295F0:
            sp = u.reg_read(UC_X86_REG_ESP)
            key = m.string(m.read32(sp + 8))
            if key in ('Rocker', 'DirectRocker'):
                reads.append(dict(section=m.string(m.read32(sp + 4)), key=key,
                    default_byte=m.read32(sp + 12) & 255,
                    stack_word_hex=f'{m.read32(sp + 12):08x}'))
    hook = u.hook_add(UC_HOOK_CODE, observe_bool)
    for filename, path in base.layers():
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            rows.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        selected, lines = base.lexical(raw, {'AP'})
        m.rules_cache(selected)
        start = len(reads)
        before = bytes(u.mem_read(warhead + 0x14E, 2)).hex()
        admitted = m.invoke(0x75D3A0, warhead, (base.RULES,)) & 255
        rows.append(dict(file=filename, bytes=len(raw), sha256=sha(raw), sections=selected,
            source_lines=lines, admitted=admitted, before=before,
            after=bytes(u.mem_read(warhead + 0x14E, 2)).hex(), reads=reads[start:]))
    u.hook_del(hook)
    result['warhead_ap'] = dict(constructor_entry='0x0075E3B0', constructor_fields_hex=original,
        fields=['Rocker +0x14E', 'DirectRocker +0x14F'], layers=rows)
    return result


def attack_prerequisites():
    """Compose the existing retail Mission world, without its fixed Cell order.

    Full original constructors, placement, object admission, event/setter,
    live aim and FireAt run. This adapter supplies a second House and chooses
    explicit operations; it does not replace the existing world or its hooks.
    """
    from tools.spatial_oracle.anytown_damage import mission, mtnk_attack as base
    from tools.spatial_oracle.engineer_repair_admission import EngineerJoinedFixture
    from tools.spatial_oracle.object_flight_height import initialize_object_scalars

    class PlacementBoundary(Exception):
        pass

    q = mission.Mission()
    m, u = q.m, q.u
    result = dict(schema='vera20k.unit-attack-prerequisites.v1',
                  native_sha256=native.NATIVE_SHA256, steps=[], aim_cases=[], fireat_cases=[])
    result['object_initializers'] = [f'0x{a:08X}' for a in initialize_object_scalars(u, m.invoke)]
    def before_fixed_cell_command():
        raise PlacementBoundary()
    try:
        q.setup(placement_observer=before_fixed_cell_command)
    except PlacementBoundary:
        pass
    else:
        raise AssertionError('Mission placement observer was not reached')
    result.update(inputs=q.inputs, world=q.world, source_after_placement=q.state())
    assert q.placement_result & 255 == 1 and m.read32(q.src + 0x2B4) == 0
    assert not any(row.get('kind') == 'event_execute' for row in q.events)
    result['elevation_layers'] = []
    for filename, path in base.layers():
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            result['elevation_layers'].append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        selected, lines = base.lexical(raw, {'ElevationModel'})
        m.rules_cache(selected)
        admitted = m.invoke(0x66D150, q.rules, (base.RULES,)) & 255
        result['elevation_layers'].append(dict(file=filename, sha256=sha(raw),
            sections=selected, source_lines=lines, admitted=admitted,
            increment=m.read32(q.rules + 0x1838)))
    text_before = sha(bytes(u.mem_read(0x401000, 0x3E0000)))
    enemy = m.alloc(0x16000)
    u.mem_write(enemy + 0x30, words(1, q.country))
    for off in (0x188, 0x1A8):
        u.mem_write(enemy + off, struct.pack('<d', 1.0))
    for off in (0x5514, 0x5564):
        u.mem_write(enemy + off + 4, words(m.alloc(4096), 1024))
    u.mem_write(enemy + 0x5788, words(2))
    u.mem_write(q.house + 0x5788, words(1))
    houses = m.alloc(8)
    u.mem_write(houses, words(q.house, enemy))
    u.mem_write(0xA8022C, words(houses))
    target = m.alloc(0x1000)
    q.phase = 'setup'
    start = len(q.events)
    m.invoke(0x7353C0, target, (q.typ, 0))
    for off in (0x21C, 0x14C):
        u.mem_write(target + off, words(enemy))
    target_coord = m.alloc(12)
    m.invoke(0x486840, q.resident.ptrs[87, 52], (target_coord,))
    q.phase = 'target_placement'
    placement = m.invoke(0x737BA0, target, (target_coord, 0x80))
    result['target_construction'] = dict(placement_al=placement & 255,
        position=base.xyz(u, target + 0x9C), events=q.events[start:],
        id=m.read32(target + 0x10), source_id=m.read32(q.src + 0x10),
        unit_deploy_state=base.i32(u, target + 0x6D8),
        source_unit_deploy_state=base.i32(u, q.src + 0x6D8),
        supplied_enemy=dict(pointer=enemy, index=1, country='Americans',
                            alliance_mask=2, source_alliance_mask=1, human=False))
    assert placement & 255 == 1
    projectile = m.read32(q.weapon + 0xA0)
    warhead = m.read32(q.weapon + 0xAC)
    result['type_inputs'] = dict(speed=base.i32(u, q.typ + 0x678),
        country_speed_bits=bytes(u.mem_read(q.country + 0x12C, 4)).hex(),
        veteran_speed_bits=bytes(u.mem_read(q.rules + 0x678, 8)).hex(),
        primary=hex(m.read32(q.typ + 0x898)), primary_name=m.string(q.weapon + 0x24),
        resolved_image=m.string(q.typ + 0x1F8), turret_offset=base.i32(u, q.typ + 0x720),
        primary_flh=base.xyz(u, q.typ + 0x89C), damage=base.i32(u, q.weapon + 0xA4),
        weapon_speed=base.i32(u, q.weapon + 0xA8), projectile_rot=base.i32(u, projectile + 0x2DC),
        projectile_floater=u.mem_read(projectile + 0x295, 1)[0], gravity=base.i32(u, q.rules + 0x16B8),
        warhead=m.string(warhead + 0x24), rocker=bool(u.mem_read(warhead + 0x14E, 1)[0]),
        direct_rocker=bool(u.mem_read(warhead + 0x14F, 1)[0]))
    event, out, heading = m.alloc(0x70), m.alloc(12), m.alloc(4)
    observed, pending = [], {}
    names = {0x70BCB0: 'aim', 0x410540: 'target_coordinate', 0x5F65A0: 'object_coordinate',
        0x4AFB80: 'drive_is_moving', 0x4DB1A0: 'foot_current_speed', 0x50C050: 'house_speed',
        0x70D0D0: 'ability', 0x70E1A0: 'current_weapon', 0x70E140: 'get_weapon',
        0x7177C0: 'type_weapon', 0x773070: 'shot_speed', 0x4C93D0: 'facing_current',
        0x7446E0: 'unit_expiry', 0x4D9960: 'foot_expiry', 0x7077C0: 'techno_expiry',
        0x65AAC0: 'radio_expiry', 0x5F5230: 'object_expiry', 0x70D4A0: 'world_detach',
        0x7258D0: 'abstract_detach', 0x5F65F0: 'uninit', 0x73FD50: 'unit_object_action',
        0x4DDED0: 'foot_object_action', 0x6FFEC0: 'techno_object_action',
        0x6FDD50: 'techno_fireat', 0x6F3330: 'weapon_selection',
        0x4F9A90: 'house_ally', 0x70D150: 'draw_timer_restart'}
    def returned(row):
        row['returned_eax'] = u.reg_read(UC_X86_REG_EAX)
        if 'output_pointer' in row:
            row['returned_coord'] = base.xyz(u, row['output_pointer'])
        if 'facing_pointer' in row:
            row['returned_facing'] = m.read32(row['facing_pointer']) & 0xFFFF
    def observe(_u, pc, _size, _data):
        if pc in pending:
            row = pending[pc].pop()
            if not pending[pc]:
                del pending[pc]
            returned(row)
        if pc in names:
            sp = u.reg_read(UC_X86_REG_ESP)
            row = dict(pc=hex(pc), name=names[pc], ecx=hex(u.reg_read(UC_X86_REG_ECX)),
                       caller=hex(m.read32(sp)))
            if pc in (0x70BCB0, 0x410540, 0x5F65A0):
                row['output_pointer'] = m.read32(sp + 4)
            if pc == 0x4C93D0:
                row['facing_pointer'] = m.read32(sp + 4)
            if pc == 0x4AFB80:
                row['com_this'] = hex(m.read32(sp + 4))
            if pc in (0x73FD50, 0x4DDED0, 0x6FFEC0, 0x6FDD50):
                row['args'] = [m.read32(sp + 4), m.read32(sp + 8)]
            if pc == 0x773070:
                row['distance'] = base.i32(u, sp + 4)
            observed.append(row)
            pending.setdefault(m.read32(sp), []).append(row)
    hook = u.hook_add(UC_HOOK_CODE, observe)
    # Reuse the OS key transport owner at its import boundaries only. Cold
    # GetKeyState IAT bytes are data and must never be executed as a function.
    transport = EngineerJoinedFixture.__new__(EngineerJoinedFixture)
    transport.u, transport.read32 = u, m.read32
    transport.trail, transport.trace, transport.platform_audio = [], [], None
    result['os_input_calls'] = []
    transport.os_input_transport = dict(key_words={}, observe=result['os_input_calls'].append)
    def platform(_u, pc, size, data):
        if pc in (0x53EC9A, 0x646F20):
            transport.phase = q.phase
            EngineerJoinedFixture.hook(transport, u, pc, size, data)
    platform_hook = u.hook_add(UC_HOOK_CODE, platform)
    def rng():
        return {key: base.sr.rng_state(u, pointer) for key, pointer in q.resident.rngs.items()}
    def state():
        loco = m.read32(target + 0x674) - 4
        return dict(source=q.state(), target=hex(m.read32(q.src + 0x2B4)),
            source_nav=hex(m.read32(q.src + 0x5A4)), source_nav_queue=m.read32(q.src + 0x598),
            targeting_timer=list(struct.unpack('<3i', u.mem_read(q.src + 0x180, 12))),
            target_position=base.xyz(u, target + 0x9C), target_nav=hex(m.read32(target + 0x5A4)),
            target_destination=base.xyz(u, loco + 0x34), target_head=base.xyz(u, loco + 0x40),
            target_applied_bits=bytes(u.mem_read(target + 0x578, 8)).hex(),
            target_rank_bits=bytes(u.mem_read(target + 0x150, 4)).hex())
    def step(name, invoke):
        q.phase = name
        start, before, before_rng = len(q.events), state(), rng()
        observed.clear()
        pending.clear()
        answer = invoke()
        for row in pending.pop(native.RET_MAGIC, []):
            returned(row)
        assert not pending, pending
        row = dict(name=name, before=before, after=state(), result=answer,
            events=q.events[start:], original_calls=list(observed), rng_before=before_rng, rng_after=rng())
        result['steps'].append(row)
        return row
    def command_attack():
        args = (0, m.read32(q.src + 0x10), 0x34, 1, m.read32(target + 0x10), 0x34, 0, 0, 0, 0)
        m.invoke(0x4C6860, event, args)
        encoded = bytes(u.mem_read(event, 0x6F)).hex()
        m.invoke(0x4C6CB0, event)
        return dict(args=args, event_hex=encoded)
    def query_aim():
        pointers = dict(source=(q.src, 0x700), target=(target, 0x700),
            source_locomotor=(m.read32(q.src + 0x674) - 4, 0x70),
            target_locomotor=(m.read32(target + 0x674) - 4, 0x70), timer=(TIMER, 12))
        before = {key: bytes(u.mem_read(*span)) for key, span in pointers.items()}
        m.invoke(0x70BCB0, q.src, (out,))
        unchanged = {key: raw == bytes(u.mem_read(*pointers[key])) for key, raw in before.items()}
        assert all(unchanged.values()), unchanged
        return dict(aim=base.xyz(u, out), state_unchanged=unchanged)
    def aim(name):
        inputs = dict(source_xyz=base.xyz(u, q.src + 0x9C), target_xyz=base.xyz(u, target + 0x9C),
            tarcom_kind='null' if not m.read32(q.src + 0x2B4) else
                ('Unit' if m.read32(q.src + 0x2B4) == target else 'Cell'),
            target_speed_fraction_bits=bytes(u.mem_read(target + 0x578, 8)).hex(),
            target_rank_bits=bytes(u.mem_read(target + 0x150, 4)).hex(),
            current_weapon_present=bool(m.read32(q.typ + 0x898)))
        row = step(name, query_aim)
        assert row['rng_before'] == row['rng_after']
        assert not any(call['name'] == 'draw_timer_restart' for call in row['original_calls'])
        for call in row['original_calls']:
            if call['name'] == 'foot_current_speed':
                inputs['target_current_speed'] = call['returned_eax']
            elif call['name'] == 'facing_current':
                inputs['target_facing'] = call['returned_facing']
            elif call['name'] == 'drive_is_moving':
                inputs['target_moving'] = bool(call['returned_eax'] & 255)
        result['aim_cases'].append(dict(name=name, input=inputs, **row['result'],
            calls=row['original_calls'], rng_unchanged=row['rng_before'] == row['rng_after']))
    m.invoke(0x4E7E20, 0)
    admitted = step('ordinary_enemy_object_action', lambda: m.invoke(0x73FD50, q.src, (target, 0)))
    assert admitted['result'] == 5, admitted['result']
    step('fresh_enemy_unit_attack_event', command_attack)
    aim('stationary_target_aim')
    step('target_move_destination', lambda: m.invoke(0x741970, target, (q.resident.ptrs[87, 53], 1)))
    step('target_full_fraction', lambda: m.invoke(0x4D3710, target, (0, 0x3FF00000)))
    for facing in (0, 0x4000, 0xC000):
        u.mem_write(heading, words(facing))
        step(f'target_facing_{facing}', lambda: m.invoke(0x4C9300, target + 0x388, (heading,)))
        aim(f'moving_target_aim_{facing}')
    step('target_veteran', lambda: m.invoke(0x750090, target + 0x150, (1,)))
    aim('veteran_target_aim')
    # Current-weapon absence is a supplied control. Restore the actual physical
    # binding before the affected full FireAt consumer executes.
    u.mem_write(q.typ + 0x898, words(0))
    aim('moving_target_missing_current_weapon')
    u.mem_write(q.typ + 0x898, words(q.weapon))
    step('repeat_enemy_unit_attack_event', command_attack)
    def fireat(name):
        argument = q.resident.ptrs[87, 54]
        get_coords = m.read32(m.read32(argument) + 0x58)
        m.invoke(get_coords, argument, (out,))
        argument_xyz = base.xyz(u, out)
        def invoke():
            answer = m.invoke(0x6FDD50, q.src, (argument, 0))
            return dict(argument=hex(argument), argument_kind='Cell', argument_cell=[87, 54],
                tarcom=hex(m.read32(q.src + 0x2B4)), returned=hex(answer),
                bullet=None if not answer else dict(position=base.xyz(u, answer + 0x9C),
                                                   velocity=base.vec(u, answer + 0xE8)))
        row = step(name, invoke)
        calls = row['original_calls']
        launch = next(c for c in calls if c['name'] == 'shot_speed' and c['caller'] == '0x6fe53f')
        aim_call = next(c for c in calls if c['name'] == 'aim')
        result['fireat_cases'].append(dict(name=name,
            input=dict(source_xyz=row['before']['source']['position'], argument_target_xyz=argument_xyz,
                argument_kind='Cell', argument_cell=[87, 54], argument_getter=f'0x{get_coords:08X}',
                tarcom_kind='null' if row['before']['target'] == '0x0' else 'Unit',
                target_xyz=row['before']['target_position']),
            aim=aim_call['returned_coord'], launch_distance=launch['distance'],
            launch_speed=launch['returned_eax'], bullet=row['result']['bullet'], calls=calls,
            rng_events=[e for e in row['events'] if e['kind'] in ('rng', 'raw')]))
    fireat('fireat_argument_cell_distinct_from_moving_tarcom_unit')
    def stop():
        m.invoke(0x4C65E0, event, (0, 6, m.read32(q.src + 0x10), 0x34))
        return m.invoke(0x4C6CB0, event)
    step('source_stop_event', stop)
    aim('targetless_aim')
    fireat('fireat_argument_cell_with_null_tarcom')
    step('attack_again', command_attack)
    u.mem_write(q.src + 0x180, words(0, 0, 30))
    u.mem_write(FRAME, words(10))
    q.frame = 10
    u.mem_write(target + 0x6C, words(0))
    u.mem_write(target + 0x90, b'\0')
    step('supplied_dead_target_expiry_active_scan', lambda: m.invoke(0x7446E0, q.src, (target, 1)))
    aim('expired_target_aim')
    u.hook_del(hook)
    u.hook_del(platform_hook)
    result['native_text_unchanged'] = text_before == sha(bytes(u.mem_read(0x401000, 0x3E0000)))
    assert result['native_text_unchanged']
    result['setup_rng_events'] = [row for row in q.events if row.get('kind') in ('rng', 'raw')
                                 and row['phase'] in ('setup', 'placement', 'target_placement')]
    result['type_readers'] = attack_type_readers()
    result['instruction_evidence'] = attack_instruction_evidence()
    return result


def generate():
    palette_row, palette_evidence = physical_palette()
    rows = [producer(c, palette_row) for c in producer_cases()]
    colors = {str(i): dict(index=i, packed=struct.unpack_from('<H', palette_row, i*2)[0],
        **house_color.house_color(palette_row, i), original_unpack='0x0050B840') for i in (3, 8)}
    result = dict(schema='vera20k.selected-unit-action-lines.v1', source='unicorn/gamemd.exe',
        size=list(rally.SIZE), background=rally.BACKGROUND, **palette_evidence,
        prepared_common=dict(actor_vtable='0x7F5C70', foot_virtual_offset='0x438',
            target=None, navcom_kind='Cell', map_size=[32, 32], foot_arguments=[0, 0],
            bridge_initializers=[f'0x{pc:08X}' for pc in BRIDGE_INITIALIZERS]),
        stock_context=stock_context(),
        option_default=dict(global_address='0x00843108', file_byte_hex=
            native.file_span(native.image_bytes(), 0x843108, 1)[1].hex(),
            section='Options', key='UnitActionLines', reader='0x005295F0',
            reader_default='retained this+0x1E byte', native_ini='0x008870C0'),
        cases=rows,
        leaf_cases=[solid(c, palette_row) for c in solid_cases()],
        clip_rect_cases=[intersect(c, palette_row) for c in clip_rect_cases()],
        timer_prerequisites=timer_prerequisites(palette_row),
        tactical_gate_cases=tactical_gates(palette_row),
        destination_cases=destination_cases(palette_row),
        batch_cases=[batch(c, palette_row) for c in batch_cases()],
        compositing_color_controls=colors,
        compositing_cases=[compositing(c, palette_row, colors) for c in compositing_cases()],
        production_cases=[dict(batch(row['input'], palette_row), size=row['input']['surface_size'],
            production=row['production']) for row in production_inputs()],
        production_reference=production_reference(),
        ordering_vtables=ordering_vtables(),
        instruction_evidence=instruction_evidence(), timer_literal_address_occurrences=timer_references(),
        attack_cases=[attack(case, palette_row) for case in attack_cases()],
        attack_instruction_evidence=attack_instruction_evidence(),
        attack_production_cases=[dict(batch(row['input'], palette_row),
            size=row['input']['surface_size'], production=row['production'])
            for row in attack_production_inputs()],
        attack_production_reference=attack_production_reference(palette_row))
    for forward, reverse in zip(result['compositing_cases'][::2], result['compositing_cases'][1::2], strict=True):
        assert forward['full_surface_sha256'] != reverse['full_surface_sha256']
    for row in result['production_cases'] + result['attack_production_cases']:
        for call in row['calls']:
            if call['name'] == 'clip_line':
                assert call['accepted_al'] == 1 and call['points_before'] == call['points_after']
            elif call['name'] == 'intersect_rect' and call['clip_rectangle'] == row['input']['clip']:
                assert call['rectangle_before'] == call['rectangle_after']
    gc.collect()
    return result


def metadata():
    return native.provenance(
        scope='Selected ground Unit Move and Attack action lines4DC060 ->7049C0, original RGB565 pixel leaves, timers, prepared Tactical admission/batches, existing Unit/Drive destination composition, twelve Move and four Attack production-input replays, prepared Attack pivot/live aim and zero-rocking origin controls; finite comparisons, not whole Scenario parity',
        assumptions=[
            'Original gamemd.exe, FPCW0E7F, physical RA2/CACHE palette assets, RGB565160x120 BSurface, fixed background sentinel. Existing original palette initialization owns conversion; index3 middle row supplies Move green.',
            'The first57 cases promote the saved ignored research corpus geometry and timer pairs. Map dimensions32x32 and original Foot bridge initializer chain are now explicit; prior flat outputs remain identical.',
            'Actor Location, selected bit, owner, Target=null, NavCom and queued Cells, cell levels/slopes/flags, camera and viewport are prepared inputs. Actual Unit vtable is used. No whole MTNK constructor, scenario loading, selection generation or movement history is claimed.',
            'Tactical gate controls execute original6D470D through exactly one gate exit or returned vt438 call, preceded by original House50B6F0 and Planning637AA0. The array, index, cached planning byte and owner/campaign state are supplied. No whole Tactical rendering or Techno-array lifetime is claimed.',
            'Five batch controls execute original6D46DD..6D4912, including Planning637AA0, the complete forward Techno-array loop, House50B6F0 and every admitted Foot4DC060 ->7049C0 call.32/33/64 selected actors have supplied flat source/Cell state, local owner, health100, empty Target/queue, null capture-manager/temporal fields and planning off. Actor input array, separate Techno order, native actor pointers and observed call order are saved. This is the real loop, not a host-side replacement; preceding Tactical passes, object constructors and array lifecycle remain outside the executable boundary.',
            'Six standalone compositing controls execute original7049C0 for2/33/64 ordered supplied XYZ pairs with alternating converted palette indices3/8 and reversed-order controls. Existing house_color.house_color executes original50B840 to unpack the unchanged original Convert row at each supplied lookup index. These rows prove finite opaque overwrite order for both colors; they do not claim Attack source, aim/lead, target or timer admission. The all-green batch reversals intentionally have identical final pixels while the observed producer order reverses.',
            'Twelve production_cases reuse the same complete prepared Tactical loop with one captured MTNK1374 state, supplied local ownership, actual selected bit, NavCom or null, final binary frame and last gesture restart frame. Existing Rally accepts bounded optional160x160 dimensions within its unchanged64KiB color/A reservations. Every resulting native endpoint box and line is asserted unclipped. Production input/file identities, original MapSize evidence, exact crop origins and explicit state boundaries are recorded in production_cases and production_reference. The first277 original rows and all earlier top-level values remain unchanged.',
            'Unit Extras order is original instruction evidence: object draw6D8DB0 calls Unit body then its later visible-Techno virtual110 sweep; Unit6F5190 rank and selected health/bracket/pip calls finish before Tactical effects/bandbox/second rally/action lines. GScreen orders Sidebar/MouseClass before Tactical pass2 and gadgets/tooltip/WWMouse cursor after it. These bodies are preserved as original bytes, not executed UI/body/status rendering; the current ordinary MTNK has no selected-building range-ring route.',
            'Foot ground, slope and deck endpoint queries execute original Cell486840, bounds568300, map lookup565730 and height578080. Queue-only state with null NavCom is rejected by the real entry gate. Ground cell centers and source subcell/height controls are bounded samples.',
            'Static timer initialization executes6F2AB0 through6F2AC9 before CRT atexit; successful-load timer reanchor executes685167 through6851A7 before the following unrelated call. Slice inputs and stack deltas are explicit. Whole successful load is not executed. Original global save/load stream ranges and their delegated globals helpers are preserved separately as instruction evidence.',
            'Ordinary globals Save67F7E0/Load67F9C0 explicitly serialize named fields and vectors, including binary frame as the third dword; none of their ranges or delegated globals539890/539AE0 overlap B0EA80..88. Caller instructions show67D421 ->globals save,67E8B5 ->globals load, then outer67E659 ->scenario load and67E685 ->postload685120. Together with the literal timer-address scan this supports retaining process-global action timer state across the ordinary globals route, then reanchoring it. This is instruction-level stream-layout evidence, not an executed full save/load, object reconstruction, arbitrary alias proof or malformed-save guarantee.',
            'Native start=-1 and signed wrapping timer arithmetic are sampled as supplied boundary controls, without claiming normal-game reachability. Timer middle dword is observed stack carry and has no elapsed-time role.',
            'Destination controls reuse track_destination.make_destination_fixture in the same VM and execute Unit741970 ->Foot4D94B0/Drive4AFD40 before drawing. Empty radio/deploy/EMP/transport state and supplied type/rules are inherited fixture boundaries. Repeated NavCom and force flag controls establish setter admission; full click/event dispatch is instruction evidence, not executed here.',
            'Solid and rectangle leaves run unchanged. Pixel cases cover finite directions, clipping and endpoint intersections; no exhaustive integer domain or native desktop/window proof. Existing bounded chop53 clipping residual remains separate from ordering correctness.',
            'Move cases reject known RNG65C780/65C7E0 entries during observed drawing/setter phases. Existing setter fixture initializes Scenario RNG before these phases. Aircraft, jumpjet, transport, pathfinding, detach and downstream mission cadence are outside this pixel corpus.',
            'The earlier Move physical MTNK/GTNK strings remain context evidence. Attack retail field/default/command evidence is generated separately by the same producer --attack-prerequisites mode.',
            'Thirty-one Attack cases execute the original Foot TarCom-priority branch, source6F3D60, original Drive4AFF60/55A730, target70BCB0, actual Unit coordinate/current-weapon/Facing virtuals, live Foot4DB1A0, and the unchanged pixel leaves. Original Drive/Facing constructors and setters establish flat actors; type/House/weapon/position/Target/NavCom/queue values remain explicit prepared inputs. Physical Speed7 -> raw17 is independently recorded by the full retail Unit reader in the prerequisite packet; target fraction/rank/crate controls reuse track_speed_native.seed_getter_state.',
            'Attack weapon_inputs supply stored speed95/ROT0/Floaterfalse/Gravity6, bound to the full physical105mm reader and postpass in the prerequisite packet. Authored Speed40 first reads102 through474810; the later7729F0 Range5 ballistic postpass produces95. speed_percent and range_cells retain that authorship, not a fitted inverse. The prepared raster fixture writes the stored value through its existing weapon setup; original773070 derives shot speed from distance and ROT0 does not consume the stored field. Offset80/body/turret controls distinguish body pivot from muzzle FLH; stock GTNK offset0 is independently read. No rocking, slope basis, damage producer or full Unit constructor is claimed for these prepared raster rows.',
            'Prepared Unit deploy state6D8=-1 matches the existing track_destination input contract and the actual constructors recorded in the prerequisite packet. Original Drive Move_To executes for moving rows; a separate original Is_Moving call asserts the requested true/false state before every draw, so a declined movement gate cannot masquerade as a moving-target comparison.',
            'Attack query snapshots compare every source/target actor byte0x700, both Drive0x70 ranges and all12 timer bytes. Known raw/ranged RNG, timer restart, world/abstract detach and UnInit entry guards remain empty during Foot drawing. Physical Convert middle-row index8 supplies red0xA800. Null Target controls reuse existing Move/empty admission without a competing raster owner.',
            'Four attack_production_cases bind the opt-in getter inputs from matched stationary/moving release captures and unselected controls. The same ActionFixture and complete Tactical batch owner execute, retaining the actual actor/Drive back-pointer rather than copying an Attack actor. Exact original IsMoving/current-speed checks match source17/target0 and source5/target17. All320 prior rows and earlier top-level values are retained; production_reference remains the earlier Move boundary.',
            'Attack production crops are192x160 inside the existing guarded64KiB surface. Original source6F3D60 and lead70BCB0 feed every unclipped endpoint/solid store. The captured fixed fraction19661/65536, direct native f32/f64 speed inputs, current105mm weapon and separate source/target headings remain explicit. Current input and native raster receipts do not substitute for a production pixel comparison.',
            'No-rocking source-basis evidence reuses native_slope.slope_matrices and compares its21 original matrices with the existing ordinary_collision packet. All21 stable pivot controls and one active slope-timer zero-angle control preserve the raw source origin; original646980 instruction stores establish zero translation after differing-slope quaternion conversion. Half sizes15.5/20.5 are supplied for the arithmetic control; VXL reading and actual moving-cell slope history are not claimed. The exact raw actor coordinates, zero offset and absent rocking remain the production-input boundary.',
        ],
        substitutions=[
            'Reuse original BSurface storage/lock/pitch methods instead of platform DirectDraw, through existing Rally fixture; original solid/fill leaves are unmodified.',
            'Existing PaletteReader replaces asset lookup and platform allocation only; original palette initialization/Convert routines execute and physical inputs are pinned in payload.',
            'Existing destination fixture substitutes only OS Interlocked increment/decrement; original class/COM/locomotor calls execute. No native gameplay instruction is patched or return value invented.',
        ],
        entry_points=dict(foot=0x4DC060, action_line=0x7049C0, restart_timer=0x70D150,
            timer_static=0x6F2AB0, load_timer=0x685167, tactical_gate=0x6D470D,
            human_player=0x50B6F0, planning=0x637AA0, cell_coords=0x486840,
            map_bounds=0x568300, ground_height=0x578080, unit_destination=0x741970,
            foot_destination=0x4D94B0, drive_move=0x4AFD40, solid_wrapper=0x7BA5E0,
            solid_line=0x7BA610, clip_line=0x7BC2B0, intersect_rect=0x421B60,
            fill_endpoint=0x7BB020))


def attack_prerequisite_metadata():
    return native.provenance(
        scope='Ordinary enemy MTNK object admission, actual Attack/Stop events, live Unit aim70BCB0 and distinct-argument FireAt6FDD50, concrete supplied-death pointer expiry, pivot/default and physical AP readers; bounded composition of existing Mission world',
        assumptions=[
            'Mission owns native constructors, physical MTNK/GTNK/105mm/Cannon/AP/country/Rules readers, retained crop map, COM factories, placement and observers. Its placement callback stops before the existing fixed Cell command. Original Object14-entry CRT table runs through object_flight_height.initialize_object_scalars; physical ElevationModel66D150 runs through each present layer before object action. These prerequisites avoid a cold-height airborne false result and a zero elevation divisor; neither native result is replaced.',
            'The source and target execute complete original Unit constructors and Unlimbo at physical cells87,50 and87,52. A second supplied House uses the existing physical Americans Country, index1/alliance2 and source alliance1. Full House constructor, bookkeeping and complete Scenario load are outside the boundary.',
            'Original Options defaults4E7E20 and Unit73FD50/Foot4DDED0/Techno6FFEC0 execute a no-modifier enemy-object query and return Attack5 through real Unit/Techno FireError. Existing EngineerJoinedFixture owns GetKeyState platform transport; all supplied key SHORTs are0. Full mouse pick/dispatch, selection generation and network queue are excluded.',
            'Attack Event4C6860/4C6CB0 uses native registered Unit IDs/tag34 and mission1. Fresh/repeated delivery, command setup, QueueMission, AssignTarget and null destination setter execute. Stop event4C65E0/4C6CB0 clears through the same original setters. Drawing timer restart belongs to the established Display caller and is not implicitly supplied by Event delivery.',
            'Target Unit741970 to real Cell87,53, speed-fraction4D3710(1), Facing4C9300 and veteran750090 execute on the retained actor. Direct70BCB0 queries use actual Unit/Drive/current-speed/current-weapon/Facing virtuals. No Process_Track tick, promotion points or crate producer runs. Missing current weapon is an explicit supplied control restored before FireAt.',
            'Each direct aim compares source/target actor and Drive storage and all recorded RNG states. The query is read-only and uses TarCom; null returnszero. Full FireAt separately uses its explicit physical Cell87,54 argument for launch distance/speed, then70BCB0 uses moving veteran Unit TarCom. The second FireAt retains that argument with null TarCom: aimzero executes, then placement failure removes the bullet. This is not a FireAt entry refusal.',
            'Full FireAt animation may consume Scenario RNG. The concrete Unit7446E0 expiry row supplies target health0/alive0 and active passive-scan timer, then executes inherited expiry and AssignTarget(NULL), including actual bounded/rejection RNG draws. Full damage, world detach traversal for target destruction and deferred target retirement remain outside this row; no query RNG is inferred from mutation phases.',
            'Original UnitType7470D0 and exact ART TurretOffset715876/ReadInt5276D0 prove constructor0, physical GTNK omission0, retained defaults/exact case and wrap controls. Original AP75E3B0/75CEC0 and full75D3A0 read physical RULESMD, absentLANGRULE, MPBattleMD and XMP03T4 in order, preserving Rocker/DirectRockerfalse. This bounds the current105mm/AP duel, not the broader rocking/damage mechanism.',
            'Physical map/crop classes, seed0 RNG state and retained original fixture boundaries do not constitute a whole game load or production capture. No pixels are compared in this prerequisite mode; prepared original surface comparisons remain attack_cases in action_lines.json.',
        ],
        substitutions=[
            'Inherited Mission/BulletReader supplies source-order INI caches, allocation/CRT/TLS/archive boundaries, selected voice/audio sinks, radar tracker sinks, OS COM activation to original Drive/Bullet factories, Interlocked/OleRun/IsBadReadPtr, empty SEH and setup wall clock. Native visual type loaders5F8110/5F8CE0 remain explicit excluded boundaries.',
            'The existing OS input transport supplies GetKeyState SHORT0 only; source actor/type/terrain/action/FireError/aim/speed/coordinate/detach return values are never substituted. All original code bytes remain unchanged.',
        ], entry_points=dict(unit_object_action=0x73FD50, unit_ctor=0x7353C0,
            unit_unlimbo=0x737BA0, object_crt_table=0x8141D8, elevation_reader=0x66D150,
            attack_event=0x4C6860, event_delivery=0x4C6CB0, assign_target=0x6FCDB0,
            unit_destination=0x741970, speed_fraction=0x4D3710, current_speed=0x4DB1A0,
            aim=0x70BCB0, current_weapon=0x70E1A0, weapon_speed=0x773070, fireat=0x6FDD50,
            stop_event=0x4C65E0, unit_expiry=0x7446E0, techno_expiry=0x7077C0,
            unit_type_ctor=0x7470D0, turret_offset_reader=0x715876,
            warhead_ctor=0x75CEC0, warhead_reader=0x75D3A0))


if __name__ == '__main__':
    prerequisites = '--attack-prerequisites' in sys.argv[1:]
    arguments = [arg for arg in sys.argv[1:] if arg != '--attack-prerequisites']
    sources = {name: Path(path) for name, path in {
            'producer': __file__, 'native_oracle': 'tools/native_oracle.py',
            'native_inspect': 'tools/native_inspect.py',
            'rally_fixture': 'tools/procedural_drawing_oracle/rally.py',
            'palette_fixture': 'tools/projectile_oracle/bridge_render_inputs_palette.py',
            'house_color_fixture': 'tools/procedural_drawing_oracle/house_color.py',
            'packed_palette_fixture': 'tools/palette_oracle/oracle.py',
            'render_inputs_fixture': 'tools/projectile_oracle/bridge_render_inputs.py',
            'archive_fixture': 'tools/sidebar_oracle/stock.py',
            'map_fixture': 'tools/spatial_oracle/bridge_damage_admission.py',
            'ini_registry_fixture': 'tools/rules_oracle/bridge_anim_inputs.py',
            'allocation_fixture': 'tools/rules_oracle/bridge_anim_lists.py',
            'ini_fixture': 'tools/spatial_oracle/building_body_rules.py',
            'crc_fixture': 'tools/projectile_oracle/flat_art.py',
            'lexical_fixture': 'tools/rules_oracle/bridge_child_sound.py',
            'destination_fixture': 'tools/spatial_oracle/track_destination.py',
            'unit_source_fixture': 'tools/spatial_oracle/unit_source_scatter.py',
            'unit_state_fixture': 'tools/spatial_oracle/unit_scatter_state.py',
            'unit_entry_fixture': 'tools/spatial_oracle/unit_entry.py',
            'map_query_fixture': 'tools/spatial_oracle/map_queries.py',
            'live_speed_fixture': 'tools/spatial_oracle/track_speed_native.py',
            'weapon_speed_fixture': 'tools/projectile_oracle/fireat_speed.py',
            'slope_fixture': 'tools/native_slope.py',
        }.items()}
    if prerequisites:
        # Resolve recurring dependencies before finish_vectors freezes their
        # identity. Every imported tools module is existing evidence ownership.
        from tools.spatial_oracle.anytown_damage import mission, mtnk_attack
        from tools.spatial_oracle import engineer_repair_admission, object_flight_height, fire_error
        from tools.rules_oracle import weapon_speed_order
        sources.update({name: Path(module.__file__) for name, module in tuple(sys.modules.items())
                        if name.startswith('tools.') and (getattr(module, '__file__', None) or '').endswith('.py')})
    native.finish_vectors(attack_prerequisites if prerequisites else generate,
        Path(__file__).with_name('action_lines_attack_prerequisites.json') if prerequisites
            else Path(__file__).with_suffix('.json'),
        provenance=attack_prerequisite_metadata if prerequisites else metadata,
        argv=arguments, source_paths=sources)
