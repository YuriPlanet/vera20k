"""Original ordinary tactical A production from stock SHROUD.SHP.

The existing rally fixture owns native Surface/ABuffer setup; stock.mix owns
archive extraction. Native fill, SHP access, edge selection, projection, clips
and shroud writes execute without gameplay substitutions. Fog/AlphaShapes are
explicitly absent; supplied cell flags are not a reveal-traversal oracle.
"""
from functools import lru_cache
import hashlib
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                              UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                              UC_X86_REG_ESP)

from tools import native_oracle as native
from tools.bridge_click_oracle import MATRIX_INITIALIZER
from tools.procedural_drawing_oracle import rally
from tools.sidebar_oracle import stock
from tools.spatial_oracle import bridge_damage_admission as bridge_fixture
from tools.spatial_oracle.crate_speed_effect import VTABLES
from tools.spatial_oracle.bridge_damage_admission import call, words, MEM, TABLE, SP, MAP

SHP, SCRATCH, CELL_BASE = MEM + 0xB0000, MEM + 0xD0000, 0x32000000
NEIGHBORS = ((-1,-1,64),(0,-1,128),(1,-1,1),(-1,0,32),(1,0,2),
             (-1,1,16),(0,1,8),(1,1,4))


def sha(data):
    return hashlib.sha256(data).hexdigest()


@lru_cache(maxsize=1)
def asset():
    outer = (native.configured_gamemd().parent / 'ra2.mix').read_bytes()
    inner = stock.mix(outer)[stock.mix_hash('conquer.mix')]
    raw = stock.mix(inner)[stock.mix_hash('shroud.shp')]
    return raw, dict(archive='ra2.mix/conquer.mix/SHROUD.SHP',
                    outer_sha256=sha(outer), inner_sha256=sha(inner), sha256=sha(raw))


