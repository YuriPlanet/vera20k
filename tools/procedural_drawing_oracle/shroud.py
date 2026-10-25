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
``--self-heal-status`` adds damaged phase601, four General defaults/readers
and bounded divide-fault ordering to the existing group/status runtime.
``--bomb-clock`` executes native Attach/visibility, the clock prefix and
complete Extras on MTNK, plus the shared GAPOWR body/late composition.
``--spawn-pips`` executes the independent CARRIER spawn row and docked query
through the same native status, palette and packed-shape owners.
``--absorber-pips`` extends that owner with stock YAPOWR Building health,
ordered INIT/YENGINEER Cargo and the native absorber passenger-pip arm.
``--radial-indicator`` executes selected GAGAP range geometry, original
ellipse/DSurface cubic rays and their shared primary A/Z admission.
``--superweapon-range`` executes the original active targeting ring loop and
shared radial helper numeric prefixes with WinMain's actual x87 control word.
``--infantry-berserk-body`` composes the existing Infantry, palette, TMP and
Surface owners for physical GI body/shadow pixels with the caller colour word.
"""
from functools import lru_cache
import hashlib
import json
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_INTR, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
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


def _building_ground_runtime(*, type_name='GAPOWR', render_ground=True, wrench=False,
                             prepared_files=None, extra_art_names=(), building_registry=False,
                             power=False, cell_geometry=False, absorber=False):
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

    m, terrain, _files = tmp.prepare(tmp_names=('clear01.tem',), prepared_files=prepared_files)
    u, layers = m.u, physical_layers()
    if cell_geometry:
        from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry
        geometry=initialize_discovery_geometry(m.invoke,m.read32,u)
        assert m.read32(0x89E7C0)==m.read32(0xABDE88)==104
    if prepared_files is not None:
        m.assets.update({name.upper(): raw for name, raw in prepared_files.items() if raw is not None})
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
    constructor_fields = _building_health_fields(m, typ, repair=wrench, power=power, absorber=absorber)
    if not render_ground:
        constructor_fields.update(max_number_occupants=m.read32(typ+0x1580),
                                  show_occupant_pips=bool(u.mem_read(typ+0x1584,1)[0]))
    type_layer_fields = []
    art, _ = lexical(m.assets['ARTMD.INI'],
                     {type_name,type_name+'_A',type_name+'_AD',type_name+'MK',*extra_art_names})
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
        type_layer_fields.append(dict(file=name, after=_building_health_fields(m, typ, repair=wrench, power=power, absorber=absorber)))
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
    if building_registry:
        # CRT initializer pointer81322C names this exact prefix. It prepares
        # growth/defaults; each subsequent43B740 performs its own append.
        # Stop only before the process-exit callback registration4E6B96.
        u.reg_write(UC_X86_REG_ESP, READER_SP)
        native.run_checked(u, 0x4E6B60, 0x4E6B96,
                           required_addresses=(0x4E6B7D, 0x4E6B91))
    building, house = m.alloc(0x800), m.alloc(0x18000)
    constructor_rng=[]
    if absorber:
        def observe_ctor(_u,pc,_size,_data):
            if pc in (0x65C780,0x65C7E0):
                sp=u.reg_read(UC_X86_REG_ESP)
                constructor_rng.append(dict(entry=f'{pc:08X}',caller=f'{m.read32(sp):08X}',
                    bounds=rally.ints(u,sp+4,2) if pc==0x65C7E0 else []))
        constructor_hook=u.hook_add(UC_HOOK_CODE,observe_ctor)
    _prepare_ground_building(m, building, typ, house)
    if absorber:
        u.hook_del(constructor_hook)
        building_runtime=dict(constructor='0043B740',whole_building_constructor=True,
            idle_initializer='0044D6A0',constructor_rng_requests=constructor_rng,
            passive_scan_timer=rally.ints(u,building+0x180,3),
            cargo_count=m.read32(building+0x114),
            supplied_placement=True,whole_unlimbo=False)
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
    result=dict(m=m,u=u,typ=typ,building=building,house=house,tactical=tactical,
                point=point,clip=clip,coord=coord,vtable=vtable,sources=sources,
                layer_reads=layer_reads,terrain=terrain,color_scheme=color_scheme,
                ground_cells=ground_cells,original_code=original_code,alpha=alpha,
                constructor_fields=constructor_fields,type_layer_fields=type_layer_fields,
                cells=cells)
    if cell_geometry:result['geometry_startup']=geometry
    if absorber:result['building_runtime']=building_runtime
    return result


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
    return dict(schema_version=1, palette=palette_evidence,
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


def _building_health_fields(m, typ, *, repair=False, power=False, absorber=False):
    result=dict(strength=m.read32(typ + 0xA0), delta=m.read32(typ + 0x3E0),
                height=m.read32(typ + 0xEF4), foundation=m.read32(typ + 0xEF0),
                pip=m.read32(typ + 0x3D0), pip_scale=m.read32(typ + 0x3D4),
                pips_draw_for_all=bool(m.u.mem_read(typ + 0x3D8, 1)[0]),
                can_be_occupied=bool(m.u.mem_read(typ + 0x157B, 1)[0]),
                spawns_number=m.read32(typ + 0xD5C))
    if repair:
        result.update(cost=m.read32(typ+0x610),repairable=bool(m.u.mem_read(typ+0xCCC,1)[0]),
                      click_repairable=bool(m.u.mem_read(typ+0x157A,1)[0]))
    if power:
        result.update(zip(('power_output','power_drain'),struct.unpack('<ii',m.u.mem_read(typ+0xEE0,8))))
    if absorber:
        result.update(passengers=m.read32(typ+0x5E0),gunner=bool(m.u.mem_read(typ+0x805,1)[0]),
            infantry_absorb=bool(m.u.mem_read(typ+0x16AF,1)[0]),
            unit_absorb=bool(m.u.mem_read(typ+0x16AE,1)[0]),
            max_number_occupants=m.read32(typ+0x1580),
            show_occupant_pips=bool(m.u.mem_read(typ+0x1584,1)[0]))
    return result


class _HealthRecorder:
    """One observer for ordinary health and phase-aware native composition."""
    def __init__(self, m, size, shape_names, *, phase_aware=False, rank=False, scales=False, group_status=False, wrench=False, self_heal=False, bomb=False, spawns=False, absorber=False):
        from tools.projectile_oracle.bridge_render_shape import PIXELS, Z, A
        u = m.u
        self.stores, self.draws, self.entries, self.rng = [], [], [], []
        self.line_calls, self.body_admissions = [], []
        self.gates, self.callbacks = [], []
        self.numeric=[]
        self.pip_tail_return_pc,self.in_pip_tail=None,False
        self.actor_index, self.phase = -1, 'health'
        self.return_pc, self.active = None, False
        def finish_draw():
            if self.draws and 'begin' in self.draws[-1]:
                self.draws[-1]['store_range'] = [self.draws[-1].pop('begin'), len(self.stores)]
        def observe(_u, pc, _size, _data):
            if absorber:
                if pc==self.pip_tail_return_pc:
                    self.in_pip_tail=False
                    self.pip_tail_return_pc=None
                if pc==0x709A90:
                    self.in_pip_tail=True
                    self.pip_tail_return_pc=m.read32(u.reg_read(UC_X86_REG_ESP))
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
            if bomb:
                if pc in (0x487950,0x586360,0x5F65A0,0x438A00):
                    callback=dict(actor=self.actor_index,entry=f'{pc:08X}')
                    if pc==0x487950:
                        callback['cell']=list(struct.unpack('<2h',u.mem_read(u.reg_read(UC_X86_REG_ECX)+0x24,4)))
                    elif pc==0x586360:
                        callback['coord']=rally.ints(u,m.read32(u.reg_read(UC_X86_REG_ESP)+4),3)
                    self.callbacks.append(callback)
                if pc==0x6F51E8:
                    self.gates.append(dict(actor=self.actor_index,gate='cell_shrouded',
                                           result=u.reg_read(UC_X86_REG_EAX)&255))
                if pc==0x6F5235:
                    self.numeric.append(dict(actor=self.actor_index,result='clock_frame',
                        value=struct.unpack('<i',words(u.reg_read(UC_X86_REG_EAX)))[0]))
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
            if spawns and pc==0x6B7D50:
                self.callbacks.append(dict(actor=self.actor_index,entry=f'{pc:08X}'))
            if spawns and pc==0x709B96:
                self.numeric.append(dict(actor=self.actor_index,result='docked_spawns',
                    value=u.reg_read(UC_X86_REG_EAX)))
            if absorber and pc==0x4598A0:
                self.callbacks.append(dict(actor=self.actor_index,entry=f'{pc:08X}'))
            if absorber and pc==0x709DD0:
                self.numeric.append(dict(actor=self.actor_index,result='is_absorb_allowed',
                    value=u.reg_read(UC_X86_REG_EAX)&255))
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
                if self_heal:
                    self.draws[-1].update(destination_words=[],dispatch=[])
                if absorber:self.draws[-1]['pip_tail']=self.in_pip_tail
            elif self_heal and pc in (0x4AF144,0x4AF1DA) and self.active:
                pointer=u.reg_read(UC_X86_REG_EAX)
                vt=m.read32(pointer)
                result=dict(return_pc=f'{pc:08X}',vtable=f'{vt:08X}',
                    blit_entry=f'{m.read32(vt+4):08X}')
                if self.draws[-1]['flags']==0x601:
                    result['half_mask']=m.read32(pointer+4)&65535
                if result not in self.draws[-1]['dispatch']:
                    self.draws[-1]['dispatch'].append(result)
            elif (pc in (0x493DF0, 0x4912B0, 0x4990E0, 0x497390)
                  or ((scales or group_status) and pc==0x4960A0)
                  or (self_heal and pc in (0x492670,0x496250))) and self.active:
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
                elif self_heal and PIXELS <= address < PIXELS + size[0] * size[1] * 2:
                    for offset in range(0,_size,2):
                        index=(address+offset-PIXELS)//2
                        value=struct.unpack('<H',u.mem_read(address+offset,2))[0]
                        self.draws[-1]['destination_words'].append([index%size[0],index//size[0],value])
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


def _prepare_unit_rank(m, typ, actor, house, *, locomotor_ctor=0x4AF540, aircraft=False,
                       existing_cells=None, reuse_existing_expiry_registry=False):
    """Existing original mobile/Foot prefix and locomotor constructor, in this VM.

    CoCreate/whole typed Unit constructor and Unlimbo are excluded priors.
    No visual-character or observer predicate result is supplied.
    Multi-actor callers may retain initialized expiry-listener capacity so
    each original Object constructor appends without discarding earlier actors.
    """
    from tools.spatial_oracle.building_body_rules import SP as READER_SP
    u = m.u
    scenario = m.read32(0xA8B230)
    rngs = (0x886B88,scenario+0x218,0xABE890)
    for stream in rngs:
        m.invoke(0x65C6D0,stream,(0,))
    if reuse_existing_expiry_registry:
        assert m.read32(0xB0F724) and m.read32(0xB0F728) > 0
        assert m.read32(0xB0F730) <= m.read32(0xB0F728)
    else:
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
    if existing_cells is None:
        _table,cells = _prepare_flat_cells(m,range(8,13),range(18,24))
    else:
        cells=existing_cells
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


def _status_reader_block(m, begin, end, registers, *, stack_delta=0):
    """One unchanged caller block in the existing native reader owner."""
    from tools.spatial_oracle.building_body_rules import SP as reader_sp
    u = m.u
    u.mem_write(reader_sp, words(native.RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, reader_sp)
    for register, value in registers.items():
        u.reg_write(register, value)
    native.run_checked(u, begin, end)
    assert u.reg_read(UC_X86_REG_ESP) == reader_sp+stack_delta


def _passenger_type_fields(m, typ, infantry=False, building=False, *, ammo=False, spawns=False):
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
    if spawns:
        child=m.read32(typ+0xD58)
        result.update(spawns=m.string(child+0x24) if child else None,
            **dict(zip(('spawns_number','spawn_regen_rate','spawn_reload_rate'),
                struct.unpack('<iii',u.mem_read(typ+0xD5C,12)))))
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
    for name in (name for name in ('E1','ENGINEER','IVAN','INIT','YENGINEER') if name in types):
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
        dict(name='fractional_size_1_5_two_passengers',
            current_type_size_override_after_read=dict(type_name='MTNK',f64_bits='3FF8000000000000'),
            actors=[dict(base,type_name='LCRF',passenger=None,passengers=['MTNK','MTNK'],health=300)]),
        dict(name='fractional_size_2_8_two_passengers',
            current_type_size_override_after_read=dict(type_name='MTNK',f64_bits='4006666666666666'),
            actors=[dict(base,type_name='LCRF',passenger=None,passengers=['MTNK','MTNK'],health=300)]),
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


def _absorber_pip_reads(m, typ):
    """Selected original type-reader blocks, through the existing INI VM owner."""
    from tools.spatial_oracle.building_body_rules import RULES
    for begin,end,building in ((0x714111,0x714145,False),
            (0x714A49,0x714A63,False),(0x714B35,0x714B56,False),
            (0x4600CB,0x4600E5,True),(0x4600FF,0x460133,True),
            (0x460988,0x4609D6,False)):
        registers={UC_X86_REG_EBP:typ,UC_X86_REG_EBX:typ+0x24,
                   UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES}
        if building:registers.update({UC_X86_REG_EBX:typ+0x24,
                                     UC_X86_REG_ESI:typ+0x24,UC_X86_REG_EDI:RULES})
        _status_reader_block(m,begin,end,registers)


def _absorber_reader_controls(m):
    """Retained defaults, not a second implementation of any native reader."""
    from tools.spatial_oracle.building_body_rules import RULES
    u=m.u
    typ=m.alloc(0x2000)
    m.invoke(0x45DD90,typ,(m.cstring('AbsorberProbe'),))
    constructor=_building_health_fields(m,typ,absorber=True)
    requests=[('constructor_defaults',{'AbsorberProbe':{'Strength':'700'}}),
        ('authored_true',{'AbsorberProbe':{'PipsDrawForAll':'yes','PipScale':'Passengers',
            'Passengers':'5','UnitAbsorb':'yes','InfantryAbsorb':'no','Gunner':'no',
            'CanBeOccupied':'yes','ShowOccupantPips':'no','MaxNumberOccupants':'2'}}),
        ('absent_section',{}),
        ('admitted_missing_keys',{'AbsorberProbe':{'Strength':'700'}}),
        ('explicit_false',{'AbsorberProbe':{'PipsDrawForAll':'no','PipScale':'Passengers',
            'Passengers':'5','UnitAbsorb':'no','InfantryAbsorb':'yes',
            'CanBeOccupied':'no','ShowOccupantPips':'yes','MaxNumberOccupants':'0'}}),
        ('unknown_scale',{'AbsorberProbe':{'PipScale':'unknown'}}),
        ('case_insensitive_scale',{'AbsorberProbe':{'PipScale':'pAsSeNgErS',
            'PipsDrawForAll':'True'}}),
        ('empty_key_retains',{'AbsorberProbe':{'PipsDrawForAll':''}})]
    controls=[]
    for name,sections in requests:
        m.rules_cache(sections)
        before=_building_health_fields(m,typ,absorber=True)
        admitted=bool(m.invoke(0x526810,RULES,(typ+0x24,)))
        if admitted:_absorber_pip_reads(m,typ)
        controls.append(dict(name=name,input=dict(sections=sections),admitted=admitted,
            before=before,after=_building_health_fields(m,typ,absorber=True)))
    return dict(constructor=constructor,controls=controls)


def _absorber_cases(base):
    dual='[YAPOWR]\nCanBeOccupied=yes\nMaxNumberOccupants=2\n'
    return [dict(name='empty_selected',a_value=0,old_z=0,
                 actors=[dict(base,passengers=[])]),
        dict(name='one_initiate'),
        dict(name='two_mixed',actors=[dict(base,passengers=['INIT','YENGINEER'])]),
        dict(name='full_loaded',actors=[dict(base,passengers=['INIT','YENGINEER','INIT','INIT','INIT'])]),
        dict(name='unload_empty',actors=[dict(base,unload=True)]),
        dict(name='hover_only',actors=[dict(base,selected=False,hovered=True)]),
        dict(name='absent',actors=[dict(base,selected=False)]),
        dict(name='sinking',actors=[dict(base,sinking=True)]),
        dict(name='other_owner',actors=[dict(base,owner_is_current=False)]),
        dict(name='all_house_pips',scenario_patch='[YAPOWR]\nPipsDrawForAll=yes\n',
             actors=[dict(base,owner_is_current=False)]),
        dict(name='occupiable_other_owner',scenario_patch='[YAPOWR]\nCanBeOccupied=yes\n',
             actors=[dict(base,owner_is_current=False)]),
        dict(name='tiberium_suppresses_tail',scenario_patch='[YAPOWR]\nPipScale=Tiberium\n'),
        dict(name='unit_absorber',scenario_patch='[YAPOWR]\nUnitAbsorb=yes\n',
             actors=[dict(base,passengers=['MTNK'])]),
        dict(name='unit_ordinary',scenario_patch='[YAPOWR]\nInfantryAbsorb=no\n',
             actors=[dict(base,passengers=['MTNK'])]),
        dict(name='independent_garrison_row',scenario_patch=dual,
             actors=[dict(base,passengers=[],occupants=[])]),
        dict(name='hidden_garrison_row',scenario_patch=dual+'ShowOccupantPips=no\n',
             actors=[dict(base,passengers=[],occupants=[])]),
        dict(name='caller_clip',a_value=63,clip=[0,0,39,160]),
        dict(name='ordered_overlap',actors=[base,dict(base,passengers=['YENGINEER'],health=300)])]


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


def _self_heal_cases(base):
    """Additive supplied status inputs; every phase and pixel is native output."""
    unit=dict(base,health=150,group_commands=[])
    infantry=dict(unit,type_name='E1',kind='infantry',health=60)
    rows=[dict(name=f'unit_frame_{frame}',binary_frame=frame,self_heal_units=1,
               actors=[unit]) for frame in (5,6,74,75)]
    rows += [dict(name='unit_heal_to_full',binary_frame=75,self_heal_units=1,
                  actors=[dict(unit,health=299,native_ai_before_draw=True)]),
             dict(name='unit_negative_frame',binary_frame=-7,self_heal_units=1,actors=[unit])]
    rows += [dict(name=f'infantry_frame_{frame}',binary_frame=frame,
                  self_heal_infantry=1,actors=[infantry]) for frame in (49,50)]
    rows += [dict(name=f'organic_unit_frame_{frame}',binary_frame=frame,
                  self_heal_infantry=1,scenario_patch='[MTNK]\nOrganic=yes\n',
                  actors=[unit]) for frame in (49,50)]
    rows += [dict(name='unit_a0_z0',binary_frame=5,self_heal_units=1,
                  a_value=0,old_z=0,actors=[unit]),
             dict(name='unit_clipped',binary_frame=5,self_heal_units=1,
                  clip=[0,0,104,120],actors=[unit]),
             dict(name='ordered_two_markers',binary_frame=5,self_heal_units=1,
                  actors=[unit,unit]),
             dict(name='ordered_marker_group',binary_frame=5,self_heal_units=1,
                  actors=[unit,dict(unit,location=[2846,5090,0],
                      group_commands=[dict(operation='assign',group=10,selected=True)])])]
    return rows


_SELF_HEAL_FIELDS=(('SelfHealInfantryFrames',0x30),('SelfHealInfantryAmount',0x34),
                   ('SelfHealUnitFrames',0x38),('SelfHealUnitAmount',0x3C))


def _self_heal_general_fields(m,rules):
    return {key:struct.unpack('<i',m.u.mem_read(rules+offset,4))[0]
            for key,offset in _SELF_HEAL_FIELDS}


def _self_heal_general_blocks(m,rules):
    # Original fourth store follows SUB ESP,1C; preserve that native frame
    # effect rather than moving the store or replacing the reader return.
    from tools.spatial_oracle.building_body_rules import RULES
    _status_reader_block(m,0x66E6E2,0x66E74B,
        {UC_X86_REG_ESI:rules,UC_X86_REG_EDI:RULES},stack_delta=-0x1C)


def _self_heal_reader_controls(m):
    """Four current-default reads in the same native cache/constructor owner."""
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.spatial_oracle.building_body_rules import RULES,SP as reader_sp
    rules=m.alloc(0x3000)
    m.invoke(0x665650,rules)
    constructor=_self_heal_general_fields(m,rules)
    rows,reads=[],[]
    u=m.u
    def observe(_u,pc,_size,_data):
        if pc==0x5276D0:
            sp=u.reg_read(UC_X86_REG_ESP)
            key=m.string(m.read32(sp+8))
            if key in constructor:
                reads.append(dict(reader=f'{pc:08X}',section=m.string(m.read32(sp+4)),
                    key=key,default=struct.unpack('<i',u.mem_read(sp+12,4))[0]))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    empty={'General':dict(Unrelated='1',**dict.fromkeys(constructor,''))}
    cases=[('missing_section',{}),('missing_keys',{'General':{'Unrelated':'1'}}),
           ('empty_values_omitted',empty),
           ('zero_values',{'General':dict.fromkeys(constructor,'0')}),
           ('signed_values',{'General':dict(zip(constructor,('-50','-20','-75','-5')))}),
           ('retained_missing_section',{}),
           ('retained_missing_keys',{'General':{'Unrelated':'1'}}),
           ('retained_empty_values_omitted',empty),
           ('exact_case',{'General':{key.lower():'99' for key in constructor}}),
           ('partial_override',{'General':{'SelfHealUnitAmount':'7'}}),
           ('raw_cached_empty_values',{'General':dict.fromkeys(constructor,'')})]
    for name,sections in cases:
        before=_self_heal_general_fields(m,rules)
        text=''.join(f'[{section}]\n'+''.join(f'{key}={value}\n' for key,value in keys.items())
                     for section,keys in sections.items())
        raw_cache=name=='raw_cached_empty_values'
        cached=sections if raw_cache else lexical(text.encode('latin1'),set(sections))[0]
        m.rules_cache(cached)
        u.mem_write(reader_sp,words(native.RET_MAGIC,RULES))
        u.reg_write(UC_X86_REG_ESP,reader_sp)
        u.reg_write(UC_X86_REG_ECX,rules)
        stop=native.run_checked(u,0x66D530,(0x66D55E,0x671E8E),
            required_addresses=(0x66D54C,0x66D558))
        admitted=stop==0x66D55E
        start=len(reads)
        if admitted:_self_heal_general_blocks(m,rules)
        rows.append(dict(name=name,input_text=text,sections=cached,raw_cached_values=raw_cache,before=before,
            general_admitted=admitted,reads=reads[start:],after=_self_heal_general_fields(m,rules)))
    u.hook_del(hook)
    return dict(constructor=constructor,retained_history=rows)


def _self_heal_execution(m,run,expected_divide=None):
    """Require the actual architectural interrupt, never an early-stop guess."""
    u=m.u
    interrupts=[]
    # A trapped emulated CPU is terminal for this control. Restore its prior
    # CPU context after recording #DE so a later independent control cannot
    # inherit Unicorn's pending exception and report a synthetic double fault.
    prior=u.context_save() if expected_divide else None
    def interrupt(_u,number,_data):
        interrupts.append(dict(number=number,pc=f'{u.reg_read(UC_X86_REG_EIP):08X}'))
        u.emu_stop()
    hook=u.hook_add(UC_HOOK_INTR,interrupt)
    try:
        try:run()
        except native.NativeExecutionError as error:
            expected=[dict(number=0,pc=f'{expected_divide:08X}')] if expected_divide else []
            if (not expected or error.diagnostics['reason']!='early_stop'
                    or interrupts!=expected):raise
            return dict(kind='x86_divide_error',interrupt=0,pc=f'{expected_divide:08X}')
        assert expected_divide is None and not interrupts
        return None
    finally:
        u.hook_del(hook)
        if prior is not None:u.context_restore(prior)


def _run_self_heal_ai(m,actor,*,expect_fault=None):
    """Original House-heal arm only, after the separate alive/per-object arm.

    Has/Step retain tools.selfheal_oracle's existing identities and bodies.
    No arithmetic, query result, smoke callback or native instruction is replaced.
    """
    from tools import selfheal_oracle as selfheal
    from tools.spatial_oracle.building_body_rules import SP as reader_sp
    u=m.u
    events,writes,rng=[],[],[]
    def observe(_u,pc,_size,_data):
        if pc in (0x6FA7EE,0x6FA8E2,selfheal.HAS_INF,selfheal.HAS_UNIT,
                  selfheal.STEP_INF,selfheal.STEP_UNIT,0x6FA897):
            events.append(f'{pc:08X}')
        if pc in (0x65C780,0x65C7E0):rng.append(f'{pc:08X}')
    def write(_u,_access,address,length,value,_data):
        if actor<=address<actor+0x900:
            writes.append(dict(offset=address-actor,bytes=length,value=value))
    hooks=[u.hook_add(UC_HOOK_CODE,observe),u.hook_add(UC_HOOK_MEM_WRITE,write)]
    before=bytes(u.mem_read(actor,0x900))
    u.mem_write(reader_sp,words(native.RET_MAGIC))
    for register,value in ((UC_X86_REG_ESP,reader_sp),(UC_X86_REG_ESI,actor),(UC_X86_REG_EBP,0)):
        u.reg_write(register,value)
    try:
        fault=_self_heal_execution(m,lambda:native.run_checked(u,0x6FA793,0x6FA941),expect_fault)
    finally:
        for hook in hooks:u.hook_del(hook)
    assert bool(fault)==bool(expect_fault) and not rng
    after=bytes(u.mem_read(actor,0x900))
    assert before[:0x6C]==after[:0x6C] and before[0x70:]==after[0x70:]
    return dict(entry='006FA793',stop=f'{expect_fault or 0x6FA941:08X}',events=events,
        fault=fault,health_before=struct.unpack_from('<i',before,0x6C)[0],
        health_after=struct.unpack_from('<i',after,0x6C)[0],actor_writes=writes,
        rng_entries=rng,timer_bytes_unchanged=True,detach_calls=[])


def _self_heal_divide_controls(m,rules,house,actors,templates,point,clip):
    """Bounded actual AI-vs-drawing divide admission and trap ordering."""
    from tools import selfheal_oracle as selfheal
    u=m.u
    saved_fields=bytes(u.mem_read(rules+0x30,16))
    rows=[]
    for type_name,offset,own,ai_divide,draw_divide in (
            ('MTNK',0x38,selfheal.HG_UNIT,0x6FA7EE,0x70A5B5),
            ('E1',0x30,selfheal.HG_INF,0x6FA8E2,0x70A549)):
        actor=actors[type_name]
        for path,period,frame,count in (
                ('ai',0,75,0),('ai',0,75,1),('ai',-1,-2147483648,0),
                ('draw',0,75,0),('draw',0,75,1),
                ('draw',-1,-2147483648,0),('draw',-1,-2147483648,1)):
            u.mem_write(actor,templates[type_name])
            u.mem_write(actor+0x21C,words(house))
            u.mem_write(actor+0x6C,words(150 if type_name=='MTNK' else 60))
            u.mem_write(actor+0x83,b'\1')
            u.mem_write(actor+0x9C,words(2688,5248,0))
            u.mem_write(house+selfheal.HG_INF,words(count if own==selfheal.HG_INF else 0))
            u.mem_write(house+selfheal.HG_UNIT,words(count if own==selfheal.HG_UNIT else 0))
            u.mem_write(rules+offset,words(period))
            u.mem_write(0xA8ED84,words(frame))
            expected=ai_divide if path=='ai' else draw_divide if count else None
            if path=='ai':
                result=_run_self_heal_ai(m,actor,expect_fault=expected)
            else:
                events,rng=[],[]
                def observe(_u,pc,_size,_data):
                    if pc in (selfheal.HAS_INF,selfheal.HAS_UNIT,draw_divide):
                        events.append(f'{pc:08X}')
                    if pc in (0x65C780,0x65C7E0):rng.append(f'{pc:08X}')
                hook=u.hook_add(UC_HOOK_CODE,observe)
                before=bytes(u.mem_read(actor,0x900))
                try:
                    fault=_self_heal_execution(m,lambda:m.invoke(0x6F5190,actor,(point,clip)),expected)
                finally:u.hook_del(hook)
                assert bool(fault)==bool(expected) and before==bytes(u.mem_read(actor,0x900))
                assert not rng
                result=dict(entry='006F5190',events=events,fault=fault,actor_unchanged=True,
                    rng_entries=rng,timer_bytes_unchanged=True,detach_calls=[])
            rows.append(dict(type_name=type_name,path=path,period=period,binary_frame=frame,
                house_count=count,**result))
    u.mem_write(rules+0x30,saved_fields)
    return rows


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


def generate_unit_health(*, building=False, rank=False, passenger=False, ammo=False, groups=False, wrench=False, self_heal=False, bomb=False, spawns=False, absorber=False):
    """Original ordinary Unit/Building health through shared physical SHP stores.

    PaletteReader owns the one VM, native INI caches and palette constructors;
    the existing shape owner supplies only Surface/circular A/Z storage.
    """
    from tools.projectile_oracle import bridge_render_inputs_palette as palette
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.spatial_oracle.building_body_rules import RULES, SP as READER_SP

    assert sum((building,rank,passenger,ammo,groups,wrench,self_heal,bomb,spawns,absorber))<=1
    groups=groups or self_heal
    building=building or wrench or absorber
    scales=passenger or ammo or spawns or absorber
    mobile_status=rank or (scales and not absorber) or groups or bomb
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
    if absorber:
        winners += (('YGPOWR.SHP','ra2md.mix','genermd.mix',None),)
    if wrench:
        winners += (('WRENCH.SHP','ra2.mix','cache.mix',
                     'd1ee6a305cd65c91a9b29db7c83c2f3acc4e15a3bb41082844e4c4dd0e2de8a2'),
                    ('MOUSEPAL.PAL','ra2.mix','cache.mix',
                     '4dcbe78b534a58fc972bce4b25aa58a2a56c9a5bb0ffbfcf5d6194be5dbde60c'))
    if bomb:
        winners += (('BOMBCURS.SHP','ra2.mix','cache.mix',
                     '03acfe1ee467a23e21658d3a308390633b6027e9be5b2ef839ce49742d5bd9e6'),
                    ('MOUSEPAL.PAL','ra2.mix','cache.mix',
                     '4dcbe78b534a58fc972bce4b25aa58a2a56c9a5bb0ffbfcf5d6194be5dbde60c'),
                    ('WRENCH.SHP','ra2.mix','cache.mix',
                     'd1ee6a305cd65c91a9b29db7c83c2f3acc4e15a3bb41082844e4c4dd0e2de8a2'))
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
    scene = (_building_ground_runtime(type_name='YAPOWR',prepared_files=assets,
                                     absorber=True) if absorber else
             _building_ground_runtime(wrench=True,cell_geometry=True) if bomb else
             _building_ground_runtime(type_name='NABNKR',render_ground=False) if passenger
             else _building_ground_runtime(wrench=wrench) if building else None)
    m = scene['m'] if scene else palette.PaletteReader({})
    if scene:
        m.assets.update(assets)
    else:
        m.assets = assets
    u = m.u
    startup_fpcw = _superweapon_winmain_fpcw(u, READER_SP) if passenger else None
    if building:
        rules, typ = m.rules, scene['typ']
    else:
        rules, typ = m.rules if passenger or bomb else m.alloc(0x3000), m.alloc(0x1900)
        if not passenger and not bomb:
            m.invoke(0x665650, rules)
            u.mem_write(0x8871E0, words(rules))
        u.mem_write(0xA8EB00, words(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        if ammo or spawns:
            u.mem_write(0xA8B218,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        m.invoke(0x41C8B0 if ammo else 0x7470D0, typ,
                 (m.cstring('CARRIER' if spawns else 'ORCA' if ammo else 'FV' if passenger else 'MTNK'),))
    if scales:
        types={'YAPOWR':typ} if absorber else {'CARRIER':typ} if spawns else {'ORCA':typ} if ammo else {'FV':typ,'NABNKR':scene['typ']}
        u.mem_write(0x887568,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        u.mem_write(0xA8E348,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        infantry_names=('E1','ENGINEER','INIT','YENGINEER') if absorber else ('E1','ENGINEER')
        for name in (('INIT','YENGINEER','MTNK') if absorber else ('HORNET','E1') if spawns else ('FV','MIND','HARV','E1') if ammo else
                     ('MIND','HARV','E1','ENGINEER','LCRF','MTNK')):
            types[name]=m.alloc(0x2000)
            m.invoke(0x41C8B0 if name=='HORNET' else 0x5236A0 if name in infantry_names else 0x7470D0,
                     types[name],(m.cstring(name),))
        type_constructors={name:_passenger_type_fields(m,p,name in infantry_names,ammo=ammo or spawns,spawns=spawns)
                           for name,p in types.items() if name not in (('NABNKR','YAPOWR') if absorber else ('NABNKR',))}
        if absorber:type_constructors['YAPOWR']=scene['constructor_fields']
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
        base = (_building_health_fields(m, typ, repair=wrench,absorber=absorber) if building else
                dict(strength=m.read32(typ + 0xA0), delta=m.read32(typ + 0x3E0),
                     pip_scale=m.read32(typ + 0x3D4)))
        if mobile_status:
            base['invisible'] = bool(u.mem_read(typ+0xC9A,1)[0])
        if groups:
            base.update(self_heal_infantry_frames=m.read32(rules+0x30),
                        self_heal_unit_frames=m.read32(rules+0x38))
            if self_heal:
                base.update(self_heal_infantry_amount=m.read32(rules+0x34),
                            self_heal_unit_amount=m.read32(rules+0x3C))
        if scales:
            base.update(_passenger_type_fields(m,typ,ammo=ammo or spawns,spawns=spawns))
        if bomb:
            base.update(zip(('ivan_damage','ivan_timed_delay','ivan_icon_flicker_rate'),
                (struct.unpack('<i',u.mem_read(rules+offset,4))[0]
                 for offset in (0xFCC,0xFD0,0xFD8))))
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
        section_name = 'YAPOWR' if absorber else 'CARRIER' if spawns else 'GAPOWR' if building else 'ORCA' if ammo else 'FV' if passenger else 'MTNK'
        sections, _ = lexical(raw, {section_name,'AudioVisual','CombatDamage','IVAN','IvanBomber','IvanBomb','IvanWH'} if bomb else {*group_types,'General','AudioVisual'} if groups else {*types,'MultipleMindControlTank','AudioVisual'} if scales
                                      else {section_name,'AudioVisual'})
        # These ordinary-tail keys are absent in the selected physical stack.
        # Keep native constructor values; don't invent a partial full reader.
        assert not ({'Organic'} if spawns else {'SpawnsNumber','Organic'} if scales else
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
                if self_heal:
                    _self_heal_general_blocks(m,rules)
                else:
                    for begin,end in ((0x66E6E2,0x66E6FB),(0x66E715,0x66E72F)):
                        _status_reader_block(m,begin,end,{UC_X86_REG_ESI:rules,UC_X86_REG_EDI:RULES})
        if scales:
            type_results={}
            for type_name,p in types.items():
                if type_name=='NABNKR' or (absorber and type_name=='YAPOWR'):
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
                    if (ammo and type_name=='ORCA') or spawns:
                        for begin,end in ((0x714179,0x71418D),(0x714198,0x71419E),
                                          (0x71474C,0x714780)):
                            _status_reader_block(m,begin,end,{UC_X86_REG_EBP:p,
                                UC_X86_REG_EBX:p+0x24,UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES})
                    if type_name in infantry_names:
                        _passenger_pip_reads(m,p)
                    if spawns:
                        _status_reader_block(m,0x714E97,0x714F1C,
                            {UC_X86_REG_EBP:p,UC_X86_REG_EBX:p+0x24,
                             UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES})
                type_results[type_name]=dict(admitted=present,
                    after=_passenger_type_fields(m,p,type_name in infantry_names,ammo=ammo or spawns,spawns=spawns))
            if 'MIND' in type_results and type_results['MIND']['admitted']:
                _status_reader_block(m,0x7129AB,0x712A1D,
                    {UC_X86_REG_EBP:types['MIND'],UC_X86_REG_EBX:types['MIND']+0x24,
                     UC_X86_REG_ESI:RULES})
            weapon=m.read32(types['MIND']+0x898) if 'MIND' in types else 0
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
        if bomb:
            combat_admitted=bool(m.invoke(0x526810,RULES,(m.read32(0x7F0C84),)))
            if combat_admitted:
                for begin,end in ((0x66C5AE,0x66C5CE),(0x66C5CE,0x66C606),
                                  (0x66C611,0x66C631)):
                    _status_reader_block(m,begin,end,{UC_X86_REG_ESI:rules,UC_X86_REG_EDI:RULES})
        layers.append(dict(file=name, archive=archive, sha256=sha(raw),
                           section_admitted=admitted, audiovisual_admitted=av_admitted,
                           selected_keys={s:{k:v for k,v in keys.items() if k in
                               ('Strength', 'PixelSelectionBracketDelta', 'PipScale',
                                'ConditionRed', 'ConditionYellow',
                                *(['PipsDrawForAll','CanBeOccupied','Pip','SpawnsNumber']
                                  if building else []), *(['Invisible'] if mobile_status else []),
                                *(['Passengers','Size','SizeLimit','Gunner','Pip','OccupyPip',
                                   'Storage','Damage','Primary','InfiniteMindControl'] if scales else []),
                                *(['InfantryAbsorb','UnitAbsorb','ShowOccupantPips','MaxNumberOccupants'] if absorber else []),
                                *(['PipWrap','InitialAmmo','Ammo'] if ammo or spawns else []),
                                *(['Spawns','SpawnsNumber','SpawnRegenRate','SpawnReloadRate'] if spawns else []))}
                               for s, keys in sections.items()},
                           reads=reads[mark:], after=fields()))
        if groups:
            layers[-1]['self_heal_keys']={section:{key:value for key,value in keys.items() if key in
                ('Organic','SelfHealInfantryFrames','SelfHealUnitFrames',
                 *(['SelfHealInfantryAmount','SelfHealUnitAmount'] if self_heal else []))}
                for section,keys in sections.items()}
        if scales:
            layers[-1]['types']=type_results
        if bomb:
            layers[-1].update(combat_damage_admitted=combat_admitted,
                bomb_keys={section:keys for section,keys in sections.items()
                    if section in ('CombatDamage','IVAN','IvanBomber','IvanBomb','IvanWH')})
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
    if wrench or bomb:
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
    if wrench or bomb:
        # Exact original filename/load/publication, after unrelated POWEROFF.
        _status_reader_block(m,0x45E98D,0x45E9A3,{})
        wrench_shape=m.read32(0x89DDC8)
        assert wrench_shape and bytes(u.mem_read(wrench_shape,len(assets['WRENCH.SHP'])))==assets['WRENCH.SHP']
        shape_names[wrench_shape]='WRENCH.SHP'
    if bomb:
        bomb_shape=m.read32(rules+0xFE0)
        assert bomb_shape and bytes(u.mem_read(bomb_shape,len(assets['BOMBCURS.SHP'])))==assets['BOMBCURS.SHP']
        shape_names[bomb_shape]='BOMBCURS.SHP'
    if building:
        house, actor, tactical, coord, point, clip = (scene[key] for key in
            ('house', 'building', 'tactical', 'coord', 'point', 'clip'))
        shape_names[m.read32(typ + 0xA4)] = 'YGPOWR.SHP' if absorber else 'GGPOWR.SHP'
        if absorber:
            assert bytes(u.mem_read(m.read32(typ+0xA4),len(assets['YGPOWR.SHP'])))==assets['YGPOWR.SHP']
        actor_template = bytes(u.mem_read(actor, 0x800))
        ground_planes = {name:bytes(u.mem_read(address, 51200))
                         for name,address in (('color',PIXELS),('z',Z),('a',A))}
        if wrench or absorber:
            other_house=m.alloc(0x18000)
        if wrench:
            render_coord,render_point=m.alloc(12),m.alloc(8)
    else:
        if not passenger and not bomb:
            u.mem_map(MEM, 0x200000)
        house, actor, tactical, coord, point, clip = (m.alloc(n) for n in
                                                      (0x18000, 0x900, 0x200, 12, 8, 16))
    if rank or groups or bomb:
        rank_runtime = _prepare_unit_rank(m, typ, actor, house,
            existing_cells=scene['cells'] if bomb else None)
        actor_template = bytes(u.mem_read(actor,0x900))
        if groups:
            prepared_infantry=_prepare_status_passengers(m,{'E1':group_types['E1']},house)['E1']
            group_actors={'MTNK':actor,'E1':prepared_infantry['actor']}
            group_templates={'MTNK':actor_template,'E1':prepared_infantry['template']}
    if bomb:
        # One existing ground VM also owns the mobile prefix and an ordinary
        # Infantry prefix. Original Attach/UpdateAll create the retained bomb;
        # no host-computed clock frame or end time initializes it.
        ground_planes={name:bytes(u.mem_read(address,51200))
            for name,address in (('color',PIXELS),('z',Z),('a',A))}
        building_template=bytes(u.mem_read(scene['building'],0x800))
        shape_names[m.read32(scene['typ']+0xA4)]='GGPOWR.SHP'
        u.mem_write(0x887568,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        ivan_type=m.alloc(0x2000)
        m.invoke(0x5236A0,ivan_type,(m.cstring('IVAN'),))
        prepared_ivan=_prepare_status_passengers(m,{'IVAN':ivan_type},house)['IVAN']
        planter=prepared_ivan['actor']
        u.mem_write(planter+0x21C,words(house))
        u.mem_write(0xA8B238,words(1))
        # Native ctor -1 sound IDs isolate drawing; audio is independently
        # consumed by bomb_class.json, not silently emulated in this VM.
        assert m.read32(rules+0x20C)==m.read32(rules+0x210)==0xFFFFFFFF
        list_items,array_items=m.alloc(4096),m.alloc(4096)
        u.mem_write(0x87F5D8,words(0x7EB6D4,list_items,1024,1,0,10))
        u.mem_write(0x87F5F0,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
        u.mem_write(0x89C668,words(0x7EB6D4,array_items,1024,1,0,10))
        def attach_bomb(receiver,supplied):
            u.mem_write(receiver+0x38,words(0));u.mem_write(receiver+0x68,b'\0')
            if supplied.get('bomb_start_frame') is None:
                return None
            u.mem_write(0xA8ED84,words(supplied['bomb_start_frame']))
            u.mem_write(0xA83D4C,words(house))
            u.mem_write(0x87F5E8,words(0));u.mem_write(0x89C678,words(0))
            entries=[]
            def observe_attach(_u,pc,_size,_data):
                if pc in (0x438E70,0x438BF0,0x50B6F0):entries.append(f'{pc:08X}')
            attach_hook=u.hook_add(UC_HOOK_CODE,observe_attach)
            m.invoke(0x438E70,0x87F5D8,(planter,receiver))
            for _ in range(2 if supplied.get('bomb_visible',True) else 1):
                m.invoke(0x438BF0,0x87F5D8)
            u.hook_del(attach_hook)
            record=m.read32(receiver+0x38)
            assert record and m.read32(record+0x2C)==receiver
            state=dict(kind=m.read32(record+0x30),
                start_frame=struct.unpack('<i',u.mem_read(record+0x34,4))[0],
                end_frame=struct.unpack('<i',u.mem_read(record+0x38,4))[0],
                visible=bool(u.mem_read(receiver+0x68,1)[0]),
                visibility_countdown=m.read32(0x87F608),
                attach_entries=[entry for entry in entries if entry=='00438E70'],
                refresh_entries=[entry for entry in entries if entry=='00438BF0'])
            assert state['kind']==0 and state['visible']==supplied.get('bomb_visible',True)
            return state
    if scales:
        status_runtimes,actor_templates={},{}
        for name in (() if absorber else ('CARRIER',) if spawns else ('ORCA','FV','MIND','HARV') if ammo else ('FV','MIND','HARV','LCRF')):
            status_runtimes[name]=_prepare_unit_rank(m,types[name],actor,house,
                locomotor_ctor=0x69EC50 if name=='CARRIER' else 0x4CC9A0 if name=='ORCA' else 0x513C20 if name=='LCRF' else 0x4AF540,
                aircraft=name=='ORCA')
            actor_templates[name]=bytes(u.mem_read(actor,0x900))
        passengers=_prepare_status_passengers(m,types,house)
        if absorber:
            actor_templates['YAPOWR']=actor_template
            passenger_pools={name:[prepared] for name,prepared in passengers.items()}
            for name in ('INIT','YENGINEER'):
                for _ in range(4):
                    passenger_pools[name].append(_prepare_status_passengers(m,{name:types[name]},house)[name])
            country=m.alloc(0x100)
            u.mem_write(country+0xB8,words(0))
            for index,p in enumerate((house,other_house)):
                u.mem_write(p+0x30,words(index))
                u.mem_write(p+0x34,words(country))
        if passenger:
            _prepare_ground_building(m,actor,types['NABNKR'],house,selected=False)
            actor_templates['NABNKR']=bytes(u.mem_read(actor,0x900))
            passenger_pools={name:[prepared] for name,prepared in passengers.items()}
            passenger_pools['MTNK'].append(
                _prepare_status_passengers(m,{'MTNK':types['MTNK']},house)['MTNK'])
        if spawns:
            # Same explicit HouseType index prior as the shared Building
            # status owner; nonallied health admission reads it before210.
            country=m.alloc(0x100)
            u.mem_write(country+0xB8,words(0))
            u.mem_write(house+0x34,words(country))
            child_prefixes=[]
            carrier_cells=list(status_runtimes['CARRIER']['cells'].values())
            for _ in range(3):
                child=m.alloc(0x900)
                runtime=_prepare_unit_rank(m,types['HORNET'],child,house,
                    locomotor_ctor=0x4CC9A0,aircraft=True,existing_cells=carrier_cells)
                child_prefixes.append(dict(actor=child,template=bytes(u.mem_read(child,0x900)),
                                           evidence=runtime['evidence']))
            manager=m.alloc(0x74)
            u.mem_write(0xB0B880,words(0x7EB6D4,m.alloc(4096),1024,1,0,10))
            u.mem_write(0xA8ED84,words(0))
            manager_rng=[]
            def observe_manager_ctor(_u,pc,_size,_data):
                if pc in (0x65C780,0x65C7E0):manager_rng.append(f'{pc:08X}')
            ctor_hook=u.hook_add(UC_HOOK_CODE,observe_manager_ctor)
            m.invoke(0x6B6C90,manager,(actor,types['HORNET'],0,
                m.read32(typ+0xD60),m.read32(typ+0xD64)))
            u.hook_del(ctor_hook)
            manager_ctor=dict(entry='006B6C90',count_argument=0,
                rng_requests=manager_rng,
                vtable=f'{m.read32(manager):08X}',
                count=m.read32(manager+0x2C),node_count=m.read32(manager+0x48),
                regen_rate=m.read32(manager+0x30),reload_rate=m.read32(manager+0x34),
                update_timer=dict(start=m.read32(manager+0x50),duration=m.read32(manager+0x58)),
                spawn_timer=dict(start=m.read32(manager+0x5C),duration=m.read32(manager+0x64)),
                target_is_null=m.read32(manager+0x68)==0,
                queued_target_is_null=m.read32(manager+0x6C)==0,mode=m.read32(manager+0x70))
            manager_items=m.alloc(12)
            manager_nodes=[m.alloc(0x18) for _ in range(3)]
            manager_template=bytes(u.mem_read(manager,0x74))
        elif not absorber:
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
    if spawns:
        base_actor.update(health=m.read32(typ+0xA0),type_name='CARRIER',kind='unit',
            passenger=None,unload=False,rank='rookie',owner_is_current=True,
            owned_by_current_house=True,cloak_state=0,cloak_progress=0,
            disguised=False,disguised_as_current_house=False,sensor_count=0,
            disguise_detect_count=0,node_states=[0,0,0])
        cases=[dict(name=f'docked_three_a{a}',a_value=a) for a in (127,63,2,0)]
        cases += [dict(name=name,actors=[dict(base_actor,node_states=states)])
            for name,states in (('docked_two',[0,6,2]),('docked_one',[6,3,7]),
                ('docked_zero',[2,4,7]),('reloading_three',[6,6,6]))]
        cases += [dict(name='occluding_z',old_z=0),
            dict(name='hover_only',actors=[dict(base_actor,selected=False,hovered=True)]),
            dict(name='absent',actors=[dict(base_actor,selected=False)]),
            dict(name='other_owner',actors=[dict(base_actor,owner_is_current=False,
                owned_by_current_house=False)]),
            dict(name='sinking',actors=[dict(base_actor,sinking=True)]),
            dict(name='clipped',actors=[dict(base_actor,location=[2412,5524,0])]),
            dict(name='caller_clip',clip=[0,0,68,120]),
            dict(name='current_capacity_two',scenario_patch='[CARRIER]\nSpawnsNumber=2\n'),
            dict(name='independent_passenger_row',scenario_patch='[CARRIER]\nPassengers=2\nSizeLimit=2\nPipScale=Passengers\n',
                actors=[dict(base_actor,passenger='E1')]),
            dict(name='ordered_overlap',actors=[base_actor,
                dict(base_actor,node_states=[2,4,7],health=200,location=[2696,5240,0])])]
    if absorber:
        base_actor.update(type_name='YAPOWR',kind='building',health=m.read32(typ+0xA0),
            owner_is_current=True,passengers=['INIT'],unload=False)
        cases=_absorber_cases(base_actor)
    if groups:
        base_actor.update(type_name='MTNK',kind='unit',rank='rookie',owner_is_current=True,
            owned_by_current_house=True,cloak_state=0,cloak_progress=0,disguised=False,
            disguised_as_current_house=False,sensor_count=0,disguise_detect_count=0,
            group_commands=[dict(operation='assign',group=1,selected=True)])
        cases=_control_group_cases(base_actor)
        if self_heal:cases=_self_heal_cases(base_actor)
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
    if bomb:
        base_actor.update(type_name='MTNK',kind='unit',rank='rookie',selected=False,
            owner_is_current=True,owned_by_current_house=True,cloak_state=0,cloak_progress=0,
            disguised=False,disguised_as_current_house=False,sensor_count=0,
            disguise_detect_count=0,bomb_start_frame=1000,bomb_visible=True)
        cases=[dict(name=f'armed_a{a}',a_value=a) for a in (127,63,2,0)]
        cases += [dict(name=f'frame_{frame}',frame=frame)
                  for frame in (1007,1008,1016,1074,1075,1375,1449,1450)]
        cases += [dict(name='occluding_z',old_z=0),
            dict(name='not_visible',actors=[dict(base_actor,bomb_visible=False)]),
            dict(name='no_bomb',actors=[dict(base_actor,bomb_start_frame=None)]),
            dict(name='selected',actors=[dict(base_actor,selected=True)]),
            dict(name='hovered',actors=[dict(base_actor,hovered=True)]),
            dict(name='sinking',actors=[dict(base_actor,sinking=True)]),
            dict(name='shrouded',cell_bits=0),dict(name='ground_open_only',cell_bits=8),
            dict(name='different_start',actors=[dict(base_actor,bomb_start_frame=925)]),
            dict(name='global_clip',clip=[0,0,10,10]),
            dict(name='clipped',actors=[dict(base_actor,location=[2364,5572,0])]),
            dict(name='elevated_projected_open',cell_bits=0,
                cell_overrides=[dict(cell=[10,20],level=2,bits=0),dict(cell=[9,19],bits=8)],
                actors=[dict(base_actor,location=[2688,5248,208])]),
            dict(name='elevated_origin_open',cell_bits=0,
                cell_overrides=[dict(cell=[10,20],level=2,bits=8)],
                actors=[dict(base_actor,location=[2688,5248,208])]),
            dict(name='ordered_overlap',actors=[dict(base_actor,selected=True),
                dict(base_actor,selected=True,health=75,bomb_start_frame=925,
                     location=[2705,5231,0])])]
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
        if bomb:
            spec.update(entry_boundary='complete_extras',frame=case.get('frame',1000),
                        tactical_clip=rally.ints(u,0x886FA0,4))
            for cell in scene['cells']:
                u.mem_write(cell+0x12C,words(spec.get('cell_bits',0x18)))
                u.mem_write(cell+0x11B,b'\0')
            for override in spec.get('cell_overrides',[]):
                cell=rank_runtime['cells'][tuple(override['cell'])]
                u.mem_write(cell+0x11B,bytes([override.get('level',0)&255]))
                u.mem_write(cell+0x12C,words(override['bits']))
        if groups:
            from tools import selfheal_oracle as selfheal
            if self_heal:u.mem_write(0xA8ED84,words(spec['binary_frame']))
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
        if spawns and spec.get('scenario_patch'):
            patch_before=dict(passengers=m.read32(typ+0x5E0),
                size_limit=struct.unpack('<d',u.mem_read(typ+0x388,8))[0],
                pip_scale=m.read32(typ+0x3D4),spawns_number=m.read32(typ+0xD5C))
            patch_sections,_=lexical(spec['scenario_patch'].encode(),{'CARRIER'})
            m.rules_cache(patch_sections)
            for begin,end in ((0x712511,0x712553),(0x714B35,0x714B56),
                              (0x714111,0x71412B),(0x714E97,0x714F1C)):
                _status_reader_block(m,begin,end,{UC_X86_REG_EBP:typ,
                    UC_X86_REG_EBX:typ+0x24,UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES})
            patch_after=_passenger_type_fields(m,typ,spawns=True)
        if absorber and spec.get('scenario_patch'):
            patch_before=_building_health_fields(m,typ,absorber=True)
            patch_sections,_=lexical(spec['scenario_patch'].encode(),{'YAPOWR'})
            m.rules_cache(patch_sections)
            _absorber_pip_reads(m,typ)
            patch_after=_building_health_fields(m,typ,absorber=True)
        if mobile_status or absorber:
            spec['actors'] = [dict(actor) for actor in spec['actors']]
        size_patch_observation=None
        size_override=case.get('current_type_size_override_after_read')
        if size_override:
            size_type=types[size_override['type_name']]
            before_size=bytes(u.mem_read(size_type+0x380,8))
            size_bits=int(size_override['f64_bits'],16)
            after_size=struct.pack('<Q',size_bits)
            u.mem_write(size_type+0x380,after_size)
            assert bytes(u.mem_read(size_type+0x380,8))==after_size
            size_patch_observation=dict(type_name=size_override['type_name'],
                address=f'{size_type+0x380:08X}',
                before_f64_bits=f'{struct.unpack("<Q",before_size)[0]:016X}',
                after_f64_bits=f'{size_bits:016X}',
                before_size=struct.unpack('<d',before_size)[0],
                after_size=struct.unpack('<d',after_size)[0],
                patch_boundary='after selected native retail type readers, before Cargo.Add and DrawExtras')
        u.mem_write(clip, words(*spec['clip']))
        buffers = prepare_surface(u, spec, size=tuple(size))
        guard = bytes([0xA5]) * 16
        for buffer in (PIXELS, Z, A):
            u.mem_write(buffer - 16, guard)
            u.mem_write(buffer + size[0] * size[1] * 2, guard)
        recorder = _HealthRecorder(m,size,shape_names,phase_aware=building or mobile_status,
                                   rank=rank,scales=scales,group_status=groups,wrench=wrench,self_heal=self_heal,bomb=bomb,spawns=spawns,absorber=absorber)
        text_recorder=group_text.TextRecorder(m,recorder,size) if groups else None
        stores, draws, entries, rng, actors = (recorder.stores, recorder.draws,
                                             recorder.entries, recorder.rng, [])
        for actor_index, supplied in enumerate(spec['actors']):
            recorder.actor_index = actor_index
            if groups:actor=group_actors[supplied['type_name']]
            is_garrison=passenger and supplied['type_name']=='NABNKR'
            u.mem_write(actor,group_templates[supplied['type_name']] if groups else actor_templates[supplied['type_name']] if scales else
                        actor_template if building or rank or groups or bomb else bytes(0x900))
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
            if absorber:
                u.mem_write(actor+0x21C,words(house if supplied['owner_is_current'] else other_house))
            if bomb:
                # Supplied placement boundary: the constructor prefix starts
                # in limbo, while both native Display visits require placement.
                u.mem_write(actor+0x81,b'\0')
                bomb_state=attach_bomb(actor,supplied)
                u.mem_write(0xA8ED84,words(spec['frame']))
            if groups:
                u.mem_write(house+0x56F9,bytes(house_rgb))
                group_events=group_text.apply_group_commands(m,actor,house,supplied['group_commands'])
                if self_heal and supplied.get('native_ai_before_draw'):
                    native_ai=_run_self_heal_ai(m,actor)
            if ammo:
                ammo_events=_prepare_ammo_state(m,actor,typ,supplied['ammo_initialization'])
            if spawns:
                u.mem_write(actor+0x81,b'\0')
                u.mem_write(manager,manager_template)
                u.mem_write(manager+0x2C,words(len(supplied['node_states'])))
                u.mem_write(manager+0x3C,words(manager_items,len(manager_nodes)))
                u.mem_write(manager+0x48,words(len(supplied['node_states'])))
                for index,state in enumerate(supplied['node_states']):
                    prepared=child_prefixes[index]
                    child=prepared['actor']
                    u.mem_write(child,prepared['template'])
                    u.mem_write(child+0x21C,words(m.read32(actor+0x21C)))
                    u.mem_write(child+0x2D4,words(actor))
                    u.mem_write(child+0x6C,words(m.read32(types['HORNET']+0xA0)))
                    u.mem_write(child+0x81,bytes([state in (0,6)]))
                    duration=m.read32(typ+0xD64) if state==6 else m.read32(typ+0xD60) if state==7 else 0
                    u.mem_write(manager_nodes[index],words(0 if state==7 else child,state,0,0,duration,0))
                    u.mem_write(manager_items+index*4,words(manager_nodes[index]))
                u.mem_write(actor+0x2D0,words(manager))
            cargo_events=[]
            if passenger or spawns or absorber:
                cargo_entries=[]
                def cargo_calls(_u,pc,_size,_data):
                    if pc in (0x4733A0,0x473430,0x51DF10,0x4DB260,0x6F6AC0):
                        cargo_entries.append(f'{pc:08X}')
                cargo_hook=u.hook_add(UC_HOOK_CODE,cargo_calls)
                requested=(supplied.get('passengers',[]) if absorber else
                           supplied.get('passengers',
                               [supplied['passenger']] if supplied.get('passenger') and not is_garrison else []))
                used={}
                for passenger_name in requested:
                    index=used.get(passenger_name,0)
                    used[passenger_name]=index+1
                    prepared=passenger_pools[passenger_name][index] if absorber or passenger else passengers[passenger_name]
                    u.mem_write(prepared['actor'],prepared['template'])
                    m.invoke(0x4733A0,actor+0x114,(prepared['actor'],))
                    head=m.read32(actor+0x118)
                    head_type=m.invoke(m.read32(m.read32(head)+0x84),head)
                    cargo_events.append(dict(operation='add',count=m.read32(actor+0x114),
                        head_type=m.string(head_type+0x24)))
                if requested and supplied['unload']:
                    expected_head=m.read32(actor+0x118)
                    removed=m.invoke(0x473430,actor+0x114)
                    assert removed==expected_head
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
                if absorber and 'occupants' in supplied:
                    assert supplied['occupants']==[] and requested==[]
                    # Both independent native containers are empty. This tests
                    # row origins without inventing a shared dual-list lifecycle.
                    u.mem_write(actor+0x688,words(0,0))
                    u.mem_write(actor+0x694,words(0))
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
            if bomb:
                record=m.read32(actor+0x38)
                bomb_before=bytes(u.mem_read(record,0x5C)) if record else None
                actors[-1].update(bomb=bomb_state,
                    clock_frame=m.invoke(0x438A00,record) if record else None)
            if passenger or spawns or absorber:
                current_type=types[supplied['type_name']]
                actors[-1].update(cargo_operations=cargo_events,cargo_entries=cargo_entries,
                    cargo_count=m.read32(actor+0x114),
                    pip_max=m.invoke(0x716290,current_type))
                if case['name']=='integer_size_passenger' or size_patch_observation:
                    actors[-1]['cargo_size']=m.invoke(0x473460,actor+0x114)
                if is_garrison:
                    actors[-1].update(garrison_slots=m.read32(current_type+0x1580),
                        show_occupant_pips=bool(u.mem_read(current_type+0x1584,1)[0]))
                if absorber:
                    cargo_types=[]
                    head=m.invoke(0x473450,actor+0x114)
                    for _ in range(m.read32(actor+0x114)):
                        assert head
                        p=m.invoke(m.read32(m.read32(head)+0x84),head)
                        cargo_types.append(m.string(p+0x24))
                        head=m.read32(head+0x30)
                    assert head==0
                    actors[-1].update(cargo_types=cargo_types,
                        is_absorb_allowed=bool(m.invoke(0x4598A0,actor)&255),
                        type_fields=_building_health_fields(m,typ,absorber=True),
                        entry_boundary='selected_health_dispatch' if direct_health else 'complete_extras')
            if spawns:
                manager_before=bytes(u.mem_read(manager,0x74))
                node_before=[bytes(u.mem_read(node,0x18)) for node in manager_nodes]
                actors[-1].update(spawn_count_docked=m.invoke(0x6B7D50,manager),
                    spawn_node_states=[struct.unpack('<i',u.mem_read(node+4,4))[0]
                                       for node in manager_nodes],
                    spawn_node_timers=[rally.ints(u,node+8,3) for node in manager_nodes])
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
                if self_heal:
                    actors[-1]['health_after_native_ai']=m.read32(actor+0x6C)
                    if supplied.get('native_ai_before_draw'):actors[-1]['native_ai']=native_ai
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
            if bomb and record:
                assert bytes(u.mem_read(record,0x5C))==bomb_before
            if spawns:
                assert bytes(u.mem_read(manager,0x74))==manager_before
                assert [bytes(u.mem_read(node,0x18)) for node in manager_nodes]==node_before
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
        row=dict(name=case['name'], input=spec, actor_results=actors,
                 draws=draws, entries=entries, stores=stores, pixels=pixels,
                 a_unchanged=True, z_unchanged=True, rng_entries=rng)
        if size_patch_observation:
            row['current_type_size_patch']=size_patch_observation
        rows.append(row)
        if groups:
            rows[-1].update(text_draws=text_recorder.rows,font_entries=text_recorder.entries)
            for text in text_recorder.rows:
                assert [stage['stage'] for stage in text['stages']]==['fill','outline','glyph']
                assert not any(stage[key] for stage in text['stages'] for key in
                               ('a_reads','z_reads','a_writes','z_writes'))
            if case['name'].startswith('ordered_') and not self_heal:
                first_text=next(text for text in text_recorder.rows if text['actor']==0)
                rows[-1]['overwritten_group_pixels']=sorted({(x,y,value,
                    struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0])
                    for x,y,value in first_text['glyph_stores']
                    if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},key=lambda p:(p[1],p[0]))
            if 'self_heal' in case['name'] or self_heal:
                marker_draws=[draw for draw in draws if draw['asset']=='PIPS.SHP' and draw['frame'] in (13,20)]
                rows[-1]['overwritten_self_heal_pixels']=sorted({(x,y,value,
                    struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0])
                    for draw in marker_draws for x,y,value in stores[slice(*draw['store_range'])]
                    if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},key=lambda p:(p[1],p[0]))
                assert marker_draws and all(draw['flags'] in ((0x600,0x601) if self_heal else (0x600,)) and not any(draw[key] for key in
                    ('a_reads','z_reads','a_writes','z_writes')) for draw in marker_draws)
                if self_heal:
                    for draw in marker_draws:
                        draw['native_stores']=stores[slice(*draw['store_range'])]
                    if case['name']=='ordered_two_markers':
                        first={(x,y):value for x,y,value in marker_draws[0]['native_stores']}
                        second={(x,y):value for x,y,value in marker_draws[1]['native_stores']}
                        rows[-1]['twice_darkened_pixels']=[[x,y,old,second[x,y]]
                            for x,y,old in marker_draws[1]['destination_words']
                            if first.get((x,y))==old and old!=second[x,y]]
                        assert rows[-1]['twice_darkened_pixels']
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
                for draw in draws if draw['actor']==0 and (draw.get('pip_tail') if absorber else draw['asset']=='PIPS2.SHP')
                for x,y,value in stores[slice(*draw['store_range'])]
                if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},key=lambda p:(p[1],p[0]))
        if spawns and case['name']=='independent_passenger_row':
            shapes=[draw for draw in draws if draw['asset']=='PIPS2.SHP']
            assert len(shapes)==5 and shapes[0]['point']==shapes[3]['point']
            rows[-1]['independent_row_overwrites']=sorted({(x,y,value,
                struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0])
                for draw in shapes[:3] for x,y,value in stores[slice(*draw['store_range'])]
                if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},key=lambda p:(p[1],p[0]))
            assert rows[-1]['independent_row_overwrites']
        if bomb and case['name']=='ordered_overlap':
            first_clock=next(draw for draw in draws if draw['asset']=='BOMBCURS.SHP')
            rows[-1]['overwritten_bomb_pixels']=sorted({(x,y,value,
                struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0])
                for x,y,value in stores[slice(*first_clock['store_range'])]
                if value!=struct.unpack_from('<H',raw,(y*size[0]+x)*2)[0]},key=lambda p:(p[1],p[0]))
            assert rows[-1]['overwritten_bomb_pixels']
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
        if bomb:
            rows[-1].update(gates=recorder.gates,callbacks=recorder.callbacks,
                           numeric_results=recorder.numeric)
            assert all(draw['flags']==0xE00 and not any(draw[key] for key in
                ('z_reads','z_writes','a_writes')) for draw in draws if draw['asset']=='BOMBCURS.SHP')
        if scales:
            rows[-1].update(numeric_results=recorder.numeric,drawing_fpcw=draw_fpcw)
            if spawns or absorber:
                rows[-1].update(gates=recorder.gates,callbacks=recorder.callbacks)
            assert all(draw['flags']==0x600 and not any(draw[key] for key in
                ('a_reads','z_reads','a_writes','z_writes')) for draw in draws
                if draw['asset']=='PIPS2.SHP' or is_garrison)
            if absorber:
                assert all(draw['flags']==0x600 and not any(draw[key] for key in
                    ('a_reads','z_reads','a_writes','z_writes')) for draw in draws
                    if draw['asset']=='PIPS.SHP')
                if case['name']=='independent_garrison_row':
                    tails=[draw for draw in draws if draw['pip_tail']]
                    assert len(tails)==7 and tails[0]['point']==tails[2]['point']
        if groups and saved_patch_fields:
            m.rules_cache({name:{key:('yes' if value else 'no') if key=='Organic' else str(value)
                                 for key,value in keys.items()} for name,keys in saved_patch_fields.items()})
            for name,keys in saved_patch_fields.items():
                for key,begin,end in (('Organic',0x715024,0x715045),('PixelSelectionBracketDelta',0x71415F,0x714179)):
                    if key in keys:_status_reader_block(m,begin,end,{UC_X86_REG_EBP:group_types[name],
                        UC_X86_REG_EBX:group_types[name]+0x24,UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES})
        if spawns and spec.get('scenario_patch'):
            rows[-1]['scenario_reader']=dict(before=patch_before,after=patch_after)
            assert patch_before['pip_scale']==0
            m.rules_cache({'CARRIER':{'Passengers':str(patch_before['passengers']),
                'SizeLimit':str(patch_before['size_limit']),'PipScale':'None',
                'SpawnsNumber':str(patch_before['spawns_number'])}})
            for begin,end in ((0x712511,0x712553),(0x714B35,0x714B56),
                              (0x714111,0x71412B),(0x714E97,0x714F1C)):
                _status_reader_block(m,begin,end,{UC_X86_REG_EBP:typ,
                    UC_X86_REG_EBX:typ+0x24,UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:RULES})
            assert m.read32(typ+0x5E0)==patch_before['passengers']
            assert m.read32(typ+0x3D4)==patch_before['pip_scale']
            assert m.read32(typ+0xD5C)==patch_before['spawns_number']
            assert struct.unpack('<d',u.mem_read(typ+0x388,8))[0]==patch_before['size_limit']
        if absorber and spec.get('scenario_patch'):
            rows[-1]['scenario_reader']=dict(before=patch_before,after=patch_after)
            restore={'PipScale':{0:'None',1:'Ammo',2:'Tiberium',3:'Passengers',4:'Power',5:'MindControl'}[patch_before['pip_scale']],
                'Passengers':str(patch_before['passengers']),
                'MaxNumberOccupants':str(patch_before['max_number_occupants'])}
            for key,field in (('PipsDrawForAll','pips_draw_for_all'),('Gunner','gunner'),
                    ('InfantryAbsorb','infantry_absorb'),('UnitAbsorb','unit_absorb'),
                    ('CanBeOccupied','can_be_occupied'),('ShowOccupantPips','show_occupant_pips')):
                restore[key]='yes' if patch_before[field] else 'no'
            m.rules_cache({'YAPOWR':restore})
            _absorber_pip_reads(m,typ)
            assert _building_health_fields(m,typ,absorber=True)==patch_before
        if size_patch_observation:
            u.mem_write(size_type+0x380,before_size)
            assert bytes(u.mem_read(size_type+0x380,8))==before_size
    assert original_code == sha(u.mem_read(0x401000, 0x3E0000))
    assert original_vtables == bytes(u.mem_read(0x7F5C70, 0x750))
    if scales:
        if not spawns:assert tail_defaults['spawns_number']==tail_type()['spawns_number']==0
        assert tail_defaults['organic']==tail_type()['organic']==False
    else:
        assert tail_defaults == tail_type()
    result = dict(schema_version=1, physical_inputs=physical,
                  readers=dict(constructor=constructor, layers=layers, result=fields(),
                               ordinary_tail_constructor=tail_defaults),
                  palette=palette_result, raster_controls=rows, original_code_unchanged=True)
    if passenger:
        result['startup_fpcw']=startup_fpcw
        result['passenger_pool_runtime']={name:[prepared['evidence'] for prepared in pool]
                                          for name,pool in passenger_pools.items()
                                          if name=='MTNK'}
    if rank:
        result['rank_runtime'] = rank_runtime['evidence']
        result['vtable_bindings'] = _rank_vtable_bindings(m)
    if groups:
        result.update(status_runtime={'MTNK':rank_runtime['evidence']},
            house_color=dict(name='Gold',native_source='house_color.json Gold scalar House50B840/500DF7',rgb=house_rgb),
            native_font=dict(chain='434AD0->433880->433990->434700',
                runtime_fields={k:v for k,v in native_font['font_meta'].items() if k not in
                                ('font_object','renderer_object','font_data','lookup','glyphs')},
                digit_metrics=native_font['native_digit_metrics'],
                file_reads=[dict(file_offset=o,size=n) for o,n in native_font['expected_reads']]))
        result['status_runtime']['E1']=prepared_infantry['evidence']
        result['readers']['type_results']={name:dict(strength=m.read32(p+0xA0),
            delta=m.read32(p+0x3E0),organic=bool(u.mem_read(p+0xD97,1)[0])) for name,p in group_types.items()}
        if self_heal:
            result['divide_controls']=_self_heal_divide_controls(m,rules,house,
                group_actors,group_templates,point,clip)
            result['reader_controls']=_self_heal_reader_controls(m)
            assert original_code==sha(u.mem_read(0x401000,0x3E0000))
            assert original_vtables==bytes(u.mem_read(0x7F5C70,0x750))
        else:
            result['assignment_controls']=group_text.assignment_controls(m,actor_template)
            assert all(next(row for row in rows if row['name']==name)['overwritten_group_pixels']
                       for name in ('ordered_groups','ordered_health'))
            assert next(row for row in rows if row['name']=='ordered_self_heal_group')['overwritten_self_heal_pixels']
    if scales:
        if passenger:result['read_pip']=pip_controls
        result['readers'].update(type_constructors=type_constructors,
            type_results={name:_passenger_type_fields(m,p,name in infantry_names,ammo=ammo or spawns,spawns=spawns)
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
        if spawns:
            result.update(manager_constructor=manager_ctor,
                child_constructor_prefixes=[prepared['evidence'] for prepared in child_prefixes],
                spawn_lifecycle_reference='tools/rocket_oracle/spawn_manager.json')
        if absorber:
            result['readers']['type_results']['YAPOWR'].update(_building_health_fields(m,typ,absorber=True))
            result['reader_controls']=_absorber_reader_controls(m)
            result['passenger_pools']={name:[prepared['evidence'] for prepared in pool]
                                      for name,pool in passenger_pools.items()}
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
        if not absorber:
            result['composition_controls'] = _building_health_composition(
                scene, ground_planes, actor_template, shape_names,wrench=wrench)
        else:
            def install_absorber_cargo(receiver):
                names=['INIT','YENGINEER']
                for name in names:
                    prepared=passenger_pools[name][0]
                    u.mem_write(prepared['actor'],prepared['template'])
                    m.invoke(0x4733A0,receiver+0x114,(prepared['actor'],))
                return names
            result.update(building_runtime=scene['building_runtime'],body_asset='YGPOWR.SHP')
            result['composition_controls']=_building_health_composition(
                scene,ground_planes,actor_template,shape_names,absorber=install_absorber_cargo)
        if wrench:
            result['health_palette']=health_palette
    if bomb:
        result.update(status_runtime={'MTNK':rank_runtime['evidence'],
                                      'IVAN':prepared_ivan['evidence']},
                      health_palette=health_palette,
                      bomb_state_reference='tools/spatial_oracle/bomb_class.json',
                      geometry_startup=scene['geometry_startup'])
        for cell in scene['cells']:
            u.mem_write(cell+0x12C,words(0x18));u.mem_write(cell+0x11B,b'\0')
        u.mem_write(0x887324,words(scene['tactical']))
        u.mem_write(0x886FA0,words(0,0,160,160));u.mem_write(0xB0CE30,words(160,160))
        result['readers'].update(building_type_layers=scene['layer_reads'],
                                building_type_layer_fields=scene['type_layer_fields'])
        result['physical_inputs']+=scene['sources']
        result['physical_inputs'].append(dict(name='ARTMD.INI',archive='ra2md.mix/localmd.mix',
            bytes=len(m.assets['ARTMD.INI']),sha256=sha(m.assets['ARTMD.INI'])))
        result['terrain_inputs']=scene['terrain']
        result['body_priors']=dict(ground_cells=scene['ground_cells'],bib_null=True,
                                  color_scheme=scene['color_scheme'])
        result['composition_controls']=_building_health_composition(scene,ground_planes,
            building_template,shape_names,wrench=True,bomb=attach_bomb)
        assert original_code==sha(u.mem_read(0x401000,0x3E0000))
    return result


def _building_health_composition(scene, ground, actor_template, shape_names, *, wrench=False,
                                 bomb=None, absorber=None):
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
        u.mem_write(actor + 0x6C, words(m.read32(typ+0xA0) if absorber else 600 if wrench else 750))
        u.mem_write(neighbor, neighbor_template)
        if wrench:
            u.mem_write(0xA8ED84,words(0));u.mem_write(0xA8EB60,words(4))
            u.mem_write(actor+0x6E8,b'\1')
            u.mem_write(neighbor+0x6C,words(187))
            for cell in scene['cells']:u.mem_write(cell+0x12C,words(0x18))
        bomb_state=None
        if bomb:
            bomb_state=bomb(actor,dict(bomb_start_frame=1000,bomb_visible=True))
            u.mem_write(0xA8ED84,words(1000))
        if absorber:composition_passengers=absorber(actor)
        recorder = _HealthRecorder(m, size, shape_names, phase_aware=True,bomb=bool(bomb),absorber=bool(absorber),scales=bool(absorber))
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
        health_draws = [draw for draw in recorder.draws if draw['asset']=='PIPS.SHP' and not draw.get('pip_tail',False)]
        assert len(health_draws)==(60 if wrench and with_neighbor else 30)
        status_draws=[draw for draw in recorder.draws if draw['asset'] in
                      ('PIPS.SHP','WRENCH.SHP',*(['BOMBCURS.SHP'] if bomb else []))] if wrench or absorber else health_draws
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
        if bomb:
            bomb_pixels={(x,y):word for draw in recorder.draws if draw['asset']=='BOMBCURS.SHP'
                for x,y,word in recorder.stores[slice(*draw['store_range'])]}
            bomb_overwrites=[[x,y,word,struct.unpack_from('<H',final,(y*160+x)*2)[0]]
                for (x,y),word in sorted(bomb_pixels.items(),key=lambda v:(v[0][1],v[0][0]))
                if struct.unpack_from('<H',final,(y*160+x)*2)[0]!=word]
        if absorber:
            absorber_touched={(x,y) for draw in recorder.draws if draw.get('pip_tail')
                for x,y,_word in recorder.stores[slice(*draw['store_range'])]}
            health_touched={(x,y) for draw in health_draws
                for x,y,_word in recorder.stores[slice(*draw['store_range'])]}
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
        if bomb:
            rows[-1].update(overwritten_bomb_pixels=bomb_overwrites,
                body_overwritten_bomb_pixels=[row for row in overwritten if tuple(row[:2]) in bomb_pixels],
                gates=recorder.gates,callbacks=recorder.callbacks,numeric_results=recorder.numeric,
                entry_boundary='complete_extras_body',frame=1000,bomb=bomb_state)
            rows[-1]['input']['actors'][0].update(bomb_start_frame=1000,bomb_visible=True)
        if absorber:
            rows[-1]['input']['actors'][0].update(type_name='YAPOWR',kind='building',
                health=m.read32(typ+0xA0),owner_is_current=True,
                passengers=composition_passengers,unload=False)
            if with_neighbor:
                rows[-1]['input']['actors'][1].update(type_name='YAPOWR',kind='building',
                    health=m.read32(typ+0xA0),owner_is_current=True,passengers=[],unload=False)
            rows[-1].update(entry_boundary='complete_extras_body',
                body_overwritten_absorber_pixels=[row for row in overwritten if tuple(row[:2]) in absorber_touched])
            rows[-1]['body_overwritten_health_pixels']=[row for row in overwritten if tuple(row[:2]) in health_touched]
    assert scene['original_code'] == sha(u.mem_read(0x401000,0x3E0000))
    return rows


def generate_primary_factory_status(*, power=False):
    """Original Ground2 Building-array label pass, sharing the status VM/font.

    Primary bytes are render-consumer priors backed by the separate executed
    manual-primary producer. This profile does not reproduce the gameplay
    history, earlier Display opportunities or factory body art a second time.
    """
    from tools import control_group_text_oracle as text_owner
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.spatial_oracle.building_body_rules import INI, SP as READER_SP
    from tools.storage_oracle.keyboard_bindings import stock_csf

    producer_path=Path(__file__).resolve().parents[1]/'spatial_oracle/building_manual_primary.json'
    producer=json.loads(producer_path.read_text())
    history=producer['cases'][0]
    assert history['input']['name']=='ordinary_self_click_switch_back_and_stale_event'
    on=history['phases'][3]['state']['primary'][1]
    off=history['phases'][5]['state']['primary'][1]
    assert (on,off)==(1,0)
    producer_priors=dict(path='tools/spatial_oracle/building_manual_primary.json',
        case=history['input']['name'],actor=2,
        primary_on=dict(phase=3,value=on),primary_off=dict(phase=5,value=off))
    # GAWEAP's full ART reader probes temperate names before loading the
    # physical generic winners. TMP's existing RawFile platform owns those
    # probes too; explicit None means physical absence, not an invented SHP.
    root=native.configured_gamemd().parent
    ra2=stock.mix((root/'ra2.mix').read_bytes())
    generic=stock.mix(ra2[stock.mix_hash('generic.mix')])
    isotemp=stock.mix(ra2[stock.mix_hash('isotemp.mix')])
    temperate=stock.mix(ra2[stock.mix_hash('temperat.mix')])
    prepared_files={}
    factory_sources=[]
    for suffix in (() if power else ('','_1','_2','_3','_4','_A','_B','BB')):
        name=f'GGWEAP{suffix}.SHP'
        raw=generic[stock.mix_hash(name)]
        prepared_files[name]=raw
        factory_sources.append(dict(name=name,archive='ra2.mix/generic.mix',
                                    bytes=len(raw),sha256=sha(raw)))
    if not power:
        name='GTWEAPMK.SHP'
        raw=isotemp[stock.mix_hash(name)]
        prepared_files[name]=raw
        factory_sources.append(dict(name=name,archive='ra2.mix/isotemp.mix',
                                    bytes=len(raw),sha256=sha(raw)))
    absent_probes=[]
    for suffix in (() if power else ('','_1','_2','_3','_4','_A','_AD','_B','_BD','BB')):
        name=f'GTWEAP{suffix}.SHP'
        assert not (root/name.lower()).exists() and not (root/name).exists()
        assert all(stock.mix_hash(name) not in archive for archive in (generic,isotemp,temperate))
        prepared_files[name]=None
        absent_probes.append(name)
    type_name='GAPOWR' if power else 'GAWEAP'
    scene=_building_ground_runtime(type_name=type_name,render_ground=False,
        prepared_files=prepared_files,building_registry=True,power=power,
        extra_art_names=() if power else tuple('GAWEAP'+suffix for suffix in ('_1','_2','_3','_4','_B','_BD')))
    m,u,typ,current,tactical=(scene[key] for key in ('m','u','typ','house','tactical'))
    if not power:
        assert m.read32(typ+0xEF0)==producer['native_type_inputs']['GAWEAP']['foundation']
        assert m.read32(typ+0xEE0)==0
    power_reader_controls=None
    if power:
        from tools.spatial_oracle.building_body_rules import RULES as INI_RULES
        control_type=m.alloc(0x2000)
        m.invoke(0x45DD90,control_type,(m.cstring('GAPOWR'),))
        constructor=_building_health_fields(m,control_type,power=True)
        controls=[]
        sequence=[('positive200',{'Power':'200'}),('missing',{}),
            ('empty',{'Power':''}),('invalid',{'Power':'junk'}),
            ('exact_key_case',{'power':'999'}),('section_absent',None),
            ('negative75',{'Power':'-75'}),('missing_after_negative',{}),
            ('empty_after_negative',{'Power':''}),('invalid_after_negative',{'Power':'junk'}),
            ('case_after_negative',{'power':'999'}),('zero',{'Power':'0'}),
            ('missing_after_zero',{}),('numeric_suffix',{'Power':'17junk'}),
            ('signed_overflow',{'Power':'2147483648'}),('missing_after_overflow',{}),
            ('restore200',{'Power':'200'})]
        pending_read=None
        power_reads=[]
        def observe_power_read(_u,pc,_size,_data):
            nonlocal pending_read
            sp=u.reg_read(UC_X86_REG_ESP)
            if pc==0x5276D0 and m.string(m.read32(sp+8))=='Power':
                assert m.string(m.read32(sp+4))=='GAPOWR'
                row=dict(entry='005276D0',default=struct.unpack('<i',u.mem_read(sp+12,4))[0])
                power_reads.append(row)
                pending_read=(m.read32(sp),row)
            if pending_read is not None and pc==pending_read[0]:
                pending_read[1]['value']=struct.unpack('<i',words(u.reg_read(UC_X86_REG_EAX)))[0]
                pending_read=None
        reader_hook=u.hook_add(UC_HOOK_CODE,observe_power_read)
        for name,keys in sequence:
            sections={} if keys is None else {'GAPOWR':dict(Strength='750',**keys)}
            raw='\n'.join('['+section+']\n'+'\n'.join(key+'='+value for key,value in values.items())
                          for section,values in sections.items())
            # Use the same physical lexical boundary as retail passes.
            # Stored-empty cache entries are a separate, nonphysical input.
            cached_sections,_=lexical(raw.encode('latin1'),{'GAPOWR'})
            m.rules_cache(cached_sections)
            power_reads.clear()
            admitted=bool(m.invoke(0x45FE50,control_type,(INI_RULES,))&255)
            assert pending_read is None
            controls.append(dict(name=name,sections=sections,cached_sections=cached_sections,admitted=admitted,
                power_reads=[dict(row) for row in power_reads],
                after=_building_health_fields(m,control_type,power=True)))
        u.hook_del(reader_hook)
        power_reader_controls=dict(constructor=constructor,history=controls)
    other=m.alloc(0x18000)
    u.mem_write(other,bytes(u.mem_read(current,0x18000)))
    country=m.alloc(0x100)
    u.mem_write(country+0xB8,words(0))
    for index,house in enumerate((current,other)):
        u.mem_write(house+0x30,words(index))
        u.mem_write(house+0x34,words(country))
        if power:
            # The original House constructor publishes its six actual
            # interface tables here. Other House state is supplied below;
            # this is not a whole House construction/lifecycle claim.
            _status_reader_block(m,0x4F5DCC,0x4F5DF6,{UC_X86_REG_EBP:house})
    actors=[scene['building'],m.alloc(0x800)]
    _prepare_ground_building(m,actors[1],typ,other)
    u.mem_write(0xA83D4C,words(current))
    # Constructor registration, not a host-sorted replacement array.
    assert m.read32(0xA8EB50)==len(actors)
    assert rally.ints(u,m.read32(0xA8EB44),len(actors))==actors

    outer=stock.mix((native.configured_gamemd().parent/'ra2.mix').read_bytes())
    font_raw=stock.mix(outer[stock.mix_hash('local.mix')])[stock.mix_hash('GAME.FNT')]
    assert sha(font_raw)=='ec8d6d85db3eedf5a862cc6ffb87bfad4c77279dd946bc2d30879ad9140c8d3f'
    font=text_owner.initialize_font(font_raw,owner=m)
    format_hook=text_owner.install_digit_format(m,native=True) if power else None
    strings,csf_sha=stock_csf()
    labels={key:strings[key] for key in (('TXT_POWER_DRAIN2','TXT_PRIMARY','TXT_PRI') if power else ('TXT_PRIMARY','TXT_PRI'))}
    assert labels['TXT_PRIMARY']=='Primary' and labels['TXT_PRI']=='Pri.'
    if power:assert labels['TXT_POWER_DRAIN2']=='Power = %d Drain = %d'
    string_pointers={key:m.alloc(len(text_owner.wide(value))) for key,value in labels.items()}
    for key,pointer in string_pointers.items():u.mem_write(pointer,text_owner.wide(labels[key]))
    csf_calls=[]
    def physical_string(_u,_pc,_size,_data):
        key=m.string(u.reg_read(UC_X86_REG_ECX))
        if key not in labels:raise RuntimeError(f'unexpected Building label CSF request {key!r}')
        csf_calls.append(dict(actor=recorder.actor_index,key=key,text=labels[key]))
        m.ret(string_pointers[key],8)
    csf_hook=u.hook_add(UC_HOOK_CODE,physical_string,begin=0x734E60,end=0x734E60)
    colors=json.loads(Path(__file__).with_name('house_color.json').read_text())
    assert colors['rulesmd_sha256']==scene['layer_reads'][0]['sha256']
    house_rgb={name:next(row for row in colors['colors'] if row['name']==name)['modes']['scalar']['rgb']
               for name in ('Gold','Red')}
    actor_templates=[bytes(u.mem_read(p,0x800)) for p in actors]
    type_template=bytes(u.mem_read(typ,0x2000))
    art,_=lexical(m.assets['ARTMD.INI'],{type_name,m.string(typ+0x1F8)})
    original_code=sha(u.mem_read(0x401000,0x3E0000))
    base=dict(type_name=type_name,location=[2688,5248,0],owner='Owner',color='Gold',
              selected=True,hovered=False,marked=True,body_opportunity=True,
              display_bits=0,primary=False if power else bool(on))
    if power:base['health']=m.read32(typ+0xA0)
    cases=[dict(name=f'primary_a{value}',a_value=value) for value in (127,63,2,0)]
    cases += [dict(name='occluding_z',old_z=0),
        dict(name='native_primary_off',actors=[dict(base,primary=bool(off))]),
        dict(name='hover_only',actors=[dict(base,selected=False,hovered=True)]),
        dict(name='unmarked',actors=[dict(base,marked=False)]),
        dict(name='no_body_opportunity',actors=[dict(base,body_opportunity=False)]),
        dict(name='one_cell_foundation',foundation_override='1x1'),
        dict(name='left_edge',camera=[-284,376]),
        dict(name='right_edge',camera=[-464,376]),
        dict(name='viewer_only_ally',owner_allied_to_current=False,current_allied_to_owner=True,
             actors=[dict(base,owner='Ally',color='Red')]),
        dict(name='owner_only_ally',owner_allied_to_current=True,current_allied_to_owner=False,
             actors=[dict(base,owner='Ally',color='Red')]),
        dict(name='constructor_order_overlap',actors=[base,
             dict(base,owner='Ally',color='Red',location=[2696,5240,0])])]
    if power:
        # Retained House output/drain are explicit consumer priors. The
        # production owner must read those cached values, not infer totals
        # from this label's Type, HP, visible buildings or CurrentPlayer.
        cases=[dict(name=f'power_a{value}',a_value=value) for value in (127,63,2,0)]
        cases += [dict(name='occluding_z',old_z=0),
            dict(name='deselected',actors=[dict(base,selected=False)]),
            dict(name='hover_only',actors=[dict(base,selected=False,hovered=True)]),
            dict(name='unmarked',actors=[dict(base,marked=False)]),
            dict(name='no_body_opportunity',actors=[dict(base,body_opportunity=False)]),
            dict(name='left_edge',camera=[-284,376]),
            dict(name='right_edge',camera=[-464,376]),
            dict(name='nonallied_zero_display',owner_allied_to_current=False,
                actors=[dict(base,owner='Ally',color='Red')]),
            dict(name='viewer_only_ally',owner_allied_to_current=False,current_allied_to_owner=True,
                actors=[dict(base,owner='Ally',color='Red')]),
            dict(name='owner_only_ally',owner_allied_to_current=True,current_allied_to_owner=False,
                actors=[dict(base,owner='Ally',color='Red')]),
            dict(name='cached_zero_differs_from_positive_type',retained_house_power={
                'Owner':dict(output=0,drain=0),'Ally':dict(output=75,drain=300)}),
            dict(name='cached_signed_owner_values',retained_house_power={
                'Owner':dict(output=-123,drain=456),'Ally':dict(output=75,drain=300)}),
            dict(name='constructor_order_overlap',actors=[base,
                dict(base,owner='Ally',color='Red',location=[2696,5240,0])]),
            dict(name='non_ground_layer1',current_layer=1),
            dict(name='non_ground_layer3',current_layer=3)]
    rows=[]
    caller_rows=[]
    for case in cases:
        spec=dict(size=[160,160],camera=[-384,376],clip=[0,0,160,160],a_value=127,
                  old_z=65535,background=rally.BACKGROUND,owner_allied_to_current=True,
                  current_allied_to_owner=False,foundation_override=None,actors=[dict(base)])
        if power:
            spec.update(current_layer=2,retained_house_power={
                'Owner':dict(output=200,drain=0),'Ally':dict(output=75,drain=300)})
        spec.update({key:value for key,value in case.items() if key!='name'})
        spec['actors']=[dict(actor) for actor in spec['actors']]
        u.mem_write(typ,type_template)
        if spec['foundation_override']:
            changed={name:dict(keys) for name,keys in art.items()}
            changed.setdefault('GAWEAP',{})['Foundation']=spec['foundation_override']
            m.make_ini(changed)
            _status_reader_block(m,0x461225,0x46125D,
                {UC_X86_REG_EBP:typ,UC_X86_REG_EBX:typ+0x24,
                 UC_X86_REG_ESI:INI,UC_X86_REG_EDI:typ+0x1F8})
            assert m.invoke(0x45EC90,typ)==1
        for index,pointer in enumerate(actors):
            u.mem_write(pointer,actor_templates[index])
            u.mem_write(pointer+0x99,b'\0')
            u.mem_write(pointer+0x83,b'\0')
            u.mem_write(pointer+0x3D3,b'\0')
        for house in (current,other):u.mem_write(house+0x5788,words(0))
        if spec['owner_allied_to_current']:u.mem_write(other+0x5788,words(1))
        if spec['current_allied_to_owner']:u.mem_write(current+0x5788,words(2))
        for index,actor in enumerate(spec['actors']):
            pointer=actors[index]
            house=current if actor['owner']=='Owner' else other
            u.mem_write(pointer+0x21C,words(house))
            u.mem_write(pointer+0x9C,words(*actor['location']))
            if power:u.mem_write(pointer+0x6C,words(actor['health']))
            # The separate non-allied discovery producer is outside this
            # ordinary owned/allied label comparison, not an invented mask.
            u.mem_write(pointer+0x210,words(actor['display_bits']))
            for offset,key in ((0x74,'marked'),(0x83,'selected'),(0x99,'body_opportunity'),
                               (0x3D3,'primary'),(0x431,'hovered')):
                u.mem_write(pointer+offset,bytes([actor[key]]))
            u.mem_write(house+0x56F9,bytes(house_rgb[actor['color']]))
        if power:
            for house_name,house in (('Owner',current),('Ally',other)):
                cached=spec['retained_house_power'][house_name]
                u.mem_write(house+0x53A4,words(cached['output'],cached['drain']))
        u.mem_write(0xA83D4C,words(current))
        u.mem_write(tactical+0xB0,words(*spec['camera']))
        u.mem_write(0x886FA0,words(*spec['clip']))
        u.mem_write(0xB0CE30,words(*spec['size']))
        buffers=prepare_surface(u,spec,size=tuple(spec['size']))
        recorder=_HealthRecorder(m,spec['size'],{},phase_aware=True)
        recorder.phase='after_ground2'
        text=text_owner.TextRecorder(m,recorder,spec['size'],direct_simple_text=True)
        entries=[]
        getters,formats=[],[]
        pending_format=None
        identities={pointer:index for index,pointer in enumerate(actors)}
        def observe(_u,pc,_size,_data):
            nonlocal pending_format
            if pc==0x43E7B0:recorder.actor_index=identities[u.reg_read(UC_X86_REG_ECX)]
            if pc in (0x6D9423,0x43E7B0,0x459EF0,0x70AA60,0x4A59E0,0x4A5EB0):
                entries.append(dict(actor=recorder.actor_index,entry=f'{pc:08X}'))
            if power and pc in (0x4F6A0A,0x4F6A1A):
                sp=u.reg_read(UC_X86_REG_ESP)
                house=m.read32(sp+4)-0x24
                assert house in (current,other)
                getters.append(dict(actor=recorder.actor_index,
                    house='Owner' if house==current else 'Ally',
                    entry=f'{pc-0xA:08X}',
                    value=struct.unpack('<i',words(u.reg_read(UC_X86_REG_EAX)))[0]))
            if power and pc==0x7CA564:
                sp=u.reg_read(UC_X86_REG_ESP)
                dest,fmt,output,drain=struct.unpack('<IIii',u.mem_read(sp+4,16))
                row=dict(actor=recorder.actor_index,entry=f'{pc:08X}',
                    format=text_owner.readwide(m,fmt),arguments=[output,drain])
                formats.append(row)
                pending_format=(m.read32(sp),dest,row)
            if pending_format is not None and pc==pending_format[0]:
                _return,dest,row=pending_format
                row.update(returned=u.reg_read(UC_X86_REG_EAX),text=text_owner.readwide(m,dest))
                pending_format=None
        observer=u.hook_add(UC_HOOK_CODE,observe)
        before=[bytes(u.mem_read(pointer,0x800)) for pointer in actors]
        house_before=[bytes(u.mem_read(house,0x18000)) for house in (current,other)] if power else None
        first_csf=len(csf_calls)
        u.mem_write(READER_SP,bytes(0x300))
        u.mem_write(READER_SP+0x2C,words(spec.get('current_layer',2)))
        u.reg_write(UC_X86_REG_ESP,READER_SP)
        native.run_checked(u,0x6D9423,0x6D95A1,count=4000000,
                           required_addresses=(0x6D9423,))
        text.finish();recorder.finish()
        u.hook_del(observer);text.close();recorder.close()
        raw=bytes(u.mem_read(PIXELS,160*160*2))
        assert before==[bytes(u.mem_read(pointer,0x800)) for pointer in actors]
        assert not recorder.rng
        assert bytes(u.mem_read(A,len(buffers['lighting'])))==buffers['lighting']
        assert bytes(u.mem_read(Z,len(buffers['depths'])))==buffers['depths']
        assert original_code==sha(u.mem_read(0x401000,0x3E0000))
        pixels=[[index%160,index//160,value[0]]
                for index,value in enumerate(struct.iter_unpack('<H',raw))
                if value[0]!=spec['background']]
        row=dict(name=case['name'],input=spec,
            native_foundation=m.read32(typ+0xEF0),native_foundation_width=m.invoke(0x45EC90,typ),
            csf_calls=csf_calls[first_csf:],caller_entries=entries,text_draws=text.rows,
            pixels=pixels,color_sha256=sha(raw),actor_bytes_unchanged=True,
            a_unchanged=True,z_unchanged=True,rng_entries=recorder.rng)
        if power:
            assert house_before==[bytes(u.mem_read(house,0x18000)) for house in (current,other)]
            assert pending_format is None
            # The renderer consumes final sparse pixels and the actual text
            # origin/stage counters, not a duplicate per-glyph memory replay.
            for draw in row['text_draws']:draw.pop('glyph_stores',None)
            row.update(getter_calls=getters,format_calls=formats,house_bytes_unchanged=True)
        (caller_rows if power and spec['current_layer']!=2 else rows).append(row)
    u.hook_del(csf_hook)
    if format_hook is not None:u.hook_del(format_hook)
    for hook in font['hooks']:u.hook_del(hook)
    physical=scene['sources']+factory_sources+[
        dict(name='ARTMD.INI',archive='ra2md.mix/localmd.mix',sha256=sha(m.assets['ARTMD.INI'])),
        dict(name='GAME.FNT',archive='ra2.mix/local.mix',sha256=sha(font_raw))]
    result=dict(schema_version=1,
        producer_priors=producer_priors,physical_inputs=physical,
        raw_file_inputs=dict(theater=scene['terrain']['theater']['name'],absent=absent_probes,
            boundary='Explicit physical temperate-name absences and GG/temperate-buildup byte winners; original filename probes/fallback execute in the existing TMP RawFile transport. Native archive search is not executed.'),
        retail_csf=dict(name='ra2md.csf',archive='langmd.mix',sha256=csf_sha,labels=labels),
        native_font=dict(runtime_fields=font['font_meta'],
                         file_reads=[dict(file_offset=row['file_offset'],size=row['size'])
                                     for row in font['loader']['reads']]),
        native_type_inputs=dict(name=type_name,foundation=scene['type_layer_fields'][-1]['after']['foundation']),
        layers=scene['layer_reads'],house_colors=house_rgb,raster_controls=rows,
        original_code_unchanged=True)
    if power:
        result.pop('producer_priors')
        result.update(constructor_fields=scene['constructor_fields'],
            reader_controls=power_reader_controls,type_layer_fields=scene['type_layer_fields'],
            caller_controls=caller_rows,
            historical_primary_guard='tools/procedural_drawing_oracle/primary_factory_status.json',
            house_interface=dict(constructor_begin='004F5DCC',constructor_stop='004F5DF6',
                interface_offset=0x24,vtable=f'{m.read32(current+0x24):08X}',
                slots={name:dict(offset=offset,entry=f'{m.read32(m.read32(current+0x24)+offset):08X}')
                       for name,offset in (('output',0x20),('drain',0x24))}))
        result['native_type_inputs'].update(scene['type_layer_fields'][-1]['after'])
    return result


def primary_factory_status_metadata(*, power=False):
    result=native.provenance(
        scope='Original Ground2 Building-array PRIMARY labels, physical CSF/GAME.FNT and actual packed Surface pixels; bounded production renderer consumer',
        assumptions=[
            'Reuse the existing PaletteReader/TMP/Building reader/runtime. Original Building-array CRT prefix4E6B60..4E6B96 (initializer pointer81322C) runs before the two43B740 constructors, which self-register in native order through43BB0F..43BB24; only process-exit callback registration at4E6B96 is omitted. No host-built Building array supplies order. GAWEAP full original retained reader consumes physical RULESMD/optional LANGRULE/Battle/XMP03T4; fixed ART Foundation and related animation sections are physical. Explicit absent GT filename probes, physical GG body/animation bytes and temperate GTWEAPMK share the existing TMP RawFile/Reader cache transport; original file-probe/fallback code executes, not native archive search. No GAWEAP body/bib/animation shape parity or whole scenario is claimed.',
            'The separate executed building_manual_primary producer supplies GAWEAP actor2 true atcase0phase3 and false atphase5. Those byte values are explicit label-consumer priors, not another execution of selection, events, Unlimbo or cleanup. Selected/hover, mark74, earlier Display opportunity99, location, House index/country/alliance, zero non-allied210 display bits and palette RGB are supplied bounds.',
            'Original6D9423..6D95A1 executes its actual currentLayer2 gate and BuildingClass::Array forward loop, mark/opportunity gates,459EF0 projection, viewport bounds and direct43E7B0. Actual selected/owner-alliance gates,70AA60 primary/width branch,45EC90,4A59E0,421B60, Surface fill/outline and4A5EB0/font raster execute. The earlier Display latch producer and later Air/Top/Extras are instruction evidence, not a whole rendered frame.',
            'A named one-cell ART override runs the original461225..46125D Foundation reader for TXT_PRI; this is a declared reader control, not a stock GAWEAP footprint. No host formula supplies a rectangle, text width, font pixel or output color. House56F9 inputs reuse the existing original house_color corpus and current physical RULES hash.',
            'Physical GAME.FNT is loaded by the existing434AD0/433990 owner in this SAME VM. RGB565 Surface, opaque backgrounds, A127/63/2/0 and oldZ65535/0 are prepared consumer planes. Original actor bytes and A/Z remain unchanged during each measured loop; drawing RNG requests are empty. Full Display, picking, fog, alpha, cloak/flash, non-allied210, arbitrary strings and device output remain separate bounds.',
        ],substitutions=[
            'Inherited allocation/delete/CRT TLS/Win32 SetRect and immutable archive/file/BSurface memory transports are unchanged. CSF734E60 is an explicit UTF16 transport returning only TXT_PRIMARY/TXT_PRI from physical langmd.mix/ra2md.csf through existing stock_csf; no branch, measurement, clip, font lookup, raster or reached pixel return is supplied.',
        ],entry_points=dict(building_registry_begin=0x4E6B60,building_registry_stop=0x4E6B96,
            caller_begin=0x6D9423,caller_stop=0x6D95A1,
            building_text=0x43E7B0,status=0x70AA60,render_coords=0x459EF0,
            foundation_width=0x45EC90,measure=0x4A59E0,clip=0x421B60,
            fill=0x7BB020,outline=0x7BAD90,text=0x4A5EB0,glyph=0x434120,
            font_ctor=0x434AD0,font_load=0x433990,csf_transport=0x734E60))
    if power:
        result.update(scope='Original ordinary selected GAPOWR Ground2 Power/Drain labels, actual retained House getters/CRT and physical CSF/GAME.FNT raster',
            assumptions=[
                'One opt-in mode extends the existing primary_factory_status generator, PaletteReader/TMP Building runtime, native reader, TextRecorder, font loader and packed Surface owners. Original45DD90/45FE50 consume physical RULESMD, absent optional LANGRULE, Battle and XMP03T4; constructor Power output/drain are0/0, stock GAPOWR retains200/0 and Strength750. Missing mode/map sections return false without clearing those fields. ARTMD is fixed. Archive search itself is supplied, not executed.',
                'reader_controls constructs a separate original GAPOWR type in the same VM and executes complete45FE50 for each raw section history. Existing lexical preparation omits physical empty entries; both raw sections and cached_sections are retained. Actual5276D0 defaults/returns and signed output/drain slots establish missing/empty retention, present-junk0, exact key case, decimal suffix, INT_MIN wrap and subsequent retention. This is cached original-reader execution, not full native INI file loading or a synthetic stored-empty entry claim.',
                'Original Building-array initialization4E6B60..4E6B96 and two43B740 constructors own registry order. Selected/hover, mark74, earlier Display opportunity99, full-health750, raw Location, House country/index/alliance, zero non-allied210 and palette RGB remain explicit consumer priors. Original ordinary Select5F4520 writes83 at5F46AE/5F4718; earlier Tactical viewport admission writes99 at6D9111. These upstream writers are instruction evidence, not a new complete placement/selection/Display execution.',
                'Every raster row executes6D9423..6D95A1 with layer2, actual marked/opportunity/viewport/selected/owner-alliance gates,43E7B0 and complete70AA60 Power arm. caller_controls separately execute layer1/3 rejection and are not direct GPU raster inputs. Full Air/Top/late Extras, non-allied210 producer and whole rendered frame remain outside this profile.',
                'Original House constructor-store prefix4F5DCC..4F5DF6 publishes the actual House+24 interface7EA834. Its slots20/24 execute4F6A00/4F6A10 on supplied retained House53A4/53A8; getter_calls records actual return EAX in drain-then-output order. The full House constructor, dirty assessment, power recomputation, blackout and health sampling remain their established owners. Signed/zero/different-owner cache inputs are explicit boundary controls, not claims that the supplied two-building scene produced them.',
                'Physical TXT_POWER_DRAIN2 from langmd.mix/ra2md.csf supplies the literal wide format. Complete original CRT7CA564/7D21BE performs both substitutions; format_calls observes native arguments, result and UTF16. The existing infantry_deploy_action atomic-import owner supplies only Win32 InterlockedIncrement/Decrement at IAT7E11C8/CC. No host-format string, rectangle, font measure, raster or packed pixel supplies expected output. The legacy single-digit formatter path is unchanged.',
                'Original4A59E0,421B60,7BB020/7BB050,7BAD90/7BADC0,4A5EB0 and glyph434120 execute after physical GAME.FNT434AD0/433990 in the same VM. Each parent completes backing/outline/glyph before the next constructor-ordered parent. Clip, A127/63/2/0, oldZ65535/0 and positive overlapping text controls are bounded component results. Original actor/House bytes and A/Z remain unchanged; drawing has no RNG request, timer write or detach call. No whole-function, full native frame/device or arbitrary-locale/string claim is made.',
                'The historical primary_factory_status corpus remains the consumed physical GAWEAP nonpositive-Power/PRIMARY fallback owner. Its exact replay is required separately; this GAPOWR profile does not manufacture a Power factory, primary latch or extra parser control.',
            ],substitutions=[
                'Inherited allocation/delete/CRT TLS, single-thread atomic imports, Win32 SetRect and immutable physical archive/file/BSurface transports remain platform boundaries. CSF734E60 returns only the physical wide labels. Native House getters, CRT formatting, all label decisions and every reached measure/clip/fill/outline/font/glyph/pixel instruction execute unchanged.',
            ])
        result['entry_points'].update(type_ctor='0x0045DD90',type_reader='0x0045FE50',
            power_read_begin='0x00461060',power_read_stop='0x004610A0',
            house_tables_begin='0x004F5DCC',house_tables_stop='0x004F5DF6',
            power_output='0x004F6A00',power_drain='0x004F6A10',
            native_wide_format='0x007CA564')
    return result


def absorber_pips_metadata():
    return native.provenance(
        scope='Stock YAPOWR ordered INIT/YENGINEER absorber pips through original Building health,709A90 and packed PIPS stores',
        assumptions=[
            'An opt-in mode extends generate_unit_health, the existing Building/TMP runtime, _HealthRecorder, PaletteReader/native INI caches and packed Surface/shape owner in one VM. Original Rules665650/BuildingType45DD90 and full BuildingType45FE50 consume physical RULESMD, absent optional LANGRULE, Battle and XMP03T4; fixed ART is unlayered. No duplicate runtime, cargo model, reader/parser, palette or rasterizer supplies expected values.',
            'Original current-default PipScale714111..12B, PipsDrawForAll71412B..145, Passengers714B35..56, UnitAbsorb/InfantryAbsorb460988..9D6, CanBeOccupied4600CB..E5, ShowOccupantPips4600FF..119 and MaxNumberOccupants460119..133 are retained. Constructor defaults are0/false/0/false/false/false/true/0. Selected native INIT/YENGINEER/MTNK scalar blocks and Infantry Pip5240D1/OccupyPip5240EE reader4748A0 run on admitted physical sections; constructor1/7 and its table-index default trap remain the shared Infantry reader owner. A fresh original probe constructor plus8 linked scalar-reader histories test explicit, absent-section, admitted-missing, unknown scale, mixed case and empty-string retention; no scalar result is supplied.',
            'Stock physical output is YAPOWR Strength700/Foundation2x2/Height4/Passengers5/PipScalePassengers/InfantryAbsorbtrue/UnitAbsorbfalse/Gunnerfalse/PipsDrawForAllfalse/MaxNumberOccupants0/ShowOccupantPipstrue/SizeLimit15, INIT Pipwhite3 and Yuri Engineer Pipblue5. Physical PIPS/PALETTE and YGPOWR from normal ra2md/genermd winners enter the existing original palette and full type/file owner. Native Body shape bytes are checked against that loaded physical asset; archive lookup/platform file storage remain the inherited transport boundary.',
            'Full original Building43B740 constructor and44D6A0 idle initializer run with supplied placement/House linkage. Existing Infantry517A50..517AE6/Walk and Unit7353C0..7354CE/Drive prefixes create genuine typed Cargo participants; whole Foot COM construction/Unlimbo/boarding are excluded. Distinct prefix objects are used for repeated full-load INIT entries. Native4733A0 prepends and473430 removes the actual head; actor_results.cargo_types records resulting linked order through actual type queries. These are Cargo/drawing consumer controls, not native radio/movement/entry/unload/House-power/animation/death lifecycle replay.',
            'Ordinary selected controls enter full6F64A0 after the separately established selected Extras dispatch; hover/unselected/sinking execute complete6F5190. Original Building health6F659E corner projection and6F682C tail caller execute.709A90 retains PIPS/origin(input.x+6,input.y-1)/step(4,2); mobile PIPS2 is not substituted.709DCB invokes actual4598A0 UnitAbsorb OR InfantryAbsorb. The absorber walks Cargo head order with raw Infantry Pip, Unitframe4/other1, no Size fillers; false takes ordinary Unitframe5/Size fillers. Every GetPipMax716290 slot draws, including empty0.',
            'Building-only PipsDrawForAll and CanBeOccupied bypass controls use actual original readers and nonallied House/index plus ctor-zero210 priors; no nonallied mask producer is invented. Tiberium suppresses the entire Building tail. Empty authored MaxNumberOccupants2 garrison emits6,6 then independent five-slot passenger row0,0,0,0,0 restarts at the same origin; ShowOccupantPipsfalse suppresses only garrison. Both native containers are explicitly empty: custom distinct dual-list lifecycle is not represented or claimed.',
            'Original4AED70 frame origins/centering/clip/pickers/row walkers/4912B0 stores execute unchanged with flags600/THEATER_PAL/intensity1000 and no A/Z accesses. Empty A0/Z0, A63 partial caller clip and positive parent overlap are finite controls. Two first-Extras/body/late-Extras controls reuse the physical YAPOWR base body and existing TMP planes; only status-addressed pixels cross the comparison boundary. Slot animations, full Display/shroud/device scenes and arbitrary body states remain outside this component evidence.',
            'Original code/vtables and raster actor bytes/A/Z remain unchanged. Drawing/query intervals draw no RNG, write no timers and call no detach. Building and Foot prefix constructor RNG requests/passive timers are recorded separately; the reused setup does not establish one joined world RNG continuation. Existing cargo, radio, HouseTracking, power, animation, unload/placement and removal owners retain their production lifecycle authority. Whole functions/classes, custom dual-list state and malformed/overfull links remain open.',
        ],substitutions=[
            'Inherited allocation/delete/CRT TLS and immutable physical archive/file/native-cache transports replace platform services. Existing RGB565 BSurface/circular A/Z storage replaces DirectDraw. Actor health/selection/owner/country-index/placement and viewport are explicit consumer priors. No reached Cargo/query/reader/admission/projection/palette/shape/pixel result or executable instruction is replaced.',
        ],entry_points=dict(rules_ctor=0x665650,building_type_ctor=0x45DD90,
            building_type_reader=0x45FE50,building_ctor=0x43B740,building_idle=0x44D6A0,
            pips_for_all_begin=0x71412B,pips_for_all_end=0x714145,
            absorb_begin=0x460988,absorb_end=0x4609D6,is_absorb_allowed=0x4598A0,
            cargo_add=0x4733A0,cargo_remove=0x473430,cargo_first=0x473450,
            cargo_size=0x473460,pip_max=0x716290,extras=0x6F5190,
            health=0x6F64A0,building_pip_caller=0x6F682C,pips=0x709A90,
            absorber_begin=0x709DD4,absorber_end=0x709E48,
            projection=0x6D2140,shape=0x4AED70,picker=0x490B90,leaf=0x4912B0))


def spawn_pips_metadata():
    return native.provenance(
        scope='Ordinary selected or hovered CARRIER independent spawn-count row through original6B7D50 and complete Extras/PIPS2 raster',
        assumptions=[
            'One opt-in spawns mode extends generate_unit_health, _HealthRecorder, existing Unit/Ship, Aircraft/Fly and Infantry/Walk prefixes, native INI cache/readers, PaletteReader and packed Surface/SHP owners in one VM. No second runtime, manager state machine, parser, palette expansion, count formula or renderer supplies expected values. Historical modes retain their allocation/startup order and references.',
            'Original Rules665650 and UnitType7470D0/TechnoType710AF0 constructors establish CARRIER spawn pointerNULL and count/regen/reload0 at71157E..711590. Exact original read sequence714E97..714F1C reads Spawns through67BD30/Aircraft41CEF0, then SpawnRegenRate, SpawnsNumber and SpawnReloadRate through current-default signed ReadInt5276D0. Physical RULESMD, absent optional LANGRULE, Battle and XMP03T4 produce HORNET/3/600/150, Strength800, PipScaleNone and Passengers0. HORNET is registered by actual AircraftType41C8B0; selected native Strength/Ammo reads yield its consumer fields. Physical lexical cache construction replaces INI file loading, not a scalar reader.',
            'Original zero-count manager6B6C90 runs completely and records native vtable, zero node/count state, retained rates, UpdateTimer start0/duration20, SpawnTimer start0/duration0, null targets/mode0 and actual RNG requests. Count0 intentionally skips child CreateObject/Limbo; this is constructor-default evidence, not successful native stock Carrier pool construction. Original Unit7353C0..7354CE, Ship69EC50, Aircraft413D20..413DB1/Fly and Infantry517A50..517AE6/Walk prefixes execute separately with their own declared seeded constructor inputs and record RNG/passive timers. These do not certify one continuous world-construction RNG stream, whole COM constructors or Unlimbo.',
            'Each draw supplies a retained3-node manager/vector prior with nonnull typed HORNET child-prefix objects for states0/2/3/4/6, null child for7, matching owner/SpawnOwner, ready/reloading limbo and explicit node timer0/reload150/regen600. Original CountDockedSpawns6B7D50 executes as actor_results.spawn_count_docked and again from709B91 when admitted; only statuses0/6 count. Manager/child lifecycle remains tools/rocket_oracle/spawn_manager.json and the canonical sim::spawn_manager owner. No manager AI, launch, return, child expiry or detach is replayed by this drawing profile.',
            'Every raster control executes whole6F5190, actual sinking/selected/hover/observer health gates,6F64A0 and admitted709A90. Nonallied health checks read supplied HouseType index0 and ctor-zero210; this does not establish a producer for nonallied display bits. The independent709B73..709C25 row loops current typeD5C and calls6B7D50 on retained manager2D0, drawing PIPS2 frame1 or0. Its ordinary anchor is the health caller point plus(-5,0), four-pixel step, before any later PipScale row. Original projection, shape/frame origins, center flags, clipping, picker/rowwalker and pixel leaf execute.',
            'Physical PIPS2 fromra2md/conqmd and PALETTE fromra2/cache enter the existing original palette startup/ConvertN53 owner. Actual spawn4AED70 calls use flags600/remap0/zAdjust0/gradient0/brightness1000; real4912B0 bypassesA/Z. A127/63/2/0, oldZ0, partial viewport/caller clipping, ready/reloading/launch/dead counts, selected/hover/unselected/nonallied/sinking and positive parent overlap are bounded consumer controls. Empty frame0 still stores12 outline pixels per slot; frame1 stores16.',
            'Authored current_capacity_two executes original reader with SpawnsNumber2 while the explicit retained pool still has3 ready nodes: native count3 yields2 draw slots. Authored independent_passenger_row reads Passengers2/SizeLimit2/PipScalePassengers and actual Cargo4733A0 adds one E1. It emits spawn frames1,1,1 then passenger3,0, restarting at the same origin rather than advancing a concatenated slot index; native overwritten pixels are retained. Patch inputs and before/after native fields are recorded, then original readers restore the prior physical fields. These are authored configuration/retained-state consumer priors, not retail lifecycle production claims.',
            'Original code/Unit vtables, prepared actor/manager/node bytes and A/Z remain unchanged through measured query/draw intervals. Drawing makes no RNG request, timer write or detach call. Constructor RNG/timers, supplied placement/House/selection/health/current-type/node states and viewport remain separate inputs. Full Display/body, naval placement/paid exit, manager/flight/child combat/audio/removal lifecycle, arbitrary PipScales/types and whole functions/classes remain open.',
        ],substitutions=[
            'Existing allocation/delete/CRT TLS and immutable physical archive/cache transports replace platform services. Existing RGB565 BSurface/circular A/Z storage replaces DirectDraw. Spare registry/vector capacity, typed actor/House linkage and retained manager/node/child states are supplied priors. No reached count, status gate, INI scalar, projection, palette, shape or pixel return or original instruction is substituted.',
        ],entry_points=dict(rules_ctor=0x665650,unit_type_ctor=0x7470D0,
            techno_type_ctor=0x710AF0,spawn_reader_begin=0x714E97,spawn_reader_end=0x714F1C,
            aircraft_lookup_reader=0x67BD30,aircraft_find=0x41CEF0,
            manager_ctor=0x6B6C90,count_docked=0x6B7D50,
            unit_ctor_begin=0x7353C0,unit_ctor_stop=0x7354CE,ship_ctor=0x69EC50,
            aircraft_ctor_begin=0x413D20,aircraft_ctor_stop=0x413DB1,
            extras=0x6F5190,health=0x6F64A0,pips=0x709A90,
            spawn_row_begin=0x709B73,spawn_row_end=0x709C27,
            projection=0x6D2140,cargo_add=0x4733A0,
            shape=0x4AED70,picker=0x490B90,leaf=0x4912B0))


def bomb_clock_metadata():
    return native.provenance(
        scope='Ordinary IVAN attached MTNK clock through complete Extras and GAPOWR clock/wrench/health body composition',
        assumptions=[
            'One opt-in bomb mode extends generate_unit_health, _HealthRecorder, existing mobile/Infantry prefixes and _building_health_composition in the same PaletteReader/TMP VM. Original allocation, native INI cache/reader, geometry startup, Surface/circular A/Z, palette, SHP and body owners are shared. No second VM, parser, palette expansion, clock formula, bomb state machine, rasterizer or body mask supplies expected output. Historical modes retain their allocation and startup order.',
            'Whole Rules665650 yields IvanDamage100, IvanTimedDelay450 and IvanIconFlickerRate8. Original exact CombatDamage HasSection526810 and current-default signed ReadInt5276D0 blocks66C5AE..66C606/66C611..66C631 consume physical RULESMD, absent optional LANGRULE, MPBattleMD and XMP03T4. Actual66C5F4 literalBOMBCURS.SHP lookup5B40B0 publishesRulesFE0. Final physical tuning450/450/8 and MTNKStrength300 are recorded per layer. Raw IVAN/IvanBomber/IvanBomb/IvanWH lexical keys identify the route but are not a claimed full native weapon reader. Full GAPOWR45DD90/45FE50 and fixed ARTMD remain the existing composition owner.',
            'Physical BOMBCURS/WRENCH/MOUSEPAL archive candidates are recorded independently. Original Init52C136..52C1EA executes literal MOUSEPAL load, copy/expansion and complete N1 Convert48E740/48EBF0 publication87F6C8. The normal STOCK_DIGITAL AssetManager consumer must independently establish archive winner and loose precedence; native archive loading is not executed. Existing PALETTE N53 remains available for health and PIPBRD.',
            'Actual original Infantry517A50..517AE6 prefix with IVAN type5236A0 and Unit7353C0..7354CE/Drive prefix prepare stable actors; whole typed constructors, COM and Unlimbo are excluded. Original438E70 Attach creates each record from supplied bomb_start_frame and original438BF0 UpdateAll runs twice for visible, once before visibility refresh for false. Kind0, native start/end, BombVisible68, countdown45/0 and clock438A00 results are outputs. Null bomb, sinking, selection/hover, health, House/placement and supplied frame are consumer controls, not a complete bullet/command/damage/reveal lifecycle.',
            'BombClass/BombList gameplay remains independently consumed by tools/spatial_oracle/bomb_class.json and src/sim/bomb_tests.rs, including actual bullet IvanBomb/BombDisarm arms, attach gates, visibility/detectors, strict Frame>end expiry, detonation/defuse and bookkeeping. The clock fixture retains native constructor soundIDs -1, so audio is deliberately outside its reached path; independent BombClass controls retain those audio boundaries. No death-kind1 or spent-record lifecycle coverage is claimed by this ordinary timed profile.',
            'Every component row executes complete6F5190, including sinking3CD, BombVisible68/AttachedBomb38 and actual Cell487950/47B3A0/586360 gates, virtual render coordinate5F65A0, Tactical6D2140, original438A00 arithmetic and actual4AED70/picker/rowwalker/493DF0. FlagsE00/remap0/zAdjust0/gradient0/brightness1000 readA and bypassZ. Physical N1 yields the same pixels forA127/63/2/0; oldZ0 does not occlude. Actor/bomb bytes andA/Z remain unchanged, with no drawing RNG, timer write or detach.',
            'Global tactical clip886FA0 differs from supplied parent bounds in global_clip; nativeclock still draws through the global clip while subsequent status families use their own bounds. Cell bit08-only is admitted. Existing initialize_discovery_geometry executes original CRT startup before native Cell construction; raised level2 yields groundZ208. Elevated_projected_open admits projected cell9,19 despite origin10,20 hidden, while elevated_origin_open rejects origin-only reveal. Terrain flags/levels and placement are declared consumer priors, not a native reveal lifecycle or whole world frame.',
            'The two-clock overlap records positive native overwritten pixels with each parent clock preceding its own health. Two GAPOWR controls reuse actual flatCells/TMP, full Building43B740/idle44D6A0, BUILDNGZ45E8F0 and43CEA0->455C20/459EF0->43D290->705E00 body/shadow. Whole Behind, complete first Extras, actual admitted body and complete late Extras execute in supplied parent order. Clock precedes repairing wrench and health inside both visits. Sparse ground/before-late packed colors/Z and final status-addressed pixels are consumed, with actual positive body/status overwrite witnesses. Full Display traversal, GPU body parity, arbitrary assets/types and whole functions/classes remain open.',
            'Attach setup writes original kind0/start/end and list visibility countdown1; UpdateAll decrements to0 then writes45 and carrier68/redraw80. Native actor constructor RNG/passive timers are recorded separately from drawing. Original code and Unit vtables remain unchanged. The measured draw/query intervals make no RNG request, timer write or detach call; no gameplay update order or simulation authority is replaced.',
        ],substitutions=[
            'Existing successful allocation/delete/CRT TLS and immutable physical file/archive transports replace platform services. RGB565 BSurface/circular A/Z memory replaces DirectDraw. Cached lexical INI indexes, retained actor/Houses and supplied traversal/terrain are explicit inputs. No reached Attach, visibility, clock, Cell, palette, projection, shape, body or pixel function result or original instruction is substituted.',
        ],entry_points=dict(rules_ctor=0x665650,combat_reader=0x66BBB0,
            damage_begin=0x66C5AE,delay_begin=0x66C5CE,flicker_begin=0x66C611,
            bomb_shape_load=0x66C5F4,unit_ctor_begin=0x7353C0,unit_ctor_stop=0x7354CE,
            infantry_ctor_begin=0x517A50,infantry_ctor_stop=0x517AE6,
            attach=0x438E70,update_all=0x438BF0,clock_frame=0x438A00,
            extras=0x6F5190,clock_end=0x6F5253,cell_shrouded=0x487950,
            cell_ground=0x47B3A0,coord_shrouded=0x586360,
            unit_render_coords=0x5F65A0,building_render_coords=0x459EF0,
            projection=0x6D2140,mouse_palette_begin=0x52C136,mouse_palette_end=0x52C1EA,
            convert_ctor=0x48E740,shape=0x4AED70,picker=0x490B90,leaf=0x493DF0,
            first_extras_caller=0x6D9153,late_extras_caller=0x6D9789,
            behind=0x6F60D0,body_admission=0x43CEA0,body=0x43D290))


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
        scope='Stock FV passenger status and bounded fractional LCRF Size controls through whole Extras and PIPS2 consumers',
        assumptions=[
            'The existing selected-health generator and _HealthRecorder have one additive passenger mode. Existing Building-health PaletteReader, native allocator/cache/physical transports and shape Surface/A/Z owners are reused in the same VM; no copied VM, INI/parser, cargo/manager/storage owner, palette, SHP decoder or rasterizer generates outputs.',
            'The existing Building type/runtime has an optional NABNKR health-only mode: complete BuildingType45DD90/45FE50 consumes physical RULESMD, absent LANGRULE, MPBattleMD and XMP03T4, with fixed ART. Actual Building43B740/44D6A0, GetCoords447AC0 and Dimension2464AF0 supply the affected garrison health consumer. Its body/ground drawing is disabled; no NABNKR body, placement or garrison-entry lifecycle is claimed.',
            'Complete UnitType7470D0/InfantryType5236A0 constructors and unchanged selected Strength/PipScale/delta/Passengers/Gunner/Storage/Size/SizeLimit/Pip/OccupyPip native read/store blocks consume the same selected physical layer stack. Original MIND Primary7129AB..712A1D resolves its physical weapon; Damage7722D4..7722EE consumes that weapon section. Full Unit/Infantry type file readers and unrelated keys do not run.',
            'ReadPip4748A0 and both actual Infantry call/store blocks execute on explicit strings/current constructor defaults. Original Abstract410A60 admission distinguishes absent sections from admitted unrelated-key passes. Missing defaults use native table indexes, including adjacent Ammo/Tiberium names at11/12. Explicit empty cache values are separate from physical empty INI omission. These controls expose the required existing retained-type pass-owner behavior, not a second INI implementation.',
            'Existing Unit7353C0->Foot4D31E0 prefix stops7354CE before typed COM activation, followed by real Drive4AF540. Infantry517A50 prefix stops517AE6 before ID/COM, followed by real Walk75AA90. Supplied interface owner/link/active priors expose their original virtuals. Original constructor Limbo1 permits whole Cargo4733A0->51DF10->4DB260->6F6AC0 and473430 without gameplay sinks. Whole typed construction, Unlimbo, boarding and unloading missions remain separate production tests. Selected mobile prefix RNG requests/passive timers are recorded separately from drawing; inherited Building/type/palette startup is outside that recorded initialization interval.',
            'Every mobile row executes complete6F5190, original rank/observer gates,6F64A0 and actual709A90/70AA60 tails. Native716290/473460/473450 and passenger array fill choose physical PIPS2 frames, centered origins and flags600. Garrison rows enter full6F64A0 directly with explicit prepared occupant vector/count, then actual709C27..709D17 uses raw OccupyPip11 or reader-evolved1 and emptyframe6. These direct-health rows do not establish whole Building Extras admission.',
            'One affected shared frame-consumer row uses stock LCRF Passengers12/SizeLimit6/Size16 and MTNK Size3 through the same constructors/read blocks. LCRF consumes real Hover513C20 with supplied interface linkage, matching physical Locomotor742; MTNK uses the existing Unit/Drive prefix with native Limbo1. Whole Cargo4733A0 reaches Unit4DB260/Techno6F6AC0 and retains one size3 passenger. Original709F0A..709F23 fills higher slots with frame3 before placing Unitframe5 in the lower slot, producing5,3,3 then nine empty0 frames. This is a render-consumer/whole-Cargo dependency control, not LCRF boarding, movement, placement or whole construction evidence.',
            'Two named fractional rows use one LCRF carrying two distinct prepared MTNK objects. Each row patches the current MTNK TechnoType+0x380 qword after the selected native stock type readers, using explicit binary64 Size bits3FF8000000000000 or4006666666666666; the patch is restored after that row. These are controlled consumer inputs, not retail or parser outputs. Whole CargoTotalSize473460, FTOL7C5F00, and the actual709EF0 passenger filler execute; PIPS2 frames and sparse pixels are recorded.',
            'Before current type reader blocks and PIPS consumers, the original WinMain controlfp/cache fragment6BBFB7..6BBFCE runs from initial word027F. Live and cached FPCW are both checked as0E3F; existing PaletteReader/Building fixture initialization before this bounded fragment remains a supplied setup boundary.',
            'MIND uses the original4717D0 manager constructor with retained count/vector and overload-counter priors, not captures/Update; original4722D0/4726C0 and708C30 execute. HARV receives explicit four native float storage slots; original6C9650/6C9680 and independent ore/gem rounding execute. Stock positive Storage40 and size-one infantry match the existing gameplay state owners. Fractional size/reload behavior, manager lifecycle, harvest lifecycle and other PipScales remain open.',
            'PIPS/PIPBRD/PALETTE retain established normal STOCK_DIGITAL winners. Original5F76B0 pushes literal PIPS2.SHP at5F7758 through CCFile65CA80/loader4A3890, retainsAC1480 at5F7776 and709AEF consumes that same asset. Independent normal STOCK_DIGITAL AssetManager lookup selects ra2md.mix/conqmd.mix PIPS2,1384bytes/19frames, no loose override or catalog-only hit; ra2.mix/conquer.mix1280bytes/18frames is shadowed. The oracle pins the selected physical bytes while archive/cache transport remains supplied. Native palette/Convert constructors,4AED70/pickers/rowwalkers/leaves execute unchanged. A127/63/2/0, oldZ0, clip, sinking and ordered overlapping parents are render-consumer controls. Sparse pixels/draws and counts are native stores; no VERA scalar or host palette/raster/count formula supplies expected output.',
            'Original code/vtables, prepared actor bytes and A/Z remain unchanged across measured draws; no drawing RNG, timer write or detach occurs. The component does not run mobile bodies, whole Display/Scenario/Map, selection generation, damage/promotion or a whole native frame.',
        ],substitutions=[
            'Inherited bounded allocation/delete/CRT TLS and immutable physical archive/file transports; BSurface/circular A/Z memory replaces DirectDraw. Constructor/interface/cargo/count/storage/occupant scene priors are explicit inputs. No reached render/predicate/Drive/Walk/cargo/palette/shape function result or native instruction is replaced.',
        ],entry_points=dict(unit_type_ctor=0x7470D0,infantry_type_ctor=0x5236A0,
            building_type_ctor=0x45DD90,building_type_reader=0x45FE50,
            infantry_ctor_begin=0x517A50,infantry_ctor_stop=0x517AE6,walk_ctor=0x75AA90,
            unit_ctor_begin=0x7353C0,unit_ctor_stop=0x7354CE,drive_ctor=0x4AF540,
            hover_ctor=0x513C20,winmain_fpcw=0x6BBFB7,
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


def self_heal_status_metadata():
    from tools import selfheal_oracle as selfheal
    return native.provenance(
        scope='Damaged MTNK House self-heal status601 and affected Infantry/Organic Unit phases, original pixels and four current-default General readers',
        assumptions=[
            'Additive mode of the existing selected-health/group-status generator. One PaletteReader owns original image, allocator, native INI cache, Rules/UnitType/InfantryType constructors, palette startup and Surface/circular A/Z. Existing mobile constructor-prefix, House query/step identities and font owners are reused; no VM/parser/palette/raster/numeric implementation is copied. Historical control_group_status payload and its scope remain separate.',
            'Physical RULESMD from expandmd01, optional loose LANGRULE, MPBattleMD and XMP03T4 feed the existing native lexical/cache transport and original selected type/General/AudioVisual read-store blocks. Original665650 defaults the four General fields1000/1/1000/1. Original66E6E2..66E74B reads exact keys in InfantryFrames/Amount, UnitFrames/Amount order with current-field defaults; no clamp/postpass is present in that block. Reader controls supply explicit cached empty strings because physical lexical transport drops empty entries. Original66D530 admission prefix executes before each retained-history read.',
            'Stock MTNK Strength300/nonorganic and E1 Strength125/Organic are native constructor/physical-reader outputs. Explicit Organic=yes uses its original reader/store block and is restored through the same reader. Supplied actor health, House counts, signed global frame, selection/hover, position, camera and legal Surface clip are render-consumer priors. They do not prove natural damage, capture/House-grant/revoke, selection, collision-valid dense placement or full simulation lifecycles.',
            'Whole6F5190->6F64A0->709A90/70AA60 executes. Admitted damaged signed Frame remainder<=5 chooses one601 draw; other phases and the actual native299->300 heal transition choose600. Native4AED70 centering/clipping, plain/RLE pickers, row walkers and492670/496250 destination-half leaves execute unchanged. Every retained marker pixel is native output, with actual dispatch/mask and destination reads observed. Original PIPS21frame/PIPBRD/PALETTE and GAME.FNT physical winners are unchanged.',
            'Two prepared same-position parents prove positive non-idempotent composition; each original health->marker->group sequence executes separately. An ordered neighboring group control also retains final native pixels. A0/Z0 and partial clipping are component controls. Existing all-word native_packed half goldens own the numeric kernel; this corpus adds no independently calculated half-word golden.',
            'Bounded original House-heal AI6FA793..6FA941 starts after the separate alive/per-object heal arm and stops before cloak update. Actual Has/Step bodies execute; the supplied attached-smoke pointer is null. Zero period and INT_MIN/-1 reach native x86 IDIV interrupt0 even at count0 in AI, while whole drawing queries the House first and skips division at count0. Trap controls restore only their prior emulator CPU context after recording the terminal exception, preventing an inherited pending exception from manufacturing a later double fault; no gameplay state or arithmetic is supplied. Ordinary phase/timer/lifecycle facts may rest on original instructions and production behavior tests.',
            'Original code/vtables, draw actor bytes and A/Z remain unchanged. Measured draw/House-heal arms request no RNG, write no timers and call no detach; the native full-heal control changes Health only. Constructor-prefix Scenario RNG/passive timer initialization is retained separately. The Unit smoke F8 call site is observed but no nonnull smoke lifecycle, whole Techno/House/DrawExtras function, GPU frame or full native game is claimed.',
        ],substitutions=[
            'Inherited allocator/delete/CRT TLS and immutable physical file/archive transports plus BSurface/circular A/Z memory replace platform services. Existing font owner supplies file reads, bounded CRT digit formatting and Win32 SetRect. No reached gameplay predicate, division, reader result, palette conversion, dispatch, shape/glyph decoder or pixel leaf is replaced.',
        ],entry_points=dict(rules_ctor=0x665650,general_admission=0x66D530,
            general_read_begin=0x66E6E2,general_read_end=0x66E74B,read_int=0x5276D0,
            ai_begin=0x6FA793,ai_end=0x6FA941,ai_unit_divide=0x6FA7EE,
            ai_infantry_divide=0x6FA8E2,has_infantry=selfheal.HAS_INF,
            has_unit=selfheal.HAS_UNIT,extras=0x6F5190,health=0x6F64A0,
            pip_tail=0x709A90,draw_unit_divide=0x70A5B5,draw_infantry_divide=0x70A549,
            self_heal_shape=0x70A6BB,shape=0x4AED70,plain_half_leaf=0x492670,
            rle_half_leaf=0x496250))


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


def _radial_type_fields(m, typ):
    u = m.u
    return dict(image=m.string(typ+0x1F8), foundation=m.read32(typ+0xEF0),
        has_radial_indicator=bool(u.mem_read(typ+0x238,1)[0]),
        concentric=bool(u.mem_read(typ+0x1708,1)[0]),
        psychic_radius=struct.unpack('<i',u.mem_read(typ+0x170C,4))[0],
        gap_generator=bool(u.mem_read(typ+0xCD1,1)[0]),
        gap_radius=struct.unpack('<b',u.mem_read(typ+0xCD2,1))[0],
        super_gap_radius=struct.unpack('<b',u.mem_read(typ+0xCD3,1))[0],
        cloak_generator=bool(u.mem_read(typ+0x16C7,1)[0]),
        sensor_array=bool(u.mem_read(typ+0x16C8,1)[0]),
        cloak_radius=struct.unpack('<b',u.mem_read(typ+0x1707,1))[0])


def _radial_type_inputs():
    """One existing native constructor/cache owner, selected real read stores.

    The whole Building->Techno->Object reader prefix establishes the supplied
    file/receiver at HasRadialIndicator. Later scalar and ART ranges are
    explicit cuts; no complete type/Scenario/resource-loading claim follows.
    """
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.spatial_oracle.engineer_bridge_cursor_caller import HutTypeReader, READER_INI
    from tools.spatial_oracle.building_body_rules import SP as reader_sp

    m = HutTypeReader()
    u = m.u
    types = {name:m.construct(name) for name in
             ('GAGAP','NAPSIS','RadialProbe','RadialOne','NoRadialProbe','SensorProbe')}
    constructors = {name:_radial_type_fields(m,p) for name,p in types.items()}
    reads = []
    keys = {'HasRadialIndicator','GapGenerator','GapRadiusInCells',
            'SuperGapRadiusInCells','CloakGenerator','SensorArray',
            'CloakRadiusInCells','PsychicDetectionRadius','ConcentricRadialIndicator',
            'Foundation','Image'}

    def observe(_u,pc,_size,_data):
        if pc not in (0x5295F0,0x5276D0,0x474DA0,0x528A10):
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        section, key, default = [m.read32(sp+n) for n in (4,8,12)]
        key = m.string(key)
        if key in keys:
            reads.append(dict(reader=f'{pc:08X}',caller=f'{m.read32(sp):08X}',
                ini='fixed_art' if u.reg_read(UC_X86_REG_ECX)==READER_INI else 'rules_pass',
                section=m.string(section),key=key,
                default=m.string(default) if pc==0x528A10 else
                        default&255 if pc==0x5295F0 else
                        struct.unpack('<i',words(default))[0]))

    hook = u.hook_add(UC_HOOK_CODE,observe)
    # Keep ART's actual singleton distinct from each supplied rules-pass INI.
    rules_ini = m.alloc(0x40)
    layers = []
    for filename,archive,raw in physical_layers():
        if raw is None:
            layers.append(dict(file=filename,archive=archive,absent=True))
            continue
        sections,_ = lexical(raw,{'GAGAP','NAPSIS'})
        m.make_ini(sections)
        u.mem_write(rules_ini,bytes(u.mem_read(READER_INI,0x40)))
        u.mem_write(READER_INI,bytes(0x40))
        rows = []
        for name in ('GAGAP','NAPSIS'):
            typ = types[name]
            mark = len(reads)
            u.mem_write(reader_sp,words(native.RET_MAGIC,rules_ini))
            u.reg_write(UC_X86_REG_ESP,reader_sp)
            u.reg_write(UC_X86_REG_ECX,typ)
            end = native.run_checked(u,0x45FE50,(0x5F954F,native.RET_MAGIC),
                                     count=2000000)
            admitted = end==0x5F954F
            if admitted:
                registers = {UC_X86_REG_ESI:rules_ini,UC_X86_REG_ECX:rules_ini,
                             UC_X86_REG_EBP:typ,UC_X86_REG_EBX:typ+0x24}
                _status_reader_block(m,0x713F92,0x713FE2,registers)
                _status_reader_block(m,0x460BDD,0x460C4C,
                    registers|{UC_X86_REG_EAX:m.read32(typ+0x16F4)})
                _status_reader_block(m,0x461030,0x46104A,registers)
            rows.append(dict(type=name,admitted=admitted,
                has_radial_prefix_endpoint=f'{end:08X}',reads=reads[mark:],
                after=_radial_type_fields(m,typ)))
        layers.append(dict(file=filename,archive=archive,bytes=len(raw),sha256=sha(raw),
                           sections={s:{k:v for k,v in d.items() if k in keys}
                                     for s,d in sections.items()},types=rows))

    yr = stock.mix((native.configured_gamemd().parent/'ra2md.mix').read_bytes())
    raw = stock.mix(yr[stock.mix_hash('localmd.mix')])[stock.mix_hash('artmd.ini')]
    names = {'GAGAP','NAPSIS'}|{m.string(types[n]+0x1F8) for n in ('GAGAP','NAPSIS')}
    art,_ = lexical(raw,names)
    m.make_ini(art)
    art_rows = []
    for name in ('GAGAP','NAPSIS'):
        typ = types[name]
        mark = len(reads)
        _status_reader_block(m,0x461225,0x46125D,
            {UC_X86_REG_EBP:typ,UC_X86_REG_EBX:typ+0x24,
             UC_X86_REG_EDI:typ+0x1F8})
        art_rows.append(dict(type=name,reads=reads[mark:],after=_radial_type_fields(m,typ)))
    fixed_art = dict(file='ARTMD.INI',archive='ra2md.mix/localmd.mix',bytes=len(raw),
                     sha256=sha(raw),types=art_rows)

    history = []
    probes = [('RadialProbe',values) for values in
                  ({'HasRadialIndicator':'yes','GapGenerator':'yes','GapRadiusInCells':'1'},
                   {'PsychicDetectionRadius':'3'}, {},
                   {'HasRadialIndicator':'junk','GapRadiusInCells':'128'},
                   {'hasradialindicator':'no','gapradiusincells':'9'},
                   {'HasRadialIndicator':'no','GapRadiusInCells':'-1'},
                   {'HasRadialIndicator':'yes','PsychicDetectionRadius':'0',
                    'GapRadiusInCells':'3'})]
    probes += [('RadialOne',{'HasRadialIndicator':'yes','GapGenerator':'yes',
                             'GapRadiusInCells':'1'}),
               ('NoRadialProbe',{'GapGenerator':'yes','GapRadiusInCells':'3'})]
    probes += [('SensorProbe',{'HasRadialIndicator':'yes','SensorArray':'yes'})]
    range_actor = m.alloc(0x800)
    u.mem_write(range_actor,words(0x7E3EBC))
    for name,values in probes:
        typ = types[name]
        sections = {name:values} if values else {}
        m.make_ini(sections)
        u.mem_write(rules_ini,bytes(u.mem_read(READER_INI,0x40)))
        u.mem_write(READER_INI,bytes(0x40))
        mark = len(reads)
        u.mem_write(reader_sp,words(native.RET_MAGIC,rules_ini))
        u.reg_write(UC_X86_REG_ESP,reader_sp);u.reg_write(UC_X86_REG_ECX,typ)
        end = native.run_checked(u,0x45FE50,(0x5F954F,native.RET_MAGIC),count=2000000)
        if end==0x5F954F:
            registers = {UC_X86_REG_ESI:rules_ini,UC_X86_REG_ECX:rules_ini,
                         UC_X86_REG_EBP:typ,UC_X86_REG_EBX:typ+0x24}
            _status_reader_block(m,0x713F92,0x713FE2,registers)
            _status_reader_block(m,0x460BDD,0x460C4C,
                registers|{UC_X86_REG_EAX:m.read32(typ+0x16F4)})
            _status_reader_block(m,0x461030,0x46104A,registers)
        u.mem_write(range_actor+0x520,words(typ))
        radii = []
        for selector in (0,1):
            u.mem_write(range_actor+0x268,bytes([selector]))
            radii.append(struct.unpack('<i',words(m.invoke(0x4566B0,range_actor)))[0])
        history.append(dict(type=name,sections=sections,admitted=end==0x5F954F,
                            reads=reads[mark:],after=_radial_type_fields(m,typ),
                            range_by_268=radii))
    u.hook_del(hook)
    m.unchanged()
    return m,types,dict(constructors=constructors,layers=layers,fixed_art=fixed_art,
                        retained_history=history,
                        final={n:_radial_type_fields(m,p) for n,p in types.items()})


class _RadialIndicator:
    """Existing native reader VM plus shared BSurface/circular-plane transport.

    Only the memory-surface +3C dispatch selects DSurface's original body;
    the established LaserPixels transport supplies the same platform seam.
    """
    def __init__(self,m,types):
        from tools.projectile_oracle import bridge_render_shape as surface
        self.m,self.u,self.types,self.surface = m,m.u,types,surface
        self.size = (160,160)
        self.u.mem_map(MEM,0x200000)
        self.vtable = m.alloc(0x100)
        self.u.mem_write(self.vtable,bytes(self.u.mem_read(0x7E2070,0x100)))
        self.u.mem_write(self.vtable+0x3C,words(0x4BC750))
        self.tactical,self.house,self.other,self.selected = (m.alloc(n) for n in (0xE20,0x6000,0x6000,32))
        self.actors = [m.alloc(0x800) for _ in range(2)]
        self.coords,self.point = m.alloc(12),m.alloc(8)
        self.clock = m.alloc(16)
        self.u.mem_write(0x7E1530,words(self.clock))
        self.u.mem_write(0x887324,words(self.tactical))
        self.u.mem_write(0xB0CD48,struct.pack('<Q',0x3FC25E5374344960))
        self.u.mem_write(0xB0CE30,words(*self.size))
        for pointer,value in ((0x8A0DD0,11),(0x8A0DD4,3),(0x8A0DD8,0),
                              (0x8A0DDC,3),(0x8A0DE0,5),(0x8A0DE4,2)):
            self.u.mem_write(pointer,words(value))
        self.active = False
        self.u.hook_add(UC_HOOK_CODE,self.observe)
        self.u.hook_add(UC_HOOK_MEM_WRITE,self.store)
        self.u.hook_add(UC_HOOK_MEM_READ,self.read_plane)

    def observe(self,u,pc,_size,_data):
        if not self.active:
            return
        m,sp = self.m,u.reg_read(UC_X86_REG_ESP)
        if pc==self.clock:
            self.os_calls.append(dict(kind='timeGetTime',caller=f'{m.read32(sp):08X}',
                milliseconds=self.clock_ms))
            m.ret(self.clock_ms)
            return
        if pc==0x7C978A:
            self.os_calls.append(dict(kind='CRT_atexit',callback=f'{m.read32(sp+4):08X}'))
            m.ret(0)
            return
        if self.pending and pc==self.pending['return_pc'] and sp==self.pending['after_sp']:
            command = self.pending['command']
            command['store_count'] = self.pending['stores']
            command['touched_pixels'] = len(self.pending['points'])
            self.pending = None
        if pc in (0x65C780,0x65C7E0,0x70D4A0):
            self.side_effects.append(f'{pc:08X}')
        if pc==0x456750:
            self.actor_order.append(self.actors.index(u.reg_read(UC_X86_REG_ECX)))
        if pc==0x45675C:
            self.ranges.append(struct.unpack('<i',words(u.reg_read(UC_X86_REG_EAX)))[0])
        if pc==0x456980:
            args = rally.ints(u,sp+4,7)
            self.radials.append(dict(draw_line=bool(u.reg_read(UC_X86_REG_ECX)&255),
                adjust_color=bool(u.reg_read(UC_X86_REG_EDX)&255),coord=args[:3],
                rgb=list(u.mem_read(sp+16,3)),radius_f32_bits=f'{args[4]&0xFFFFFFFF:08X}',
                pixel_radius=bool(args[5]),truncate_radius=bool(args[6])))
        if pc in (0x7BB350,0x4BC750):
            assert self.pending is None
            if pc==0x7BB350:
                args = rally.ints(u,sp+4,9)
                command = dict(kind='ellipse',center=args[:2],radius=args[2:4],
                               clip=args[4:8],color=args[8]&0xFFFF)
                cleanup = 36
            else:
                args = rally.ints(u,sp+4,11)
                command = dict(kind='ray',clip=rally.ints(u,args[0],4),
                    start=rally.ints(u,args[1],2),end=rally.ints(u,args[2],2),
                    rgb=list(u.mem_read(args[3],3)),z=args[4:6],write_z=bool(args[6]),
                    channels=[bool(n) for n in args[7:10]],strength_f32_bits=f'{args[10]&0xFFFFFFFF:08X}')
                cleanup = 44
            command['actor'] = self.actor_order[-1] if self.actor_order else None
            self.commands.append(command)
            self.pending = dict(command=command,return_pc=m.read32(sp),after_sp=sp+4+cleanup,
                                points=set(),stores=0)

    def read_plane(self,u,_access,address,size,_value,_data):
        if not self.active:
            return
        for name,base in (('z',self.surface.Z),('a',self.surface.A)):
            if base<=address<base+51200:
                self.plane_reads[name].append((address-base)//2)

    def store(self,u,_access,address,size,value,_data):
        if not self.active or not self.surface.PIXELS<=address<self.surface.PIXELS+51200:
            return
        for offset in range(0,size,2):
            index=(address+offset-self.surface.PIXELS)//2
            if index in self.points:
                before = struct.unpack('<H',u.mem_read(address+offset,2))[0]
                after = (value>>(offset*8))&0xFFFF
                if before!=after:
                    self.overwritten.add(index)
                    if len(self.overwrite_witnesses)<8:
                        self.overwrite_witnesses.append(dict(point=[index%160,index//160],
                            before=before,after=after,command=len(self.commands)-1))
                    actor = self.pending['command']['actor'] if self.pending else None
                    previous = self.point_actor.get(index)
                    if actor is not None and previous is not None and actor!=previous:
                        self.cross_actor_overwritten.add(index)
                        if len(self.cross_actor_witnesses)<8:
                            self.cross_actor_witnesses.append(dict(point=[index%160,index//160],
                                before=before,after=after,actor=actor,previous_actor=previous))
            self.points.add(index)
            self.point_actor[index] = self.pending['command']['actor'] if self.pending else None
            self.store_order.append(index)
            if self.pending:
                self.pending['points'].add(index)
        if self.pending:
            self.pending['stores'] += 1

    def reset(self,case):
        self.active = False
        u,s = self.u,self.surface
        prior = dict(depth_baseline=32768,old_z=case.get('old_z',65535),
                     a_value=case.get('a_value',127),background=case.get('background',0x39E7),
                     a_patches=case.get('a_patches',[]),z_patches=case.get('z_patches',[]))
        self.priors = prior
        self.before = s.prepare_surface(u,prior,size=self.size,initialize_buffer_height=True)
        self.guard = b'\xA5'*32
        u.mem_write(s.PIXELS-32,self.guard);u.mem_write(s.PIXELS+51200,self.guard)
        u.mem_write(s.SURFACE,words(self.vtable))
        u.mem_write(0x886FA0,words(*case.get('clip',[0,0,*self.size])))
        u.mem_write(self.tactical+0xB0,words(*case.get('camera',[-80,300])))
        u.mem_write(0xA8ED84,words(case.get('frame',0)))
        u.mem_write(0xA8B238,words(1));u.mem_write(0xA83D4C,words(self.house))
        u.mem_write(0xA8EB7E,b'\0')
        self.commands,self.radials,self.actor_order,self.ranges = [],[],[],[]
        self.points,self.store_order,self.side_effects = set(),[],[]
        self.overwritten,self.overwrite_witnesses = set(),[]
        self.point_actor,self.cross_actor_overwritten,self.cross_actor_witnesses = {},set(),[]
        self.plane_reads = dict(z=[],a=[])
        self.pending = None
        self.clock_ms = case.get('time_get_time_ms',0)
        self.os_calls = []
        self.rng = [bytes(u.mem_read(p,0x3F4)) for p in (0x886B88,0xABE890)]
        self.fp_before = u.reg_read(UC_X86_REG_FPCW)

    def result(self,case):
        self.active = False
        # Unicorn's terminal address can end before invoking code hooks there.
        # Check the actual native RET/stack outcome for a direct leaf control.
        if self.pending:
            assert self.u.reg_read(UC_X86_REG_EIP)==self.pending['return_pc']
            assert self.u.reg_read(UC_X86_REG_ESP)==self.pending['after_sp']
            self.pending['command']['store_count'] = self.pending['stores']
            self.pending['command']['touched_pixels'] = len(self.pending['points'])
            self.pending = None
        assert self.pending is None
        u,s = self.u,self.surface
        raw = bytes(u.mem_read(s.PIXELS,51200))
        assert bytes(u.mem_read(s.Z,51200))==self.before['depths']
        assert bytes(u.mem_read(s.A,51200))==self.before['lighting']
        assert bytes(u.mem_read(s.PIXELS-32,32))==self.guard
        assert bytes(u.mem_read(s.PIXELS+51200,32))==self.guard
        assert self.rng==[bytes(u.mem_read(p,0x3F4)) for p in (0x886B88,0xABE890)]
        assert not self.side_effects
        self.m.unchanged()
        return dict(input=case,planes=self.priors,commands=self.commands,
            radials=self.radials,actor_order=self.actor_order,ranges=self.ranges,
            pixels=[[i%160,i//160,struct.unpack_from('<H',raw,i*2)[0]] for i in sorted(self.points)],
            first_stores=self.store_order[:8],color_sha256=sha(raw),
            overwritten_pixel_count=len(self.overwritten),overwrite_witnesses=self.overwrite_witnesses,
            cross_actor_overwritten_pixel_count=len(self.cross_actor_overwritten),
            cross_actor_overwrite_witnesses=self.cross_actor_witnesses,
            fp_control_before=f'{self.fp_before:04X}',fp_control_after=f'{u.reg_read(UC_X86_REG_FPCW):04X}',
            plane_reads={name:dict(count=len(indices),first_points=[[i%160,i//160] for i in indices[:8]])
                         for name,indices in self.plane_reads.items()},
            a_unchanged=True,z_unchanged=True,rng_unchanged=True,guard_unchanged=True,
            rng_detach_entries=self.side_effects,original_code_unchanged=True)

    def scene(self,case):
        case = dict(camera=[-80,300],clip=[0,0,*self.size],frame=0,time_get_time_ms=0)|case
        supplied = case.get('actors',[{}])
        case['actors'] = [dict(type='GAGAP',location=[3200,3200,0],online=True,
                               other_owner=False,super_gap=False,rgb=[240,248,64])|actor
                          for actor in supplied]
        case.setdefault('selected',list(range(len(supplied))))
        self.reset(case)
        u,m = self.u,self.m
        supplied = case['actors']
        geometries = []
        for index,actor in enumerate(supplied):
            p,typ = self.actors[index],self.types[actor.get('type','GAGAP')]
            u.mem_write(p,words(0x7E3EBC));u.mem_write(p+0x520,words(typ))
            u.mem_write(p+0x9C,words(*actor.get('location',[3200,3200,0])))
            u.mem_write(p+0x21C,words(self.other if actor.get('other_owner') else self.house))
            u.mem_write(p+0x660,bytes([actor.get('online',True)]))
            u.mem_write(p+0x268,bytes([actor.get('super_gap',False)]))
            u.mem_write(self.house+0x56F9,bytes(actor.get('rgb',[240,248,64])))
            u.mem_write(self.other+0x56F9,bytes([32,180,240]))
            m.invoke(0x447AC0,p,(self.coords,))
            coord = rally.ints(u,self.coords,3)
            m.invoke(0x6D2140,self.tactical,(self.coords,self.point))
            geometries.append(dict(actor=index,type=actor.get('type','GAGAP'),
                                   coords=coord,projected=rally.ints(u,self.point,2),
                                   radius=m.invoke(0x4566B0,p)))
        selected = case.get('selected',list(range(len(supplied))))
        u.mem_write(self.selected,words(*(self.actors[i] for i in selected)))
        u.mem_write(0xA8ECBC,words(self.selected));u.mem_write(0xA8ECC8,words(len(selected)))
        self.active = True
        actor_bytes = [bytes(u.mem_read(p,0x800)) for p in self.actors[:len(supplied)]]
        m.invoke(0x6DBE20,self.tactical,instruction_limit=12000000)
        result = self.result(case)
        assert actor_bytes==[bytes(u.mem_read(p,0x800)) for p in self.actors[:len(supplied)]]
        result['actor_bytes_unchanged'] = True
        result['geometry'] = geometries
        result['os_calls'] = self.os_calls
        return result

    def ray(self,case):
        case = dict(clip=[0,0,*self.size],rgb=[240,248,64],z=[-500,-500],
                    strength_f32_bits='3F800000')|case
        self.reset(case)
        m,u = self.m,self.u
        clip,start,end,color = (m.alloc(n) for n in (16,8,8,4))
        u.mem_write(clip,words(*case.get('clip',[0,0,*self.size])))
        u.mem_write(start,words(*case['start']));u.mem_write(end,words(*case['end']))
        u.mem_write(color,bytes(case.get('rgb',[240,248,64]))+b'\0')
        self.active = True
        m.invoke(0x4BC750,self.surface.SURFACE,
            (clip,start,end,color,*case.get('z',[-500,-500]),0,1,1,1,
             int(case.get('strength_f32_bits','3F800000'),16)),instruction_limit=12000000)
        result = self.result(case)
        return result


def generate_radial_indicator():
    m,types,readers = _radial_type_inputs()
    u = m.u
    from tools.spatial_oracle.building_body_rules import SP as reader_sp
    fp_before = u.reg_read(UC_X86_REG_FPCW)
    guard_before = u.mem_read(0x8A0DF1,1)[0]
    assert guard_before==0
    u.reg_write(UC_X86_REG_ESP,reader_sp);u.reg_write(UC_X86_REG_ECX,0)
    native.run_checked(u,0x4BC750,0x4BC7C2,count=3000000,
        required_addresses=(0x4BC780,0x7C8FB0,0x7C5F00,0x4BC7AC))
    table = bytes(u.mem_read(0x8A0980,257*4))
    lut = dict(entry='004BC750',end_exclusive='004BC7C2',
        fp_control_before=f'{fp_before:04X}',initialization_guard_before=guard_before,
        fp_control=f'{u.reg_read(UC_X86_REG_FPCW):04X}',
        constants_f64_bits={f'{p:08X}':f'{struct.unpack("<Q",u.mem_read(p,8))[0]:016X}'
                            for p in (0x7E8670,0x7E1718,0x7E3D90,0x7E2808)},
        values=list(struct.unpack('<257i',table)),sha256=sha(table))
    assert lut['sha256']=='c5e88231f605a69d7a266d34cd198ab12a85a2b648a740470f36b35d11b2709c'
    tangent_raw = native.file_span(native.image_bytes(),0x85D0A4,4096*4)[1]
    tangent_controls = []
    for angle in (-20.,-6.3,-3.2,-1.6,-0.1,-0.001,0.,0.001,0.1,1.6,3.2,6.3,20.):
        bits = struct.pack('<d',angle)
        output = native.call(0x4CAD50,stack_args=list(struct.unpack('<2I',bits)),
            capture_st0=True,fpcw=native.NATIVE_FPCW,
            required_addresses=(0x4CAD54,0x4CAD5F,0x4CAD74))
        tangent_controls.append(dict(angle_f64_bits=f'{struct.unpack("<Q",bits)[0]:016X}',
            table_index=output['eax'],returned_f64_bits=f'{output["st0_bits"]:016X}'))
    tangent = dict(address='0085D0A4',u32_bits=list(struct.unpack('<4096I',tangent_raw)),
        sha256=sha(tangent_raw),entry='004CAD50',
        fp_control=f'{native.NATIVE_FPCW:04X}',controls=tangent_controls)
    draw = _RadialIndicator(m,types)
    scenes = [dict(name='stock_center_frame0'),dict(name='stock_center_frame628',frame=628),
        dict(name='stock_center_frame314',frame=314),
        dict(name='stock_left_arc',camera=[360,300]),
        dict(name='stock_top_arc',camera=[-80,520]),
        dict(name='stock_clip_origin',camera=[360,300],clip=[12,9,120,135]),
        dict(name='stock_a0',a_value=0),dict(name='stock_a63',a_value=63),
        dict(name='stock_z0',old_z=0),dict(name='empty',actors=[],selected=[]),
        dict(name='stock_arc_a0_z0',camera=[360,300],a_value=0,old_z=0),
        dict(name='deselected',selected=[]),
        dict(name='other_owner',actors=[dict(other_owner=True)]),
        dict(name='offline',actors=[dict(online=False)]),
        dict(name='probe_radius3',actors=[dict(type='RadialProbe')]),
        dict(name='probe_radius1',actors=[dict(type='RadialOne')]),
        dict(name='no_indicator',actors=[dict(type='NoRadialProbe')]),
        dict(name='stock_overlap',actors=[{},dict(location=[3264,3136,0])]),
        dict(name='stock_overlap_reverse',actors=[{},dict(location=[3264,3136,0])],selected=[1,0])]
    scene_rows = [draw.scene(case) for case in scenes]
    assert scene_rows[-2]['color_sha256']!=scene_rows[-1]['color_sha256']
    assert scene_rows[-2]['cross_actor_overwritten_pixel_count']>0
    for name,milliseconds,camera in (
        ('napsis_phase0_outer',0,[600,300]),
        ('napsis_phase51_threshold',102,[-80,300]),
        ('napsis_phase52_threshold',104,[-80,300]),
        ('napsis_phase256',512,[100,300]),
        ('napsis_phase512',1024,[260,300]),
        ('napsis_phase768',1536,[420,300])):
        scene_rows.append(draw.scene(dict(name=name,actors=[dict(type='NAPSIS')],
            time_get_time_ms=milliseconds,camera=camera)))
    rays = []
    for name,start,end in (
        ('horizontal',[20,20],[50,20]),('vertical',[20,20],[20,50]),
        ('x_positive',[20,20],[50,28]),('x_negative',[20,40],[50,28]),
        ('y_positive',[20,20],[28,50]),('y_negative',[20,50],[28,20]),
        ('reversed',[50,28],[20,20]),('clip_left',[-20,20],[50,28]),
        ('same_point',[20,20],[20,20])):
        rays.append(draw.ray(dict(name=name,start=start,end=end)))
    for strength in ('3D4CCCCD','3E4CCCCD','3ECCCCCD'):
        rays.append(draw.ray(dict(name='strength_'+strength,start=[20,20],end=[50,28],
                                  strength_f32_bits=strength)))
    baseline = draw.ray(dict(name='pair_baseline',start=[20,20],end=[21,20]))
    rays.append(baseline)
    primary,secondary = baseline['first_stores'][:2]
    rays.append(draw.ray(dict(name='pair_secondary_blocked',start=[20,20],end=[21,20],
        a_patches=[[secondary,0]],z_patches=[[secondary,0]],derived_from='pair_baseline.first_stores')))
    rays.append(draw.ray(dict(name='pair_primary_blocked',start=[20,20],end=[21,20],
        z_patches=[[primary,0]],derived_from='pair_baseline.first_stores')))
    assert secondary in rays[-2]['first_stores'] and not rays[-1]['pixels']
    transport = dict(size=list(draw.size),bytes_per_pixel=2,pitch_bytes=320,
        surface_u32_by_offset={'04':160,'08':160,'0C':0,'10':2,'18':51200,'1C':0},
        surface_backing_pointer_offset='14',
        vtable=dict(memory_surface='007E2070',cubic_slot='3C',cubic_body='004BC750'),
        rgb565_globals={'008A0DD0':11,'008A0DD4':3,'008A0DD8':0,
                       '008A0DDC':3,'008A0DE0':5,'008A0DE4':2},
        circular_planes=dict(scroll_bytes=0,row_width=160,height=160),
        projection_factor_f64_bits='3FC25E5374344960',session_mode=1,placement_preview=False,
        selected_ray=dict(z=[-500,-500],write_z=False,channels=[True,True,True]))
    return dict(schema_version=1,size=list(draw.size),
        transport=transport,
        reader_inputs=readers,cubic_lut=lut,tangent=tangent,
        scenes=scene_rows,ray_controls=rays)


def radial_indicator_metadata():
    return native.provenance(
        scope='Selected stock GAGAP radial indicator, bounded NAPSIS concentric drawing and shared DSurface cubic-line controls',
        assumptions=[
            'Original BuildingType45DD90 constructors and Building45FE50->Techno712170->Object5F92D0 prefix execute to5F954F, after current-default HasRadialIndicator read/store. Later named original scalar/read-store cuts and fixed ART Foundation execute. Physical archive/INI lexical cache transport is supplied; whole type/Scenario/resource loading is excluded.',
            'Actual selected-list6DBE20 traverses prepared CurrentObjects and real Building+130=456750;4566B0, House50B6F0, Building447AC0 and Tactical6D2140 execute. Location, online660, owner/current-house, alternate268, camera, frame, House56F9 RGB and selected vector are declared consumer priors, not their gameplay lifecycle.',
            'Existing bridge_render_shape.prepare_surface owns160x160 RGB565 BSurface/circular A/Z storage. A copied original memory-surface vtable changes only+3C to original DSurface4BC750, matching the established LaserPixels platform-lock transport. Original ellipse7BB350 and complete4BC750 execute, including original257-entry cubic initialization under FPCW0E7F; BSurface no-op7BAB70 supplies no ray evidence.',
            'Sparse final pixels include every actually stored address, including overwritten/background-valued stores. Geometry/commands, clipping, primary-gated paired writes, A/Z invariance and Main/Mapgen RNG snapshots are original outputs on supplied planes. Reached RNG65C780/65C7E0 and detach70D4A0 are observed; drawing actor bytes stay identical, with no timer write/call in the selected bodies. Constructor timers are not a lifecycle claim. Component crops are not whole native frames or production GPU comparisons; no positive overlap claim is inferred without an actual store witness.',
            'Original tangent4096 float bytes at85D0A4 are retained in full. Thirteen signed input-bit controls execute4CAD50/MathFTOL and the real index/lookup. Existing native.call captures ST0 with its declared six-byte FSTP64 return observation outside original memory; it does not replace the lookup or supply a tangent result.',
            'Every direct ray uses the selected caller constant Z=-500/-500, all three RGB channels and no Z write. Octant/clipping/strength/paired-gate controls do not prove varying-Z, channel-disabled, WriteZ or explicit-color Laser callers. NAPSIS scenes execute actual456750 concentric color/radius/order at declared timeGetTime milliseconds0/102/104/512/1024/1536, cropping outer/expanding contours. This supplies bounded shared-consumer coverage, without world lifecycle or native wall-clock scheduling. Placement remains separate.',
            'Techno+268 defaults false at6F2DA2; original Mission_Unload44E29D..44E36B toggles it with gap removal/recreation, operational state and House dirty flag, and operational-off454B91..454BEB clears it. That gap/action lifecycle is not executed here. GAGAP physical normal/super radii10/10 make the selected radius invariant; controls declare the retained selector instead of inventing its lifecycle.',
        ],substitutions=[
            'Existing HutTypeReader supplies allocator/delete/CRT TLS and native INI cache objects. Shared BSurface virtual memory locks/pitch/unlock replace DirectDraw OS storage; original drawing bodies, type predicates, projection, table lookups, conversions and pixel stores are unchanged.',
            'Only imported timeGetTime IAT7E1530 returns each declared u32 millisecond input through the existing Reader return transport. Original456750 consumes it. CRT atexit7C978A returns0 for original white-color lazy initialization process-exit registration; no color or branch result is supplied.',
        ],entry_points=dict(type_ctor=0x45DD90,type_prefix=0x45FE50,has_radial=0x5F9521,
            selected_caller=0x6DBE20,building_ring=0x456750,range=0x4566B0,
            ring=0x456980,coords=0x447AC0,projection=0x6D2140,
            ellipse=0x7BB350,cubic_line=0x4BC750,cubic_end=0x4BC7C2,tangent=0x4CAD50))


def _superweapon_control_machine():
    u = native.Uc(native.UC_ARCH_X86, native.UC_MODE_32)
    native.load_image(u)
    u.mem_map(native.STACK_BASE, native.STACK_SIZE)
    u.mem_map(native.RET_MAGIC, 0x1000)
    return u


def _superweapon_winmain_fpcw(u, sp):
    # Supply the Windows startup x87 state, then execute the real WinMain
    # controlfp/cache fragment instead of assuming native.NATIVE_FPCW.
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_FPCW, 0x027F)
    native.run_checked(u, 0x006BBFB7, 0x006BBFCE, count=1000,
        required_addresses=(0x007CBF49, 0x007C5EE4),
        context={'case':'WinMain x87 control/cache initialization'})
    live = u.reg_read(UC_X86_REG_FPCW)
    cached = struct.unpack('<H', u.mem_read(0x00822D80, 2))[0]
    if (live, cached) != (0x0E3F, 0x0E3F):
        raise native.OracleError(
            f'WinMain FPCW mismatch: live={live:04X}, cached={cached:04X}')
    return live


def _superweapon_loop_control(name, range_bits, line_multiplier, *, draws=1):
    u = _superweapon_control_machine()
    fake = 0x50000000
    type_ptr, player_ptr = fake + 0x1000, fake + 0x2000
    house_array, house_ptr = fake + 0x3000, fake + 0x4000
    u.mem_map(fake, 0x10000)
    u.mem_write(type_ptr + 0xF8, struct.pack('<I', range_bits))
    u.mem_write(type_ptr + 0xFC, struct.pack('<i', line_multiplier))
    # ABI-shaped CurrentPlayer/House inputs. The native loop and its shared
    # RGBClass adjustment still execute; only radial geometry is intercepted.
    u.mem_write(0x00A83D4C, struct.pack('<I', player_ptr))
    u.mem_write(player_ptr + 0x30, struct.pack('<i', 0))
    u.mem_write(0x00A8022C, struct.pack('<I', house_array))
    u.mem_write(house_array, struct.pack('<I', house_ptr))
    house_rgb = [33, 67, 101]
    u.mem_write(house_ptr + 0x56F9, bytes(house_rgb) + b'\x00')

    sp = native.STACK_BASE + native.STACK_SIZE - 0x4000
    fpcw = _superweapon_winmain_fpcw(u, sp)
    # B0E64C is zero-initialized image BSS; 842954 is the native data word +1.
    phase_before = struct.unpack('<i', u.mem_read(0x00B0E64C, 4))[0]
    direction_before = struct.unpack('<i', u.mem_read(0x00842954, 4))[0]
    if phase_before != 0 or direction_before != 1:
        raise native.OracleError(
            f'{name}: native pulse startup changed ({phase_before}, {direction_before})')
    u.mem_write(0x00B0CD70, b'\x00\x00\x00')

    events = {'color_calls': [], 'radial_calls': []}

    def capture_color_input(machine, _address, _size, _data):
        call_sp = machine.reg_read(UC_X86_REG_ESP)
        source = machine.reg_read(UC_X86_REG_ECX)
        amount = struct.unpack('<I', machine.mem_read(call_sp + 4, 4))[0]
        target = struct.unpack('<I', machine.mem_read(call_sp + 8, 4))[0]
        events['color_calls'].append(dict(
            factor=amount,
            source_rgb=list(machine.mem_read(source, 3)),
            target_rgb=list(machine.mem_read(target, 3))))

    def capture_radial_args(machine, _address, _size, _data):
        call_sp = machine.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack('<I', machine.mem_read(call_sp, 4))[0]
        radius_bits = struct.unpack('<I', machine.mem_read(call_sp + 0x14, 4))[0]
        # The helper reads separate 32-bit stack arguments: pixel radius at
        # entry ESP+0x18 (EBP+0x1C), truncate radius at ESP+0x1C (EBP+0x20).
        flags = [
            struct.unpack('<I', machine.mem_read(call_sp + 0x18, 4))[0],
            struct.unpack('<I', machine.mem_read(call_sp + 0x1C, 4))[0],
        ]
        events['radial_calls'].append(dict(
            return_address=f'0x{ret:08X}',
            radius_f32_bits=f'{radius_bits:08X}',
            rgb_after_caller_adjust=list(machine.mem_read(call_sp + 0x10, 3)),
            pixel_radius_and_truncate_flags=flags,
            draw_rays=machine.reg_read(UC_X86_REG_ECX) & 0xFF,
            adjust_color=machine.reg_read(UC_X86_REG_EDX) & 0xFF))
        # Retain caller registers; perform only DrawRadialIndicator's cdecl
        # stack cleanup at this declared external geometry boundary.
        machine.reg_write(UC_X86_REG_ESP, call_sp + 4 + 0x1C)
        machine.reg_write(UC_X86_REG_EIP, ret)

    u.hook_add(UC_HOOK_CODE, capture_color_input,
               begin=0x006612C0, end=0x006612C0)
    u.hook_add(UC_HOOK_CODE, capture_radial_args,
               begin=0x00456980, end=0x00456980)

    def run_draw(index):
        u.mem_write(type_ptr + 0xF8, struct.pack('<I', range_bits))
        u.mem_write(type_ptr + 0xFC, struct.pack('<i', line_multiplier))
        u.mem_write(sp + 0x18, b'\x00\x00\x00\x00')
        u.mem_write(sp + 0x20, struct.pack('<I', range_bits))
        u.mem_write(sp + 0x30, struct.pack('<i', 0x12345))
        u.mem_write(sp + 0x34, struct.pack('<i', -0x23456))
        u.mem_write(sp + 0x44, struct.pack('<i', 0x34567))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_EDI, type_ptr)
        u.reg_write(UC_X86_REG_EBP, 1)
        u.reg_write(UC_X86_REG_ESI, 0)
        u.reg_write(UC_X86_REG_EBX, 0x34567)
        events['color_calls'] = []
        events['radial_calls'] = []
        phase_start = struct.unpack('<i', u.mem_read(0x00B0E64C, 4))[0]
        direction_start = struct.unpack('<i', u.mem_read(0x00842954, 4))[0]
        end = 0x006DC3A3 if line_multiplier >= 1 else 0x006DC3F3
        required = ((0x006DC379, 0x006612C0, 0x00456980)
                    if line_multiplier >= 1 else (0x006612C0, 0x00456980))
        stop = native.run_checked(u, 0x006DC25A, end,
            required_addresses=required,
            context={'case':name,'draw':index + 1,
                     'range_f32_bits':f'{range_bits:08X}',
                     'line_multiplier':line_multiplier,'fpcw':f'{fpcw:04X}'})
        calls = events['radial_calls']
        colors = events['color_calls']
        expected_return = 0x006DC37E if line_multiplier >= 1 else 0x006DC3F3
        if not calls or any(call['return_address'] != f'0x{expected_return:08X}'
                            for call in calls):
            raise native.OracleError(f'{name}: unexpected radial call return address')
        expected_flags = [0, 0] if line_multiplier >= 1 else [0, 1]
        if any((call['draw_rays'], call['adjust_color']) != (0, 1)
               or call['pixel_radius_and_truncate_flags'] != expected_flags
               for call in calls):
            raise native.OracleError(f'{name}: native radial call ABI changed')
        if len(colors) != len(calls) or any(
                color['source_rgb'] != house_rgb or color['target_rgb'] != [0, 0, 0]
                for color in colors):
            raise native.OracleError(f'{name}: caller color adjustment sequence changed')
        phase_end = struct.unpack('<i', u.mem_read(0x00B0E64C, 4))[0]
        direction_end = struct.unpack('<i', u.mem_read(0x00842954, 4))[0]
        return dict(name=f'{name}_draw_{index + 1}',
            phase_direction_before=[phase_start, direction_start],
            phase_direction_after=[phase_end, direction_end],
            caller_factors=[call['factor'] for call in colors],
            caller_rgb_after_adjust=[call['rgb_after_caller_adjust'] for call in calls],
            radial_calls=calls, stop_address=f'0x{stop:08X}')

    pulse_draws = [run_draw(index) for index in range(draws)]
    first_radii = [call['radius_f32_bits'] for call in pulse_draws[0]['radial_calls']]
    return dict(name=name, range_f32_bits=f'{range_bits:08X}',
                line_multiplier=line_multiplier, radii_f32_bits=first_radii,
                pulse_draws=pulse_draws, house_rgb=house_rgb, fpcw=f'{fpcw:04X}')


def _superweapon_inactive_controls():
    controls = []
    for name, selected_index, range_bits in (
            ('no_selected_superweapon', -1, 0x40E00000),
            ('zero_range_selected_superweapon', 0, 0x00000000)):
        u = _superweapon_control_machine()
        fake = 0x50000000
        type_ptr, types = fake + 0x1000, fake + 0x3000
        u.mem_map(fake, 0x10000)
        sp = native.STACK_BASE + native.STACK_SIZE - 0x1000
        fpcw = _superweapon_winmain_fpcw(u, sp)
        u.mem_write(0x00842954, struct.pack('<i', -1))
        u.mem_write(0x00B0E64C, struct.pack('<i', 37))
        u.mem_write(0x00A8EB7E, b'\x01')
        u.mem_write(0x008809A0, struct.pack('<i', selected_index))
        u.mem_write(0x00A8E334, struct.pack('<I', types))
        u.mem_write(types, struct.pack('<I', type_ptr))
        u.mem_write(type_ptr + 0xF8, struct.pack('<I', range_bits))
        before = [struct.unpack('<i', u.mem_read(0x00B0E64C, 4))[0],
                  struct.unpack('<i', u.mem_read(0x00842954, 4))[0]]
        required = ((0x006DC1AF, 0x006DC1B4) if selected_index < 0 else
                    (0x006DC1AF, 0x006DC1C6, 0x006DC1D8))
        stop = native.run_checked(u, 0x006DC1AF, 0x006DC3F3,
            required_addresses=required,
            context={'case':name,'fpcw':f'{fpcw:04X}'})
        after = [struct.unpack('<i', u.mem_read(0x00B0E64C, 4))[0],
                 struct.unpack('<i', u.mem_read(0x00842954, 4))[0]]
        if before != after:
            raise native.OracleError(f'{name}: inactive superweapon changed pulse state')
        controls.append(dict(name=name, range_f32_bits=f'{range_bits:08X}',
            phase_direction_before=before, phase_direction_after=after,
            radial_calls=0, stop_address=f'0x{stop:08X}'))
    return controls


def _superweapon_radius_control(name, radius_bits, truncate=False):
    u = _superweapon_control_machine()
    sp = native.STACK_BASE + native.STACK_SIZE - 0x1000
    _superweapon_winmain_fpcw(u, sp)
    u.mem_write(sp, struct.pack('<I', native.RET_MAGIC))
    u.mem_write(sp + 4, struct.pack('<iii', 0x12345, -0x23456, 0x34567))
    u.mem_write(sp + 16, bytes((0x21, 0x43, 0x65, 0)))
    u.mem_write(sp + 20, struct.pack('<I', radius_bits))
    u.mem_write(sp + 24, struct.pack('<I', 0))
    u.mem_write(sp + 28, struct.pack('<I', int(truncate)))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, 0)
    u.reg_write(UC_X86_REG_EDX, 1)
    native.run_checked(u, 0x00456980, 0x004569E3,
        required_addresses=(0x0045698D, 0x004569B9, 0x004569DC),
        context={'case':name,'radius_f32_bits':f'{radius_bits:08X}',
                 'truncate_input':truncate,'fpcw':'0E3F'})
    return dict(name=name, input_radius_f32_bits=f'{radius_bits:08X}',
                truncate_input=truncate,
                output_radius_pixels=u.reg_read(UC_X86_REG_ESI))


def generate_superweapon_range_controls():
    loop_controls = [
        _superweapon_loop_control('nuke_special_retail_7_2', 0x40E00000, 2, draws=2),
        _superweapon_loop_control('iron_curtain_special_retail_1_4_3', 0x3FB33333, 3),
        _superweapon_loop_control('synthetic_2_8_5', 0x40333333, 5),
    ]
    expected = [(15, '40E00000'), (5, '3FAAAAAA'), (14, '40266666')]
    for row, (count, last) in zip(loop_controls, expected):
        radii = row['radii_f32_bits']
        if len(radii) != count or radii[-1] != last:
            raise native.OracleError(
                f"{row['name']}: native ring controls changed ({len(radii)}, {radii[-1]})")
    radius_controls = [
        _superweapon_radius_control('positive_lm_zero_iteration_radius', 0x00000000),
        _superweapon_radius_control('retail_i4_over_lm3', 0x3FAAAAAA),
        _superweapon_radius_control('synthetic_fractional_2_6', 0x40266666),
        _superweapon_radius_control('retail_lm_below_one_single_ring', 0x3FB33333,
                                    truncate=True),
    ]
    expected_pixels = [21, 77, 131, 63]
    if [row['output_radius_pixels'] for row in radius_controls] != expected_pixels:
        raise native.OracleError('Native radial helper numeric controls changed')
    fallback = _superweapon_loop_control(
        'retail_lm_zero_single_ring', 0x3FB33333, 0)['pulse_draws'][0]
    return dict(fpcw='0E3F',
                loop_controls=loop_controls,
                pulse_controls=dict(house_rgb=[33, 67, 101],
                    lm_below_one=fallback,
                    inactive=_superweapon_inactive_controls()),
                primitive_radius_controls=radius_controls)


def finish_superweapon_range_controls(argv=None):
    import argparse
    parser = argparse.ArgumentParser(description='Execute native superweapon ring controls')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--check', action='store_true', help='compare without writing (default)')
    mode.add_argument('--write', action='store_true', help='update only the superweapon corpus section')
    args = parser.parse_args(argv)
    source_paths = {'oracle':Path(__file__), 'shared_runner':Path(native.__file__)}
    sources = native.oracle_sources.source_identity(source_paths)
    generated = generate_superweapon_range_controls()
    payload = json.dumps(generated, sort_keys=True, separators=(',', ':'),
                         allow_nan=False).encode('utf-8')
    metadata = native.provenance(
        scope='Original Tactical superweapon targeting pulse, ring loop and shared radial radius prefix controls',
        assumptions=[
            'Original WinMain controlfp/cache fragment6BBFB7..6BBFCE executes from Windows initial x87 control word027F and reaches7CBF49/7C5EE4; live FPCW and cached822D80 both become0E3F.',
            'Loop controls enter the native post-admission pulse block6DC25A after Tactical__DrawUnitActionVisuals has selected an armed type with Range>0. The native caller pulse, positive-LM/local per-ring pulse and shared RGB adjustment6612C0 all execute. CurrentPlayer/House/type memory is ABI-shaped test input; DrawRadialIndicator456980 is intercepted only at the external geometry boundary to record its original arguments and preserve caller registers while applying its stack cleanup.',
            'The shared radial helper reads its pixel-radius and truncate-radius flags as separate DWORD arguments at EBP+1C and EBP+20. The positive-LM caller pushes 0/0; its LM<1 fallback pushes DWORD1 then DWORD0, yielding pixel=0/truncate=1. The capture reads the two independent stack words.',
            'NukeSpecial controls run two consecutive native pulse blocks in one Unicorn machine so persistent B0E64C/842954 state is observed across draws. Inactive controls execute the selected-index=-1 and Range=0 branches at6DC1AF and verify that those gates leave pulse state unchanged.',
            'Helper controls execute original DrawRadialIndicator456980 through sqrt4CAC40 and ftol7C5F00, stopping before world projection/drawing at4569E3. This compares only the numeric prefix, not pixels.',
            'NukeSpecial7/2 and IronCurtainSpecial1.4/3 are authored Steam RulesMD values; 2.8/5 is a synthetic near-integer product control, not a retail type.',
        ],
        substitutions=[
            'The current-player pointer, house array/current House RGB, selected type and fixed cell locals are supplied as explicit consumer inputs. The final ellipse/raster body is not executed by loop controls; only its radius/color arguments and ordering are observed. Helper controls stop before projection.',
        ],
        entry_points=dict(winmain_control=0x6BBFB7, pulse=0x6DC25A,
            superweapon_loop=0x6DC2F5, superweapon_loop_stop=0x6DC3A3,
            fallback_ring=0x6DC3AB, fallback_stop=0x6DC3F3,
            inactive_gate=0x6DC1AF, caller_rgb_adjust=0x6612C0,
            radial_helper=0x456980,
            radial_helper_numeric_stop=0x4569E3, sqrt=0x4CAC40, ftol=0x7C5F00))
    metadata.update(
        fpcw_initial='0x027F', fpcw_live_and_cached='0x0E3F',
        fpcw_cache_address='0x00822D80',
        required_winmain_callees=['0x007CBF49', '0x007C5EE4'],
        replay_command='python -m tools.procedural_drawing_oracle.shroud --superweapon-range --check',
        generation_command='python -m tools.procedural_drawing_oracle.shroud --superweapon-range --write',
        required_environment='VERA20K_GAMEMD_EXE must identify a supported original image; --check requires the recorded SHA256')
    metadata['payload_sha256'] = hashlib.sha256(payload).hexdigest()
    target = Path(__file__).with_name('radial_indicator.json')
    sidecar = Path(__file__).with_name('superweapon_range.meta.json')
    if difference := native.first_difference(sources,
            native.oracle_sources.source_identity(source_paths)):
        raise native.OracleError(f'Source changed during native generation: {difference}')
    if args.write:
        corpus = json.loads(target.read_text(encoding='utf-8'))
        corpus['superweapon_range'] = generated
        target.write_text(json.dumps(corpus, indent=2, allow_nan=False) + '\n',
                          encoding='utf-8')
        sidecar.write_text(json.dumps(metadata, indent=2, allow_nan=False) + '\n',
                           encoding='utf-8')
        print(f'WROTE {target} and {sidecar}; review the native controls before accepting them')
        return
    if not target.is_file() or not sidecar.is_file():
        raise native.OracleError('Superweapon corpus or sidecar is missing; use --write deliberately')
    corpus = json.loads(target.read_text(encoding='utf-8'))
    if difference := native.first_difference(corpus.get('superweapon_range'), generated):
        raise native.OracleMismatch(f'Superweapon corpus mismatch: {difference}')
    expected_metadata = json.loads(sidecar.read_text(encoding='utf-8'))
    if difference := native.first_difference(expected_metadata, metadata):
        raise native.OracleMismatch(f'Superweapon provenance mismatch: {difference}')
    print(f'PASS {target}: native superweapon controls match; no files written')


def generate_infantry_berserk_body():
    """Eight body draws and 28 joined Object/viewport draws in one owner.

    The shared owner supplies only immutable file/cache and platform storage.
    Actor/House/Cell admission is a declared prior, not a simulated gas hit.
    """
    from types import SimpleNamespace
    from tools.rules_oracle.weapon_speed_order import RetailRulesInputs
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.input_oracle.campaign_start import CampaignReader
    from tools.projectile_oracle.bridge_render_inputs_palette import (
        PaletteReader, initialize, color_scheme_palette)
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A
    from tools.spatial_oracle.building_body_rules import INI, RULES, dwords
    from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry
    from tools.spatial_oracle.infantry_deploy_action import Fixture as AtomicTransportOwner
    from tools.spatial_oracle.tmp_clear_render import (
        physical_assets, load_tmp_files, theater_tile_files)
    from tools.native_inspect import selected_ranges

    catalog = RetailRulesInputs(native.configured_gamemd().parent)
    raw_rules = physical_layers()[0][2]
    rules = sections(raw_rules)
    raw_art = catalog.lookup('ARTMD.INI')
    art = sections(raw_art)
    assets = {name:catalog.lookup(name) for name in
              ('GI.SHP', 'UNITTEM.PAL', 'PALETTE.PAL', 'ANIM.PAL')}
    assets.update({'RULESMD.INI':raw_rules, 'ARTMD.INI':raw_art})
    assert all(assets.values())
    selected_art = {name:art[name] for name in ('GI', 'GISequence')}
    m = PaletteReader(selected_art, root=Path('/nonexistent'))
    m.assets.update(assets)
    m.original_code = selected_ranges(native.image_bytes(), None, None, code_only=True)
    u = m.u

    # Reuse the established Windows atomic ABI seam; no native code is patched.
    atomic = SimpleNamespace(u=u, imports=[m.alloc(16), m.alloc(16)], read=m.read32)
    u.mem_write(0x7E11C8, dwords(*atomic.imports))
    def atomic_transport(_u, pc, size, data):
        if pc in atomic.imports:
            AtomicTransportOwner.observe(atomic, _u, pc, size, data)
    atomic_hook = u.hook_add(UC_HOOK_CODE, atomic_transport)
    art_cache = bytes(u.mem_read(INI, 0x40))
    u.mem_write(0x8205D0, dwords(2))
    u.mem_write(0x829D20, dwords(2))
    u.mem_write(0x84E860, bytes((1, 0, 1)))
    initialize(m)
    _storage, scheme = color_scheme_palette('DarkRed', owner=m, physical_assets=assets)

    rules_object = m.alloc(0x2000)
    u.mem_write(0x8871E0, dwords(rules_object))
    m.invoke(0x665650, rules_object)
    selected_rules = {'ColorAdd':rules['ColorAdd'],
        'AudioVisual':{key:rules['AudioVisual'][key] for key in
                       ('BerserkColor', 'ExtraInfantryLight')}}
    CampaignReader.ordered_ini(m, selected_rules)
    m.invoke(0x66D480, rules_object, (INI,))
    m.invoke(0x6691E0, rules_object, (INI,))
    native_rules = dict(berserk_color=m.read32(rules_object+0x18AC),
                       extra_infantry_light=m.read32(rules_object+0x17D8))
    native_rules['colour_add_rgb'] = list(u.mem_read(
        rules_object+0x1874+3*native_rules['berserk_color'], 3))
    u.mem_write(INI, art_cache)
    typ = m.alloc(0x1900)
    m.invoke(0x5236A0, typ, (m.cstring('E1'),))
    selected_rules['E1'] = {key:rules['E1'][key] for key in ('Image', 'Strength', 'Armor')}
    m.rules_cache({'E1':selected_rules['E1']})
    admitted = bool(m.invoke(0x5240A0, typ, (RULES,)) & 255)
    shp = m.read32(typ+0xA4)
    native_type = dict(name='E1', image=m.string(typ+0x1F8),
                       strength=m.read32(typ+0xA0), admitted=admitted)
    assert admitted and shp and native_type['image'] == 'GI'

    streams = dict(main=0x886B88, scenario=m.read32(0xA8B230)+0x218, mapgen=0xABE890)
    for pointer in streams.values():
        m.invoke(0x65C6D0, pointer, (31,))
    u.mem_write(0xB0F720, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    house = m.alloc(0x18000)
    passengers = _prepare_status_passengers(m, {'E1':typ}, house)
    actor = passengers['E1']['actor']
    u.mem_write(actor+0x21C, dwords(house))
    u.mem_write(house+0x16054, dwords(scheme['color_index']))
    u.mem_write(0xA83D4C, dwords(house))
    u.mem_write(actor+0x41A, b'\x01')
    u.mem_write(actor+0x9C, dwords(2688, 5248, 0))
    u.mem_write(actor+0x81, b'\x00')
    u.mem_write(actor+0x90, b'\x01')
    u.mem_write(actor+0x6C, dwords(125))
    u.mem_write(actor+0x6C4, dwords(0))
    u.mem_write(actor+0xF8, dwords(0))

    # GetZAdjust704350 needs the real physical TMP registry and geometry.
    tile_assets, _tile_sources = physical_assets(tile=True, tmp_names=('clear01.tem',))
    m.assets.update(tile_assets)
    m.phase = 'selected_flat_TMP'
    _tiles, tile_mapping = theater_tile_files(m, tile_assets['TEMPERATMD.INI'], 0, 'tem')
    _heads, _io = load_tmp_files(m, {'clear01.tem':tile_assets['CLEAR01.TEM']})
    _table, cells = _prepare_flat_cells(m, range(9, 12), range(19, 22))
    for cell in cells:
        u.mem_write(cell+0x10A, struct.pack('<h', 1000))
    initialize_discovery_geometry(m.invoke, m.read32, u)
    tactical = m.alloc(0x1000)
    u.mem_write(0x887324, dwords(tactical))
    u.mem_write(0x886FA0, dwords(0, 0, 64, 96))
    u.mem_write(0xB0CE30, dwords(64, 96))
    point, clip, facing = m.alloc(8), m.alloc(16), m.alloc(4)
    u.mem_write(point, dwords(32, 48))
    u.mem_write(clip, dwords(0, 0, 64, 96))
    u.mem_write(facing, dwords(0))
    m.invoke(0x4C9300, actor+0x388, (facing,))
    u.mem_map(0x21000000, 0x100000)

    draws, reached, rng_requests = [], set(), []
    viewport_admission = None
    expected_entries = {0x518F90, 0x518D80, 0x41C090, 0x705E00,
                        0x705D70, 0x704350, 0x4AED70, 0x487950}
    def observe(_u, pc, _size, _data):
        if pc in expected_entries:
            reached.add(pc)
        sp = u.reg_read(UC_X86_REG_ESP)
        if viewport_admission is not None:
            if pc in (0x5F4B10, 0x41BE00, 0x5F65A0, 0x6D2140, 0x518F90):
                viewport_admission['events'].append(f'{pc:08X}')
            if pc == 0x6D2140:
                # Original RET8 ABI: XYZ pointer, then output Point2D pointer.
                viewport_admission['_projection_output'] = m.read32(sp+8)
            elif pc == 0x5F4B6F:
                # Observe after the actual CALL6D2140, before TEST AL,AL.
                viewport_admission['projection_admitted'] = bool(u.reg_read(UC_X86_REG_EAX)&255)
                viewport_admission['point'] = list(struct.unpack('<2i',
                    u.mem_read(viewport_admission['_projection_output'], 8)))
            elif pc == 0x518F90:
                viewport_admission['body_entry_count'] += 1
                viewport_admission['body_point'] = list(struct.unpack('<2i',
                    u.mem_read(m.read32(sp+4), 8)))
        if pc == 0x4AED70:
            args = list(struct.unpack('<14i', u.mem_read(sp+4, 56)))
            draws.append(dict(return_pc=m.read32(sp), frame=args[1],
                point=list(struct.unpack('<2i', u.mem_read(args[2], 8))),
                clip=list(struct.unpack('<4i', u.mem_read(args[3], 16))),
                flags=args[4], z_adjust=args[6], gradient=args[7],
                intensity=args[8], colour_word=args[9]&65535))
        elif pc in (0x490B90, 0x490E50):
            draws[-1]['picker'] = f'{pc:08X}'
        elif pc in (0x494B60, 0x494C40, 0x497FD0, 0x498150):
            draw = draws[-1]
            if 'leaf' not in draw:
                draw['leaf'] = f'{pc:08X}'
                draw['first_candidate'] = m.read32(sp+(20 if pc in (0x497FD0, 0x498150) else 16))
            assert draw['leaf'] == f'{pc:08X}'
        elif pc in (0x70645D, 0x706532, 0x706602) and draws:
            if draws[-1]['return_pc'] == pc:
                draws[-1]['rgb565_le_hex'] = bytes(u.mem_read(PIXELS, 64*96*2)).hex()
        elif pc in (0x65C780, 0x65C7E0):
            rng_requests.append(f'{pc:08X}')

    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        facing_table_bytes = native.file_span(native.image_bytes(),0x7EAEFC,128)[1]
        lut = list(struct.unpack('<32I',facing_table_bytes))
        rows = []
        for frame in (0, 1):
            angle = lut.index(frame)*2048
            for label, active, shrouded, old_z in (
                ('ordinary', False, False, 2000), ('berserk', True, False, 2000),
                ('shrouded_berserk', True, True, 2000), ('blocked_berserk', True, False, 0)):
                buffers = prepare_surface(u, dict(old_z=old_z, a_value=127, background=0x39E7))
                u.mem_write(facing, dwords(angle))
                m.invoke(0x4C9300, actor+0x388, (facing,))
                u.mem_write(actor+0x298, bytes((active,)))
                for cell in cells:
                    u.mem_write(cell+0x12C, dwords(0 if shrouded else 0x18))
                before = {name:bytes(u.mem_read(pointer, 0x3F4)) for name, pointer in streams.items()}
                reached.clear()
                draws.clear()
                rng_requests.clear()
                m.invoke(0x518F90, actor, (point, clip),
                         timeout_us=120000000, instruction_limit=10000000)
                assert expected_entries <= reached and len(draws) == 2
                body, shadow = (dict(draw) for draw in draws)
                assert body.pop('return_pc') == 0x70645D and body['frame'] == frame
                assert shadow.pop('return_pc') == 0x706602
                final = shadow.pop('rgb565_le_hex')
                invariants = dict(
                    source_unchanged=bytes(u.mem_read(shp, len(assets['GI.SHP']))) == assets['GI.SHP'],
                    a_unchanged=bytes(u.mem_read(A, len(buffers['lighting']))) == buffers['lighting'],
                    z_unchanged=bytes(u.mem_read(Z, len(buffers['depths']))) == buffers['depths'],
                    rng_unchanged={name:bytes(u.mem_read(pointer, 0x3F4)) == before[name]
                                   for name, pointer in streams.items()},
                    rng_requests=rng_requests.copy())
                assert all(invariants[key] for key in ('source_unchanged', 'a_unchanged', 'z_unchanged'))
                assert all(invariants['rng_unchanged'].values()) and not rng_requests
                CampaignReader.check_code(m)
                invariants['original_code_unchanged'] = True
                rows.append(dict(name=f'frame{frame}_{label}',
                    input=dict(frame=frame, facing=angle, berserk=active,
                               shrouded=shrouded, old_z=old_z),
                    body=body, shadow=shadow, final_rgb565_le_hex=final, invariants=invariants))
        # Original selected-frame accessors avoid a second host SHP decoder.
        canvas_width, canvas_height = struct.unpack('<2H', u.mem_read(shp+2, 4))
        rectangle = m.alloc(16)
        frames = []
        for frame in (0, 1, 372, 373):
            m.invoke(0x69E7E0, shp, (rectangle, frame))
            frames.append(dict(frame=frame,
                native_rect=list(struct.unpack('<4i', u.mem_read(rectangle, 16))),
                compressed=bool(m.invoke(0x69E900, shp, (frame,)) & 255)))

        # The original eight rows above retain their exact inputs and outputs.
        # Join Object admission to that same physical GI/palette/surface owner;
        # EntityReveal establishes this viewport prefix, not a second VM.
        from tools.spatial_oracle.building_body_rules import SP as reader_sp
        u.mem_write(reader_sp+4, dwords(0x886FA0))
        _status_reader_block(m, 0x6D5F60, 0x6D5F8D,
            {UC_X86_REG_ECX:tactical}, stack_delta=-12)
        assert list(struct.unpack('<4i',u.mem_read(0xB0CE28,16))) == [0,0,64,96]
        vtable = m.read32(actor)
        assert vtable == VTABLES['infantry']
        assert [m.read32(vtable+offset) for offset in (0xAC,0x104,0x114)] == [
            0x41BE00,0x5F4B10,0x518F90]
        original_vtable = native.file_span(native.image_bytes(),vtable,0x600)[1]
        assert bytes(u.mem_read(vtable,0x600)) == original_vtable
        u.mem_write(0xB73550,dwords(1))
        u.mem_write(0xA8ED6B,b'\0')
        u.mem_write(tactical+0xB0,dwords(0,0))
        coords = m.alloc(12)
        m.invoke(m.read32(vtable+0xAC),actor,(coords,))
        zero_return = m.invoke(0x6D2140,tactical,(coords,point))
        zero_point = list(struct.unpack('<2i',u.mem_read(point,8)))
        viewport_rows = []
        for frame in (0,1):
            #49152 is the physical production fixture's authored facing192.
            angle = 49152 if frame == 1 else lut.index(frame)*2048
            assert lut[(((angle>>10)+1)>>1)&31] == frame
            selected = next(entry for entry in frames if entry['frame']==frame)
            x,y,w,h = selected['native_rect']
            left,top = x-canvas_width//2,y-canvas_height//2
            right,bottom = left+w,top+h
            # Camera inputs come from original zero-camera projection and
            # original frame headers. No VERA point or raster is consulted.
            cases = (
                ('center',(32,48),'positive'),
                ('left_partial',(-left-w//2,48),'positive'),
                ('right_partial',(64-left-w//2,48),'positive'),
                ('top_partial',(32,-top-h//2),'positive'),
                ('bottom_partial',(32,96-top-h//2),'positive'),
                ('left_adjacent_outside',(-right,48),'empty'),
                ('right_adjacent_outside',(64-left,48),'empty'),
                ('top_adjacent_outside',(32,-bottom),'empty'),
                ('bottom_adjacent_outside',(32,96-top),'empty'),
                #6D2223..2269 admits inclusive[-360,W+360]/[-180,H+180].
                ('left_projection_rejected',(-361,48),'rejected'),
                ('right_projection_rejected',(64+361,48),'rejected'),
                ('top_projection_rejected',(32,-181),'rejected'),
                ('bottom_projection_rejected',(32,96+181),'rejected'),
                ('return_center',(32,48),'positive'))
            for label,target,result_kind in cases:
                camera = [zero_point[axis]-target[axis] for axis in range(2)]
                buffers = prepare_surface(u,dict(old_z=2000,a_value=127,background=0x39E7))
                u.mem_write(facing,dwords(angle))
                m.invoke(0x4C9300,actor+0x388,(facing,))
                u.mem_write(actor+0x298,b'\0')
                u.mem_write(actor+0x80,b'\0\0')  # Redraw0/Limbo0; force1 owns admission.
                for cell in cells:
                    u.mem_write(cell+0x12C,dwords(0x18))
                u.mem_write(tactical+0xB0,dwords(*camera))
                u.mem_write(clip,dwords(0,0,64,96))
                before = {name:bytes(u.mem_read(pointer,0x3F4)) for name,pointer in streams.items()}
                reached.clear()
                draws.clear()
                rng_requests.clear()
                viewport_admission = dict(projection_admitted=None,point=None,
                    body_point=None,body_entry_count=0,events=[])
                raw_return = m.invoke(0x5F4B10,actor,(clip,1,0),
                    timeout_us=120000000,instruction_limit=10000000)
                viewport_admission.pop('_projection_output',None)
                viewport_admission.update(raw_return=raw_return,returned_al=raw_return&255,
                    body_admitted=bool(viewport_admission['body_entry_count']),
                    clip=list(struct.unpack('<4i',u.mem_read(clip,16))),
                    redraw_after=bool(u.mem_read(actor+0x80,1)[0]))
                body,shadow = None,None
                for original_draw in draws:
                    draw = dict(original_draw)
                    return_pc = draw.pop('return_pc')
                    # One final buffer below suffices for these empty shadows.
                    draw.pop('rgb565_le_hex',None)
                    if return_pc == 0x70645D:
                        assert body is None and draw['frame'] == frame
                        body = draw
                    else:
                        assert return_pc == 0x706602 and shadow is None
                        shadow = draw
                final = bytes(u.mem_read(PIXELS,64*96*2))
                changed = sum(value[0] != 0x39E7 for value in struct.iter_unpack('<H',final))
                # These assertions require original pixels and dispatch, not
                # the row label or a supplied VERA answer, to establish them.
                assert viewport_admission['point'] is not None
                assert viewport_admission['returned_al'] == int(viewport_admission['body_admitted'])
                if result_kind == 'positive':
                    assert changed > 0 and body is not None
                else:
                    assert changed == 0
                if result_kind == 'rejected':
                    assert not viewport_admission['projection_admitted']
                    assert viewport_admission['body_entry_count'] == 0 and not draws
                else:
                    assert viewport_admission['projection_admitted']
                    assert viewport_admission['body_entry_count'] == 1
                invariants = dict(
                    source_unchanged=bytes(u.mem_read(shp,len(assets['GI.SHP']))) == assets['GI.SHP'],
                    a_unchanged=bytes(u.mem_read(A,len(buffers['lighting']))) == buffers['lighting'],
                    z_unchanged=bytes(u.mem_read(Z,len(buffers['depths']))) == buffers['depths'],
                    rng_unchanged={name:bytes(u.mem_read(pointer,0x3F4)) == before[name]
                        for name,pointer in streams.items()},rng_requests=rng_requests.copy(),
                    vtable_unchanged=bytes(u.mem_read(vtable,0x600)) == original_vtable,
                    facing_table_unchanged=bytes(u.mem_read(0x7EAEFC,128)) == facing_table_bytes)
                assert all(invariants[key] for key in ('source_unchanged','a_unchanged',
                    'z_unchanged','vtable_unchanged','facing_table_unchanged'))
                assert all(invariants['rng_unchanged'].values()) and not rng_requests
                CampaignReader.check_code(m)
                invariants['original_code_unchanged'] = True
                viewport_rows.append(dict(name=f'frame{frame}_{label}',
                    input=dict(frame=frame,facing=angle,berserk=False,shrouded=False,
                        old_z=2000,camera=camera,caller_clip=[0,0,64,96],
                        redraw=False,force=True,limbo=False),
                    admission=viewport_admission,body=body,shadow=shadow,
                    changed_pixel_count=changed,final_rgb565_le_hex=final.hex(),invariants=invariants))
                viewport_admission = None
            assert viewport_rows[-14]['final_rgb565_le_hex'] == viewport_rows[-1]['final_rgb565_le_hex']
        assert len(viewport_rows) == 28
        assets.update({name:tile_assets[name] for name in ('TEMPERATMD.INI', 'CLEAR01.TEM')})
        shared = dict(
            assets=[dict(name=name, bytes=len(raw), sha256=sha(raw)) for name, raw in assets.items()],
            rule_sections=selected_rules, art_sections=selected_art,
            native_rules=native_rules, native_type=native_type,
            colour_scheme={key:scheme[key] for key in ('name', 'lexical', 'hsv', 'color_index',
                'shade_count', 'expanded_palette_sha256', 'table_sha256')},
            actor_constructor=passengers['E1']['evidence'],
            actor=dict(xyz=[2688,5248,0], health=125, alive=True, limbo=False,
                doing=0, stage=0, body_guard=u.mem_read(actor+0x684,1)[0],
                cloak=False, disguise=False, curtain=False, flash=False),
            house=dict(scheme_name='DarkRed', scheme_index=scheme['color_index'], current_house_is_owner=True),
            cells=dict(x_range=[9,11], y_range=[19,21], tile_id=0, subtile=0,
                level=0, slope=0, top_light=1000, visible_flags=0x18, shrouded_flags=0,
                no_overlay=True, no_bridge=True, tile_mapping=tile_mapping),
            surface=dict(width=64, height=96, point=[32,48], clip=[0,0,64,96],
                format=2, bytes_per_pixel=2, background=0x39E7, depth_baseline=1024,
                a_value=127, scroll_rows=0, fpcw=0x0E7F, cmov=True, mmx=True,
                canvas=[canvas_width,canvas_height]),
            frames=frames)
        shared['viewport'] = dict(rect=[0,0,64,96],caller_clip=[0,0,64,96],
            graphical_client=1,editor=False,force=True,redraw=False,
            vtable=f'{vtable:08X}',render_coords='0041BE00',get_coords='005F65A0',
            draw_if_visible='005F4B10',draw_it='00518F90',viewport_writer=['006D5F60','006D5F8D'],
            projection='006D2140',projection_return='005F4B6F',
            projection_inclusive_padding=[360,180],zero_camera_point=zero_point,
            zero_camera_raw_return=zero_return,zero_camera_returned_al=zero_return&255,
            camera_frame='Original Tactical+B0/B4 projected-world top-left; no VERA row bias')
        return dict(schema_version=1, shared=shared,
            rows=rows,viewport_rows=viewport_rows)
    finally:
        u.hook_del(hook)
        u.hook_del(atomic_hook)


def infantry_berserk_body_metadata():
    return native.provenance(
        scope='Eight prepared GI body/shadow controls and28 joined Object viewport controls through original admission/projection, physical raw/RLE SHP, DarkRed Convert and packed RGB565 pixels',
        assumptions=[
            'Original Rules665650/66D480/6691E0 read physical root ColorAdd and AudioVisual BerserkColor/ExtraInfantryLight. InfantryType5236A0/full5240A0 receive only physical E1 Image/Strength/Armor plus fixed ART GI/GISequence. Native Image/sequence loading executes; unrelated type fields retain constructor defaults. This is not full retail type/scenario fill or a layered rules history.',
            'The existing shroud status preparation runs Infantry prefix517A50..517AE6 including Foot/Techno/Object and whole Walk75AA90. Prepared owner/current House, local DarkRed scheme index1, Alive1/Limbo0/HP125, Location2688/5248/0, Ready/Stage0 and ordinary guardFF admit the draw. Whole Infantry construction, COM activation, Unlimbo, display admission and CAOS receiver/refresh/expiry are not executed.',
            'Existing PaletteReader startup and color_scheme_palette execute original palette expansion, paired selected DarkRed1/53 ColorScheme construction and complete Convert initialization on physical palettes. Local scheme1 is not a lobby/world scheme index. RGB565 format2, CMOV/MMX and FPCW0E7F are declared inputs, not pixel-fitted outputs.',
            'GetZAdjust704350 runs with original geometry initialization and existing theater_tile_files/load_tmp_files native TMP registry/lazy loader. Prepared native-constructed3x3 Cells around10/20 have flat level/slope0, tile0/subtile0, no bridge/overlay and top light1000. Selected theater registration base0, actor pose, tactical rectangle/point and Cell visibility are explicit priors, not a whole map/topology lifecycle.',
            'Whole Infantry518F90->Foot41C090->Techno705E00 and actual705D70/518D80/GetZAdjust/shape4AED70/pickers/rowwalkers/raw or RLE leaves run unchanged. Facing inputs are selected from original table7EAEFC and snapped by4C9300; no frame, word, intensity, shape, shadow or pixel result is substituted. Physical frames0/1 plus shadows372/373 are bounded controls, not whole DrawIt/DrawSHP coverage.',
            'Each body buffer is observed immediately at actual shape return70645D before shadow; final buffer is observed after actual shadow return706602. Physical shadow372/373 have original empty rectangles, so final equals body; real shadow calls/pickers execute but no positive shadow-store/overlap is established. Source holes, packed colour, A/Z predicates and blocked stores are native results. The shrouded control deliberately enters the body and proves caller-word clearing, not that full DrawIfVisible admits a shrouded GI. All three full RNG buffers, physical GI bytes, A/Z and original executable code are checked unchanged after every measured draw; constructor RNG/timer evidence is separate.',
            'Prepared64x96 BSurface/circular planes use A127, baseline1024, background39E7 and oldZ2000 or0. No VERA raster/table provides expected pixels. These component crops do not establish GPU, whole frame, CAOS lifecycle, arbitrary stance/effects/custom values/nonflat sites/global ordering or Behind behavior.',
            'The additive viewport_rows reuse the SAME PaletteReader/actor/physical GI/TMP/palette/surface owners after the eight legacy rows. The original retained viewport prefix6D5F60..6D5F8D installs dimensions64x96; no downstream COM/window work executes. GraphicalB73550=1/editorA8ED6B=0, Redraw0/Limbo0, force1 and callerclip0/0/64/96 are declared ordinary caller priors, not a whole Tactical6D8DB0 or display-registration run.',
            'Original actor RenderCoords+AC41BE00/GetCoords5F65A0 and6D2140 establish the zero-camera point. Cameras are selected from that ORIGINAL point and original69E7E0 physical frame headers. Every viewport row then executes whole Object5F4B10 through actual+114Infantry518F90 where admitted. No VERA coordinate, frame, clip, admission, raster or framebuffer initializes a native expected output. Frame0 retains facing53248; frame1 uses49152, the authored192<<8 production pose; native7EAEFC and518D80 select the frames.',
            'Each frame has center, four positive partial-header overlaps, four adjacent empty placements, four projection-rejected placements and center return. Actual6D2223..2269 branches admit inclusive X[-360,W+360]/Y[-180,H+180]; far controls are one beyond those limits. Actual projection AL/point is observed at caller5F4B6F; actual body point and entry count at518F90, raw ObjectEAX/AL after whole return and actual mutated callerclip are recorded separately. A body entry is not a shape call or proof of pixels; adjacent outside may enter518F90 yet emit no shape. Final pixels establish positive versus empty controls; original ObjectAL does not supply that answer.',
            'Every joined row reads final RGB565 including rejected draws and checks unchanged physical GI, A/Z, all three full RNG buffers, original code, actual Infantry vtable and facing table7EAEFC/128B against original file bytes. Center/return final bytes must match. Object5F4B10 clears Redraw+80 before projection and has no RNG draw, timer write or detach call; its main-Tactical force1 prior needs no simulation dirty latch. The selected28 controls do not establish earlier Tactical world-bound admissions, viewport/camera production, minimap input, arbitrary caller clips, other classes, zoom, nonflat height, whole gameplay/frame equivalence or Behind.',
        ], substitutions=[
            'Existing PaletteReader/BulletReader/HutTypeReader supplies immutable archive-selected file bytes, lexical linked/cache INI objects, successful allocator/delete/CRT/TLS and OS resource transport. Existing stock MIX decoding selects physical inputs; original native readers/frame accessors/shape/blitters produce draw outputs. The full native archive/scenario startup is outside this prepared fixture.',
            'Existing infantry_deploy_action atomic service handles only Windows InterlockedIncrement/Decrement through IAT7E11C8/CC. Existing bridge_render_shape BSurface storage/locks/pitch replace DirectDraw backing. Existing TMP RawFile service supplies exact selected physical bytes. No original executable bytes or selected gameplay/draw/pixel results are patched.',
        ], entry_points=dict(rules_ctor=0x665650, color_add=0x66D480, audio_visual=0x6691E0,
            infantry_type_ctor=0x5236A0, infantry_type_read=0x5240A0,
            infantry_prefix=0x517A50, infantry_prefix_stop=0x517AE6, walk_ctor=0x75AA90,
            viewport_begin=0x6D5F60, viewport_stop=0x6D5F8D,
            render_coords=0x41BE00, get_coords=0x5F65A0, projection=0x6D2140,
            draw_if_visible=0x5F4B10, projection_return=0x5F4B6F,
            infantry_body=0x518F90, berserk_reader=0x518FDA, frame=0x518D80,
            foot_shape=0x41C090, draw_shp=0x705E00, house_convert=0x705D70,
            get_z_adjust=0x704350, tmp_query=0x547150, tmp_load=0x544CB0,
            shape=0x4AED70, frame_rect=0x69E7E0, frame_compressed=0x69E900,
            raw_picker=0x490B90, rle_picker=0x490E50,
            raw_plain=0x494B60, raw_colour=0x494C40, rle_plain=0x497FD0, rle_colour=0x498150,
            body_return=0x70645D, shadow_return=0x706602))


if __name__=='__main__':
    import sys
    if '--superweapon-range' in sys.argv[1:]:
        finish_superweapon_range_controls(
            argv=[arg for arg in sys.argv[1:] if arg!='--superweapon-range'])
        raise SystemExit(0)
    if '--infantry-berserk-body' in sys.argv[1:]:
        native.finish_vectors(generate_infantry_berserk_body,
            Path(__file__).with_name('infantry_berserk_body.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--infantry-berserk-body'],
            provenance=infantry_berserk_body_metadata,
            source_paths={'oracle':Path(__file__),
                'native_owner':Path(native.__file__),
                'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'physical_inputs':Path('tools/rules_oracle/weapon_speed_order.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'ini_cache_owner':Path('tools/input_oracle/campaign_start.py'),
                'lexical_owner':Path('tools/rules_oracle/bridge_child_sound.py'),
                'tmp_owner':Path('tools/spatial_oracle/tmp_clear_render.py'),
                'geometry_owner':Path('tools/spatial_oracle/trigger_action_values.py'),
                'atomic_owner':Path('tools/spatial_oracle/infantry_deploy_action.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--radial-indicator' in sys.argv[1:]:
        native.finish_vectors(generate_radial_indicator,
            Path(__file__).with_name('radial_indicator.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--radial-indicator'],
            provenance=radial_indicator_metadata,
            source_paths={'oracle':Path(__file__),
                'reader_owner':Path('tools/spatial_oracle/engineer_bridge_cursor_caller.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--absorber-pips' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_unit_health(absorber=True),
            Path(__file__).with_name('absorber_pips.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--absorber-pips'],
            provenance=absorber_pips_metadata,
            source_paths={'oracle':Path(__file__),
                'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--building-power-status' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_primary_factory_status(power=True),
            Path(__file__).with_name('building_power_status.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--building-power-status'],
            provenance=lambda:primary_factory_status_metadata(power=True),
            source_paths={'oracle':Path(__file__),
                'font_formatter_owner':Path('tools/control_group_text_oracle.py'),
                'atomic_import_owner':Path('tools/spatial_oracle/infantry_deploy_action.py'),
                'csf_owner':Path('tools/storage_oracle/keyboard_bindings.py'),
                'tmp_owner':Path('tools/spatial_oracle/tmp_clear_render.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--self-heal-status' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_unit_health(self_heal=True),
            Path(__file__).with_name('self_heal_status.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--self-heal-status'],
            provenance=self_heal_status_metadata,
            source_paths={'oracle':Path(__file__),
                'house_arithmetic_owner':Path('tools/selfheal_oracle.py'),
                'font_owner':Path('tools/control_group_text_oracle.py'),
                'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--primary-factory-status' in sys.argv[1:]:
        native.finish_vectors(generate_primary_factory_status,
            Path(__file__).with_name('primary_factory_status.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--primary-factory-status'],
            provenance=primary_factory_status_metadata,
            source_paths={'oracle':Path(__file__),
                'font_owner':Path('tools/control_group_text_oracle.py'),
                'csf_owner':Path('tools/storage_oracle/keyboard_bindings.py'),
                'tmp_owner':Path('tools/spatial_oracle/tmp_clear_render.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--spawn-pips' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_unit_health(spawns=True),
            Path(__file__).with_name('spawn_pips.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--spawn-pips'],
            provenance=spawn_pips_metadata,
            source_paths={'oracle':Path(__file__),
                'spawn_lifecycle_owner':Path('tools/rocket_oracle/spawn_manager.py'),
                'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
    if '--bomb-clock' in sys.argv[1:]:
        native.finish_vectors(lambda:generate_unit_health(bomb=True),
            Path(__file__).with_name('bomb_clock.json'),
            argv=[arg for arg in sys.argv[1:] if arg!='--bomb-clock'],
            provenance=bomb_clock_metadata,
            source_paths={'oracle':Path(__file__),
                'bomb_gameplay_owner':Path('tools/spatial_oracle/bomb_class.py'),
                'geometry_owner':Path('tools/spatial_oracle/trigger_action_values.py'),
                'palette_owner':Path('tools/projectile_oracle/bridge_render_inputs_palette.py'),
                'surface_owner':Path('tools/projectile_oracle/bridge_render_shape.py'),
                'reader_owner':Path('tools/projectile_oracle/bridge_render_inputs.py'),
                'tmp_owner':Path('tools/spatial_oracle/tmp_clear_render.py'),
                'physical_layers':Path('tools/rules_oracle/weapon_laser.py'),
                'stock_owner':Path(stock.__file__)})
        raise SystemExit(0)
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
