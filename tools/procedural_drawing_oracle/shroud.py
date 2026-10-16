"""Original ordinary tactical A production from stock SHROUD.SHP.

The existing rally fixture owns native Surface/ABuffer setup; stock.mix owns
archive extraction. Native fill, SHP access, edge selection, projection, clips
and shroud writes execute without gameplay substitutions. Fog/AlphaShapes are
explicitly absent; supplied cell flags are not a reveal-traversal oracle.

The independent ``--health-hover`` mode reuses BuildingReveal to execute the
selected/hover health dispatch. Health pixels and selected bracket geometry
are declared boundaries; it does not regenerate ABuffer/reveal corpora.

``--building-brackets`` composes the existing type-reader, palette-startup and
packed Surface owners. Original GAPOWR geometry/projection and packed A/Z
pixels execute on either side of an explicit Surface command boundary.
"""
from functools import lru_cache
import hashlib
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                              UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI,
                              UC_X86_REG_ESP)

from tools import native_oracle as native
from tools.bridge_click_oracle import MATRIX_INITIALIZER
from tools.procedural_drawing_oracle import rally
from tools.sidebar_oracle import stock
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
        self.size, self.tactical = rally.SIZE, rally.TACTICAL
        self.abuffer, self.alpha_address, self.scratch = rally.ABUFFER, rally.ALPHA, SCRATCH
        self.invoke = lambda entry, receiver, args: call(u, entry, receiver, args)
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

    @classmethod
    def attach(cls, m, *, size, tactical, abuffer, alpha, cells):
        """Attach drawing to an existing native world, without enrolling flags.

        The caller owns the already initialized VM, actual Cells, physical
        resource transport and surface. No constructor or knowledge is replaced.
        """
        self = cls.__new__(cls)
        self.u, self.size, self.tactical = m.u, tuple(size), tactical
        self.abuffer, self.alpha_address = abuffer, alpha
        self.scratch, self.cells = m.alloc(0x200), cells
        self.invoke, self.scroll_rows = m.invoke, 0
        self.draws, self.cell_calls = [], []
        self.u.hook_add(UC_HOOK_CODE, self.observe)
        return self

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
        self.invoke(0x4112D0, self.abuffer, (127,))
        assert self.alpha() == bytes([127]) * (self.size[0]*self.size[1])
        self.draws, self.cell_calls = [], []

    def alpha(self):
        data = [v[0] for v in struct.iter_unpack('<H',self.u.mem_read(
            self.alpha_address,self.size[0]*self.size[1]*2))]
        assert max(data) <= 255
        offset = self.scroll_rows*self.size[0]
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
        native_output = self.draw_current(case)
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

    def draw_current(self, case, *, preserve_dirty=False):
        """Full native A drawing from current Cells; no supplied flag writes."""
        self.reset()
        if not preserve_dirty:
            dirty = case.get('dirty_cells',[[10,20]])
            self.u.mem_write(self.tactical+0xE0, words(len(dirty),*[self.cells[tuple(xy)] for xy in dirty]))
        self.u.mem_write(self.scratch+0x100,words(0,0,0,0))
        self.u.mem_write(self.scratch+0x120,words(0,0,*self.size))
        self.u.mem_write(SP,words(native.RET_MAGIC,self.scratch+0x100,self.scratch+0x100,
                                  self.scratch+0x120,int(case.get('full_redraw',True))))
        self.u.reg_write(UC_X86_REG_ESP,SP)
        self.u.reg_write(UC_X86_REG_ECX,self.tactical)
        native.run_checked(self.u,0x6D3660,native.RET_MAGIC,
                           count=case.get('instruction_limit',2_000_000),
                           timeout_us=case.get('timeout_us',10_000_000),
                           required_addresses=[0x6D3660,0x4801F0,0x47EFE0])
        return self.output()


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
        elif self.phase == 'health_hover':
            if pc == 0x6F539C:
                # The selected Building branch draws its cuboid before the
                # shared selected health call. Observe that boundary; this
                # corpus does not execute or certify bracket geometry.
                self.events.append(dict(kind='selected_bracket_geometry_boundary', pc=pc))
                u.reg_write(UC_X86_REG_EIP,0x6F5E17)
                u.reg_write(UC_X86_REG_ESI,self.RECT)
            elif pc == 0x6F64A0:
                sp=u.reg_read(UC_X86_REG_ESP)
                self.events.append(dict(kind='health_draw_boundary',pc=pc,
                    arguments=rally.ints(u,sp+4,3)))
                self.fixture_return(0,12)
            elif pc in (0x41C020,0x41BE70):
                self.events.append(dict(kind='original_disguise_query',pc=pc))
            elif pc in (0x586360,0x487950):
                self.events.append(dict(kind='shroud_query',pc=pc))

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

    def health_hover(self, case):
        self.prepare_visibility(dict(top_revealed=case['raw_mapped']))
        u=self.u
        original_vtable=bytes(u.mem_read(0x7E3EBC,0x600))
        for offset,key in ((0x83,'selected'),(0x431,'hovered'),
                           (0x1D8,'disguised'),(0x41B,'discovered')):
            u.mem_write(self.OBJECT+offset,bytes([case[key]]))
        u.mem_write(self.OBJECT+0x41A,b'\0')
        u.mem_write(self.OBJECT+0x99,b'\1')
        # This is the actual post-rank selected/hover block. Supply its two
        # incoming drawing arguments and ordinary nonsinking Building shell.
        u.mem_write(SP,bytes(0x100));u.mem_write(SP+0x9C,words(self.COORD))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_EBP,self.OBJECT)
        u.reg_write(UC_X86_REG_ESI,self.RECT)
        self.events,self.phase=[],'health_hover'
        native.run_checked(u,0x6F5388,0x6F5E8D,count=1000,
                           required_addresses=[0x6F5388,0x6F5E37])
        self.phase=None
        assert u.reg_read(UC_X86_REG_ESP)==SP
        assert bytes(u.mem_read(0x401000,4063232))==self.original_text
        assert bytes(u.mem_read(0x7E3EBC,0x600))==original_vtable
        return dict(name=case['name'],input={key:case[key] for key in
            ('selected','hovered','disguised','raw_mapped','discovered')},
            draw_health_bar=any(e['kind']=='health_draw_boundary' for e in self.events),
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


def generate_health_hover():
    physical=rally.stock_type_inputs()
    f=BuildingReveal(physical)
    base=dict(selected=False,hovered=True,disguised=False,
              raw_mapped=False,discovered=False)
    cases=[dict(base,name=name,**changes) for name,changes in (
        ('hover_unmapped_undiscovered',{}),
        ('hover_mapped_undiscovered',dict(raw_mapped=True)),
        ('hover_unmapped_discovered',dict(discovered=True)),
        ('no_selection_or_hover',dict(hovered=False)),
        ('selected_unmapped_undiscovered',dict(selected=True,hovered=False)),
        ('selected_and_hovered',dict(selected=True)),
        ('hover_disguised_building',dict(disguised=True)),
        ('selected_disguised_building',dict(selected=True,disguised=True)),
    )]
    return dict(controls=[f.health_hover(case) for case in cases],
        physical_foundation=physical,
        original_text_and_vtables_unchanged=True)


def health_hover_metadata():
    return native.provenance(
        scope='Original ordinary Building selected/hover health dispatch independent of raw anchor mapping or discovery; no status/bracket pixels or complete Tactical frame.',
        assumptions=[
            'Reuse Shroud/Rally/BuildingReveal original image, surface and supplied32x32 map owner. Its existing GAPILE constructor/Foundation prerequisite supplies only generic Building geometry; this corpus does not run CATIME01 rules or a whole retail scenario. The measured class-level dispatch reads no type identity, Insignificant, Nominal, Strength, Powered or alliance key.',
            'Original Building vtable7E3EBC has110=6F5190,44C=6F64A0,C8=41C020,D0=41BE70. Start6F5388 after ordinary DrawExtras rank work with supplied EBP Building, ESI clip and stack point argument. Actual selected+83 and hovered+431 branches, Building IsDisguisedTo+1D8 and null GetDisguiseHouse execute unchanged.',
            'The selected Building cuboid drawing at6F539C is an explicit skipped geometry boundary resuming at original selected health call6F5E17 with its incoming clip restored. Actual5399 push and5E2D pop balance. This is not an executed selection-bracket or whole selected DrawExtras comparison.',
            'Raw mapped-ground and discovered+41B zero/one histories are supplied, not produced by reveal traversal. Object+99=1 is a supplied earlier Tactical draw-opportunity latch. Static6D8F48 clears it,6D907E/6D9111 set it before body dispatch, and6D95D2 tests it before projected/viewport-clipped6D9789 virtual110; it is not a successful-pixel result or an IsShrouded gate.',
            'Static Display4AAEBC casts the resolved object,4AAED9 tests BuildingType InvisibleInGame+1701, then4AAEEC sets hover+431 outside Armageddon. Hover production, pick and visibility are not executed here. Ordinary selected/hover health dispatch contains no586360,487950,+41B or raw-cell check;487950 calls earlier in DrawExtras belong to bomb-clock/repair-wrench branches.',
            'A disguised Building C8=true and GetDisguiseHouse=NULL suppresses only the unselected hover arm; selected still draws. These explicit controls do not establish legal building disguise production. No RNG draw, timer write or detach call lies in this measured dispatch block.'],
        substitutions=[
            'Original selected Building bracket geometry is an observed skipped boundary6F539C->6F5E17; no bracket pixels are claimed.',
            'Original DrawHealthBar entry6F64A0 is a recorded RET12 terminal. No health ratio, pip count, positions, textures or pixels are executed or certified.'],
        entry_points=dict(extras=0x6F5190,selected_gate=0x6F5388,
            selected_geometry_boundary=0x6F539C,selected_health_call=0x6F5E27,
            hover_gate=0x6F5E37,hover_health_call=0x6F5E87,
            health_boundary=0x6F64A0,disguised=0x41C020,disguise_house=0x41BE70))


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


def building_bracket_geometry(palette_word):
    """Original GAPOWR readers/callers, stopping at the packed Surface entry.

    HutTypeReader owns construction, native INI caches and the loaded-image VM.
    This is a selected field/caller composition, not whole type loading or a
    Building instance constructor. No alternate geometry implementation exists.
    """
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.spatial_oracle.engineer_bridge_cursor_caller import (
        HutTypeReader, READER_INI, READER_SP,
    )

    m = HutTypeReader()
    u, typ = m.u, m.construct('GAPOWR')

    def fields():
        return dict(image=m.string(typ + 0x1F8), foundation=m.read32(typ + 0xEF0),
                    height=m.read32(typ + 0xEF4))

    constructor, readers, reads = fields(), [], []

    def read_observer(_u, pc, _size, _data):
        if pc in (0x528A10, 0x5276D0, 0x474DA0):
            sp = u.reg_read(UC_X86_REG_ESP)
            section, key, default = [m.read32(sp + n) for n in (4, 8, 12)]
            reads.append(dict(phase=m.phase, reader=f'{pc:08X}',
                              section=m.string(section), key=m.string(key),
                              default=m.string(default) if pc == 0x528A10 else default))

    read_hook = u.hook_add(UC_HOOK_CODE, read_observer)
    for filename, archive, raw in physical_layers():
        if raw is None:
            readers.append(dict(file=filename, archive=archive, absent=True))
            continue
        sections, _ = lexical(raw, {'GAPOWR'})
        selected = {name: {key: value for key, value in keys.items() if key == 'Image'}
                    for name, keys in sections.items()}
        m.make_ini(selected)
        m.phase = 'image_' + filename
        admitted = bool(m.invoke(0x526810, READER_INI, (typ + 0x24,)) & 255)
        mark = len(reads)
        if admitted:
            # Original ObjectType Image caller frame, after section admission.
            u.mem_write(READER_SP, words(native.RET_MAGIC))
            u.mem_write(READER_SP + 0x1B4, words(READER_INI))
            for reg, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, typ),
                               (UC_X86_REG_EBX, typ), (UC_X86_REG_ESI, READER_INI),
                               (UC_X86_REG_EDI, typ + 0x1F8)):
                u.reg_write(reg, value)
            native.run_checked(u, 0x5F92F9, 0x5F9340, count=200000,
                               required_addresses=(0x528A10,))
        readers.append(dict(file=filename, archive=archive, bytes=len(raw), sha256=sha(raw),
                            sections=selected, native_section_present=admitted,
                            reads=reads[mark:], image_after=fields()['image']))

    # ART is fixed, not another rules layer. Reuse stock's archive transport.
    yr = stock.mix((native.configured_gamemd().parent / 'ra2md.mix').read_bytes())
    local = stock.mix(yr[stock.mix_hash('localmd.mix')])
    raw = local[stock.mix_hash('artmd.ini')]
    sections, _ = lexical(raw, {'GAPOWR', fields()['image']})
    selected = {name: {key: value for key, value in keys.items()
                       if key in ('Height', 'OccupyHeight', 'Foundation')}
                for name, keys in sections.items()}
    m.make_ini(selected)
    m.phase, mark = 'fixed_art', len(reads)
    for begin, end in ((0x4610D8, 0x461112), (0x461225, 0x46125D)):
        u.mem_write(READER_SP, words(native.RET_MAGIC))
        for reg, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, typ),
                           (UC_X86_REG_EBX, typ + 0x24), (UC_X86_REG_ESI, READER_INI),
                           (UC_X86_REG_EDI, typ + 0x1F8)):
            u.reg_write(reg, value)
        native.run_checked(u, begin, end, count=200000)
    readers.append(dict(file='ARTMD.INI', archive='ra2md.mix/localmd.mix', bytes=len(raw),
                        sha256=sha(raw), sections=selected, reads=reads[mark:]))
    u.hook_del(read_hook)
    u.mem_write(0x89DDB8, words(104))  # declared native startup scalar prior
    out = m.alloc(12)
    m.invoke(0x464AF0, typ, (out,))
    type_result = dict(fields(), dimensions=rally.ints(u, out, 3))

    building, house, tactical, surface, convert, lookup, point, rect = (
        m.alloc(n) for n in (0x800, 0x6000, 0x200, 0x100, 0x200, 512, 8, 16))
    supplied = dict(location=[2688, 5248, 0], camera=[-384, 376], size=[160, 160],
                    clip=[0, 0, 160, 160], ground_relative_height=0, height_factor=104,
                    projection_factor_f64_bits='3FC25E5374344960')
    u.mem_write(building, words(0x7E3EBC))
    u.mem_write(building + 0x520, words(typ))
    u.mem_write(building + 0x9C, words(*supplied['location']))
    u.mem_write(building + 0x21C, words(house))
    u.mem_write(0xA83D4C, words(house))
    u.mem_write(tactical, words(0x7F4348))
    u.mem_write(tactical + 0xB0, words(*supplied['camera']))
    u.mem_write(0x887324, words(tactical))
    u.mem_write(surface, words(0x7E85D4))
    u.mem_write(0x887314, words(surface))
    u.mem_write(0xB0CD48, struct.pack('<Q', int(supplied['projection_factor_f64_bits'], 16)))
    u.mem_write(0x89E7C0, words(104))
    u.mem_write(convert + 4, words(2))
    u.mem_write(convert + 0x174, words(lookup))
    u.mem_write(lookup + 30, struct.pack('<H', palette_word))
    u.mem_write(0x87F6C4, words(convert))
    u.mem_write(rect, words(*supplied['clip']))
    u.mem_write(0x886FA0, words(*supplied['clip']))
    u.mem_write(point, words(0, 0))
    m.invoke(0x447AC0, building, (out,))
    type_result['center_coords'] = rally.ints(u, out, 3)
    passes, gate_controls, rng_entries, height_calls = [], [], [], []
    phase, segments, pending = '', [], None

    def geometry_observer(_u, pc, _size, _data):
        nonlocal pending
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x65C780, 0x65C7E0):
            rng_entries.append(dict(phase=phase, entry=f'{pc:08X}'))
        if pc == 0x5F5F40:
            height_calls.append(dict(phase=phase, supplied=0))
            m.ret(0, 0)
        elif pc == 0x6DBB60:
            a, b, color, write_z = rally.ints(u, sp + 4, 4)
            assert pending is None
            pending = dict(from_world=rally.ints(u, a, 3), to_world=rally.ints(u, b, 3),
                           packed_color=color, write_z=write_z)
        elif pc == 0x4BFD30:
            args = rally.ints(u, sp + 4, 7)
            assert pending is not None
            assert (args[3], args[6]) == (pending['packed_color'], pending['write_z'])
            segments.append(dict(pending, from_screen=rally.ints(u, args[1], 2),
                                 to_screen=rally.ints(u, args[2], 2),
                                 z_adjust=args[4:6], clip=rally.ints(u, args[0], 4)))
            pending = None
            m.ret(1, 28)  # command boundary; the existing pixel owner runs it below

    geometry_hook = u.hook_add(UC_HOOK_CODE, geometry_observer)
    for case, selected_flag, sinking in (('normal', True, False),
                                         ('deselected', False, False),
                                         ('sinking_control', True, True)):
        u.mem_write(building + 0x83, bytes([selected_flag]))
        u.mem_write(building + 0x3CD, bytes([sinking]))
        counts = {}
        for leaf, entry in (('behind', 0x6F60D0), ('extras', 0x6F5190)):
            phase, segments = case + '/' + leaf, []
            u.mem_write(READER_SP, words(native.RET_MAGIC, point, rect))
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            u.reg_write(UC_X86_REG_ECX, building)
            endpoint = native.run_checked(u, entry, (native.RET_MAGIC, 0x6F5E17),
                                          count=1000000)
            assert pending is None
            counts[leaf] = len(segments)
            if case == 'normal':
                passes.append(dict(name=leaf, entry=entry, endpoint=endpoint, segments=segments))
        if case != 'normal':
            gate_controls.append(dict(name=case, selected=selected_flag, sinking=sinking,
                                      segment_counts=counts))
    u.hook_del(geometry_hook)
    assert not rng_entries
    assert [len(row['segments']) for row in passes] == [10, 11]
    assert all(s['write_z'] == 0 for row in passes for s in row['segments'])
    m.unchanged()
    return dict(constructor=constructor, readers=readers, type_result=type_result,
                input=supplied, passes=passes, gate_controls=gate_controls,
                height_boundary_calls=height_calls, rng_entries=rng_entries,
                original_code_unchanged=True)