class Shroud:
    def __init__(self, case=None):
        self.case = case or {}
        self.rally = rally.Rally(dict(camera=self.case.get('camera', [-320,440])))
        self.u = u = self.rally.u
        self.draws, self.cell_calls = [], []
        raw, _identity = asset()
        u.mem_write(SHP, raw)
        u.mem_write(0x89E7C5, b'\1')  # already-loaded SHROUD/FOG startup cache
        u.mem_write(0x89E794, words(SHP))
        u.mem_write(0xA8B230, words(SCRATCH))
        u.mem_write(SCRATCH, words(0))  # ordinary FogOfWar=no
        u.mem_write(0xB73550, words(1))  # active graphical-client gate
        u.mem_write(0x88A100, words(0))  # no AlphaShape objects
        u.mem_write(0xB0CE88, words(0))  # no additional dirty rectangles
        u.mem_map(CELL_BASE, 0x100000)
        u.mem_write(MAP + 0xF4, words(16,16))
        self.cells = {}
        for y in range(32):
            for x in range(32):
                p = CELL_BASE + (y*32+x)*0x200
                self.cells[x,y] = p
                u.mem_write(p, words(0x7E4EEC))
                u.mem_write(p+0x24, struct.pack('<hh',x,y))
                u.mem_write(p+0x120, b'\xfe\xfe')
                u.mem_write(p+0x130, words(1,0,0,0))
                u.mem_write(p+0x38, words(-1))
                u.mem_write(TABLE+(y*512+x)*4, words(p))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, rally.TACTICAL)
        u.reg_write(UC_X86_REG_EBX, 0)
        native.run_checked(u, *MATRIX_INITIALIZER, count=100)
        self.scroll_rows = self.case.get('scroll_rows', 0)
        u.mem_write(rally.ABUFFER+0x10, words(self.scroll_rows*rally.SIZE[0]*2))
        u.mem_write(rally.ABUFFER+0x2C, words(rally.SIZE[1]))
        u.hook_add(UC_HOOK_CODE, self.observe)
        self.reset()

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x47EFE0:
            point, clip, frame = struct.unpack('<3I',u.mem_read(sp+4,12))
            self.draws.append(dict(point=rally.ints(u,point,2),
                                   clip=rally.ints(u,clip,4), frame=frame))
        elif pc == 0x4801F0:
            p = u.reg_read(UC_X86_REG_ECX)
            self.cell_calls.append(list(struct.unpack('<hh',u.mem_read(p+0x24,4))))

    def reset(self):
        call(self.u, 0x4112D0, rally.ABUFFER, (127,))
        assert self.alpha() == bytes([127]) * (rally.SIZE[0]*rally.SIZE[1])
        self.draws, self.cell_calls = [], []

    def alpha(self):
        data = [v[0] for v in struct.iter_unpack('<H',self.u.mem_read(
            rally.ALPHA,rally.SIZE[0]*rally.SIZE[1]*2))]
        assert max(data) <= 255
        offset = self.scroll_rows*rally.SIZE[0]
        return bytes(data[offset:]+data[:offset])

    def output(self):
        data = self.alpha()
        return dict(alpha_hex=data.hex(), alpha_sha256=sha(data),
                    draws=self.draws.copy(), cell_order=self.cell_calls.copy())

    def frame_rect(self, frame):
        call(self.u,0x69E7E0,SHP,(SCRATCH+0x180,frame))
        return rally.ints(self.u,SCRATCH+0x180,4)

    def leaf(self, case):
        self.reset()
        point, clip = case['point'], case.get('clip',[0,0,*rally.SIZE])
        self.u.mem_write(SCRATCH+0x100, words(*point))
        self.u.mem_write(SCRATCH+0x120, words(*clip))
        call(self.u,0x47EFE0,0,(SCRATCH+0x100,SCRATCH+0x120,case['frame']))
        return dict(input=case, frame_rect=self.frame_rect(case['frame']), **self.output())

    def set_flags(self, cell, flags):
        self.u.mem_write(self.cells[tuple(cell)]+0x12C,words(flags))

    def selector(self, mask, flags=0x18):
        center = (10,20)
        self.set_flags(center,flags)
        for dx,dy,bit in NEIGHBORS:
            self.set_flags((center[0]+dx,center[1]+dy),0 if mask&bit else 0x18)
        self.reset()
        self.u.mem_write(SCRATCH+0x100,words(40,30))
        self.u.mem_write(SCRATCH+0x120,words(0,0,*rally.SIZE))
        pointer = self.cells[center]
        call(self.u,0x4801F0,pointer,(SCRATCH+0x100,SCRATCH+0x120))
        return dict(flags=flags, mask=mask,
                    caches=list(struct.unpack('<bb',self.u.mem_read(pointer+0x120,2))),
                    frame=self.draws[0]['frame'], alpha_sha256=sha(self.alpha()))

    def scene(self, case):
        for xy in self.cells:
            self.set_flags(xy, case.get('default_flags',0))
        if (rect := case.get('revealed_rectangle')) is not None:
            for xy in self.cells:
                if rect[0]<=xy[0]<=rect[2] and rect[1]<=xy[1]<=rect[3]:
                    self.set_flags(xy,0x18)
        for override in case.get('cells',[]):
            self.set_flags(override['cell'],override['flags'])
            self.u.mem_write(self.cells[tuple(override['cell'])]+0x11B,
                             bytes([override.get('level',0)]))
        self.reset()
        dirty = case.get('dirty_cells',[[10,20]])
        self.u.mem_write(rally.TACTICAL+0xE0, words(len(dirty),*[self.cells[tuple(xy)] for xy in dirty]))
        self.u.mem_write(SCRATCH+0x100,words(0,0,0,0))
        self.u.mem_write(SCRATCH+0x120,words(0,0,*rally.SIZE))
        self.u.mem_write(SP,words(native.RET_MAGIC,SCRATCH+0x100,SCRATCH+0x100,
                                  SCRATCH+0x120,int(case.get('full_redraw',True))))
        self.u.reg_write(UC_X86_REG_ESP,SP)
        self.u.reg_write(UC_X86_REG_ECX,rally.TACTICAL)
        native.run_checked(self.u,0x6D3660,native.RET_MAGIC,count=2_000_000,
                           required_addresses=[0x6D3660,0x4801F0,0x47EFE0])
        native_output = self.output()
        # Replay the exact original per-cell calls in Rust's row order. No
        # Python pixel generator is used; original4801F0/47EFE0 perform stores.
        calls = list(zip(native_output['cell_order'],native_output['draws']))
        self.reset()
        for xy,draw in sorted(calls,key=lambda item:(item[0][1],item[0][0])):
            self.u.mem_write(SCRATCH+0x100,words(*draw['point']))
            self.u.mem_write(SCRATCH+0x120,words(0,0,*rally.SIZE))
            self.u.mem_write(SP,words(native.RET_MAGIC,SCRATCH+0x100,SCRATCH+0x120))
            self.u.reg_write(UC_X86_REG_ESP,SP)
            self.u.reg_write(UC_X86_REG_ECX,self.cells[tuple(xy)])
            native.run_checked(self.u,0x4801F0,native.RET_MAGIC,count=100_000)
        assert self.alpha().hex()==native_output['alpha_hex']
        result = dict(input=case, **native_output, row_major_full_plane_replay_equal=True)
        if case.get('rally'):
            self.rally.setup_building()
            self.rally.alpha = [v[0] for v in struct.iter_unpack('<H',self.u.mem_read(
                rally.ALPHA,rally.SIZE[0]*rally.SIZE[1]*2))]
            result['rally_passes'] = self.rally.draw_passes()
        return result


