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

``--unit-health`` reuses PaletteReader and the shared shape Surface transport
for ordinary MTNK Extras/health/pip-tail and physical PIPBRD/PIPS color stores.
The additive ``--building-health`` mode borrows the existing native ground/body
scene and same health recorder for GAPOWR pips and both Extras phases.
``--unit-rank`` extends that owner with original rank setters, observer gates,
rank chevrons and rank-before-health ordering.
``--ammo-pips`` uses the same mobile-prefix/recorder owner for Aircraft/Fly,
ordinary ORCA ammo, actual receiver rearm and shared708C30 numeric controls.
"""
from functools import lru_cache
import hashlib
import json
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                              UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                              UC_X86_REG_ESP, UC_X86_REG_FPCW)

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
        admitted = bool(m.invoke(0x526810, READER_INI, (typ + 0x24,)))
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


def _prepare_ground_building(m, building, typ, house, location=(2688,5248,0), selected=True):
    """Native constructor/idle plus explicit placed-object consumer priors."""
    u = m.u
    m.invoke(0x43B740, building, (typ, 0))
    for offset in (0x21C, 0x14C):
        u.mem_write(building + offset, words(house))
    u.mem_write(house + 0x1EC, b'\1\1')
    u.mem_write(house + 0x16054, words(1))
    u.mem_write(0xA83D4C, words(house))
    u.mem_write(0xA8B238, words(1))
    u.mem_write(building + 0x9C, words(*location))
    u.mem_write(building + 0xAC, words(5))
    u.mem_write(building + 0xB4, words(-1))
    u.mem_write(building + 0x74, b'\1')
    u.mem_write(building + 0x80, b'\1\0')
    u.mem_write(building + 0x83, bytes([selected]))
    u.mem_write(building + 0x6E4, b'\1')
    m.invoke(0x44D6A0, building, (0, 0))


def _prepare_flat_cells(m, xs, ys):
    """Sparse constructed flat/revealed Cell priors shared by draw consumers."""
    u = m.u
    table = m.alloc(512 * 512 * 4)
    u.mem_write(MAP + 0x13C, words(table, 512 * 512))
    u.mem_write(MAP + 0xF4, words(16, 16))
    cells = []
    for y in ys:
        for x in xs:
            cell = m.alloc(0x148)
            m.invoke(0x47BBF0, cell)
            u.mem_write(cell + 0x24, struct.pack('<hh', x, y))
            u.mem_write(cell + 0x38, words(0))
            u.mem_write(cell + 0x48, words(-1))
            u.mem_write(cell + 0x11A, bytes(4))
            u.mem_write(cell + 0x12C, words(0x18))
            u.mem_write(table + (y * 512 + x) * 4, words(cell))
            cells.append(cell)
    return table, cells


def _building_ground_runtime(*, type_name='GAPOWR', render_ground=True, wrench=False):
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
    m.invoke(0x45DD90, typ, (m.cstring(type_name),))
    constructor_fields = _building_health_fields(m, typ, repair=wrench)
    if not render_ground:
        constructor_fields.update(max_number_occupants=m.read32(typ+0x1580),
                                  show_occupant_pips=bool(u.mem_read(typ+0x1584,1)[0]))
    type_layer_fields = []
    art, _ = lexical(m.assets['ARTMD.INI'],
                     {type_name,type_name+'_A',type_name+'_AD',type_name+'MK'})
    m.make_ini(art)
    layer_reads = []
    for name, archive, raw in layers:
        if raw is None:
            layer_reads.append(dict(file=name, archive=archive, absent=True))
            continue
        sections, _ = lexical(raw, {type_name})
        m.rules_cache(sections)
        mark = len(m.asset_loaded)
        admitted = m.invoke(0x45FE50, typ, (INI_RULES,)) & 255
        layer_reads.append(dict(file=name, archive=archive, sha256=sha(raw),
                                admitted=bool(admitted), loads=m.asset_loaded[mark:]))
        type_layer_fields.append(dict(file=name, after=_building_health_fields(m, typ, repair=wrench)))
        if not render_ground:
            type_layer_fields[-1]['after'].update(max_number_occupants=m.read32(typ+0x1580),
                show_occupant_pips=bool(u.mem_read(typ+0x1584,1)[0]))
    if render_ground:
        m.invoke(0x45E8F0, 0)  # original physical BUILDNGZ load and byte remap
        assert m.read32(typ + 0xA4) and m.read32(0x89DDBC)
        assert m.read32(typ + 0x1518) == 0  # no GAPOWR BibShape, no invented bib
    for entry in (0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
        m.invoke(entry, 0)
    u.mem_write(0x89DDB8, words(104))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    building, house = m.alloc(0x800), m.alloc(0x18000)
    _prepare_ground_building(m, building, typ, house)
    table, cells = _prepare_flat_cells(m, range(7,16), range(17,26))
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
    for cell in cells if render_ground else ():
        m.invoke(0x47FF80, cell, (bounds,))
        x, y, w, h = rally.ints(u, bounds, 4)
        if x < 160 and y < 160 and x + w > 0 and y + h > 0:
            u.mem_write(READER_SP, words(native.RET_MAGIC, bounds, clip, 0))
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            u.reg_write(UC_X86_REG_ECX, cell)
            native.run_checked(u, 0x480350, native.RET_MAGIC, count=12000000, timeout_us=120000000)
            ground_cells += 1
    return dict(m=m,u=u,typ=typ,building=building,house=house,tactical=tactical,
                point=point,clip=clip,coord=coord,vtable=vtable,sources=sources,
                layer_reads=layer_reads,terrain=terrain,color_scheme=color_scheme,
                ground_cells=ground_cells,original_code=original_code,alpha=alpha,
                constructor_fields=constructor_fields,type_layer_fields=type_layer_fields,
                cells=cells)


def building_bracket_ground():
    """Legacy bracket control, borrowing the single ground/body runtime."""
    from tools.projectile_oracle.bridge_render_shape import PIXELS, Z, A
    from tools.spatial_oracle.building_body_rules import SP as READER_SP
    scene = _building_ground_runtime()
    m, u, building, tactical, coord, point, clip = (
        scene[key] for key in ('m', 'u', 'building', 'tactical', 'coord', 'point', 'clip'))
    sources, layer_reads, terrain, color_scheme, ground_cells, original_code, alpha = (
        scene[key] for key in ('sources', 'layer_reads', 'terrain', 'color_scheme',
                              'ground_cells', 'original_code', 'alpha'))
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


def _building_health_fields(m, typ, *, repair=False):
    result=dict(strength=m.read32(typ + 0xA0), delta=m.read32(typ + 0x3E0),
                height=m.read32(typ + 0xEF4), foundation=m.read32(typ + 0xEF0),
                pip=m.read32(typ + 0x3D0), pip_scale=m.read32(typ + 0x3D4),
                pips_draw_for_all=bool(m.u.mem_read(typ + 0x3D8, 1)[0]),
                can_be_occupied=bool(m.u.mem_read(typ + 0x157B, 1)[0]),
                spawns_number=m.read32(typ + 0xD5C))
    if repair:
        result.update(cost=m.read32(typ+0x610),repairable=bool(m.u.mem_read(typ+0xCCC,1)[0]),
                      click_repairable=bool(m.u.mem_read(typ+0x157A,1)[0]))
    return result


class _HealthRecorder:
    """One observer for ordinary health and phase-aware native composition."""
    def __init__(self, m, size, shape_names, *, phase_aware=False, rank=False, scales=False, group_status=False, wrench=False):
        from tools.projectile_oracle.bridge_render_shape import PIXELS, Z, A
        u = m.u
        self.stores, self.draws, self.entries, self.rng = [], [], [], []
        self.line_calls, self.body_admissions = [], []
        self.gates, self.callbacks = [], []
        self.numeric=[]
        self.actor_index, self.phase = -1, 'health'
        self.return_pc, self.active = None, False
        def finish_draw():
            if self.draws and 'begin' in self.draws[-1]:
                self.draws[-1]['store_range'] = [self.draws[-1].pop('begin'), len(self.stores)]
        def observe(_u, pc, _size, _data):
            if phase_aware and self.return_pc == pc:
                finish_draw()
                self.active = False
                self.return_pc = None
            if pc in (0x65C780, 0x65C7E0):
                self.rng.append(pc)
            if wrench:
                if pc in (0x487950,0x586360,0x459EF0,0x5FB2E0):
                    self.callbacks.append(dict(actor=self.actor_index,entry=f'{pc:08X}'))
                if pc==0x6F52AF:
                    self.gates.append(dict(actor=self.actor_index,gate='cell_shrouded',
                                           result=u.reg_read(UC_X86_REG_EAX)&255))
            if rank or scales:
                if pc in (0x746750,0x5227F0,0x4DA4E0,0x55ABC0,0x703860,
                          0x5657A0,0x4870D0,0x4870F0,0x4F9A50):
                    self.callbacks.append(dict(actor=self.actor_index,entry=f'{pc:08X}'))
                if pc in (0x6F535F,0x6F536F):
                    value = u.reg_read(UC_X86_REG_EAX)
                    self.gates.append(dict(actor=self.actor_index,
                        gate='disguised_to' if pc==0x6F535F else 'visual_character',
                        result=value & 255 if pc==0x6F535F else value))
            if scales and pc in (0x70A26C,0x70A29C):
                value=struct.unpack('<i',words(u.reg_read(UC_X86_REG_EAX)))[0]
                self.numeric.append(dict(actor=self.actor_index,
                    result='ore_pips' if pc==0x70A26C else 'gem_pips',value=value,
                    fpcw=u.reg_read(UC_X86_REG_FPCW)))
            if (pc in (0x6F5190, 0x6F64A0, 0x709A90, 0x70AA60)
                    or ((rank or scales) and pc==0x70A990)
                    or (scales and pc in (0x716290,0x473460,0x473450,0x708C30,
                                          0x4722D0,0x4726C0,0x6C9650,0x6C9680))):
                self.entries.append(dict(actor=self.actor_index, entry=f'{pc:08X}'))
                if phase_aware:
                    self.entries[-1]['phase'] = self.phase
            if phase_aware and pc == 0x4BFD30:
                self.line_calls.append(dict(phase=self.phase, actor=self.actor_index))
            if phase_aware and pc == 0x43D290:
                args = rally.ints(u, u.reg_read(UC_X86_REG_ESP) + 4, 2)
                self.body_admissions.append(dict(phase=self.phase, actor=self.actor_index,
                    point=rally.ints(u,args[0],2), clip=rally.ints(u,args[1],4)))
            if pc == 0x4AED70:
                finish_draw()
                args = rally.ints(u, u.reg_read(UC_X86_REG_ESP) + 4, 14)
                self.active = True
                self.return_pc = m.read32(u.reg_read(UC_X86_REG_ESP))
                self.draws.append(dict(actor=self.actor_index, asset=shape_names[args[0]], frame=args[1],
                                  point=rally.ints(u, args[2], 2), clip=rally.ints(u, args[3], 4),
                                  flags=args[4], remap=args[5], z_adjust=args[6], gradient=args[7],
                                  brightness=args[8], color=args[9], begin=len(self.stores),
                                  a_reads=0, z_reads=0, a_writes=0, z_writes=0, leaves=[]))
                if phase_aware:
                    self.draws[-1].update(phase=self.phase, has_zshape=bool(args[10]),
                        zshape_frame=args[11], zshape_offset=args[12:14])
            elif (pc in (0x493DF0, 0x4912B0, 0x4990E0, 0x497390)
                  or ((scales or group_status) and pc==0x4960A0)) and self.active:
                vt = m.read32(u.reg_read(UC_X86_REG_ECX))
                leaf = dict(entry=f'{pc:08X}', vtable=f'{vt:08X}')
                if leaf not in self.draws[-1]['leaves']:
                    self.draws[-1]['leaves'].append(leaf)
        def read(_u, _access, address, _size, _value, _data):
            if self.draws and (self.active or not phase_aware):
                if A <= address < A + size[0] * size[1] * 2:
                    self.draws[-1]['a_reads'] += 1
                elif Z <= address < Z + size[0] * size[1] * 2:
                    self.draws[-1]['z_reads'] += 1
        def write(_u, _access, address, length, value, _data):
            if PIXELS <= address < PIXELS + size[0] * size[1] * 2:
                for offset in range(0, length, 2):
                    index = (address + offset - PIXELS) // 2
                    self.stores.append([index % size[0], index // size[0],
                                   (value >> (offset * 8)) & 65535])
            elif self.draws and (self.active or not phase_aware):
                if A <= address < A + size[0] * size[1] * 2:
                    self.draws[-1]['a_writes'] += 1
                elif Z <= address < Z + size[0] * size[1] * 2:
                    self.draws[-1]['z_writes'] += 1
        self.finish = finish_draw
        self.u = u
        self.hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_READ, read),
                      u.hook_add(UC_HOOK_MEM_WRITE, write)]

    def close(self):
        self.finish()
        for hook in self.hooks:
            self.u.hook_del(hook)


def _prepare_unit_rank(m, typ, actor, house, *, locomotor_ctor=0x4AF540, aircraft=False):
    """Existing original mobile/Foot prefix and locomotor constructor, in this VM.

    CoCreate/whole typed Unit constructor and Unlimbo are excluded priors.
    No visual-character or observer predicate result is supplied.
    """
    from tools.spatial_oracle.building_body_rules import SP as READER_SP
    u = m.u
    scenario = m.read32(0xA8B230)
    rngs = (0x886B88,scenario+0x218,0xABE890)
    for stream in rngs:
        m.invoke(0x65C6D0,stream,(0,))
    u.mem_write(0xB0F720,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    requests=[]
    def observe_constructor(_u,pc,_size,_data):
        if pc in (0x65C780,0x65C7E0):
            sp=u.reg_read(UC_X86_REG_ESP)
            receiver=u.reg_read(UC_X86_REG_ECX)
            requests.append(dict(entry=f'{pc:08X}',caller=f'{m.read32(sp):08X}',
                stream=('main','scenario','mapgen')[rngs.index(receiver)]
                       if receiver in rngs else 'other',
                bounds=rally.ints(u,sp+4,2) if pc==0x65C7E0 else []))
    ctor_hook=u.hook_add(UC_HOOK_CODE,observe_constructor)
    # Existing astar_mtnk_inputs constructor boundary: stop BEFORE CoCreate.
    u.mem_write(READER_SP,words(native.RET_MAGIC,typ,0))
    u.reg_write(UC_X86_REG_ESP,READER_SP)
    u.reg_write(UC_X86_REG_ECX,actor)
    begin,end = (0x413D20,0x413DB1) if aircraft else (0x7353C0,0x7354CE)
    native.run_checked(u,begin,end,count=300000,
                       required_addresses=(0x4D31E0,0x4D3217,0x4D321D))
    locomotor = m.alloc(0x100)
    m.invoke(locomotor_ctor,locomotor)
    u.hook_del(ctor_hook)
    u.mem_write(locomotor+0xC,words(actor))
    u.mem_write(locomotor+0x14,words(1))
    u.mem_write(actor+0x674,words(locomotor+4))
    enemy = m.alloc(0x18000)
    u.mem_write(house+0x30,words(0))
    u.mem_write(enemy+0x30,words(1))
    table,cells = _prepare_flat_cells(m,range(8,13),range(18,24))
    by_coord = {tuple(struct.unpack('<2h',u.mem_read(cell+0x24,4))):cell for cell in cells}
    locomotor_evidence = dict(drive_constructor='004AF540',
        drive_visual_callback=f'{m.read32(m.read32(locomotor+4)+0x34):08X}')
    if locomotor_ctor != 0x4AF540:
        locomotor_evidence = dict(locomotor_constructor=f'{locomotor_ctor:08X}',
            locomotor_visual_callback=f'{m.read32(m.read32(locomotor+4)+0x34):08X}')
    evidence=dict(constructor_begin=f'{begin:08X}',constructor_stop=f'{end:08X}',
        whole_unit_constructor=False,**locomotor_evidence,
        flags=m.read32(actor+0x14),unit_vtable=f'{m.read32(actor):08X}',
        control_group=struct.unpack('<i',u.mem_read(actor+0x214,4))[0],
        ctor_veterancy_raw_bits=m.read32(actor+0x150),flat_cells=len(cells),
        constructor_rng_requests=requests,passive_scan_timer=rally.ints(u,actor+0x180,3),
        house_indices=[0,1],mutually_allied=False)
    if aircraft:
        evidence['whole_aircraft_constructor']=evidence.pop('whole_unit_constructor')
        evidence['aircraft_vtable']=evidence.pop('unit_vtable')
    return dict(enemy=enemy,cells=by_coord,evidence=evidence)


def _prepare_rank_actor(m,actor,house,supplied,runtime):
    """Actual native rank setter and declared per-observer consumer priors."""
    name = supplied.pop('rank')
    setter,args = {'rookie':(0x750080,(1,)),'veteran':(0x750090,(1,)),
                   'elite':(0x7500B0,(1,)),'negative':(0x7500E0,(-100,))}[name]
    m.invoke(setter,actor+0x150,args)
    u=m.u
    u.mem_write(actor+0x21C,words(house if supplied['owner_is_current'] else runtime['enemy']))
    u.mem_write(actor+0x41A,bytes([supplied['owned_by_current_house']]))
    u.mem_write(actor+0x220,words(supplied['cloak_state']))
    u.mem_write(actor+0x224,words(supplied['cloak_progress']))
    u.mem_write(actor+0x1D8,bytes([supplied['disguised']]))
    u.mem_write(actor+0x51C,words(house if supplied['disguised_as_current_house'] else 0))
    for cell in runtime['cells'].values():
        u.mem_write(cell+0x7C,struct.pack('<h',supplied['sensor_count']))
        u.mem_write(cell+0xAC,struct.pack('<h',supplied['disguise_detect_count']))
    return setter,args


def _unit_rank_cases(base):
    cases = [dict(name=name,actors=[dict(base,rank=rank)]) for name,rank in
             (('rookie','rookie'),('veteran','veteran'),('elite','elite'),('negative','negative'))]
    cases += [dict(name='selected_veteran',actors=[dict(base,selected=True)]),
              dict(name='hovered_veteran',actors=[dict(base,hovered=True)]),
              *[dict(name=f'veteran_a{a}',a_value=a) for a in (63,2,0)],
              dict(name='veteran_occluding_z',old_z=0),
              dict(name='sinking',actors=[dict(base,sinking=True)]),
              dict(name='clipped',actors=[dict(base,location=[2976,4960,0])]),
              dict(name='ordered_overlap',actors=[dict(base,selected=True),
                   dict(base,rank='elite',selected=True,health=300,location=[2968,5528,0])]),
              dict(name='owned_cloaked',actors=[dict(base,cloak_state=2)]),
              dict(name='enemy_cloaked',actors=[dict(base,cloak_state=2,
                   owner_is_current=False,owned_by_current_house=False)]),
              dict(name='enemy_cloaked_sensed',actors=[dict(base,cloak_state=2,
                   owner_is_current=False,owned_by_current_house=False,sensor_count=1)]),
              dict(name='enemy_disguised',actors=[dict(base,disguised=True,
                   disguised_as_current_house=True,owner_is_current=False,
                   owned_by_current_house=False)]),
              dict(name='enemy_disguised_detected',actors=[dict(base,disguised=True,
                   disguised_as_current_house=True,owner_is_current=False,
                   owned_by_current_house=False,disguise_detect_count=1)])]
    return cases


def _rank_vtable_bindings(m):
    """Actual active RTTI slots, not a second class-specific draw implementation."""
    rows=[]
    for kind,table in VTABLES.items():
        slots={name:f'{m.read32(table+offset):08X}' for name,offset in
               (('extras',0x110),('rank',0x454),('visual_character',0x68),
                ('is_disguised_to',0xC8),('what_am_i',0x2C))}
        assert slots['extras']=='006F5190' and slots['rank']=='0070A990'
        rows.append(dict(kind=kind,vtable=f'{table:08X}',slots=slots))
    return rows


def _status_reader_block(m, begin, end, registers):
    """One unchanged caller block in the existing native reader owner."""
    from tools.spatial_oracle.building_body_rules import SP as reader_sp
    u = m.u
    u.mem_write(reader_sp, words(native.RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, reader_sp)
    for register, value in registers.items():
        u.reg_write(register, value)
    native.run_checked(u, begin, end)
    assert u.reg_read(UC_X86_REG_ESP) == reader_sp


def _passenger_type_fields(m, typ, infantry=False, building=False, *, ammo=False):
    u = m.u
    result = dict(strength=m.read32(typ+0xA0), delta=m.read32(typ+0x3E0),
        pip_scale=m.read32(typ+0x3D4), passengers=m.read32(typ+0x5E0),
        size=struct.unpack('<d',u.mem_read(typ+0x380,8))[0],
        size_limit=struct.unpack('<d',u.mem_read(typ+0x388,8))[0],
        gunner=bool(u.mem_read(typ+0x805,1)[0]), storage=m.read32(typ+0x800))
    if infantry:
        result.update(pip=m.read32(typ+0xDFC),occupy_pip=m.read32(typ+0xE00))
    if building:
        result.update(max_number_occupants=m.read32(typ+0x1580),
            show_occupant_pips=bool(u.mem_read(typ+0x1584,1)[0]),
            can_be_occupied=bool(u.mem_read(typ+0x157B,1)[0]),
            height=m.read32(typ+0xEF4),foundation=m.read32(typ+0xEF0))
    if ammo:
        result.update(pip_wrap=struct.unpack('<i',u.mem_read(typ+0x3E4,4))[0],
            initial_ammo=struct.unpack('<i',u.mem_read(typ+0x680,4))[0],
            ammo=struct.unpack('<i',u.mem_read(typ+0x684,4))[0])
    primary=m.read32(typ+0x898)
    if primary:
        result.update(primary=m.string(primary+0x24),weapon_damage=m.read32(primary+0xA4),
                      infinite_mind_control=bool(u.mem_read(primary+0x140,1)[0]))
    return result


def _passenger_pip_reads(m, typ):
    from tools.spatial_oracle.building_body_rules import RULES
    registers = {UC_X86_REG_ESI:typ,UC_X86_REG_EBP:RULES,UC_X86_REG_EDI:typ+0x24}
    _status_reader_block(m,0x5240D1,0x5240EE,registers)
    _status_reader_block(m,0x5240EE,0x524102,registers)
    # Preserve original EAX from the second reader, skip only unrelated
    # following ReadString argument preparation, and execute its actual store.
    _status_reader_block(m,0x52411F,0x524125,registers)


def _read_pip_controls(m, typ, constructor):
    """Both original Infantry callers, including admitted missing-key passes."""
    from tools.spatial_oracle.building_body_rules import RULES, SP as reader_sp
    u = m.u
    table = []
    for index in range(13):
        pointer=m.read32(0x81B958+index*8)
        table.append(dict(index=index,name=m.string(pointer),
                          value=m.read32(0x81B95C+index*8),in_pip_table=index<11))
    controls=[]
    requests=[(1,name) for name in [row['name'] for row in table[:11]]]
    requests += [(default,None) for default in range(13)]
    requests += [(1,raw) for raw in ('','unknown','WHITE','  Blue\t','3','white suffix',
                                   ' '*26+'white',' '*27+'white')]
    for default,raw in requests:
        keys={'Strength':'125'}
        if raw is not None:
            keys['Pip']=raw
        sections={'PipProbe':keys}
        m.rules_cache(sections)
        result=m.invoke(0x4748A0,RULES,(m.cstring('PipProbe'),m.cstring('Pip'),default))
        controls.append(dict(input=dict(section='PipProbe',key='Pip',raw=raw,
                                        default=default,sections=sections),result=result))
    u.mem_write(typ+0xDFC,words(constructor['pip'],constructor['occupy_pip']))
    passes=[]
    for name,sections in (
            ('base',{'E1':{'Pip':'white','OccupyPip':'PersonBlue'}}),
            ('absent_section',{}),('unrelated_map_section',{'E1':{'Strength':'125'}}),
            ('unrelated_later_section',{'E1':{'Strength':'130'}}),
            ('unrelated_third_section',{'E1':{'Strength':'135'}}),
            ('explicit_again',{'E1':{'Pip':'white','OccupyPip':'PersonBlue'}})):
        m.rules_cache(sections)
        before=dict(pip=m.read32(typ+0xDFC),occupy_pip=m.read32(typ+0xE00))
        u.mem_write(reader_sp,words(native.RET_MAGIC,RULES))
        u.reg_write(UC_X86_REG_ESP,reader_sp)
        u.reg_write(UC_X86_REG_ECX,typ)
        stop=native.run_checked(u,0x410A60,(0x410A8C,0x410B7D))
        admitted=stop==0x410A8C
        if admitted:
            _passenger_pip_reads(m,typ)
        passes.append(dict(name=name,input=dict(sections=sections),admitted=admitted,
            admission_stop=f'{stop:08X}',before=before,
            after=dict(pip=m.read32(typ+0xDFC),occupy_pip=m.read32(typ+0xE00))))
    return dict(constructor={key:constructor[key] for key in ('pip','occupy_pip')},
                table=table,controls=controls,section_passes=passes)


def _prepare_status_passengers(m, types, house):
    """Existing Infantry/Walk and Unit/Drive prefixes for genuine Cargo calls.

    The initial Limbo1 is native constructor output; placement/boarding and
    whole typed COM construction remain separate production prerequisites.
    """
    from tools.spatial_oracle.building_body_rules import SP as reader_sp
    u=m.u
    passengers={}
    for name in (name for name in ('E1','ENGINEER') if name in types):
        actor,walk=m.alloc(0x900),m.alloc(0x100)
        requests=[]
        def rng(_u,pc,_size,_data):
            if pc in (0x65C780,0x65C7E0):
                sp=u.reg_read(UC_X86_REG_ESP)
                requests.append(dict(entry=f'{pc:08X}',caller=f'{m.read32(sp):08X}'))
        hook=u.hook_add(UC_HOOK_CODE,rng)
        u.mem_write(reader_sp,words(native.RET_MAGIC,types[name],0))
        u.reg_write(UC_X86_REG_ESP,reader_sp)
        u.reg_write(UC_X86_REG_ECX,actor)
        native.run_checked(u,0x517A50,0x517AE6,required_addresses=(0x4D31E0,))
        m.invoke(0x75AA90,walk)
        u.hook_del(hook)
        u.mem_write(walk+0xC,words(actor))
        u.mem_write(walk+0x14,words(1))
        u.mem_write(actor+0x674,words(walk+4))
        passengers[name]=dict(actor=actor,template=bytes(u.mem_read(actor,0x900)),
            evidence=dict(constructor_begin='00517A50',constructor_stop='00517AE6',
                whole_infantry_constructor=False,walk_constructor='0075AA90',
                limbo=bool(u.mem_read(actor+0x81,1)[0]),vtable=f'{m.read32(actor):08X}',
                constructor_rng_requests=requests,passive_scan_timer=rally.ints(u,actor+0x180,3)))
        assert passengers[name]['evidence']['limbo']
    if 'MTNK' in types:
        actor = m.alloc(0x900)
        runtime = _prepare_unit_rank(m,types['MTNK'],actor,house)
        passengers['MTNK'] = dict(actor=actor,template=bytes(u.mem_read(actor,0x900)),
            evidence=runtime['evidence'] | dict(limbo=bool(u.mem_read(actor+0x81,1)[0])))
        assert passengers['MTNK']['evidence']['limbo']
    return passengers


def _passenger_cases(base):
    return [dict(name='empty',actors=[dict(base,passenger=None)]),
        *[dict(name=f'loaded_a{a}',a_value=a) for a in (127,63,2,0)],
        dict(name='loaded_occluding_z',old_z=0),
        dict(name='engineer',actors=[dict(base,passenger='ENGINEER')]),
        dict(name='integer_size_passenger',actors=[dict(base,type_name='LCRF',
            passenger='MTNK',health=300)]),
        dict(name='unload_empty',actors=[dict(base,unload=True)]),
        dict(name='hover_only',actors=[dict(base,selected=False,hovered=True)]),
        dict(name='absent',actors=[dict(base,selected=False)]),
        dict(name='sinking',actors=[dict(base,sinking=True)]),
        dict(name='clipped',actors=[dict(base,location=[2416,5520,0])]),
        dict(name='ordered_overlap',actors=[dict(base,rank='veteran'),
             dict(base,rank='elite',passenger='ENGINEER',health=75,location=[2987,5547,0])]),
        dict(name='affected_mind_control',actors=[dict(base,type_name='MIND',passenger=None,
                                                      health=500,capture_count=4)]),
        *[dict(name=name,actors=[dict(base,type_name='HARV',passenger=None,health=1000,
                                    ore_bales=ore,gem_bales=gems,storage=[ore,gems,0,0])]) for name,ore,gems in
          (('tiberium_ore_tie',4,0),('tiberium_gem_tie',0,4),
           ('tiberium_below_tie',3,0),('tiberium_mixed_ties',4,4),
           ('tiberium_ore_above_tie',5,0),('tiberium_gem_above_tie',0,5),
           ('tiberium_mixed_above_tie',5,5),
           ('tiberium_full_mixed',20,20),('tiberium_mixed_partial',16,8))]]


def _ammo_cases(base):
    return [*[dict(name=f'full_a{a}',a_value=a) for a in (127,2,0)],
        dict(name='depleted',actors=[dict(base,ammo_initialization='depleted')]),
        dict(name='rearmed',actors=[dict(base,ammo_initialization='rearmed')]),
        dict(name='occluding_z',old_z=0),
        dict(name='hover_only',actors=[dict(base,selected=False,hovered=True)]),
        dict(name='absent',actors=[dict(base,selected=False)]),
        dict(name='sinking',actors=[dict(base,sinking=True)]),
        dict(name='clipped',actors=[dict(base,location=[2416,5520,0])]),
        dict(name='airborne',actors=[dict(base,location=[4544,7104,1500])]),
        dict(name='ordered_overlap',actors=[base,dict(base,health=75,location=[2987,5547,0])])]


def _control_group_cases(base):
    assign=lambda group:dict(operation='assign',group=group,selected=True)
    return [dict(name='group_1'),
        dict(name='group_0',actors=[dict(base,group_commands=[assign(10)])]),
        dict(name='reassign',actors=[dict(base,group_commands=[assign(1),assign(10)])]),
        dict(name='clear',actors=[dict(base,group_commands=[assign(1),dict(operation='clear',group=1)])]),
        dict(name='unassigned',actors=[dict(base,group_commands=[])]),
        *[dict(name=f'group_a{a}',a_value=a) for a in (63,2,0)],
        dict(name='occluding_z',old_z=0),
        dict(name='hover_only',actors=[dict(base,selected=False,hovered=True)]),
        dict(name='absent',actors=[dict(base,selected=False)]),
        dict(name='sinking',actors=[dict(base,sinking=True)]),
        dict(name='clipped',actors=[dict(base,location=[2394,5542,0])]),
        dict(name='ordered_groups',actors=[base,dict(base,health=75,group_commands=[assign(10)])]),
        dict(name='ordered_health',actors=[base,dict(base,health=75,group_commands=[],location=[2718,5278,0])]),
        dict(name='self_heal_units',self_heal_units=1),
        dict(name='self_heal_infantry',self_heal_infantry=1,
             actors=[dict(base,type_name='E1',kind='infantry',health=125)]),
        dict(name='self_heal_organic_unit',self_heal_infantry=1,
             scenario_patch='[MTNK]\nOrganic=yes\n'),
        dict(name='self_heal_delta',self_heal_units=1,
             scenario_patch='[MTNK]\nPixelSelectionBracketDelta=11\n'),
        dict(name='ordered_self_heal_group',self_heal_units=1,
             actors=[base,dict(base,location=[2846,5090,0],group_commands=[assign(10)])])]


def _prepare_ammo_state(m,actor,typ,mode):
    """Native initialization, admitted-release prefix and actual radio receiver.

    Entry to the pending writer is an explicit successful-release prior; no
    shot, mission dispatch, navigation or airfield service is substituted.
    """
    u=m.u
    def current():
        return struct.unpack('<i',u.mem_read(actor+0x2FC,4))[0]
    _status_reader_block(m,0x41403A,0x414051,{UC_X86_REG_ESI:actor,UC_X86_REG_EAX:typ})
    events=[dict(entry='0041403A..00414051',operation='initialize',ammo=current())]
    if mode!='full':
        _status_reader_block(m,0x41840E,0x418415,{UC_X86_REG_ESI:actor})
        events.append(dict(entry='0041840E..00418415',operation='admitted_release_pending',
                           ammo=current(),pending=bool(u.mem_read(actor+0x6C8,1)[0])))
        _status_reader_block(m,0x418031,0x418056,{UC_X86_REG_ESI:actor})
        events.append(dict(entry='00418031..00418056',operation='consume_release',
                           ammo=current(),pending=bool(u.mem_read(actor+0x6C8,1)[0])))
    if mode=='rearmed':
        response=m.invoke(0x4190B0,actor,(0,0x1F,0))
        events.append(dict(entry='004190B0',operation='radio_reload',response=response,ammo=current()))
    return events


def _ammo_fill_controls(m,types,templates,actor,manager,manager_template,nodes,passengers):
    """Original708C30 for existing signed ammo/storage/manager/cargo owners."""
    from tools.spatial_oracle.building_body_rules import RULES
    u=m.u
    cases=[dict(type_name='ORCA',current_ammo=n) for n in
           (-2147483648,-2,-1,0,1,2,2147483647)]
    cases += [dict(type_name='HARV',storage=[n,0,0,0]) for n in (0,1,4,5,20,40)]
    cases += [dict(type_name='MIND',capture_count=n) for n in (0,3,4)]
    cases += [dict(type_name='FV',cargo_count=n) for n in (0,1)]
    cases += [dict(type_name='HARV',storage=[n,0,0,0],configuration={'Storage':str(capacity)})
              for capacity in (0,-1) for n in (0,1)]
    cases += [dict(type_name='ORCA',current_ammo=n,configuration={'Ammo':'0'}) for n in (0,1)]
    cases += [dict(type_name='ORCA',current_ammo=1,configuration={'PipScale':name})
              for name in ('Power','None')]
    rows=[]
    for supplied in cases:
        name=supplied['type_name'];typ=types[name]
        configuration=supplied.get('configuration',{})
        def configure(keys):
            m.rules_cache({name:keys})
            registers={UC_X86_REG_EBP:typ,UC_X86_REG_EBX:typ+0x24,
                       UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES}
            for key,begin,end in (('Storage',0x713129,0x713143),('Ammo',0x714766,0x714780),
                                  ('PipScale',0x714111,0x71412B)):
                if key in keys:_status_reader_block(m,begin,end,registers)
        if configuration:configure(configuration)
        u.mem_write(actor,templates[name])
        if 'current_ammo' in supplied:u.mem_write(actor+0x2FC,words(supplied['current_ammo']))
        if 'storage' in supplied:u.mem_write(actor+0x33C,struct.pack('<4f',*supplied['storage']))
        if 'capture_count' in supplied:
            u.mem_write(manager,manager_template)
            u.mem_write(manager+0x28,words(nodes,supplied['capture_count']))
            u.mem_write(manager+0x34,words(supplied['capture_count'],4))
            u.mem_write(actor+0x2BC,words(manager))
        if supplied.get('cargo_count'):
            prepared=passengers['E1'];u.mem_write(prepared['actor'],prepared['template'])
            m.invoke(0x4733A0,actor+0x114,(prepared['actor'],))
        numeric=[];rng=[];operands={}
        def observe(_u,pc,_size,_data):
            sp=u.reg_read(UC_X86_REG_ESP)
            if pc==0x6C9677:
                operands['storage_total']=struct.unpack('<i',u.mem_read(sp+8,4))[0]
            if pc==0x708D15:
                operands.update(amount=struct.unpack('<d',u.mem_read(sp+8,8))[0],
                    denominator=struct.unpack('<d',u.mem_read(sp+0x10,8))[0],
                    maximum=struct.unpack('<i',u.mem_read(sp+4,4))[0])
            if pc==0x708D2A:
                numeric.append(dict(eax=u.reg_read(UC_X86_REG_EAX),edx=u.reg_read(UC_X86_REG_EDX),
                    fpcw=u.reg_read(UC_X86_REG_FPCW)))
            if pc in (0x65C780,0x65C7E0):rng.append(f'{pc:08X}')
        hook=u.hook_add(UC_HOOK_CODE,observe)
        before=bytes(u.mem_read(actor,0x900));raw=m.invoke(0x708C30,actor)
        u.hook_del(hook)
        rows.append(dict(input=supplied,type_fields=_passenger_type_fields(m,typ,ammo=True),
            pip_max=m.invoke(0x716290,typ),result=struct.unpack('<i',words(raw))[0],
            arithmetic=operands,ftol=numeric,rng_entries=rng,
            actor_unchanged=before==bytes(u.mem_read(actor,0x900))))
        assert not rng and rows[-1]['actor_unchanged']
        if configuration:
            configure({key:{'Storage':'40','Ammo':'1','PipScale':'Ammo'}[key] for key in configuration})
    return rows


def generate_unit_health(*, building=False, rank=False, passenger=False, ammo=False, groups=False, wrench=False):
    """Original ordinary Unit/Building health through shared physical SHP stores.

    PaletteReader owns the one VM, native INI caches and palette constructors;
    the existing shape owner supplies only Surface/circular A/Z storage.
    """
    from tools.projectile_oracle import bridge_render_inputs_palette as palette
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.spatial_oracle.building_body_rules import RULES, SP as READER_SP

    assert sum((building,rank,passenger,ammo,groups,wrench))<=1
    building=building or wrench
    scales=passenger or ammo
    mobile_status=rank or scales or groups
    # These named archive bytes are the normal STOCK_DIGITAL AssetManager
    # winners, independently resolved before admitting the historical modes.
    # New WRENCH/MOUSEPAL archive candidates require the production consumer's
    # normal AssetManager hash gate; this transport does not choose overrides.
    winners = (
        ('PIPS.SHP', 'ra2md.mix', 'conqmd.mix',
         '7525b25f80b5b67e9a5c2977b7624c4b3ce7ad9ce332169c4622eddb91c61486'),
        ('PIPBRD.SHP', 'ra2.mix', 'conquer.mix',
         '8da08a64beb8c7916a3ae51fc7239d9474ef3366e73e78e893e24e31c119812e'),
        ('PALETTE.PAL', 'ra2.mix', 'cache.mix',
         'e85c535002573b5f95f7c361c4bdb95ee43e4914ddc926afda9ace995c54242b'),
        ('ANIM.PAL', 'ra2.mix', 'cache.mix', None),
    )
    if scales:
        # Native5F7758 asks for PIPS2.SHP through normal filename lookup.
        # Independent STOCK_DIGITAL AssetManager lookup selects the YR copy.
        winners += (('PIPS2.SHP','ra2md.mix','conqmd.mix',
                     '0afe9935160c04bdabed5bfefd0d55aad645ae38ec7dddeb5b3f4f2b2d2712d4'),)
    if wrench:
        winners += (('WRENCH.SHP','ra2.mix','cache.mix',
                     'd1ee6a305cd65c91a9b29db7c83c2f3acc4e15a3bb41082844e4c4dd0e2de8a2'),
                    ('MOUSEPAL.PAL','ra2.mix','cache.mix',
                     '4dcbe78b534a58fc972bce4b25aa58a2a56c9a5bb0ffbfcf5d6194be5dbde60c'))
    physical, assets = [], {}
    for name, outer_name, inner_name, expected in winners:
        outer = (native.configured_gamemd().parent / outer_name).read_bytes()
        inner = stock.mix(outer)[stock.mix_hash(inner_name)]
        raw = stock.mix(inner)[stock.mix_hash(name)]
        if expected is not None:
            assert sha(raw) == expected, (name, sha(raw))
        assets[name] = raw
        physical.append(dict(name=name, archive=f'{outer_name}/{inner_name}',
                             bytes=len(raw), sha256=sha(raw)))
    if groups:
        from tools import control_group_text_oracle as group_text
        outer=(native.configured_gamemd().parent/'ra2.mix').read_bytes()
        font_raw=stock.mix(stock.mix(outer)[stock.mix_hash('local.mix')])[stock.mix_hash('GAME.FNT')]
        assert sha(font_raw)=='ec8d6d85db3eedf5a862cc6ffb87bfad4c77279dd946bc2d30879ad9140c8d3f'
        physical.append(dict(name='GAME.FNT',archive='ra2.mix/local.mix',bytes=len(font_raw),sha256=sha(font_raw)))
    scene = (_building_ground_runtime(type_name='NABNKR',render_ground=False) if passenger
             else _building_ground_runtime(wrench=wrench) if building else None)
    m = scene['m'] if scene else palette.PaletteReader({})
    if scene:
        m.assets.update(assets)
    else:
        m.assets = assets
    u = m.u
    if building:
        rules, typ = m.rules, scene['typ']
    else:
        rules, typ = m.rules if passenger else m.alloc(0x3000), m.alloc(0x1900)
        if not passenger:
            m.invoke(0x665650, rules)
            u.mem_write(0x8871E0, words(rules))
        u.mem_write(0xA8EB00, words(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        if ammo:
            u.mem_write(0xA8B218,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        m.invoke(0x41C8B0 if ammo else 0x7470D0, typ,
                 (m.cstring('ORCA' if ammo else 'FV' if passenger else 'MTNK'),))
    if scales:
        types={'ORCA':typ} if ammo else {'FV':typ,'NABNKR':scene['typ']}
        u.mem_write(0x887568,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        u.mem_write(0xA8E348,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        for name in (('FV','MIND','HARV','E1') if ammo else
                     ('MIND','HARV','E1','ENGINEER','LCRF','MTNK')):
            types[name]=m.alloc(0x2000)
            m.invoke(0x5236A0 if name in ('E1','ENGINEER') else 0x7470D0,
                     types[name],(m.cstring(name),))
        type_constructors={name:_passenger_type_fields(m,p,name in ('E1','ENGINEER'),ammo=ammo)
                           for name,p in types.items() if name!='NABNKR'}
        if passenger:
            type_constructors['NABNKR']=scene['constructor_fields']
            pip_controls=_read_pip_controls(m,types['E1'],type_constructors['E1'])
            # Restore exact constructor fields before physical-layer reads.
            u.mem_write(types['E1']+0xDFC,words(type_constructors['E1']['pip'],
                                             type_constructors['E1']['occupy_pip']))
    if groups:
        group_types={'MTNK':typ,'E1':m.alloc(0x2000)}
        u.mem_write(0x887568,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        m.invoke(0x5236A0,group_types['E1'],(m.cstring('E1'),))

    def fields():
        yellow, red = struct.unpack('<2d', u.mem_read(rules + 0x1700, 16))
        base = (_building_health_fields(m, typ, repair=wrench) if building else
                dict(strength=m.read32(typ + 0xA0), delta=m.read32(typ + 0x3E0),
                     pip_scale=m.read32(typ + 0x3D4)))
        if mobile_status:
            base['invisible'] = bool(u.mem_read(typ+0xC9A,1)[0])
        if groups:
            base.update(self_heal_infantry_frames=m.read32(rules+0x30),
                        self_heal_unit_frames=m.read32(rules+0x38))
        if scales:
            base.update(_passenger_type_fields(m,typ,ammo=ammo))
        return dict(base, condition_yellow=yellow, condition_red=red)

    def tail_type():
        return dict(spawns_number=m.read32(typ + 0xD5C),
                    passengers=m.read32(typ + 0x5E0),
                    organic=bool(u.mem_read(typ + 0xD97, 1)[0]))

    constructor, tail_defaults, layers, reads = fields(), tail_type(), [], []
    if building:
        constructor.update(scene['constructor_fields'])
    def read_observer(_u, pc, _size, _data):
        if pc in (0x5276D0, 0x5283D0, 0x474940) or (scales and pc in (0x4748A0,0x5295F0)):
            sp = u.reg_read(UC_X86_REG_ESP)
            section, key = [m.read32(sp + n) for n in (4, 8)]
            default = (struct.unpack('<d', u.mem_read(sp + 12, 8))[0]
                       if pc == 0x5283D0 else m.read32(sp + 12))
            reads.append(dict(reader=f'{pc:08X}', section=m.string(section),
                              key=m.string(key), default=default))
    hook = u.hook_add(UC_HOOK_CODE, read_observer)
    for name, archive, raw in physical_layers():
        if raw is None:
            layers.append(dict(file=name, archive=archive, absent=True))
            continue
        section_name = 'GAPOWR' if building else 'ORCA' if ammo else 'FV' if passenger else 'MTNK'
        sections, _ = lexical(raw, {*group_types,'General','AudioVisual'} if groups else {*types,'MultipleMindControlTank','AudioVisual'} if scales
                                      else {section_name,'AudioVisual'})
        # These ordinary-tail keys are absent in the selected physical stack.
        # Keep native constructor values; don't invent a partial full reader.
        assert not ({'SpawnsNumber','Organic'} if scales else
                    {'SpawnsNumber','Passengers','Organic'}).intersection(sections.get(section_name,{}))
        if mobile_status:
            # The visual predicate consumes this retained native ctor default.
            # No physical selected layer may silently override an unread key.
            assert 'Invisible' not in sections.get(section_name,{})
        m.rules_cache(sections)
        mark = len(reads)
        #526810 returns a Section pointer, not AL; a valid pointer may end00.
        admitted = bool(m.invoke(0x526810, RULES, (typ + 0x24,)))
        if admitted and not building:
            for begin, end, regs in (
                    (0x5F94D3, 0x5F94F3, {UC_X86_REG_EBX:typ, UC_X86_REG_EBP:typ+0x24}),
                    (0x714111, 0x71412B, {UC_X86_REG_EBP:typ, UC_X86_REG_EBX:typ+0x24}),
                    (0x71415F, 0x714179, {UC_X86_REG_EBP:typ, UC_X86_REG_EBX:typ+0x24})):
                _status_reader_block(m,begin,end,{UC_X86_REG_ESI:RULES}|regs)
        if groups:
            for type_name,p in group_types.items():
                if not m.invoke(0x526810,RULES,(p+0x24,)):continue
                blocks=[] if p==typ else [(0x5F94D3,0x5F94F3),(0x714111,0x71412B),
                                          (0x71415F,0x714179)]
                blocks.append((0x715024,0x715045))
                for begin,end in blocks:
                    registers={UC_X86_REG_EBP:p,UC_X86_REG_EBX:p+0x24,
                               UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES}
                    if begin==0x5F94D3:registers.update({UC_X86_REG_EBX:p,UC_X86_REG_EBP:p+0x24})
                    _status_reader_block(m,begin,end,registers)
            if m.invoke(0x526810,RULES,(m.cstring('General'),)):
                for begin,end in ((0x66E6E2,0x66E6FB),(0x66E715,0x66E72F)):
                    _status_reader_block(m,begin,end,{UC_X86_REG_ESI:rules,UC_X86_REG_EDI:RULES})
        if scales:
            type_results={}
            for type_name,p in types.items():
                if type_name=='NABNKR':
                    continue  # Complete original reader already ran in scene.
                present=bool(m.invoke(0x526810,RULES,(p+0x24,)))
                if present:
                    blocks=[(0x712511,0x712553), (0x714A49,0x714A63),
                            (0x714B35,0x714B56), (0x713129,0x713143)]
                    if type_name!=section_name:
                        blocks += [(0x5F94D3,0x5F94F3),(0x714111,0x71412B),
                                   (0x71415F,0x714179)]
                    for begin,end in blocks:
                        registers={UC_X86_REG_EBP:p,UC_X86_REG_EBX:p+0x24,
                                   UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES}
                        if begin==0x5F94D3:
                            registers.update({UC_X86_REG_EBX:p,UC_X86_REG_EBP:p+0x24})
                        _status_reader_block(m,begin,end,registers)
                    if ammo and type_name=='ORCA':
                        for begin,end in ((0x714179,0x71418D),(0x714198,0x71419E),
                                          (0x71474C,0x714780)):
                            _status_reader_block(m,begin,end,{UC_X86_REG_EBP:p,
                                UC_X86_REG_EBX:p+0x24,UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES})
                    if type_name in ('E1','ENGINEER'):
                        _passenger_pip_reads(m,p)
                type_results[type_name]=dict(admitted=present,
                    after=_passenger_type_fields(m,p,type_name in ('E1','ENGINEER'),ammo=ammo))
            if type_results['MIND']['admitted']:
                _status_reader_block(m,0x7129AB,0x712A1D,
                    {UC_X86_REG_EBP:types['MIND'],UC_X86_REG_EBX:types['MIND']+0x24,
                     UC_X86_REG_ESI:RULES})
            weapon=m.read32(types['MIND']+0x898)
            if weapon and m.invoke(0x526810,RULES,(weapon+0x24,)):
                _status_reader_block(m,0x7722D4,0x7722EE,
                    {UC_X86_REG_ESI:weapon,UC_X86_REG_EBX:weapon+0x24,UC_X86_REG_EDI:RULES})
                _status_reader_block(m,0x772204,0x77221E,
                    {UC_X86_REG_ESI:weapon,UC_X86_REG_EBX:weapon+0x24,UC_X86_REG_EDI:RULES})
            if weapon:
                type_results['MIND']['after'].update(primary=m.string(weapon+0x24),
                    weapon_damage=m.read32(weapon+0xA4),
                    infinite_mind_control=bool(u.mem_read(weapon+0x140,1)[0]))
        av_admitted = bool(m.invoke(0x526810, RULES, (m.cstring('AudioVisual'),)))
        if av_admitted:
            for begin, end in ((0x66B337, 0x66B35E), (0x66B35E, 0x66B385)):
                u.reg_write(UC_X86_REG_ESP, READER_SP)
                u.reg_write(UC_X86_REG_ESI, rules)
                u.reg_write(UC_X86_REG_EDI, RULES)
                native.run_checked(u, begin, end)
                assert u.reg_read(UC_X86_REG_ESP) == READER_SP
        layers.append(dict(file=name, archive=archive, sha256=sha(raw),
                           section_admitted=admitted, audiovisual_admitted=av_admitted,
                           selected_keys={s:{k:v for k,v in keys.items() if k in
                               ('Strength', 'PixelSelectionBracketDelta', 'PipScale',
                                'ConditionRed', 'ConditionYellow',
                                *(['PipsDrawForAll','CanBeOccupied','Pip','SpawnsNumber']
                                  if building else []), *(['Invisible'] if mobile_status else []),
                                *(['Passengers','Size','SizeLimit','Gunner','Pip','OccupyPip',
                                   'Storage','Damage','Primary','InfiniteMindControl'] if scales else []),
                                *(['PipWrap','InitialAmmo','Ammo'] if ammo else []))}
                               for s, keys in sections.items()},
                           reads=reads[mark:], after=fields()))
        if groups:
            layers[-1]['self_heal_keys']={section:{key:value for key,value in keys.items() if key in
                ('Organic','SelfHealInfantryFrames','SelfHealUnitFrames')}
                for section,keys in sections.items()}
        if scales:
            layers[-1]['types']=type_results
    u.hook_del(hook)
    if not scene:
        palette.initialize(m)
    if groups:
        native_font=group_text.initialize_font(font_raw,owner=m)
        group_text.install_digit_format(m)
        house_colors=json.loads(Path(__file__).with_name('house_color.json').read_text())
        assert house_colors['rulesmd_sha256']==layers[0]['sha256']
        gold=[row for row in house_colors['colors'] if row['name']=='Gold'][0]
        house_rgb=gold['modes']['scalar']['rgb']
    convert = m.read32(0x87F6C4)
    table, count = m.read32(convert + 0x170), m.read32(convert + 0x16C)
    palette_result = dict(global_address='0x87F6C4', bytes_per_pixel=m.read32(convert + 4),
                          shade_count=count, table_sha256=sha(u.mem_read(table, count * 512)))
    if wrench:
        health_palette=palette_result
        palette.initialize_mouse(m)
        convert=m.read32(0x87F6C8)
        table,count=m.read32(convert+0x170),m.read32(convert+0x16C)
        palette_result=dict(global_address='0x87F6C8',bytes_per_pixel=m.read32(convert+4),
                            shade_count=count,table_sha256=sha(u.mem_read(table,count*512)))
    shape_names = {}
    for name, global_ in (('PIPBRD.SHP',0xAC1478),('PIPS.SHP',0xAC147C),
                         *([('PIPS2.SHP',0xAC1480)] if scales else [])):
        pointer = m.alloc(len(assets[name]))
        u.mem_write(pointer, assets[name])
        u.mem_write(global_, words(pointer))
        shape_names[pointer] = name
    if wrench:
        # Exact original filename/load/publication, after unrelated POWEROFF.
        _status_reader_block(m,0x45E98D,0x45E9A3,{})
        wrench_shape=m.read32(0x89DDC8)
        assert wrench_shape and bytes(u.mem_read(wrench_shape,len(assets['WRENCH.SHP'])))==assets['WRENCH.SHP']
        shape_names[wrench_shape]='WRENCH.SHP'
    if building:
        house, actor, tactical, coord, point, clip = (scene[key] for key in
            ('house', 'building', 'tactical', 'coord', 'point', 'clip'))
        shape_names[m.read32(typ + 0xA4)] = 'GGPOWR.SHP'
        actor_template = bytes(u.mem_read(actor, 0x800))
        ground_planes = {name:bytes(u.mem_read(address, 51200))
                         for name,address in (('color',PIXELS),('z',Z),('a',A))}
        if wrench:
            other_house=m.alloc(0x18000)
            render_coord,render_point=m.alloc(12),m.alloc(8)
    else:
        if not passenger:
            u.mem_map(MEM, 0x200000)
        house, actor, tactical, coord, point, clip = (m.alloc(n) for n in
                                                      (0x18000, 0x900, 0x200, 12, 8, 16))
    if rank or groups:
        rank_runtime = _prepare_unit_rank(m, typ, actor, house)
        actor_template = bytes(u.mem_read(actor,0x900))
        if groups:
            prepared_infantry=_prepare_status_passengers(m,{'E1':group_types['E1']},house)['E1']
            group_actors={'MTNK':actor,'E1':prepared_infantry['actor']}
            group_templates={'MTNK':actor_template,'E1':prepared_infantry['template']}
    if scales:
        status_runtimes,actor_templates={},{}
        for name in (('ORCA','FV','MIND','HARV') if ammo else ('FV','MIND','HARV','LCRF')):
            status_runtimes[name]=_prepare_unit_rank(m,types[name],actor,house,
                locomotor_ctor=0x4CC9A0 if name=='ORCA' else 0x513C20 if name=='LCRF' else 0x4AF540,
                aircraft=name=='ORCA')
            actor_templates[name]=bytes(u.mem_read(actor,0x900))
        passengers=_prepare_status_passengers(m,types,house)
        if passenger:
            _prepare_ground_building(m,actor,types['NABNKR'],house,selected=False)
            actor_templates['NABNKR']=bytes(u.mem_read(actor,0x900))
        manager=m.alloc(0x60)
        u.mem_write(0x89E0F0,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        maximum=m.read32(m.read32(types['MIND']+0x898)+0xA4)
        infinite=u.mem_read(m.read32(types['MIND']+0x898)+0x140,1)[0]
        m.invoke(0x4717D0,manager,(actor,maximum,infinite))
        manager_nodes=m.alloc(16)
        manager_template=bytes(u.mem_read(manager,0x60))
    u.mem_write(0xA83D4C, words(house))
    u.mem_write(0x887324, words(tactical))
    u.mem_write(tactical, words(0x7F4348))
    camera, size = ([-384, 376], [160, 160]) if building else ([-380, 385], [160, 120])
    u.mem_write(tactical + 0xB0, words(*camera))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    u.mem_write(0x886FA0, words(0, 0, *size))
    u.mem_write(0xB0CE30, words(*size))
    u.mem_write(clip, words(0, 0, *size))
    u.mem_write(0xB73550, words(1))
    base_actor = dict(health=750 if building else 300, selected=True, hovered=False, sinking=False,
                      location=[2688, 5248, 0])
    cases = [dict(name=f'healthy_a{a}', a_value=a) for a in (127, 63, 2, 0)]
    cases += [dict(name='healthy_occluding_z', old_z=0),
              *[dict(name=name, actors=[dict(base_actor, health=hp)])
                for name,hp in ((('yellow',375),('red',187),('critical',1),
                                 ('above_yellow',376),('above_red',188)) if building else
                                (('yellow',150),('red',75),('critical',1),
                                 ('above_yellow',151),('above_red',76)))],
              dict(name='hover_only', actors=[dict(base_actor, selected=False, hovered=True)]),
              dict(name='absent', actors=[dict(base_actor, selected=False)]),
              dict(name='sinking', actors=[dict(base_actor, sinking=True)]),
              dict(name='clipped', actors=[dict(base_actor, location=([2364,5572,0] if not building else [2368,5568,0]))]),
              dict(name='ordered_overlap', actors=[base_actor,
                  dict(base_actor, health=187 if building else 75, location=[2705,5231,0])])]
    if rank:
        base_actor.update(rank='veteran',selected=False,owner_is_current=True,
                          owned_by_current_house=True,cloak_state=0,cloak_progress=0,
                          disguised=False,disguised_as_current_house=False,
                          sensor_count=0,disguise_detect_count=0)
        cases = _unit_rank_cases(base_actor)
    if passenger:
        base_actor.update(health=m.read32(typ+0xA0),type_name='FV',kind='unit',
            passenger='E1',unload=False,rank='rookie',owner_is_current=True,
            owned_by_current_house=True,cloak_state=0,cloak_progress=0,
            disguised=False,disguised_as_current_house=False,sensor_count=0,
            disguise_detect_count=0)
        cases=_passenger_cases(base_actor)
        cases += [dict(name='garrison_'+name,actors=[dict(base_actor,type_name='NABNKR',
            kind='building',health=m.read32(types['NABNKR']+0xA0),passenger='E1',
            occupants=['E1'],occupy_pip_override=frame,unrelated_e1_pass=frame==1)])
            for name,frame in (('physical',m.read32(types['E1']+0xE00)),('missing_pass',1))]
    if ammo:
        base_actor.update(health=m.read32(typ+0xA0),type_name='ORCA',kind='aircraft',
            ammo_initialization='full',rank='rookie',owner_is_current=True,
            owned_by_current_house=True,cloak_state=0,cloak_progress=0,disguised=False,
            disguised_as_current_house=False,sensor_count=0,disguise_detect_count=0)
        cases=_ammo_cases(base_actor)
    if groups:
        base_actor.update(type_name='MTNK',kind='unit',rank='rookie',owner_is_current=True,
            owned_by_current_house=True,cloak_state=0,cloak_progress=0,disguised=False,
            disguised_as_current_house=False,sensor_count=0,disguise_detect_count=0,
            group_commands=[dict(operation='assign',group=1,selected=True)])
        cases=_control_group_cases(base_actor)
    if wrench:
        base_actor.update(type_name='GAPOWR',kind='building',health=600,selected=False,
                          repairing=True,owner_is_current=True)
        cases=[dict(name=f'repairing_a{a}',a_value=a) for a in (127,63,2,0)]
        cases += [dict(name='frame4',frame=4),dict(name='occluding_z',old_z=0),
            dict(name='repair_off',actors=[dict(base_actor,repairing=False)]),
            dict(name='selected',actors=[dict(base_actor,selected=True)]),
            dict(name='hovered',actors=[dict(base_actor,hovered=True)]),
            dict(name='other_owner',actors=[dict(base_actor,owner_is_current=False)]),
            dict(name='sinking',actors=[dict(base_actor,sinking=True)]),
            dict(name='shrouded',cell_bits=0),dict(name='ground_open_only',cell_bits=8),
            dict(name='clipped',actors=[dict(base_actor,location=[2368,5568,0])])]
    rows = []
    original_code = sha(u.mem_read(0x401000, 0x3E0000))
    original_vtables = bytes(u.mem_read(0x7F5C70, 0x750))
    for case in cases:
        spec = dict(size=size, camera=camera, background=0xF940, a_value=127,
                    old_z=65535, depth_baseline=32768, clip=[0, 0, *size],
                    actors=[base_actor]) | case
        spec.pop('name')
        if wrench:
            spec.update(entry_boundary='extras_prefix',frame=case.get('frame',0),speed=4)
            u.mem_write(0xA8ED84,words(spec['frame']))
            u.mem_write(0xA8EB60,words(spec['speed']))
            for cell in scene['cells']:u.mem_write(cell+0x12C,words(spec.get('cell_bits',0x18)))
        if groups:
            from tools import selfheal_oracle as selfheal
            for key,offset in (('self_heal_infantry',selfheal.HG_INF),('self_heal_units',selfheal.HG_UNIT)):
                u.mem_write(house+offset,words(spec.get(key,0)))
            patch_sections,_=lexical(spec.get('scenario_patch','').encode(),set(group_types))
            saved_patch_fields={}
            if patch_sections:
                m.rules_cache(patch_sections)
                for type_name,keys in patch_sections.items():
                    p=group_types[type_name]
                    saved_patch_fields[type_name]={key:bool(u.mem_read(p+0xD97,1)[0]) if key=='Organic' else m.read32(p+0x3E0) for key in keys}
                    for key,begin,end in (('Organic',0x715024,0x715045),('PixelSelectionBracketDelta',0x71415F,0x714179)):
                        if key in keys:_status_reader_block(m,begin,end,{UC_X86_REG_EBP:p,UC_X86_REG_EBX:p+0x24,
                            UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES})
        if mobile_status:
            spec['actors'] = [dict(actor) for actor in spec['actors']]
        u.mem_write(clip, words(*spec['clip']))
        buffers = prepare_surface(u, spec, size=tuple(size))
        guard = bytes([0xA5]) * 16
        for buffer in (PIXELS, Z, A):
            u.mem_write(buffer - 16, guard)
            u.mem_write(buffer + size[0] * size[1] * 2, guard)
        recorder = _HealthRecorder(m,size,shape_names,phase_aware=building or mobile_status,
                                   rank=rank,scales=scales,group_status=groups,wrench=wrench)
        text_recorder=group_text.TextRecorder(m,recorder,size) if groups else None
        stores, draws, entries, rng, actors = (recorder.stores, recorder.draws,
                                             recorder.entries, recorder.rng, [])
        for actor_index, supplied in enumerate(spec['actors']):
            recorder.actor_index = actor_index
            if groups:actor=group_actors[supplied['type_name']]
            is_garrison=passenger and supplied['type_name']=='NABNKR'
            u.mem_write(actor,group_templates[supplied['type_name']] if groups else actor_templates[supplied['type_name']] if scales else
                        actor_template if building or rank or groups else bytes(0x900))
            if not building and not mobile_status:
                u.mem_write(actor, words(0x7F5C70))
                u.mem_write(actor + 0x6C4, words(typ))
            if mobile_status and not is_garrison:
                setter,args=_prepare_rank_actor(m,actor,house,supplied,
                    status_runtimes[supplied['type_name']] if scales else rank_runtime)
                supplied['veterancy_raw_bits'] = m.read32(actor+0x150)
            else:
                u.mem_write(actor + 0x21C, words(house))
            # Techno ctor6F2B99/6F2D0F stores-1: no control-group assignment.
            u.mem_write(actor + 0x214, words(-1))
            u.mem_write(actor + 0x6C, words(supplied['health']))
            u.mem_write(actor + 0x83, bytes([supplied['selected']]))
            u.mem_write(actor + 0x431, bytes([supplied['hovered']]))
            u.mem_write(actor + 0x3CD, bytes([supplied['sinking']]))
            u.mem_write(actor + 0x9C, words(*supplied['location']))
            if wrench:
                u.mem_write(actor+0x6E8,bytes([supplied['repairing']]))
                u.mem_write(actor+0x21C,words(house if supplied['owner_is_current'] else other_house))
            if groups:
                u.mem_write(house+0x56F9,bytes(house_rgb))
                group_events=group_text.apply_group_commands(m,actor,house,supplied['group_commands'])
            if ammo:
                ammo_events=_prepare_ammo_state(m,actor,typ,supplied['ammo_initialization'])
            cargo_events=[]
            if passenger:
                cargo_entries=[]
                def cargo_calls(_u,pc,_size,_data):
                    if pc in (0x4733A0,0x473430,0x51DF10,0x4DB260,0x6F6AC0):
                        cargo_entries.append(f'{pc:08X}')
                cargo_hook=u.hook_add(UC_HOOK_CODE,cargo_calls)
                if supplied['passenger'] and not is_garrison:
                    prepared=passengers[supplied['passenger']]
                    u.mem_write(prepared['actor'],prepared['template'])
                    m.invoke(0x4733A0,actor+0x114,(prepared['actor'],))
                    head=m.read32(actor+0x118)
                    head_type=m.invoke(m.read32(m.read32(head)+0x84),head)
                    cargo_events.append(dict(operation='add',count=m.read32(actor+0x114),
                        head_type=m.string(head_type+0x24)))
                    if supplied['unload']:
                        removed=m.invoke(0x473430,actor+0x114)
                        assert removed==prepared['actor']
                        cargo_events.append(dict(operation='remove',count=m.read32(actor+0x114),
                                                 head_is_null=m.read32(actor+0x118)==0))
                u.hook_del(cargo_hook)
                if 'capture_count' in supplied:
                    u.mem_write(manager,manager_template)
                    # Explicit retained manager vector prior, not captures or Update.
                    u.mem_write(manager+0x28,words(manager_nodes,supplied['capture_count']))
                    u.mem_write(manager+0x34,words(supplied['capture_count'],4))
                    u.mem_write(actor+0x2BC,words(manager))
                if 'storage' in supplied:
                    # Storage values are explicit render-consumer priors. Actual
                    #6C9650/6C9680 and independent70A2xx rounding read them.
                    u.mem_write(actor+0x33C,struct.pack('<4f',*supplied['storage']))
                if is_garrison:
                    e1=types['E1']
                    e1_before_pips=bytes(u.mem_read(e1+0xDFC,8))
                    if supplied['unrelated_e1_pass']:
                        m.rules_cache({'E1':{'Strength':'125'}})
                        _passenger_pip_reads(m,e1)
                    assert m.read32(e1+0xE00)==supplied['occupy_pip_override']
                    occupants=m.alloc(4*len(supplied['occupants']))
                    for index,name in enumerate(supplied['occupants']):
                        u.mem_write(occupants+index*4,words(passengers[name]['actor']))
                    u.mem_write(actor+0x688,words(occupants,len(supplied['occupants'])))
                    u.mem_write(actor+0x694,words(len(supplied['occupants'])))
            m.invoke(0x447AC0 if building or is_garrison else 0x5F65A0, actor, (coord,))
            projected = bool(m.invoke(0x6D2140, tactical, (coord, point)) & 255)
            actors.append(dict(point=rally.ints(u, point, 2), projection_admitted=projected))
            actor_size = 0x800 if building else 0x900
            before = bytes(u.mem_read(actor, actor_size))
            direct_health = is_garrison or (building and not wrench and case['name'] not in ('hover_only','absent','sinking'))
            entry = 0x6F64A0 if direct_health else 0x6F5190
            if building or mobile_status:
                actors[-1].update(coords=rally.ints(u, coord, 3), entry=f'{entry:08X}')
            if wrench:
                m.invoke(0x459EF0,actor,(render_coord,))
                render_admitted=bool(m.invoke(0x6D2140,tactical,(render_coord,render_point))&255)
                actors[-1].update(render_coords=rally.ints(u,render_coord,3),
                    render_point=rally.ints(u,render_point,2),render_projection_admitted=render_admitted)
            if mobile_status and not is_garrison:
                actors[-1].update(rank_setter=dict(entry=f'{setter:08X}',args=list(args)))
            if passenger:
                current_type=types[supplied['type_name']]
                actors[-1].update(cargo_operations=cargo_events,cargo_entries=cargo_entries,
                    cargo_count=m.read32(actor+0x114),
                    pip_max=m.invoke(0x716290,current_type))
                if case['name']=='integer_size_passenger':
                    actors[-1]['cargo_size']=m.invoke(0x473460,actor+0x114)
                if is_garrison:
                    actors[-1].update(garrison_slots=m.read32(current_type+0x1580),
                        show_occupant_pips=bool(u.mem_read(current_type+0x1584,1)[0]))
            if ammo:
                actors[-1].update(ammo_events=ammo_events,
                    initialized_ammo=ammo_events[0]['ammo'],
                    current_ammo=struct.unpack('<i',u.mem_read(actor+0x2FC,4))[0],
                    pending_release=bool(u.mem_read(actor+0x6C8,1)[0]),
                    pip_max=m.invoke(0x716290,typ),fill_count=m.invoke(0x708C30,actor))
            if groups:
                actors[-1].update(group_slot=struct.unpack('<i',u.mem_read(actor+0x214,4))[0],
                    group_events=group_events,house_rgb=list(u.mem_read(house+0x56F9,3)),
                    type_fields=dict(organic=bool(u.mem_read(group_types[supplied['type_name']]+0xD97,1)[0]),
                        delta=struct.unpack('<i',u.mem_read(group_types[supplied['type_name']]+0x3E0,4))[0]),
                    self_heal_queries=dict(infantry=bool(m.invoke(selfheal.HAS_INF,house)&255),
                                           units=bool(m.invoke(selfheal.HAS_UNIT,house)&255)))
            draw_fpcw=u.reg_read(UC_X86_REG_FPCW)
            if wrench:
                u.mem_write(READER_SP,words(native.RET_MAGIC,point,clip))
                u.reg_write(UC_X86_REG_ESP,READER_SP);u.reg_write(UC_X86_REG_ECX,actor)
                stop=native.run_checked(u,entry,(native.RET_MAGIC,0x6F534E),count=1000000)
                actors[-1]['stop']=f'{stop:08X}'
            else:
                m.invoke(entry, actor, (point, clip, 0) if direct_health else (point, clip))
            recorder.finish()
            assert bytes(u.mem_read(actor, actor_size)) == before
            if is_garrison:
                u.mem_write(types['E1']+0xDFC,e1_before_pips)
        if text_recorder:text_recorder.close()
        recorder.close()
        if building or mobile_status:
            assert not recorder.line_calls
        raw = bytes(u.mem_read(PIXELS, size[0] * size[1] * 2))
        pixels = [[i % size[0], i // size[0], value[0]] for i, value in
                  enumerate(struct.iter_unpack('<H', raw)) if value[0] != spec['background']]
        assert not rng
        assert bytes(u.mem_read(A, len(buffers['lighting']))) == buffers['lighting']
        assert bytes(u.mem_read(Z, len(buffers['depths']))) == buffers['depths']
        for buffer in (PIXELS, Z, A):
            assert bytes(u.mem_read(buffer - 16, 16)) == guard
            assert bytes(u.mem_read(buffer + size[0] * size[1] * 2, 16)) == guard
        rows.append(dict(name=case['name'], input=spec, actor_results=actors,
                         draws=draws, entries=entries, stores=stores, pixels=pixels,
                         a_unchanged=True, z_unchanged=True, rng_entries=rng))
        if groups:
            rows[-1].update(text_draws=text_recorder.rows,font_entries=text_recorder.entries)
            for text in text_recorder.rows:
                assert [stage['stage'] for stage in text['stages']]==['fill','outline','glyph']
                assert not any(stage[key] for stage in text['stages'] for key in
                               ('a_reads','z_reads','a_writes','z_writes'))
            if case['name'].startswith('ordered_'):
                first_text=next(text for text in text_recorder.rows if text['actor']==0)
                rows[-1]['overwritten_group_pixels']=sorted({(x,y,value,
                    struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0])
                    for x,y,value in first_text['glyph_stores']
                    if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},key=lambda p:(p[1],p[0]))
            if 'self_heal' in case['name']:
                marker_draws=[draw for draw in draws if draw['asset']=='PIPS.SHP' and draw['frame'] in (13,20)]
                rows[-1]['overwritten_self_heal_pixels']=sorted({(x,y,value,
                    struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0])
                    for draw in marker_draws for x,y,value in stores[slice(*draw['store_range'])]
                    if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},key=lambda p:(p[1],p[0]))
                assert marker_draws and all(draw['flags']==0x600 and not any(draw[key] for key in
                    ('a_reads','z_reads','a_writes','z_writes')) for draw in marker_draws)
        if rank and case['name']=='ordered_overlap':
            first_rank=next(draw for draw in draws if draw['actor']==0 and draw['frame']==14)
            rows[-1]['overwritten_rank_pixels']=sorted({(x,y,value,
                struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0])
                for x,y,value in stores[slice(*first_rank['store_range'])]
                if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},
                key=lambda p:(p[1],p[0]))
        if scales and case['name']=='ordered_overlap':
            rows[-1]['overwritten_scale_pixels']=sorted({(x,y,value,
                struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0])
                for draw in draws if draw['actor']==0 and draw['asset']=='PIPS2.SHP'
                for x,y,value in stores[slice(*draw['store_range'])]
                if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},key=lambda p:(p[1],p[0]))
        if building or mobile_status:
            rows[-1]['store_count'] = len(rows[-1].pop('stores'))
            for draw in draws:
                begin, end = draw.pop('store_range')
                draw['color_stores'] = end - begin
        if mobile_status:
            rows[-1].update(gates=recorder.gates,callbacks=recorder.callbacks)
        if wrench:
            rows[-1].update(gates=recorder.gates,callbacks=recorder.callbacks)
            assert all(draw['asset']=='WRENCH.SHP' and draw['flags']==0xE00
                and not any(draw[key] for key in ('z_reads','z_writes','a_writes')) for draw in draws)
        if scales:
            rows[-1].update(numeric_results=recorder.numeric,drawing_fpcw=draw_fpcw)
            assert all(draw['flags']==0x600 and not any(draw[key] for key in
                ('a_reads','z_reads','a_writes','z_writes')) for draw in draws
                if draw['asset']=='PIPS2.SHP' or is_garrison)
        if groups and saved_patch_fields:
            m.rules_cache({name:{key:('yes' if value else 'no') if key=='Organic' else str(value)
                                 for key,value in keys.items()} for name,keys in saved_patch_fields.items()})
            for name,keys in saved_patch_fields.items():
                for key,begin,end in (('Organic',0x715024,0x715045),('PixelSelectionBracketDelta',0x71415F,0x714179)):
                    if key in keys:_status_reader_block(m,begin,end,{UC_X86_REG_EBP:group_types[name],
                        UC_X86_REG_EBX:group_types[name]+0x24,UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES})
    assert original_code == sha(u.mem_read(0x401000, 0x3E0000))
    assert original_vtables == bytes(u.mem_read(0x7F5C70, 0x750))
    if scales:
        assert tail_defaults['spawns_number']==tail_type()['spawns_number']==0
        assert tail_defaults['organic']==tail_type()['organic']==False
    else:
        assert tail_defaults == tail_type()
    result = dict(schema_version=1, native_sha256=native.image_sha256(), physical_inputs=physical,
                  readers=dict(constructor=constructor, layers=layers, result=fields(),
                               ordinary_tail_constructor=tail_defaults),
                  palette=palette_result, raster_controls=rows, original_code_unchanged=True)
    if rank:
        result['rank_runtime'] = rank_runtime['evidence']
        result['vtable_bindings'] = _rank_vtable_bindings(m)
    if groups:
        result.update(status_runtime={'MTNK':rank_runtime['evidence']},
            assignment_controls=group_text.assignment_controls(m,actor_template),
            house_color=dict(name='Gold',native_source='house_color.json Gold scalar House50B840/500DF7',rgb=house_rgb),
            native_font=dict(chain='434AD0->433880->433990->434700',
                runtime_fields={k:v for k,v in native_font['font_meta'].items() if k not in
                                ('font_object','renderer_object','font_data','lookup','glyphs')},
                digit_metrics=native_font['native_digit_metrics'],
                file_reads=[dict(file_offset=o,size=n) for o,n in native_font['expected_reads']]))
        result['status_runtime']['E1']=prepared_infantry['evidence']
        result['readers']['type_results']={name:dict(strength=m.read32(p+0xA0),
            delta=m.read32(p+0x3E0),organic=bool(u.mem_read(p+0xD97,1)[0])) for name,p in group_types.items()}
        assert all(next(row for row in rows if row['name']==name)['overwritten_group_pixels']
                   for name in ('ordered_groups','ordered_health'))
        assert next(row for row in rows if row['name']=='ordered_self_heal_group')['overwritten_self_heal_pixels']
    if scales:
        if passenger:result['read_pip']=pip_controls
        result['readers'].update(type_constructors=type_constructors,
            type_results={name:_passenger_type_fields(m,p,name in ('E1','ENGINEER'),ammo=ammo)
                          for name,p in types.items()})
        result['status_runtime']={name:runtime['evidence'] for name,runtime in status_runtimes.items()}
        result['passenger_runtime']={name:prepared['evidence'] for name,prepared in passengers.items()}
        for name,p in types.items():
            result['readers']['type_results'][name]['pip_max']=m.invoke(0x716290,p)
        if passenger:
            result['readers'].update(building_layers=scene['layer_reads'],
                                    building_layer_fields=scene['type_layer_fields'])
            result['garrison_controls']=[row for row in rows if row['name'].startswith('garrison_')]
            result['raster_controls']=[row for row in rows if not row['name'].startswith('garrison_')]
            result['physical_inputs'].append(dict(name='ARTMD.INI',archive='ra2md.mix/localmd.mix',
                bytes=len(m.assets['ARTMD.INI']),sha256=sha(m.assets['ARTMD.INI'])))
            result['readers']['type_results']['NABNKR']=_passenger_type_fields(
                m,types['NABNKR'],building=True)
            result['readers']['type_results']['NABNKR']['pip_max']=m.invoke(0x716290,types['NABNKR'])
            dims=m.alloc(12)
            m.invoke(0x464AF0,types['NABNKR'],(dims,))
            result['readers']['building_dimensions']=rally.ints(u,dims,3)
            assert all(row['admitted'] for row in scene['layer_reads'] if row['file']=='RULESMD.INI')
            assert result['readers']['type_results']['NABNKR']['max_number_occupants']>0
        if ammo:
            result['fill_controls']=_ammo_fill_controls(m,types,actor_templates,actor,
                manager,manager_template,manager_nodes,passengers)
            assert original_code==sha(u.mem_read(0x401000,0x3E0000))
        assert next(row for row in rows if row['name']=='ordered_overlap')['overwritten_scale_pixels']
    if building:
        dims = m.alloc(12)
        m.invoke(0x464AF0, typ, (dims,))
        result['readers'].update(type_layers=scene['layer_reads'],
                                type_layer_fields=scene['type_layer_fields'],
                                dimensions=rally.ints(u,dims,3))
        result['physical_inputs'] += scene['sources']
        result['physical_inputs'].append(dict(name='ARTMD.INI',archive='ra2md.mix/localmd.mix',
            bytes=len(m.assets['ARTMD.INI']),sha256=sha(m.assets['ARTMD.INI'])))
        result['terrain_inputs'] = scene['terrain']
        result['body_priors'] = dict(ground_cells=scene['ground_cells'],bib_null=True,
                                    color_scheme=scene['color_scheme'])
        result['composition_controls'] = _building_health_composition(
            scene, ground_planes, actor_template, shape_names,wrench=wrench)
        if wrench:
            result['health_palette']=health_palette
    return result


def _building_health_composition(scene, ground, actor_template, shape_names, *, wrench=False):
    """Whole Extras/body/late Extras in the existing native ground owner.

    Only pixels addressed by actual health stores cross the consumer boundary;
    full native color/Z planes are hashed rather than retained as body goldens.
    """
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, SURFACE, PIXELS, Z, A
    m, u = scene['m'], scene['u']
    actor, typ, house, tactical, coord, point, clip = (scene[key] for key in
        ('building', 'typ', 'house', 'tactical', 'coord', 'point', 'clip'))
    size = [160, 160]
    rows = []
    neighbor = m.alloc(0x800)
    _prepare_ground_building(m, neighbor, typ, house,
        location=(3200,5248,0) if wrench else (2176,5248,0), selected=wrench)
    neighbor_template = bytes(u.mem_read(neighbor, 0x800))
    for with_neighbor in (False, True):
        prepare_surface(u, dict(depth_baseline=32768, old_z=65535, a_value=127,
                                background=rally.BACKGROUND), size=tuple(size))
        u.mem_write(SURFACE, words(scene['vtable']))
        for name,address in (('color',PIXELS),('z',Z),('a',A)):
            u.mem_write(address, ground[name])
        u.mem_write(actor, actor_template)
        u.mem_write(actor + 0x6C, words(600 if wrench else 750))
        u.mem_write(neighbor, neighbor_template)
        if wrench:
            u.mem_write(0xA8ED84,words(0));u.mem_write(0xA8EB60,words(4))
            u.mem_write(actor+0x6E8,b'\1')
            u.mem_write(neighbor+0x6C,words(187))
            for cell in scene['cells']:u.mem_write(cell+0x12C,words(0x18))
        recorder = _HealthRecorder(m, size, shape_names, phase_aware=True)
        snapshots, planes, depth_planes = [], {}, {}
        def run_phase(phase, receiver, entry, actor_index):
            recorder.actor_index, recorder.phase = actor_index, phase
            begin = len(recorder.stores)
            if entry == 0x43CEA0:
                m.invoke(entry, receiver, (clip, 1, 0))
            else:
                m.invoke(0x447AC0, receiver, (coord,))
                m.invoke(0x6D2140, tactical, (coord, point))
                m.invoke(entry, receiver, (point, clip))
            recorder.finish()
            raw = bytes(u.mem_read(PIXELS, 51200))
            planes[phase] = raw
            depth_planes[phase] = bytes(u.mem_read(Z,51200))
            snapshots.append(dict(phase=phase, stores=len(recorder.stores)-begin,
                color_sha256=sha(raw), z_sha256=sha(u.mem_read(Z,51200))))
        if with_neighbor and not wrench:
            run_phase('neighbor_behind',neighbor,0x6F60D0,1)
            run_phase('neighbor_first_extras',neighbor,0x6F5190,1)
            run_phase('neighbor_body',neighbor,0x43CEA0,1)
        run_phase('behind',actor,0x6F60D0,0)
        run_phase('first_extras',actor,0x6F5190,0)
        run_phase('body',actor,0x43CEA0,0)
        if with_neighbor and wrench:
            run_phase('neighbor_behind',neighbor,0x6F60D0,1)
            run_phase('neighbor_first_extras',neighbor,0x6F5190,1)
            run_phase('neighbor_body',neighbor,0x43CEA0,1)
        if with_neighbor and not wrench:
            run_phase('neighbor_late_extras',neighbor,0x6F5190,1)
        run_phase('late_extras',actor,0x6F5190,0)
        if with_neighbor and wrench:
            run_phase('neighbor_late_extras',neighbor,0x6F5190,1)
        recorder.close()
        assert not recorder.rng and bytes(u.mem_read(A,51200)) == ground['a']
        health_draws = [draw for draw in recorder.draws if draw['asset']=='PIPS.SHP']
        assert len(health_draws)==(60 if wrench and with_neighbor else 30)
        status_draws=[draw for draw in recorder.draws if draw['asset'] in ('PIPS.SHP','WRENCH.SHP')] if wrench else health_draws
        touched = sorted({(x,y) for draw in status_draws
            for x,y,_word in recorder.stores[slice(*draw['store_range'])]}, key=lambda p:(p[1],p[0]))
        def samples(raw):
            return [[x,y,struct.unpack_from('<H',raw,(y*160+x)*2)[0]] for x,y in touched]
        after_body = planes['neighbor_body' if wrench and with_neighbor else 'body']
        final = planes['neighbor_late_extras' if wrench and with_neighbor else 'late_extras']
        before = planes['first_extras']
        overwritten = [[x,y,struct.unpack_from('<H',before,(y*160+x)*2)[0],
                       struct.unpack_from('<H',after_body,(y*160+x)*2)[0],
                       struct.unpack_from('<H',final,(y*160+x)*2)[0]]
                      for x,y in touched
                      if before[(y*160+x)*2:(y*160+x)*2+2] !=
                         after_body[(y*160+x)*2:(y*160+x)*2+2]]
        if wrench:
            health_touched={(x,y) for draw in health_draws
                for x,y,_word in recorder.stores[slice(*draw['store_range'])]}
            wrench_pixels={(x,y):word for draw in recorder.draws if draw['asset']=='WRENCH.SHP'
                for x,y,word in recorder.stores[slice(*draw['store_range'])]}
            wrench_overwrites=[[x,y,word,struct.unpack_from('<H',final,(y*160+x)*2)[0]]
                for (x,y),word in sorted(wrench_pixels.items(),key=lambda v:(v[0][1],v[0][0]))
                if struct.unpack_from('<H',final,(y*160+x)*2)[0]!=word]
        for draw in recorder.draws:
            begin, end = draw.pop('store_range')
            draw['color_stores'] = end - begin
        rows.append(dict(name=('neighbor_after' if wrench else 'neighbor_before') if with_neighbor else 'single',
            input=dict(size=size,camera=[-384,376],a_value=127,depth_baseline=32768,
                       actors=[dict(location=[2688,5248,0],health=600 if wrench else 750,selected=True,**(dict(type_name='GAPOWR',kind='building',repairing=True) if wrench else {}))]+
                              ([dict(location=[3200,5248,0] if wrench else [2176,5248,0],selected=wrench,**(dict(type_name='GAPOWR',kind='building',health=187,repairing=False) if wrench else {}))] if with_neighbor else []),
                       placed_byte_6e4=True,height_factor=104),
            snapshots=snapshots,entries=recorder.entries,line_calls=recorder.line_calls,
            draws=recorder.draws,body_admissions=recorder.body_admissions,
            ground_pixels=samples(ground['color']),ground_z=samples(ground['z']),
            before_body_pixels=samples(before),before_late_pixels=samples(after_body),
            before_late_z=samples(depth_planes['neighbor_body' if wrench and with_neighbor else 'body']),
            pixels=samples(final),body_overwritten_health_pixels=overwritten,
            a_unchanged=True,rng_entries=recorder.rng,original_code_unchanged=True))
        if wrench:
            wrench_touched=set(wrench_pixels)
            # Pixel families are identified from the original per-shape stores,
            # before compacting their ranges above. Wrench witnesses are exact.
            rows[-1].update(overwritten_wrench_pixels=wrench_overwrites,
                body_overwritten_wrench_pixels=[row for row in overwritten if tuple(row[:2]) in wrench_touched],
                entry_boundary='complete_extras_body',frame=0,speed=4)
            rows[-1]['body_overwritten_health_pixels']=[row for row in overwritten if tuple(row[:2]) in health_touched]
    assert scene['original_code'] == sha(u.mem_read(0x401000,0x3E0000))
    return rows


def repair_wrench_metadata():
    return native.provenance(
        scope='Ordinary repairing GAPOWR wrench prefix and complete first-Extras/body/late-Extras composition',
        assumptions=[
            'One optional wrench mode extends the existing selected-health generator, _HealthRecorder, Building ground fixture and PaletteReader. Original allocator/INI/file transports, Surface/circular A/Z, TMP ground, native palette and SHP/body/line owners are shared. No second VM, parser, palette table, rasterizer, repair state or body mask produces expected output. Historical modes retain their startup and allocation order.',
            'Whole Rules665650/BuildingType45DD90 and45FE50 consume physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4; fixed ARTMD remains unlayered. Constructor and each admitted layer retain native Strength/Cost/Repairable/ClickRepairable. Repairable defaults true711454 and current-default5295F0 reads843950 at714A8C; ClickRepairable defaults true45E076 and current-default5295F0 reads81ADCC at4600C0. These fields are repair-command prerequisites; wrench drawing itself reads the authoritative Building repair byte6E8.',
            'Original45E98D..45E9A3 executes literal WRENCH.SHP lookup5B40B0 and publishes89DDC8. Existing palette owner additionally executes unchanged Init52C136..52C1EA: literal MOUSEPAL.PAL load, copy/byte expansion and full Convert48E740/48EBF0 N1 constructor publish87F6C8. Physical archive candidates and exact hashes are recorded; the production normal STOCK_DIGITAL AssetManager consumer must independently establish winner/loose-override precedence. The oracle does not execute native archive search.',
            'Component rows enter original6F5190 and stop before6F534E, after the complete wrench branch and before observer/rank/health. Actual sinking returns to the sentinel earlier. Original RTTI6, repair byte, Cell487950/586360, virtual459EF0 and Tactical6D2140 execute. Raw Location, repairing/selection/hover/sinking/owner fields, frame, speed and sparse Cell flags are declared consumer priors, not repair start/payment/Flash/stop or reveal lifecycle. Prepared sinking/other-owner/shrouded controls do not imply ordinary GAPOWR production of those states.',
            'Native SpeedNormalize5FB2E0 and signed frame arithmetic choose actual frame0 or6. Native4AED70/picker490B90/rowwalker/493DF0 run unchanged with flagsE00, remap0,zAdjust0,gradient0,brightness1000. The real leaf reads A and bypasses Z; N1 table construction yields identical pixels for A127/63/2/0. A2 is the existing stock SHROUD frame15 consumer value. OldZ0 and viewport-edge clipping are actual raster controls, with no substituted frame, projection, admission or pixel result.',
            'Composition reuses actual flat Cells47BBF0/TMP480350, original Building43B740/idle44D6A0, BUILDNGZ45E8F0, full Behind6F60D0 and full6F5190 in first and late phases, with original43CEA0->455C20/459EF0->43D290->705E00 body/shadow between them. GAPOWR BibShape is NULL. The disjoint two-cell-separated neighbor follows actor0 in both supplied parent traversals. Original Display traversal/admission is an explicit scheduling prior, not a whole Display/Map-load execution.',
            'Sparse samples retain actual ground and before-late color/Z at status-addressed pixels; original full crop hashes identify intervening body intervals without retaining full planes. body_overwritten_wrench_pixels and overwritten_wrench_pixels are actual store witnesses, including a positive later-neighbor overwrite; health/bracket stores follow wrench inside each complete Extras. No visual regression is inferred merely from an instruction count. Full application/body GPU parity, arbitrary buildings, missing assets, huge foundations, other DrawExtras families, repair lifecycle and whole functions remain OPEN/PARTIAL.',
            'Drawing preserves original code/vtables, prepared actor bytes and A; component Z is unchanged. No Main/Scenario draw RNG, timer write or detach is reached. Building constructor/state initialization and supplied frame/repair/House/Cell priors are separate from draw effects. Repair command Flash7/payment/stop remains the existing production_repair/flashing evidence owner, not a new oracle state machine.',
        ], substitutions=[
            'Inherited successful allocation/delete/CRT TLS and immutable physical file/archive transport replace platform services. Existing RGB565 BSurface/circular A/Z memory replaces DirectDraw. No reached wrench, Cell, frame, palette, body, line, shape or pixel function result or original instruction is substituted.',
        ], entry_points=dict(type_ctor=0x45DD90,type_reader=0x45FE50,
            extras_begin=0x6F5190,component_stop=0x6F534E,wrench=0x6F5253,
            render_coords=0x459EF0,projection=0x6D2140,cell_shrouded=0x487950,
            coord_shrouded=0x586360,speed_normalize=0x5FB2E0,
            wrench_load_begin=0x45E98D,wrench_load_end=0x45E9A3,
            mouse_palette_begin=0x52C136,mouse_palette_end=0x52C1EA,
            convert_ctor=0x48E740,shape=0x4AED70,picker=0x490B90,leaf=0x493DF0,
            behind=0x6F60D0,body_admission=0x43CEA0,body=0x43D290))


def building_health_metadata():
    return native.provenance(
        scope='Ordinary selected GAPOWR Building health component and whole Extras/body/late-Extras composition',
        assumptions=[
            'The existing selected-health generator/recorder has one Building mode. The existing tmp_clear_render PaletteReader owns original image, native allocator/INI caches, palette/TMP/shape transports and original BSurface/circular A/Z storage; no second VM, INI parser, palette expansion, rasterizer or body mask supplies expected output.',
            'Whole Rules665650/BuildingType45DD90 constructors and complete45FE50 reader consume physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4; ARTMD is fixed. Native constructor and per-layer type fields are retained. Original ConditionRed/Yellow read/store blocks consume each matching AudioVisual section. Supplied startup height factor104 feeds native464AF0; GetCoords447AC0 and Tactical6D2140 derive points from raw Object Location/camera.',
            'PIPS21frame/PIPBRD/PALETTE, GGPOWR6frame/UNITTEM/BUILDNGZ and fixed ART archive bytes match normal STOCK_DIGITAL production AssetManager winners, with no loose override. Existing physical archive transport is supplied rather than a native archive search. Full original palette/LightGold constructors execute in the same owner.',
            'Positive component rows enter complete6F64A0 directly with third argument0: these are an explicit health consumer boundary and do not establish caller admission. Hover-only/absent/sinking rows instead run complete6F5190 so actual caller gates execute without selected bracket contamination. Actor linkage, health, selection, hover, sinking and positions are supplied inputs, not damage/selection/placement lifecycle proof. The sinking field is a prepared negative control, not stock GAPOWR reachability.',
            'Every actual4AED70 frame/point/flags/clip and picker/rowwalker/leaf runs unchanged. Building uses physical PIPS0/1/2/4, flags600 plain/noA/noZ, including empty pips; this is distinct from mobile PIPBRD E00. Components cover HP750/375/187/1, immediate-above thresholds, A127/63/2/0, oldZ0, real viewport-edge clipping and ordered strips. A2 is existing original SHROUD frame15 output, not regenerated reveal lifecycle.',
            'Composition reuses actual original flat constructed/revealed Cells47BBF0 and TMP480350, full Building43B740/idle44D6A0, BUILDNGZ45E8F0 and43CEA0->455C20/459EF0->43D290->705E00->4AED70 body/shadow. Native GAPOWR BibShape isNULL. Whole Behind, complete first Extras, actual admitted body and complete late Extras execute in that order; the neighbor case adds an actual earlier unselected stock body and its empty Extras. Sparse Map enrollment, traversal order, placed byte6E4, House/remap/lighting/frame and viewport are supplied priors, not whole Display/Scenario/Map loading.',
            'The measured single and earlier-neighbor controls have zero body-overwritten health pixels: their first and late health stores are opaque/idempotent at these sampled locations. They establish the original call order with an actual intervening body, not a positive visible ordering regression.',
            'Phase-aware records close each shape at its real caller return before bracket lines. Only health-addressed native ground color/Z, RGB565 samples before body/before late/final and actual after-body Z and body-overwrite witnesses are retained for composition consumers; full native crop color/Z hashes identify the actual body interval. A/Z component invariance, no RNG and unchanged original code are asserted. These controls do not establish a GPU body renderer, other Building types/status tails, whole frame, or health damage lifecycle.',
        ], substitutions=[
            'Inherited native Reader allocator/delete/CRT TLS and immutable physical file/archive transport; existing BSurface/circular A/Z memory replaces DirectDraw. No reached health/tail/body/line/palette/shape function result or native instruction is substituted.',
        ], entry_points=dict(rules_ctor=0x665650,type_ctor=0x45DD90,type_reader=0x45FE50,
            dimension2=0x464AF0,get_coords=0x447AC0,projection=0x6D2140,
            behind=0x6F60D0,extras=0x6F5190,health=0x6F64A0,pip_tail=0x709A90,
            body_admission=0x43CEA0,body=0x43D290,render_coords=0x459EF0,
            shape=0x4AED70,picker=0x490B90,fill_leaf=0x4912B0,
            building_zshape=0x45E8F0,cell_tile=0x480350,
            pips_draw_for_all_ctor=0x710D22,pips_draw_for_all_reader=0x71413A,
            can_be_occupied_ctor=0x45E07D,can_be_occupied_reader=0x4600DA))


def unit_rank_metadata():
    return native.provenance(
        scope='Ordinary MTNK rank prefix through complete DrawExtras and physical PIPS14/15/19 pixels',
        assumptions=[
            'One existing selected-health generator and _HealthRecorder own the Unit-rank mode. Existing PaletteReader owns original image/allocator/native INI cache, palette constructors and file transports; existing bridge_render_shape owns Surface/circular A/Z storage. No second VM, INI parser, rank state, palette, SHP decoder or rasterizer produces expected output.',
            'Whole Rules665650/UnitType7470D0 constructors and the unchanged selected native Strength/PipScale/PixelSelectionBracketDelta/ConditionRed/Yellow reader blocks consume physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4. Selected physical layers leave Invisible unset; the visual predicate consumes its recorded native constructorfalse. Fixed ART supplies no rank keys;70A990 has no Trainable or tunable frame/threshold key. Existing physical PIPS21frame/PIPBRD/PALETTE winners and palette initialize52BE61..52BFCE execute in this owner.',
            'The established original Unit7353C0->Foot4D31E0 constructor prefix stops7354CE before the unrelated typed COM activation. Original Drive4AF540/base constructor executes; supplied owner/link/active fields expose its real virtual55ABC0, not a supplied callback return. Spare native registry capacity and seeded Main/Scenario/MapGen streams are fixture priors. Constructor RNG requests and actual retained fields are recorded. Whole Unit constructor, CoCreate, Unlimbo, movement, damage/kill/promotion and whole Scenario/Display are excluded.',
            'Original750080/750090/7500B0 and signed-percent7500E0(-100) initialize raw rank bits. Actor input.veterancy_raw_bits is the actual after-setter single, not a host rank calculation. Prepared health, selected/hover/sinking, raw Location, owner/current-house-owned, cloak state/progress, disguise/fake-house and cell counter values are explicit consumer priors. Ordinary stock MTNK does not produce the prepared cloak/disguise negatives in this oracle.',
            'Every row runs complete6F5190. Native746750/41C010/4F9A50 and4DA4E0->realDrive55ABC0->703860 observer predicates execute unchanged. Sparse flat/revealed47BBF0 Cells share the existing ground fixture owner; currentHouse index0, enemyHouse index1, no mutual alliance, supplied Cell+7C sensor and+AC disguise-detection signed counters feed original4870D0/4870F0. No observer result, body gate, rank admission or pixel leaf is substituted.',
            'Complete70A990 executes before selected/hover health dispatch. Veteran PIPS14, elite15 and negative19 use flagsE00, remap0, signed zAdjust-2, gradient0, brightness1000 and Convert87F6C4. Original4AED70/picker/rowwalker/493DF0 run; actual A reads, zero Z accesses, ordered shapes and final sparse RGB565 pixels are recorded. A127/63/2/0 and Z0 are consumer controls; A2 is already established physical SHROUD frame15 output, not regenerated vision lifecycle.',
            'Owned/sensed fully cloaked actors have visual character3 but retain the same opaque rank material: body cloak opacity/tint is not inherited by70A990. Selected and hover cases continue through actual6F64A0/709A90/70AA60 tails. Ordered two-parent rank+health, real viewport clip and prepared sinking are bounded drawing controls. Whole original code/vtable and actor bytes, A/Z remain unchanged; no drawing RNG request is observed. Rank interval has no timer update or detach. Native VT bindings prove Infantry/Aircraft use the same6F5190/70A990 bodies; their different RTTI anchor and observer-gate dispatch are instruction evidence, not full constructor/shape controls for those classes.',
        ], substitutions=[
            'Inherited successful allocation/delete/CRT TLS, immutable physical archive/file transport and Surface memory replace platform services. Actor admission/lifecycle and sparse Map traversal are supplied priors. No reached rank/predicate/Drive/palette/shape function result or original instruction is replaced.',
        ], entry_points=dict(unit_type_ctor=0x7470D0,unit_ctor_begin=0x7353C0,
            unit_ctor_stop=0x7354CE,foot_ctor=0x4D31E0,drive_ctor=0x4AF540,
            drive_visual=0x55ABC0,rookie_setter=0x750080,veteran_setter=0x750090,
            elite_setter=0x7500B0,percent_setter=0x7500E0,extras=0x6F5190,
            disguised_to=0x746750,visual_character=0x4DA4E0,techno_visual=0x703860,
            cell_sensor=0x4870D0,cell_disguise_detect=0x4870F0,rank=0x70A990,
            veteran_test=0x74FF90,elite_test=0x750010,negative_test=0x74FFF0,
            shape=0x4AED70,picker=0x490B90,rank_leaf=0x493DF0))


def passenger_pips_metadata():
    return native.provenance(
        scope='Stock FV passenger status through whole Extras, shared ReadPip passes and affected PIPS2/PIPS consumers',
        assumptions=[
            'The existing selected-health generator and _HealthRecorder have one additive passenger mode. Existing Building-health PaletteReader, native allocator/cache/physical transports and shape Surface/A/Z owners are reused in the same VM; no copied VM, INI/parser, cargo/manager/storage owner, palette, SHP decoder or rasterizer generates outputs.',
            'The existing Building type/runtime has an optional NABNKR health-only mode: complete BuildingType45DD90/45FE50 consumes physical RULESMD, absent LANGRULE, MPBattleMD and XMP03T4, with fixed ART. Actual Building43B740/44D6A0, GetCoords447AC0 and Dimension2464AF0 supply the affected garrison health consumer. Its body/ground drawing is disabled; no NABNKR body, placement or garrison-entry lifecycle is claimed.',
            'Complete UnitType7470D0/InfantryType5236A0 constructors and unchanged selected Strength/PipScale/delta/Passengers/Gunner/Storage/Size/SizeLimit/Pip/OccupyPip native read/store blocks consume the same selected physical layer stack. Original MIND Primary7129AB..712A1D resolves its physical weapon; Damage7722D4..7722EE consumes that weapon section. Full Unit/Infantry type file readers and unrelated keys do not run.',
            'ReadPip4748A0 and both actual Infantry call/store blocks execute on explicit strings/current constructor defaults. Original Abstract410A60 admission distinguishes absent sections from admitted unrelated-key passes. Missing defaults use native table indexes, including adjacent Ammo/Tiberium names at11/12. Explicit empty cache values are separate from physical empty INI omission. These controls expose the required existing retained-type pass-owner behavior, not a second INI implementation.',
            'Existing Unit7353C0->Foot4D31E0 prefix stops7354CE before typed COM activation, followed by real Drive4AF540. Infantry517A50 prefix stops517AE6 before ID/COM, followed by real Walk75AA90. Supplied interface owner/link/active priors expose their original virtuals. Original constructor Limbo1 permits whole Cargo4733A0->51DF10->4DB260->6F6AC0 and473430 without gameplay sinks. Whole typed construction, Unlimbo, boarding and unloading missions remain separate production tests. Selected mobile prefix RNG requests/passive timers are recorded separately from drawing; inherited Building/type/palette startup is outside that recorded initialization interval.',
            'Every mobile row executes complete6F5190, original rank/observer gates,6F64A0 and actual709A90/70AA60 tails. Native716290/473460/473450 and passenger array fill choose physical PIPS2 frames, centered origins and flags600. Garrison rows enter full6F64A0 directly with explicit prepared occupant vector/count, then actual709C27..709D17 uses raw OccupyPip11 or reader-evolved1 and emptyframe6. These direct-health rows do not establish whole Building Extras admission.',
            'One affected shared frame-consumer row uses stock LCRF Passengers12/SizeLimit6/Size16 and MTNK Size3 through the same constructors/read blocks. LCRF consumes real Hover513C20 with supplied interface linkage, matching physical Locomotor742; MTNK uses the existing Unit/Drive prefix with native Limbo1. Whole Cargo4733A0 reaches Unit4DB260/Techno6F6AC0 and retains one size3 passenger. Original709F0A..709F23 fills higher slots with frame3 before placing Unitframe5 in the lower slot, producing5,3,3 then nine empty0 frames. This is a render-consumer/whole-Cargo dependency control, not LCRF boarding, movement, placement or whole construction evidence.',
            'MIND uses the original4717D0 manager constructor with retained count/vector and overload-counter priors, not captures/Update; original4722D0/4726C0 and708C30 execute. HARV receives explicit four native float storage slots; original6C9650/6C9680 and independent ore/gem rounding execute. Stock positive Storage40 and size-one infantry match the existing gameplay state owners. Fractional size/reload behavior, manager lifecycle, harvest lifecycle and other PipScales remain open.',
            'PIPS/PIPBRD/PALETTE retain established normal STOCK_DIGITAL winners. Original5F76B0 pushes literal PIPS2.SHP at5F7758 through CCFile65CA80/loader4A3890, retainsAC1480 at5F7776 and709AEF consumes that same asset. Independent normal STOCK_DIGITAL AssetManager lookup selects ra2md.mix/conqmd.mix PIPS2,1384bytes/19frames, no loose override or catalog-only hit; ra2.mix/conquer.mix1280bytes/18frames is shadowed. The oracle pins the selected physical bytes while archive/cache transport remains supplied. Native palette/Convert constructors,4AED70/pickers/rowwalkers/leaves execute unchanged. A127/63/2/0, oldZ0, clip, sinking and ordered overlapping parents are render-consumer controls. Sparse pixels/draws and counts are native stores; no VERA scalar or host palette/raster/count formula supplies expected output.',
            'Original code/vtables, prepared actor bytes and A/Z remain unchanged across measured draws; no drawing RNG, timer write or detach occurs. The component does not run mobile bodies, whole Display/Scenario/Map, selection generation, damage/promotion or a whole native frame.',
        ],substitutions=[
            'Inherited bounded allocation/delete/CRT TLS and immutable physical archive/file transports; BSurface/circular A/Z memory replaces DirectDraw. Constructor/interface/cargo/count/storage/occupant scene priors are explicit inputs. No reached render/predicate/Drive/Walk/cargo/palette/shape function result or native instruction is replaced.',
        ],entry_points=dict(unit_type_ctor=0x7470D0,infantry_type_ctor=0x5236A0,
            building_type_ctor=0x45DD90,building_type_reader=0x45FE50,
            infantry_ctor_begin=0x517A50,infantry_ctor_stop=0x517AE6,walk_ctor=0x75AA90,
            unit_ctor_begin=0x7353C0,unit_ctor_stop=0x7354CE,drive_ctor=0x4AF540,
            hover_ctor=0x513C20,
            pip_ctor=0x5236BD,occupy_pip_ctor=0x5236C7,read_pip=0x4748A0,
            pip_caller=0x5240E3,occupy_pip_caller=0x5240FD,section_gate=0x410A60,
            cargo_add=0x4733A0,cargo_remove=0x473430,cargo_head=0x473450,cargo_size=0x473460,
            pip_max=0x716290,fill_count=0x708C30,manager_ctor=0x4717D0,
            storage_sum=0x6C9650,storage_get=0x6C9680,extras=0x6F5190,health=0x6F64A0,
            pip_tail=0x709A90,status_tail=0x70AA60,shape=0x4AED70,picker=0x490B90,
            plain_leaf=0x4912B0,rank_border_leaf=0x493DF0))


def ammo_pips_metadata():
    return native.provenance(
        scope='Ordinary selected ORCA ammo through whole Extras, shared708C30 arithmetic and existing status consumers',
        assumptions=[
            'Additive ammo mode extends the existing selected-health generator, mobile constructor-prefix helper, _HealthRecorder, PaletteReader/cache/palette/shape Surface owners. No copied VM, native loader, INI/parser, numeric emulator, cargo/manager/storage owner or rasterizer produces outputs. All historical modes retain their defaults and output payloads.',
            'Whole Rules665650/AircraftType41C8B0/UnitType7470D0/InfantryType5236A0 constructors execute in one VM. Selected original Strength/PipScale/delta/Size/SizeLimit/Passengers/Gunner/Storage and ORCA PipWrap/InitialAmmo/Ammo reader/store blocks consume physical RULESMD, absent LANGRULE, MPBattleMD and XMP03T4. Full type readers, unrelated keys and fixed ART/body inputs are excluded; these status keys are RULES inputs and ART is not layered. ORCA ctor PipWrap0/InitialAmmo-1/Ammo-1 becomes stock0/-1/1; nonzero PipWrap remains a dormant configuration branch, not unreachable.',
            'Existing mobile prefix uses original Aircraft413D20..413DB1 before COM followed by real Fly4CC9A0, or the unchanged Unit/Foot/Drive and Infantry/Walk prefixes for numeric dependencies. Supplied interface owner/link/active, selection/hover/sinking, raw Location, owner/observer, health and rank priors are render inputs. Original constructor Scenario RNG requests/passive timer initialization are recorded separately; no whole construction, Unlimbo, selection generation, flight/body or scenario lifecycle is claimed.',
            'ORCA41403A..414051 initializes actual signed current Ammo. Depleted/rearmed rows enter the original admitted-release pending writer41840E..418415 then next-entry consume418031..418056; actual whole Aircraft radio4190B0 reaches Foot4D8FB0/Techno6F4C9C for rearm1F. Successful release admission is supplied at its original pending boundary; no shot, burst, mission scheduler, dock search/landing or Building service executes in this oracle. Production Commands/captures cover those separate owners.',
            'Every raster row executes whole6F5190, original observer/Fly gates,6F64A0 and709A90/70AA60. Actual708C30/716290 choose filled-only PIPS2frame13 for stock Ammo/PipWrap0 at projected GetCoords point[-15,+7],step4. Actual4AED70/picker/4912B0 leaves use flags600 and bypass A/Z. PIPBRD keeps its independent E00 A-consuming material. A127/2/0, oldZ0, viewport clip, supplied airborne height and positive ordered overlap are bounded consumer controls, not a complete native world frame.',
            'fill_controls run complete original708C30 on small signed ORCA counts and existing physical HARV storage, MIND manager count and FV Cargo receivers. MIND4717D0/counter vectors and four float storage slots are explicit retained consumer priors; FV4733A0 uses original constructor-Limbo E1. Explicit zero/negative Storage, zero Ammo and Power/None configuration boundaries execute actual native ReadInt/ReadPipScale blocks, then restore through those readers. They do not claim stock reachability. Native53/chop division/multiplication/addition, ftol EDX:EAX and positive-total minimum1 execute without a host arithmetic substitute.',
            'PIPS/PIPBRD/PALETTE and YR PIPS2 ra2md.mix/conqmd.mix bytes match independently confirmed normal STOCK_DIGITAL AssetManager winners with no loose overrides. Original5F76B0 asks for literal PIPS2.SHP through normal lookup and709AEF consumesAC1480. Immutable archive/cache transport remains supplied. Original palette/Convert, projection, shape access, pickers and stores execute; no Rust values supply a golden.',
            'Original instructions/vtables, actor bytes and A/Z remain unchanged across measured draws and numeric queries. Drawing/query RNG is empty and no timer write or detach occurs in those intervals. This excludes setup constructor RNG/timer effects and the separate combat/flight/reload scheduler side effects; whole709A90/DrawExtras across other status families remains open.',
        ],substitutions=[
            'Inherited allocation/delete/CRT TLS, immutable physical file/archive transport and BSurface/circular A/Z backing replace platform services. Actor/interface/count/storage/manager/cargo priors are declared inputs; no reached gameplay/predicate/Fly/palette/shape function return or original instruction is replaced.',
        ],entry_points=dict(aircraft_type_ctor=0x41C8B0,unit_type_ctor=0x7470D0,
            aircraft_ctor_begin=0x413D20,aircraft_ctor_stop=0x413DB1,foot_ctor=0x4D31E0,
            fly_ctor=0x4CC9A0,visual_character=0x4DA4E0,locomotor_visual=0x55ABC0,
            ammo_init_begin=0x41403A,ammo_init_end=0x414051,pending=0x41840E,
            consume_begin=0x418031,consume_end=0x418056,aircraft_radio=0x4190B0,
            techno_reload=0x6F4C9C,fill_count=0x708C30,pip_max=0x716290,
            extras=0x6F5190,health=0x6F64A0,pip_tail=0x709A90,status_tail=0x70AA60,
            shape=0x4AED70,picker=0x490B90,plain_leaf=0x4912B0))


def control_group_status_metadata():
    return native.provenance(
        scope='Ordinary MTNK assigned group digit through whole Extras/health/tail and physical GAME.FNT fill/outline/glyph pixels',
        assumptions=[
            'One existing selected-health PaletteReader/INI/palette/Surface runtime owns MTNK Rules/UnitType constructors, physical RULESMD/LANGRULE/MPBattleMD/XMP03T4 selected reader blocks and original Unit/Foot/Drive prefix. Constructor RNG/passive timer initialization is separate from measured draw/assignment zero-RNG intervals. Whole Unit construction/COM, Unlimbo, selection/ownership lifecycle and VXL body are excluded.',
            'The existing control_group_text_oracle font-loader owner accepts the same VM; original434AD0/433880/433990/434700 load unchanged GAME.FNT bytes. Normal STOCK_DIGITAL AssetManager resolves ra2.mix/local.mix without loose override; original House50B840/500DF7 Gold scalar output from consumed house_color.json supplies House+56F9, then original70A8xx RGB565 packing executes. Shared PIPS/PIPBRD/PALETTE use their established physical winners.',
            'Original731060/7310D0 mutate signed Techno+214 after supplied current-house, mode, selected and Techno-array enrollment priors. Commands assign parameters1/10 and clear through actual code; visual selected/hover/sinking and raw XYZ are subsequent supplied render priors. Assignment controls exercise mode0 PlayerControl and mode1 current-house identity including latent ineligible membership; actor timer bytes stay unchanged and no RNG/detach executes.',
            'Affected self-heal controls use the same House counters/prior offsets from existing selfheal_oracle and execute original50D9C0/50D9D0 queries and whole709A90. Actual General66E6E2/66E715 ReadInt blocks consume physical SelfHealInfantryFrames50/UnitFrames75. Stock MTNK/E1 type/Unit-or-Infantry/Drive-or-Walk owners are reused. Explicit MTNK Organic=yes and PixelSelectionBracketDelta11 scenario boundaries execute actual715024 ReadBool/71415F ReadInt caller blocks, then restore via those same readers. Healthy frame20 Unit and frame13 Infantry/Organic Unit draw600 without A/Z; the damaged601 pulse, house grant/revoke lifecycle and arbitrary affected types remain separate residuals.',
            'Whole6F5190->6F64A0->709A90/70AA60->70A6FD->4A66D0->4A5EB0->434B90->434500->434120 executes. Original7BB020/7BB050 black fill,7BAD90/7BADC0 outline and glyph bitmap stores use original BSurface bounds/locks/pitch. They bypass A/Z; these rectangles do not use4BFD30. Clip/A127/63/2/0, oldZ0 and two positive opaque/group-health parent overlaps are component controls, not collision-valid placement or a complete native frame.',
            'Native constructor field-1 and assigned0..9 map to no digit/1..9,0. Ordinary MTNK PipScale0 reaches the text tail. Building Type PipScale2 means Tiberium and709B61 suppresses the complete tail; this is not an Ammo1 claim. Existing Building/Infantry/Aircraft legacy tail submission cases are preserved separately; whole other-type callers remain open.',
            'Original .text/vtables, measured draw actor bytes and A/Z remain unchanged; drawing RNG is empty and no timer write/detach occurs. Full Display scheduling, body/shroud/AlphaShapes, non-allied+210 admission, deploy/undeploy/bunker/scripted groups and other status branches remain separate residuals.',
        ],substitutions=[
            'Inherited native allocation/delete/CRT TLS, immutable physical file/archive transport and BSurface/circular A/Z memory replace platform services. Existing font owner supplies virtual file reads, bounded CRT %d formatting and Win32 SetRect; no reached health/group decision, font lookup/measure, fill/outline, shape/glyph raster return or native instruction is replaced.',
        ],entry_points=dict(unit_ctor_begin=0x7353C0,unit_ctor_stop=0x7354CE,
            drive_ctor=0x4AF540,assign=0x731060,clear=0x7310D0,extras=0x6F5190,
            health=0x6F64A0,pip_tail=0x709A90,status_tail=0x70AA60,group_tail=0x70A6FD,
            font_ctor=0x434AD0,font_load=0x433990,measure=0x4A59E0,
            fill=0x7BB020,outline=0x7BAD90,text=0x4A66D0,glyph=0x434120,
            has_infantry_self_heal=0x50D9C0,has_unit_self_heal=0x50D9D0,
            self_heal_shape=0x70A6BB,self_heal_plain_leaf=0x4960A0))


def unit_health_metadata():
    return native.provenance(
        scope='Ordinary rookie MTNK selected/hover health strip through original Extras/health/pip-tail and physical SHP pixels',
        assumptions=[
            'One existing PaletteReader owns original image/allocator/native INI cache and complete Rules665650/UnitType7470D0 constructors. Selected Strength, PixelSelectionBracketDelta, PipScale and ConditionRed/Yellow original read/store blocks consume physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4 lexical layers; fixed ART supplies none of these keys. Whole Scenario and Rules/UnitType file readers do not run.',
            'Physical PIPS21frame and PIPBRD/PALETTE copies match independently resolved normal STOCK_DIGITAL production AssetManager winners, including loose/expansion/language precedence. The oracle consumes unchanged agreed archive bytes; it does not emulate native archive opening/search. ANIM.PAL is an inherited palette-startup dependency, not the health Convert.',
            'Original palette initialize52BE61..52BFCE and full Convert/LUT constructors execute in the same VM. Existing bridge_render_shape.prepare_surface supplies RGB565 BSurface/circular A/Z storage, baseline32768, raw backgroundF940, A127/63/2/0 and oldZ65535/0. A2 is physical stock unrevealed SHROUD-frame15 output established by existing shroud owner; its whole writer is not rerun here.',
            'Prepared original Unit-vtable/type/House linkage, rookie state, no cargo/spawn/capture/group/selfheal, selected/hover/sinking and health fields are supplied consumer priors, not Unit constructor, damage, selection or sinking lifecycle proof. Original UnitGetCoords5F65A0 and Tactical6D2140 derive each actor point from supplied XYZ/camera. Supplied overlap positions are a drawing-order control, not proven legal dense-unit placement.',
            'Original complete DrawExtras6F5190 executes ordinary no-bomb/no-repair/rookie state through full DrawHealth6F64A0 and its actually reached ordinary709A90/70AA60 tail. Every actual4AED70 frame/centering/clip, picker490B90, rowwalker and leaf executes unchanged. No MTNK VXL body, whole Display loop, shroud/AlphaShapes, rank/text/selfheal rendering or other PipScale is established.',
            'Ordered color stores and final sparse RGB565 pixels are native outputs. Per-draw A/Z plane accesses distinguish backgroundE00 consumesA/noZ from fill600 bypassesA/Z. Original code/vtables, A/Z and prepared actor bytes stay unchanged; no drawing RNG entry, timer write or detach occurs in these measured ordinary intervals.',
        ], substitutions=[
            'Inherited Reader successful allocation/delete/CRT TLS and immutable physical asset file transports; existing BSurface and circular A/Z memory replace DirectDraw storage. No health ratio/count/frame, selected/hover gate, gameplay function result, palette table, shape decoder or pixel leaf is substituted.',
        ], entry_points=dict(rules_ctor=0x665650, unit_type_ctor=0x7470D0,
            strength_begin=0x5F94D3, strength_end=0x5F94F3,
            pip_scale_begin=0x714111, pip_scale_end=0x71412B,
            delta_begin=0x71415F, delta_end=0x714179,
            red_begin=0x66B337, red_end=0x66B35E,
            yellow_begin=0x66B35E, yellow_end=0x66B385,
            extras=0x6F5190, health=0x6F64A0, pip_tail=0x709A90,
            status_tail=0x70AA60, shape=0x4AED70, picker=0x490B90,
            background_leaf=0x493DF0, fill_leaf=0x4912B0))


if __name__=='__main__':
    import sys
    if '--repair-wrench' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_unit_health(wrench=True),
            Path(__file__).with_name('repair_wrench.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--repair-wrench'],
            provenance=repair_wrench_metadata,
            source_paths={'oracle':Path(__file__),
                'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'tmp_owner':Path('tools/spatial_oracle/tmp_clear_render.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--control-group-status' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_unit_health(groups=True),
            Path(__file__).with_name('control_group_status.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--control-group-status'],
            provenance=control_group_status_metadata,
            source_paths={'oracle':Path(__file__),
                'font_owner':Path('tools/control_group_text_oracle.py'),
                'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--ammo-pips' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_unit_health(ammo=True),
            Path(__file__).with_name('ammo_pips.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--ammo-pips'],
            provenance=ammo_pips_metadata,
            source_paths={'oracle':Path(__file__),
                          'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                          'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                          'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                          'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                          'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--passenger-pips' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_unit_health(passenger=True),
            Path(__file__).with_name('passenger_pips.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--passenger-pips'],
            provenance=passenger_pips_metadata,
            source_paths={'oracle':Path(__file__),
                          'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                          'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                          'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                          'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                          'tmp_owner':Path('tools/spatial_oracle/tmp_clear_render.py'),
                          'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--unit-rank' in sys.argv[1:]:
        native.finish_vectors(lambda: generate_unit_health(rank=True),
            Path(__file__).with_name('unit_rank.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--unit-rank'],
            provenance=unit_rank_metadata,
            source_paths={'oracle':Path(__file__),
                          'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                          'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                          'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                          'constructor_boundary':Path('tools/spatial_oracle/astar_mtnk_inputs.py'),
                          'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                          'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--building-health' in sys.argv[1:]:
        native.finish_vectors(lambda: generate_unit_health(building=True),
            Path(__file__).with_name('building_health.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--building-health'],
            provenance=building_health_metadata,
            source_paths={'oracle':Path(__file__),
                          'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                          'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                          'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                          'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                          'tmp_owner':Path('tools/spatial_oracle/tmp_clear_render.py'),
                          'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--unit-health' in sys.argv[1:]:
        native.finish_vectors(generate_unit_health,
            Path(__file__).with_name('unit_health.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--unit-health'],
            provenance=unit_health_metadata,
            source_paths={'oracle':Path(__file__),
                          'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                          'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                          'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                          'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                          'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
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