def building_bracket_pixels(geometry, case):
    """Execute original packed lines on LaserPixels -> Rally's existing A/Z VM."""
    from tools.spatial_oracle import building_prism

    supplied = dict(size=geometry['input']['size'], camera=geometry['input']['camera'],
                    clip=geometry['input']['clip'], background=rally.BACKGROUND,
                    alpha='clear', z=65535)
    supplied.update(case)
    f = building_prism.LaserPixels(supplied)
    u, (w, h) = f.u, f.surface.size
    if 'native_z_runs' in supplied:
        for x, y, length, value in supplied['native_z_runs']:
            assert 0 <= y < h and 0 <= x < x + length <= w
            u.mem_write(f.z_pixels + (y * w + x) * 2, struct.pack('<H', value) * length)
    if 'occluding_rect' in supplied:
        left, top, right, bottom = supplied['occluding_rect']
        z = [supplied['occluding_z'] if left <= x < right and top <= y < bottom else 65535
             for y in range(h) for x in range(w)]
        u.mem_write(f.z_pixels, struct.pack('<' + 'H' * len(z), *z))
    before_a = bytes(u.mem_read(rally.ALPHA, f.pixel_bytes))
    before_z = bytes(u.mem_read(f.z_pixels, f.pixel_bytes))
    stores, segments = [], []
    z_stores = []

    def store_observer(_u, _access, address, _size, value, _data):
        if rally.PIXELS <= address < rally.PIXELS + f.pixel_bytes:
            i = (address - rally.PIXELS) // 2
            stores.append([i % w, i // w, value & 65535])
        elif f.z_pixels <= address < f.z_pixels + f.pixel_bytes:
            i = (address - f.z_pixels) // 2
            z_stores.append([i % w, i // w, value & 65535])

    hook = u.hook_add(UC_HOOK_MEM_WRITE, store_observer)
    from_point, to_point = rally.MEM + 0x62000, rally.MEM + 0x62020
    for draw_pass in geometry['passes']:
        for index, segment in enumerate(draw_pass['segments']):
            u.mem_write(from_point, words(*segment['from_screen']))
            u.mem_write(to_point, words(*segment['to_screen']))
            begin = len(stores)
            f.call(building_prism.LASER_PACKED, rally.SURFACE, 0x886FA0,
                   from_point, to_point, segment['packed_color'],
                   *segment['z_adjust'], segment['write_z'])
            segments.append(dict(draw_pass=draw_pass['name'], index=index,
                                 store_range=[begin, len(stores)]))
    u.hook_del(hook)
    raw = bytes(u.mem_read(rally.PIXELS, f.pixel_bytes))
    assert before_a == bytes(u.mem_read(rally.ALPHA, f.pixel_bytes))
    assert before_z == bytes(u.mem_read(f.z_pixels, f.pixel_bytes))
    assert not z_stores and not f.rng_entries
    assert sha(u.mem_read(building_prism.TEXT_BEGIN, building_prism.TEXT_SIZE)) == f.text_sha256
    assert f.guard == bytes(u.mem_read(rally.PIXELS - 32, 32))
    assert f.guard == bytes(u.mem_read(rally.PIXELS + f.pixel_bytes, 32))
    return dict(name=supplied.pop('name'), input=supplied, segments=segments,
                stores=stores, pixels=[[i % w, i // w, word[0]]
                    for i, word in enumerate(struct.iter_unpack('<H', raw))
                    if raw[i * 2:i * 2 + 2] != f.before[i * 2:i * 2 + 2]],
                final_color_sha256=sha(raw), a_unchanged=True, z_unchanged=True,
                rng_entries=f.rng_entries, original_code_unchanged=True)


def building_bracket_ground():
    """Actual flat TMP + placed GAPOWR body depth, on the existing TMP VM.

    Only the native depth plane's row runs cross to the line-consumer control.
    Complete colors are hashed, not retained as a second body-render corpus.
    """
    from tools.projectile_oracle import bridge_render_inputs_palette as palette
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, SURFACE, PIXELS, Z, A
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.spatial_oracle import tmp_clear_render as tmp
    from tools.spatial_oracle.building_body_rules import RULES as INI_RULES, SP as READER_SP

    m, terrain, _files = tmp.prepare(tmp_names=('clear01.tem',))
    u, layers = m.u, physical_layers()
    sources = []
    for outer_name, inner_name, names in (
            ('ra2.mix', 'generic.mix', ('GGPOWR.SHP',)),
            ('ra2.mix', 'cache.mix', ('UNITTEM.PAL',)),
            ('ra2md.mix', 'conqmd.mix', ('BUILDNGZ.SHA',))):
        outer = (native.configured_gamemd().parent / outer_name).read_bytes()
        inner = stock.mix(outer)[stock.mix_hash(inner_name)]
        files = stock.mix(inner)
        for name in names:
            raw = files[stock.mix_hash(name)]
            m.assets[name] = raw
            sources.append(dict(name=name, archive=f'{outer_name}/{inner_name}',
                                bytes=len(raw), sha256=sha(raw)))
    m.assets['RULESMD.INI'] = layers[0][2]
    assert all(raw is None or not lexical(raw, {'Colors'})[0].get('Colors', {}).get('LightGold')
               for _name, _archive, raw in layers[1:])
    _palette_storage, color_scheme = palette.color_scheme_palette(
        owner=m, physical_assets=m.assets)
    m.invoke(0x45B1C0, 0)
    m.rules = m.alloc(0x5000)
    u.mem_write(0x8871E0, words(m.rules))
    m.invoke(0x665650, m.rules)
    m.invoke(0x4E7CF0, 0)
    typ = m.alloc(0x2000)
    m.invoke(0x45DD90, typ, (m.cstring('GAPOWR'),))
    art, _ = lexical(m.assets['ARTMD.INI'], {'GAPOWR', 'GAPOWR_A', 'GAPOWR_AD', 'GAPOWRMK'})
    m.make_ini(art)
    layer_reads = []
    for name, archive, raw in layers:
        if raw is None:
            layer_reads.append(dict(file=name, archive=archive, absent=True))
            continue
        sections, _ = lexical(raw, {'GAPOWR'})
        m.rules_cache(sections)
        mark = len(m.asset_loaded)
        admitted = m.invoke(0x45FE50, typ, (INI_RULES,)) & 255
        layer_reads.append(dict(file=name, archive=archive, sha256=sha(raw),
                                admitted=bool(admitted), loads=m.asset_loaded[mark:]))
    m.invoke(0x45E8F0, 0)  # original physical BUILDNGZ load and byte remap
    assert m.read32(typ + 0xA4) and m.read32(0x89DDBC)
    assert m.read32(typ + 0x1518) == 0  # no GAPOWR BibShape, no invented bib
    for entry in (0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
        m.invoke(entry, 0)
    u.mem_write(0x89DDB8, words(104))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    building, house = m.alloc(0x800), m.alloc(0x18000)
    m.invoke(0x43B740, building, (typ, 0))
    for offset in (0x21C, 0x14C):
        u.mem_write(building + offset, words(house))
    u.mem_write(house + 0x1EC, b'\1\1')
    u.mem_write(house + 0x16054, words(1))
    u.mem_write(0xA83D4C, words(house))
    u.mem_write(0xA8B238, words(1))
    u.mem_write(building + 0x9C, words(2688, 5248, 0))
    u.mem_write(building + 0xAC, words(5))
    u.mem_write(building + 0xB4, words(-1))
    u.mem_write(building + 0x74, b'\1')
    u.mem_write(building + 0x80, b'\1\0')
    u.mem_write(building + 0x83, b'\1')
    u.mem_write(building + 0x6E4, b'\1')
    m.invoke(0x44D6A0, building, (0, 0))
    table = m.alloc(512 * 512 * 4)
    u.mem_write(MAP + 0x13C, words(table, 512 * 512))
    u.mem_write(MAP + 0xF4, words(16, 16))
    cells = []
    for y in range(17, 26):
        for x in range(7, 16):
            cell = m.alloc(0x148)
            m.invoke(0x47BBF0, cell)
            u.mem_write(cell + 0x24, struct.pack('<hh', x, y))
            u.mem_write(cell + 0x38, words(0))
            u.mem_write(cell + 0x48, words(-1))
            u.mem_write(cell + 0x11A, bytes(4))
            u.mem_write(cell + 0x12C, words(0x18))
            u.mem_write(table + (y * 512 + x) * 4, words(cell))
            cells.append(cell)
    tactical, point, clip, coord, bounds = (m.alloc(n) for n in (0x200, 8, 16, 12, 16))
    u.mem_write(tactical, words(0x7F4348))
    u.mem_write(tactical + 0xB0, words(-384, 376))
    u.mem_write(0x887324, words(tactical))
    u.mem_write(0xB73550, words(1))
    u.mem_write(clip, words(0, 0, 160, 160))
    u.mem_write(0x886FA0, words(0, 0, 160, 160))
    u.mem_write(0xB0CE30, words(160, 160))
    prepare_surface(u, dict(depth_baseline=32768, old_z=65535, a_value=127,
                            background=rally.BACKGROUND), size=(160, 160))
    vtable = m.alloc(0x100)
    u.mem_write(vtable, bytes(u.mem_read(0x7E2070, 0x100)))
    u.mem_write(vtable + 0x34, words(0x4BFD30))
    u.mem_write(SURFACE, words(vtable))
    original_code = sha(u.mem_read(0x401000, 0x3E0000))
    alpha = bytes(u.mem_read(A, 51200))
    # Original flat bounds supply each Cell's draw point. Registration and
    # traversal are prepared; no host formula supplies a TMP pixel or depth.
    ground_cells = 0
    for cell in cells:
        m.invoke(0x47FF80, cell, (bounds,))
        x, y, w, h = rally.ints(u, bounds, 4)
        if x < 160 and y < 160 and x + w > 0 and y + h > 0:
            u.mem_write(READER_SP, words(native.RET_MAGIC, bounds, clip, 0))
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            u.reg_write(UC_X86_REG_ECX, cell)
            native.run_checked(u, 0x480350, native.RET_MAGIC, count=12000000, timeout_us=120000000)
            ground_cells += 1
    def snapshot(name):
        return dict(name=name, color_sha256=sha(u.mem_read(PIXELS, 51200)),
                    z_sha256=sha(u.mem_read(Z, 51200)))
    snapshots = [snapshot('ground')]
    phase, lines, draws, admissions, rng = '', [], [], [], []
    line_stores = []
    def finish_line():
        # The Surface kernel has early returns before its final RET. Finalize
        # at the next call/parent boundary so rejected lines retain zero stores.
        if lines and 'begin' in lines[-1]:
            lines[-1]['stores'] = len(line_stores) - lines[-1].pop('begin')
    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x65C780, 0x65C7E0):
            rng.append(dict(phase=phase, entry=pc))
        elif pc == 0x4BFD30:
            finish_line()
            args = rally.ints(u, sp + 4, 7)
            lines.append(dict(phase=phase, from_screen=rally.ints(u, args[1], 2),
                              to_screen=rally.ints(u, args[2], 2), z_adjust=args[4:6],
                              write_z=args[6], begin=len(line_stores)))
        elif pc == 0x43D290:
            args = rally.ints(u, sp + 4, 2)
            admissions.append(dict(point=rally.ints(u, args[0], 2), clip=rally.ints(u, args[1], 4)))
        elif pc == 0x4AED70:
            args = rally.ints(u, sp + 4, 14)
            draws.append(dict(frame=args[1], point=rally.ints(u, args[2], 2),
                              flags=args[4], z_adjust=args[6], gradient=args[7],
                              intensity=args[8], color=args[9], has_zshape=bool(args[10]),
                              zshape_frame=args[11], zshape_offset=args[12:14]))
    def store(_u, _access, address, _size, value, _data):
        if phase != 'body' and PIXELS <= address < PIXELS + 51200:
            i = (address - PIXELS) // 2
            line_stores.append([i % 160, i // 160, value & 65535])
    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, store)]
    for phase, entry in (('behind', 0x6F60D0), ('extras', 0x6F5190),
                          ('body', 0x43CEA0), ('late_extras', 0x6F5190)):
        if phase == 'body':
            m.invoke(entry, building, (clip, 1, 0))
        else:
            m.invoke(0x447AC0, building, (coord,))
            m.invoke(0x6D2140, tactical, (coord, point))
            u.mem_write(READER_SP, words(native.RET_MAGIC, point, clip))
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            u.reg_write(UC_X86_REG_ECX, building)
            native.run_checked(u, entry, (native.RET_MAGIC, 0x6F5E17), count=2000000)
        finish_line()
        snapshots.append(snapshot(phase))
    for hook in hooks:
        u.hook_del(hook)
    assert not rng and alpha == bytes(u.mem_read(A, 51200))
    assert original_code == sha(u.mem_read(0x401000, 0x3E0000))
    assert admissions == [dict(point=[84, 74], clip=[0, 0, 160, 160])]
    pixels = sorted({tuple(row) for row in line_stores}, key=lambda row: (row[1], row[0]))
    assert all(struct.unpack('<H', u.mem_read(PIXELS + (y * 160 + x) * 2, 2))[0] == value
               for x, y, value in pixels)
    depth = list(struct.unpack('<25600H', u.mem_read(Z, 51200)))
    runs = []
    for y in range(160):
        x = 0
        while x < 160:
            value, start = depth[y * 160 + x], x
            while x < 160 and depth[y * 160 + x] == value:
                x += 1
            if value != 65535:
                runs.append([start, y, x - start, value])
    return dict(native_z_runs=runs, physical_inputs=sources, type_layers=layer_reads,
                terrain_inputs=terrain, color_scheme=color_scheme, ground_cells=ground_cells,
                body_admission=admissions, body_draws=draws, bib_null=True,
                input=dict(size=[160, 160], camera=[-384, 376], location=[2688, 5248, 0],
                           placed_byte_6e4=True, selected=True, alpha=127, depth_baseline=32768,
                           initial_depth=65535, graphical_client=True, height_factor=104),
                snapshots=snapshots, line_calls=lines, final_line_pixels=pixels,
                a_unchanged=True, rng_entries=rng, original_code_unchanged=True)


def generate_building_brackets():
    from tools.procedural_drawing_oracle.action_lines import physical_palette

    row, palette_evidence = physical_palette()
    word = struct.unpack_from('<H', row, 30)[0]
    # The existing palette owner reports action-line index3; this caller uses15.
    palette_evidence['native_convert'].update(selected_index=15, selected_word=word)
    geometry = building_bracket_geometry(word)
    ground = building_bracket_ground()
    controls = [dict(name='clear'), dict(name='mixed_a', alpha='mixed'),
                dict(name='occluding_z', occluding_rect=[40, 40, 120, 120], occluding_z=32700),
                dict(name='native_ground_body_z', native_z_runs=ground.pop('native_z_runs'))]
    return dict(schema_version=1, native_sha256=native.image_sha256(), palette=palette_evidence,
                geometry=geometry, ground_occlusion=ground,
                raster_controls=[building_bracket_pixels(geometry, case) for case in controls])


def building_brackets_metadata():
    return native.provenance(
        scope='Ordinary selected GAPOWR original bracket geometry/projection and packed4BFD30 A/Z controls',
        assumptions=[
            'HutTypeReader owns the loaded original image, native allocation/INI cache fixture and complete BuildingType45DD90 constructor. Native HasSection526810 and Image caller5F92F9..5F9340 consume exact-case strings in selected physical RULESMD, optional loose LANGRULE, Battle MPBattleMD and XMP03T4 layers. Fixed ARTMD from ra2md.mix/localmd.mix supplies only Height/OccupyHeight/Foundation to original4610D8..461112 and461225..46125D. Physical archive transport/cache preparation, full type reader/Scenario and unrelated keys are excluded.',
            'Original Dimension2464AF0 derives dimensions from native Height/Foundation and a declared supplied startup height factor104. Prepared Building/House/type linkage, selected byte, location2688,5248,0, camera[-384,376], viewport160x160 and ordinary no-effect fields are priors. Original GetCoords, helper6F5EF0 and Tactical6DBB60 execute. Complete DrawBehind6F60D0 and DrawExtras6F5190 prefix stop6F5E17 before health; selection generation, placement, full parent/display scheduling and body raster do not execute.',
            'Existing action_lines.physical_palette/PaletteReader execute original palette initialization on physical ra2.mix/cache.mix ANIM.PAL and PALETTE.PAL. Global87F6C4 middle lookup row index15 supplies the packed word to the geometry VM; it is not a marker color. Original6DBB60 produces integer screen points, paired Z adjustments and writeZ0 from original world endpoints.',
            'The21 recorded normal Surface commands cross an explicit geometry-to-raster boundary into existing building_prism.LaserPixels->rally.Rally. Original4BFD30 runs with native BSurface lock/stride/unlock, supplied RGB565/A/Z planes, circular offset0 and native clip. No new runtime, INI parser, rasterizer, A/Z authority or packed-color formula produces expected pixels.',
            'Clear A127/Z65535, existing Rally mixedA pattern(0,1,63,127,255), and a supplied rectangle[40,40,120,120] Z32700 are consumer controls. The rectangle is synthetic depth, not a GAPOWR body/bib/BUILDNGZ result. stores are original ordered color writes; pixels are final row-major changed words. All calls writeZ0; original code, A and Z remain unchanged. No RNG entry is observed in these bounded drawing intervals.',
            'The fourth native_ground_body_z control instead consumes row runs from actual original flat ClearTile/TMP480350/547CF0 plus placed GAPOWR43CEA0->43D290->705E00->4AED70 body/shadow in the existing tmp_clear_render PaletteReader VM. The full native45FE50 reader consumes the physical layer stack; original45E8F0 loads/remaps physical BUILDNGZ. Existing color_scheme_palette constructs LightGold in this same owner. Native body point[84,74] comes from original virtual+AC459EF0 render coordinates, distinct from bracket center[84,104]; original455C20 bounds and normal graphical-client admission execute.',
            'That composed control supplies sparse constructed/flat/revealed Cells, one selected ClearTile registry, placement byte6E4=1, local House/color index1, viewport/camera, A127 and startup104. Original43B740/44D6A0/43EF90 own health/idle/frame; active Anim slots stay empty. Native GAPOWR BibShape isNULL. Whole Behind and pre-health Extras run before the body, then pre-health Extras again. All12 lower commands make zero color stores on this flat ground;132 roof pixels remain. Compact final native Z runs become an explicit input to the independent original packed-line/GPU consumer; this does not replace or establish GPU body raster parity.',
            'Deselected and prepared sinking controls exercise native gates; sinking is not claimed reachable by ordinary stock GAPOWR. Behind/initial Extras are separate command lists here. Native per-parent ordering, intervening building body, later Extras sweep, actual shroud/AlphaShape production, nearby overlapping bodies, GPU composition and a whole native frame remain separate validation gates.',
        ], substitutions=[
            'Inherited HutTypeReader supplies bounded allocation/delete/CRT TLS boundaries; physical asset loading in that VM is rejected. Original INI cache lookups and field parsers execute.',
            'GetHeight5F5F40 returns supplied ground-relative0. Geometry4BFD30 returns1 withRET1C after recording its actual seven arguments. Its full original body executes independently in the existing LaserPixels Surface runtime; this is not whole-native-frame composition.',
            'Existing PaletteReader physical file/palette transport and existing Rally memory-surface storage replace operating-system/archive I/O and DirectDraw. Native palette conversion, clipping, A/Z tests, packed modulation and stores execute unchanged.',
            'Composed ground/body mode reuses tmp_clear_render physical archives, native lazy TMP/RawFile transport, palette and Cell drawing owners. Sparse map enrollment and per-Cell traversal are supplied, not whole Map loading or Tactical scheduling. Original render/body/shape/packed-line calls execute without function-result substitutions. Full color/depth crop hashes are retained; bulk planes and screenshots are not corpus payloads.',
        ], entry_points=dict(type_ctor=0x45DD90, has_section=0x526810,
            image_begin=0x5F92F9, image_end=0x5F9340, art_height_begin=0x4610D8,
            art_height_end=0x461112, art_foundation_begin=0x461225,
            art_foundation_end=0x46125D, dimension2=0x464AF0, behind=0x6F60D0,
            extras=0x6F5190, extras_stop=0x6F5E17, bracket_pair=0x6F5EF0,
            tactical_line=0x6DBB60, packed_line=0x4BFD30, building_type_reader=0x45FE50,
            body_admission=0x43CEA0, body=0x43D290, render_coords=0x459EF0,
            render_bounds=0x455C20, techno_shape=0x705E00, shape=0x4AED70,
            building_zshape=0x45E8F0, cell_tile=0x480350, tmp=0x547CF0))


if __name__=='__main__':
    import sys
    if '--building-brackets' in sys.argv[1:]:
        from tools.procedural_drawing_oracle import action_lines
        from tools.spatial_oracle import building_prism, engineer_bridge_cursor_caller
        native.finish_vectors(generate_building_brackets,
            Path(__file__).with_name('building_brackets.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--building-brackets'],
            provenance=building_brackets_metadata,
            source_paths={'oracle':Path(__file__), 'surface_fixture':Path(rally.__file__),
                          'packed_surface_owner':Path(building_prism.__file__),
                          'type_reader_owner':Path(engineer_bridge_cursor_caller.__file__),
                          'palette_owner':Path(action_lines.__file__),
                          'palette_runtime_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                          'tmp_owner':Path('tools/spatial_oracle/tmp_clear_render.py'),
                          'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--frontier' in sys.argv[1:]:
        from tools.procedural_drawing_oracle import shroud_frontier
        native.finish_vectors(shroud_frontier.generate,
            Path(__file__).with_name('shroud_frontier.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--frontier'],
            provenance=shroud_frontier.metadata,
            source_paths={name:Path(__file__).parents[2]/path for name,path in {
                'frontier':'tools/procedural_drawing_oracle/shroud_frontier.py',
                'shroud':'tools/procedural_drawing_oracle/shroud.py',
                'mission':'tools/spatial_oracle/anytown_damage/mission.py',
                'mission_inputs':'tools/spatial_oracle/anytown_damage/mtnk_attack.py',
                'cell':'tools/spatial_oracle/trigger_action_values.py',
                'discovery':'tools/spatial_oracle/jumpjet_entry_discovery.py',
                'tmp':'tools/spatial_oracle/tmp_clear_render.py',
                'surface':'tools/projectile_oracle/bridge_render_shape.py',
                'palette':'tools/projectile_oracle/bridge_render_inputs_palette.py',
                'assets':'tools/spatial_oracle/bridge_shadow_render.py',
            }.items()})
        raise SystemExit(0)
    if '--health-hover' in sys.argv[1:]:
        native.finish_vectors(generate_health_hover,
            Path(__file__).with_name('health_hover.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--health-hover'],
            provenance=health_hover_metadata,
            source_paths={'oracle':Path(__file__),'surface_fixture':Path(rally.__file__),
                          'stock_owner':Path(stock.__file__),
                          'projection_fixture':Path('tools/bridge_click_oracle.py')})
        raise SystemExit(0)
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