class BuildingReveal(Shroud):
    """Building admission/tint controls using the existing tactical fixture.

    Original 6D9920/43CEA0 decide admission. The render rectangle is a supplied
    geometry boundary; 43D290 is a recording terminal, not an executed raster.
    Original building-center and center-cell shroud helpers execute unchanged.
    """

    OBJECT, TYPE, RECT, DISPLAY, COORD, CELL_COORD = (
        MEM + offset for offset in (0xE0000, 0xE1000, 0xE3000, 0xE3100,
                                    0xE3200, 0xE3300))

    def __init__(self, physical_type):
        super().__init__()
        self.events = []
        self.phase = None
        # Existing native Map startup sequence derives the height divisor used
        # by 586360; do not replace its arithmetic with a host constant.
        for entry in (0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
            call(self.u, entry, rally.TACTICAL, ())
        self.level_height = rally.ints(self.u, 0xABDE88, 1)[0]
        self.u.mem_write(self.OBJECT, words(0x7E3EBC))
        self.u.mem_write(self.OBJECT + 0x520, words(self.TYPE))
        self.u.mem_write(self.TYPE + 0xEF0,
                         words(physical_type['result']['foundation']))
        self.u.mem_write(self.OBJECT + 0x9C, words(2688, 5248, 0))
        call(self.u, 0x447AC0, self.OBJECT, (self.COORD,))
        self.center_coords = rally.ints(self.u, self.COORD, 3)
        self.u.mem_write(self.COORD + 0x9C, words(*self.center_coords))
        call(self.u, 0x41BEA0, self.COORD, (self.CELL_COORD,))
        self.center_cell = list(struct.unpack('<hh', self.u.mem_read(self.CELL_COORD, 4)))
        call(self.u, 0x41BEA0, self.OBJECT, (self.CELL_COORD,))
        self.top_cell = list(struct.unpack('<hh', self.u.mem_read(self.CELL_COORD, 4)))
        assert self.top_cell != self.center_cell
        self.original_text = bytes(self.u.mem_read(0x401000, 4063232))
        self.u.hook_add(UC_HOOK_CODE, self.building_observe)

    def fixture_return(self, result, arg_bytes):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        target = struct.unpack('<I', self.u.mem_read(sp, 4))[0]
        self.u.reg_write(UC_X86_REG_EAX, result)
        self.u.reg_write(UC_X86_REG_ESP, sp + 4 + arg_bytes)
        self.u.reg_write(UC_X86_REG_EIP, target)

    def building_observe(self, u, pc, _size, _data):
        if self.phase == 'admission':
            if pc == 0x455C20:
                self.events.append(dict(kind='supplied_render_rectangle', pc=pc))
                self.fixture_return(self.RECT, 4)
            elif pc == 0x43CEA0:
                sp = u.reg_read(UC_X86_REG_ESP)
                self.events.append(dict(kind='draw_if_visible', pc=pc,
                                        arguments=rally.ints(u, sp + 4, 3)))
            elif pc == 0x43D290:
                sp = u.reg_read(UC_X86_REG_ESP)
                point, clip = struct.unpack('<2I', u.mem_read(sp + 4, 8))
                self.events.append(dict(kind='base_draw_terminal', pc=pc,
                                        point=rally.ints(u, point, 2),
                                        clip=rally.ints(u, clip, 4)))
                self.fixture_return(0, 8)
        elif self.phase == 'tint' and pc == 0x487950:
            cell = u.reg_read(UC_X86_REG_ECX)
            self.events.append(dict(kind='center_cell_shroud_query', pc=pc,
                                    cell=list(struct.unpack('<hh', u.mem_read(cell + 0x24, 4)))))

    def prepare_visibility(self, case):
        for xy in self.cells:
            self.set_flags(xy, 0)
        self.set_flags(self.top_cell, 0x18 if case.get('top_revealed') else 0)
        self.set_flags(self.center_cell, 0x18 if case.get('center_revealed') else 0)

    def admission(self, case):
        self.prepare_visibility(case)
        u = self.u
        u.mem_write(self.OBJECT + 0x74, bytes([case.get('active', True)]))
        u.mem_write(self.OBJECT + 0x80, bytes([1, case.get('limbo', False)]))
        u.mem_write(self.OBJECT + 0x6E7, bytes([case.get('is_fogged', False)]))
        u.mem_write(0xA8ED6B, bytes([case.get('armageddon', False)]))
        u.mem_write(self.RECT, words(*case.get('rectangle', [10, 0, 70, 70])))
        u.mem_write(self.DISPLAY, words(self.OBJECT))
        u.mem_write(0x8A0394, words(self.DISPLAY))
        u.mem_write(0x8A03A0, words(1))
        u.mem_write(SCRATCH + 0x100, words(0, 0, *rally.SIZE))
        self.events, self.phase = [], 'admission'
        call(u, 0x6D9920, rally.TACTICAL,
             (1, SCRATCH + 0x100, SCRATCH + 0x100))
        self.phase = None
        assert bytes(u.mem_read(0x401000, 4063232)) == self.original_text
        return dict(input=case, base_draw_called=any(
            event['kind'] == 'base_draw_terminal' for event in self.events),
            events=self.events.copy())

    def tint(self, case):
        self.prepare_visibility(case)
        self.events, self.phase = [], 'tint'
        u = self.u
        u.mem_write(SP + 0x80, words(case['packed_tint']))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, self.OBJECT)
        u.reg_write(UC_X86_REG_EBP, 1000)
        native.run_checked(u, 0x706389, 0x7063EB, count=20000,
                           required_addresses=[0x447AC0, 0x487950, 0x586360])
        self.phase = None
        assert bytes(u.mem_read(0x401000, 4063232)) == self.original_text
        return dict(input=case, packed_tint=u.reg_read(UC_X86_REG_EAX),
                    light_intensity=u.reg_read(UC_X86_REG_EBP),
                    events=self.events.copy())


def generate_building_reveal():
    physical = rally.stock_type_inputs()
    f = BuildingReveal(physical)
    admission_inputs = [
        dict(name='both_unexplored'),
        dict(name='center_revealed_top_unexplored', center_revealed=True),
        dict(name='top_revealed_center_unexplored', top_revealed=True),
        dict(name='both_revealed', top_revealed=True, center_revealed=True),
        dict(name='fogged_snapshot', is_fogged=True),
        dict(name='fogged_armageddon', is_fogged=True, armageddon=True),
        dict(name='inactive', active=False),
        dict(name='limbo', limbo=True),
        dict(name='offscreen', rectangle=[500, 500, 70, 70]),
    ]
    tint_inputs = [dict(name=f'{visibility}_tint_{value}',
                        top_revealed=top, center_revealed=center, packed_tint=value)
                   for visibility, top, center in (
                       ('both_unexplored', False, False),
                       ('center_only', False, True),
                       ('top_only', True, False), ('both_revealed', True, True))
                   for value in (0, 0xF800)]
    return dict(physical_type=physical, object_coords=[2688, 5248, 0],
                native_level_height=f.level_height,
                native_top_cell=f.top_cell, native_center_coords=f.center_coords,
                native_center_cell=f.center_cell,
                admission_cases=[f.admission(case) for case in admission_inputs],
                tint_cases=[f.tint(case) for case in tint_inputs],
                original_text_unchanged=True)


def stock_geometry():
    raw, identity = asset()
    w,h,frames = stock.shp(raw)
    masks=[]
    for frame in frames[:47]:
        masks.append({(frame['x']+i%frame['w'],frame['y']+i//frame['w'])
                      for i,v in enumerate(frame['pixels']) if v!=254})
    assert len({tuple(sorted(mask)) for mask in masks})==1
    overlaps=[]
    for dx,dy in ((30,15),(-30,15),(0,30),(60,0)):
        count=len(masks[0]&{(x+dx,y+dy) for x,y in masks[0]})
        assert count==0
        overlaps.append(dict(offset=[dx,dy],nontransparent_intersection=count))
    return dict(**identity,canvas=[w,h],frame_count=len(frames),
                active_frame_count=47,common_mask_pixels=len(masks[0]),
                frame15_values=sorted(set(frames[15]['pixels'])),
                active_values=sorted({v for f in frames[:47] for v in f['pixels']}),
                neighbor_mask_overlaps=overlaps)


def generate():
    f=Shroud()
    leaves=[f.leaf(dict(name=f'frame_{i}',frame=i,point=[40,30])) for i in range(47)]
    for frame in (0,15,33,46):
        for point in ([-60,-30],[-20,-10],[0,0],[130,100],[160,120]):
            leaves.append(f.leaf(dict(name=f'clip_{frame}_{point[0]}_{point[1]}',frame=frame,point=point)))
        leaves.append(f.leaf(dict(name=f'interior_clip_{frame}',frame=frame,
                                  point=[40,30],clip=[48,33,21,13])))
    for row in (1,119):
        wrapped=Shroud(dict(scroll_rows=row))
        leaves.append(wrapped.leaf(dict(name=f'circular_wrap_{row}',frame=15,
                                        point=[130,100],scroll_rows=row)))
    selectors=[f.selector(mask) for mask in range(256)]
    flag_controls=[f.selector(mask,flags) for flags in (0,0x08,0x10,0x18)
                   for mask in (0,1,0xAA,0xFF)]
    scene_inputs=[
        dict(name='full_clear',default_flags=0x18),
        dict(name='full_unrevealed',default_flags=0),
        dict(name='factory_frontier',revealed_rectangle=[0,0,12,31],rally=True),
        dict(name='island',revealed_rectangle=[9,18,12,21]),
        dict(name='frontier_camera_scroll',camera=[-331,437],revealed_rectangle=[0,0,12,31]),
        dict(name='frontier_circular_wrap',camera=[-289,451],scroll_rows=119,
             revealed_rectangle=[0,0,12,31]),
        dict(name='dirty_flat_cell',full_redraw=False,default_flags=0),
        dict(name='dirty_raised_cell_flat_A',full_redraw=False,default_flags=0,
             cells=[dict(cell=[10,20],flags=0,level=8)]),
        dict(name='dirty_partial_0x10_boundary',full_redraw=False,
             cells=[dict(cell=[10,20],flags=0x10)]),
    ]
    scenes=[Shroud(case).scene(case) for case in scene_inputs]
    assert scenes[-3]['alpha_hex']==scenes[-2]['alpha_hex']
    return dict(size=list(rally.SIZE),initial_value=127,world_y_bias=15,
                stock=stock_geometry(),leaf_cases=leaves,selector_cases=selectors,
                flag_controls=flag_controls,scenes=scenes)


def generate_wave_admission():
    """Original Wave DrawIt admission, stopping before its raster consumer.

    The active YR Map5865E0 body is a constant false, not an explored-cell
    query. Prepared endpoint cells are negative controls, not native reveal
    traversal or a replacement Python visibility implementation.
    """
    rows = []
    for scenario_fog_gate in (False, True):
        for wave_type, stop in ((0, 0x75FA47), (3, 0x75FA5C)):
            for source_flags, target_flags in ((0, 0), (0, 0x18),
                                                (0x18, 0), (0x18, 0x18)):
                u = bridge_fixture.base({})
                obj = MEM + 0x19000
                u.mem_write(0xA8B230, words(bridge_fixture.SCENARIO))
                u.mem_write(bridge_fixture.SCENARIO,
                            words(0x1000 if scenario_fog_gate else 0))
                u.mem_write(obj, words(0x7F6BF4))
                u.mem_write(obj + 0xB0, words(wave_type))
                source = [10 * 256 + 128, 20 * 256 + 128, 0]
                target = [9 * 256 + 128, 20 * 256 + 128, 0]
                u.mem_write(obj + 0xB4, words(*source))
                u.mem_write(obj + 0xC0, words(*target))
                u.mem_write(bridge_fixture.CELL + 0x12C, words(source_flags))
                u.mem_write(bridge_fixture.ANCHOR + 0x12C, words(target_flags))
                state_before = bytes(u.mem_read(obj, 0x200))
                rng_before = bytes(u.mem_read(bridge_fixture.SCENARIO + 0x218, 0x100))
                u.mem_write(SP, words(native.RET_MAGIC, 0, 0))
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_ECX, obj)
                visited = []
                hook = u.hook_add(UC_HOOK_CODE,
                    lambda _u, pc, _size, _data: visited.append(pc))
                try:
                    required = (0x75F9F0, 0x75FA29)
                    if scenario_fog_gate:
                        required += (0x5865E0, 0x75FA10)
                    boundary = native.run_checked(u, 0x75F9F0, stop,
                        count=120, required_addresses=required)
                finally:
                    u.hook_del(hook)
                if bytes(u.mem_read(obj, 0x200)) != state_before:
                    raise native.OracleError('Wave admission changed prepared object state')
                if bytes(u.mem_read(bridge_fixture.SCENARIO + 0x218, 0x100)) != rng_before:
                    raise native.OracleError('Wave admission changed prepared Scenario RNG')
                rows.append(dict(input=dict(scenario_fog_gate=scenario_fog_gate,
                    wave_type=wave_type, source=source, target=target,
                    source_raw_flags=source_flags, target_raw_flags=target_flags),
                    admitted=True, dispatch_boundary=f'{boundary:08X}',
                    fog_leaf_entries=visited.count(0x5865E0),
                    object_unchanged=True, scenario_rng_unchanged=True,
                    visited=[f'{pc:08X}' for pc in visited]))
    return dict(entry='0075F9F0', fog_leaf='005865E0',
                fog_leaf_bytes=native.file_span(native.image_bytes(), 0x5865E0, 5)[1].hex(),
                rows=rows)


class EntityReveal(Shroud):
    """Original Unit/Infantry/Aircraft DrawIfVisible; real DrawIt stop boundary.

    Shroud/Rally own mapped cells, original vtables, native camera scalar and
    surface setup. No mobile type/art or native draw body is substituted.
    """
    OBJECT, RECT = MEM + 0xE0000, MEM + 0xE3000

    def __init__(self, kind):
        super().__init__()
        self.kind = kind
        self.vtable = VTABLES[kind]
        self.entry = rally.ints(self.u, self.vtable + 0x104, 1)[0]
        self.body = rally.ints(self.u, self.vtable + 0x114, 1)[0]
        self.u.mem_write(self.OBJECT, words(self.vtable))
        self.u.mem_write(self.OBJECT + 0x9C, words(2688, 5248, 0))
        self.u.mem_write(self.OBJECT + 0x418, b'\0')  # Unit temporal target absent.
        # Same native retained viewport writer used by SetView and FullInit.
        self.u.mem_write(SP, words(native.RET_MAGIC, 0x886FA0))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ECX, rally.TACTICAL)
        native.run_checked(self.u, 0x6D5F60, 0x6D5F8D, count=1000)
        self.original_text = bytes(self.u.mem_read(0x401000, 4063232))
        self.original_vtable = bytes(self.u.mem_read(self.vtable, 0x600))

    def admission(self, case):
        u = self.u
        self.set_flags((10, 20), case.get('cell_bits', 0))
        u.mem_write(self.OBJECT + 0x74, bytes([case.get('marked', True)]))
        u.mem_write(self.OBJECT + 0x80,
                    bytes([case.get('redraw', True), case.get('limbo', False)]))
        u.mem_write(self.OBJECT + 0x90, words(case.get('alive', True)))
        u.mem_write(0xA8ED6B, b'\0')  # ordinary graphical client, no Armageddon.
        u.mem_write(rally.TACTICAL + 0xB0, words(*case.get('camera', [-320, 440])))
        u.mem_write(self.RECT, words(0, 0, *rally.SIZE))
        u.mem_write(SP, words(native.RET_MAGIC, self.RECT, 0, 0))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, self.OBJECT)
        events, cell_reads = [], []
        cell = self.cells[10, 20]

        def code(uc, pc, _size, _data):
            if pc in (self.entry, self.body, 0x5F4B10, 0x6D2140,
                       0x586360, 0x5865E0, 0x487950):
                events.append(pc)

        def read(uc, _access, address, size, _value, _data):
            if cell <= address < cell + 0x200:
                cell_reads.append(dict(pc=uc.reg_read(UC_X86_REG_EIP),
                                        offset=address-cell, size=size))

        h1 = u.hook_add(UC_HOOK_CODE, code)
        h2 = u.hook_add(UC_HOOK_MEM_READ, read)
        endpoint = native.run_checked(u, self.entry,
                                     (native.RET_MAGIC, self.body), count=20000)
        u.hook_del(h1)
        u.hook_del(h2)
        result = dict(input=case, class_name=self.kind, vtable=self.vtable,
                      entry=self.entry, body=self.body, endpoint=endpoint,
                      body_admitted=endpoint == self.body, events=events,
                      anchor_cell_reads=cell_reads)
        if endpoint == self.body:
            sp = u.reg_read(UC_X86_REG_ESP)
            point, rect = struct.unpack('<2I', u.mem_read(sp+4, 8))
            result.update(point=rally.ints(u, point, 2), clip=rally.ints(u, rect, 4))
        assert bytes(u.mem_read(0x401000, 4063232)) == self.original_text
        assert bytes(u.mem_read(self.vtable, 0x600)) == self.original_vtable
        return result


def generate_entity_reveal():
    cases = [dict(name=f'raw_anchor_{bits:02x}', cell_bits=bits)
             for bits in (0, 0x08, 0x18)]
    cases += [dict(name='not_redraw_ready', redraw=False),
              dict(name='limbo', limbo=True),
              dict(name='anchor_outside_projection', camera=[10000, 10000]),
              dict(name='dead_retained_member', alive=False),
              dict(name='unmarked_retained_member', marked=False)]
    rows = []
    for kind in ('unit', 'infantry', 'aircraft'):
        f = EntityReveal(kind)
        rows += [f.admission(case) for case in cases]
    return dict(object_coords=[2688, 5248, 0], anchor_cell=[10, 20],
                size=list(rally.SIZE), admission_cases=rows,
                original_text_and_vtables_unchanged=True)


def entity_reveal_metadata():
    return native.provenance(
        scope='Original Unit/Infantry/Aircraft DrawIfVisible anchor-shroud independence',
        assumptions=[
            'Shroud/Rally own mapped32x32cells, native projection startup scalar and graphicalclientB73550=1. Original6D5F60..6D5F8D viewport writer executes with supplied ordinary viewport; full window/camera clamp initialization excluded.',
            'Original class vtables from existing crate_speed_effect owner: Unit7F5C70, Infantry7EB058, Aircraft7E22A4. Their actual+104 entries execute to actual+114 body entries as stop boundaries; no body, vtable, or instruction is replaced.',
            'Prepared Object coordinate2688,5248,0 is anchor10,20 at cellcenter. Raw Cell+12C=0/0x08/0x18 contrasts do not claim reveal lifecycle; full scenario, retail map/INI/class constructors and art pixels are not measured.',
            'Ordinary redraw-ready, not-limbo and Unit+418=0 inputs exclude temporal-target ownership, cloak/warp/disguise and all body effects. Existing DrawState remains their Rustowner; effect parity does not follow from these gate controls.',
            'Object+90alive and+74marked negative controls are malformed retained Display members: original5F4B10 gate does not read them. Native lifecycle/Display cleanup must remain authoritative; they are not claims that dead objects remain registered in ordinary play.',
            'Original Tactical6D8F55..6D909E active Techno route uses projection, camera and optional5865E0 (active YRbodyalwaysfalse), not586360 or Cell+12C. Full Display scan and extra-draw scheduling are instruction-established here rather than executed.',
            'No RNG draw, timer write or detach call occurs in executed admission/projection boundaries. Object+80redraw is consumed; lifecycle cleanup/creation and full frame scheduling remain excluded.',
        ], substitutions=[],
        entry_points=dict(object_admission=0x5F4B10, unit_admission=0x73B0B0,
                          unit_body=0x73CEC0, infantry_body=0x518F90,
                          aircraft_body=0x4144B0, projection=0x6D2140,
                          viewport_writer=0x6D5F60))


if __name__=='__main__':
    import sys
    if '--entity-reveal' in sys.argv[1:]:
        native.finish_vectors(generate_entity_reveal,
            Path(__file__).with_name('entity_reveal.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--entity-reveal'],
            provenance=entity_reveal_metadata,
            source_paths={'oracle':Path(__file__),'surface_fixture':Path(rally.__file__),
                          'stock_owner':Path(stock.__file__),
                          'vtable_owner':Path('tools/spatial_oracle/crate_speed_effect.py'),
                          'projection_fixture':Path('tools/bridge_click_oracle.py')})
        raise SystemExit(0)
    if '--wave-admission' in sys.argv[1:]:
        native.finish_vectors(generate_wave_admission,
            Path(__file__).with_name('wave_admission.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--wave-admission'],
            provenance=lambda: native.provenance(
                scope='Original YR Wave75F9F0 admission through pre-raster type0/3 dispatch calls, including original constant-false5865E0',
                assumptions=[
                    'Existing bridge_damage_admission.base owns verified PE and prepared map/stack setup. The object is a supplied Wave-shaped instance, not an executed constructor or live Display registration.',
                    'Scenario bit0x1000, type0/3, endpoint coordinates and raw Cell+12C flags0/0x18 are explicit prepared inputs. These raw words are not assigned Rust visibility meanings; no reveal traversal or FogOfWar state producer executes.',
                    'Original75F9F0 executes unchanged until75FA47/75FA5C before the raster call. When bit0x1000 is set, original5865E0 executes once and its falseAL bypasses the second endpoint query. Every fixture saves the original instruction path.',
                    'Admission performs no RNG draw, timer write or detach call. Prepared Wave bytes and Scenario+218 RNG prefix remain unchanged; this is not the constructor/AI/lifetime or raster consumer chain.',
                    'No whole-object camera/rectangle admission, Display traversal, distortion pixels, retail weapon binding or rendered Wave parity is claimed. Existing white-pixel rendering remains a separate framebuffer-distortion residual.',
                ], substitutions=[],
                entry_points={'draw_it':0x75F9F0,'fog_leaf':0x5865E0,
                    'type0_raster_call_boundary':0x75FA47,
                    'type3_raster_call_boundary':0x75FA5C}),
            source_paths={'oracle':Path(__file__),
                          'map_fixture':Path(bridge_fixture.__file__)})
        raise SystemExit(0)
    if '--building-reveal' in sys.argv[1:]:
        native.finish_vectors(generate_building_reveal,
            Path(__file__).with_name('building_reveal.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--building-reveal'],
            provenance=lambda: native.provenance(
                scope='Ordinary Building6D9920->43CEA0 base-draw admission and706389..7063EB center-cell packed tint policy',
                assumptions=[
                    'Existing Shroud/Rally own the original loaded-image, cell grid, stock SHROUD asset and native map/surface fixture. No live native Scenario, whole map placement or asset-loader parity is claimed.',
                    'Original Map startup561710/5617A0/5617C0/5617E0 derivesABDE88 height divisor from original floating-point arithmetic. Zero-level cells isolate center selection from slope/bridge geometry.',
                    'The existing stock_type_inputs executes constructor and native retail GAPILE Foundation reader. Actual447AC0/45EC90/45ECA0 compute building center coordinates;41BEA0 computes both top and center cell coordinates. Inputs do not take their center cell from VERA.',
                    'Whole6D9920 and43CEA0 execute with one registered Building in the native display array, active graphical client, supplied rectangle and ordinary FogOfWar flag clear. +6E7 controls represent prepared IsFogged snapshot state, not an executed snapshot lifecycle.',
                    '706389..7063EB executes actual447AC0/487950/586360. It records EAX packed tint and unchanged EBP baseline light. This isolates the caller decision, not the full705E00 body or colored pixel rasterization. Packed tintF800 is an explicit synthetic nonzero input; neutral0 is the ordinary Building default.',
                    'Native .text remains byte-identical. Map cells use prepared flags and zero terrain level; reveal traversal, arbitrary slopes/bridges, enabled FogOfWar, observers and special effect producer parity are not established.',
                ],
                substitutions=[
                    '455C20 GetRenderDimensions returns a supplied rectangle and applies its native RET4 stack convention, isolating geometry from admission.',
                    '43D290 base DrawIt is a recording terminal with RET8 convention. Admission, clip intersection, render coordinates and projection are native; no building asset raster executes.',
                ],
                entry_points={'building_scan':0x6D9920,'draw_if_visible':0x43CEA0,
                    'base_draw_terminal':0x43D290,'center_coords':0x447AC0,
                    'coord_to_cell':0x41BEA0,'center_shroud':0x487950,
                    'shrouded_coord':0x586360,'tint_begin':0x706389,
                    'tint_end_exclusive':0x7063EB,'height_scale':0x5617E0}),
            source_paths={'oracle':Path(__file__),'surface_fixture':Path(rally.__file__),
                          'stock_owner':Path(stock.__file__),
                          'projection_fixture':Path('tools/bridge_click_oracle.py')})
        raise SystemExit(0)
    native.finish_vectors(generate,Path(__file__).with_suffix('.json'),
        provenance=lambda:native.provenance(
            scope='Stock SHROUD ABuffer values; original4112D0 reset,47EFE0 blit,4801F0/6D8700 selectors and whole6D3660 ordinary dirty/full draw including6D71E0',
            assumptions=[
                'Retail ra2.mix/conquer.mix/SHROUD.SHP raw bytes and container hashes are recorded. Existing stock.mix decodes archives. No full native archive loader or mod/loose override resolver executes; production asset tests must establish the same SHA identity.',
                'rally.Rally owns original BSurface/ABuffer setup. Original4112D0 resets every fixture to127. Whole47EFE0 reads stock raw format0 frames through original69E7E0/69E740, clips, obtains circular rows4114B0 and stores bytes except254. Physical circular offset is supplied for two leaf controls and one full scene.',
                'Whole4801F0/6D8700 executes all256neighbor masks with center flags0x18;16separate flags0/0x08/0x10/0x18 controls record caches and frame choice. Cell flags/counters/table are supplied, not an executed reveal traversal. Partial0x10 controls establish the consumer boundary, not ordinary reachability.',
                'Whole6D3660 executes ordinary full-redraw6D71E0 or dirty-cell projection. Original matrix constructor stores6D1DC5..6D1E1E execute. Cell table32x32 and Size16x16 are supplied; reported scenes stay inside the ordinary map diamond. Native integer camera/clip points are saved; Rust world camera adds the existing15pixel Y bias. Fractional zoom is a separate presentation extension.',
                'Scenario FogOfWar flag isclear; active graphical-client gate is1; AlphaShape list and extra dirty-rectangle list areempty. Dynamic AlphaShapes, enabledFog, startup loader and incremental invalidation/scroll lifecycle are outside this common route.',
                'Row-order/full-plane equivalence replays exact native cell calls using original4801F0/47EFE0 in ry/rx order and a full-plane clip; no alternate Python rasterizer generates the reference. All47selected stock masks are disjoint on the flat lattice. Supplied factory rally fields reuse rally.Rally; one scene feeds actual original A output to6DA9D0/4C0750.',
            ], substitutions=['The optional visible rally surface inherits original BSurface storage/locking in place of DirectDraw from rally.Rally; original shroud ABuffer calls and gameplay leaves are not replaced.'],
            entry_points={'fill':0x4112D0,'shroud_blit':0x47EFE0,'cell_edges':0x4801F0,
                'selector':0x6D8700,'tactical':0x6D3660,'full_scan':0x6D71E0,
                'frame_rect':0x69E7E0,'frame_data':0x69E740,'circular_row':0x4114B0,
                'cell_center':0x480A30,'projection':0x6D1F10}),
        source_paths={'oracle':Path(__file__),'surface_fixture':Path(rally.__file__),
                      'stock_owner':Path(stock.__file__),
                      'projection_fixture':Path('tools/bridge_click_oracle.py')})
