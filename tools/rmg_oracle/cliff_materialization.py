"""Original post-connector cliff controls; no native Generate input claim.

The existing reader/Cell/TMP/allocator/RNG owners provide the execution fixture.
This module supplies explicit component priors and observes original
instructions; Python computes no function return or expected value. The
sidecar records the supplied boundaries. Importing it performs no asset reads,
VM construction or native execution. With RA2_DIR and VERA20K_GAMEMD_EXE set,
`python -m tools.rmg_oracle.cliff_materialization --check` replays the controls
and compares the corpus `cliff_native_tests` consumes; `--write` replaces it.

The corpus keeps only consumed observations: the Cell/Dummy fields Rust models,
per-record scratch region changes, ordered MapGen draws and every `57B440`
call's AL and caller out-bool. Projection refuses a native change outside those
fields instead of dropping it.
"""
from __future__ import annotations

from collections import Counter
from functools import lru_cache
from pathlib import Path
import struct

MAP = 0x87F7E8
DUMMY = 0xABDC50
SCRATCH_PTR = 0xABED10
STRIDE_AT = 0x89C2DC
RECORD_BYTES = 0x50
CELL_BYTES = 0x148
DOMAIN = (0, 0, 32, 32)
MAP_SIZE = (16, 16)
STRIDE = 33
CENTER = (16, 16)
# Original49F2F0 initializes this direction identity; these are supplied scene
# coordinates, never an expected mask or substitute neighbor implementation.
RING = ((1, -1), (1, 0), (1, 1), (0, 1),
        (-1, 1), (-1, 0), (-1, -1), (0, -1))
ENTRY = dict(mask=0x579B70, select=0x579620, stamp=0x57B440,
             variant=0x5A1350, jitter=0x5A19E0, restamp=0x5A17F0,
             sweep=0x578E60, joined=0x578E60, range=0x598030,
             family_broad=0x4863D0, family_narrow=0x578D80)
FAMILY_GLOBALS = (0xAA1020, 0xAA073C, 0xABB110, 0xAA1050, 0xAA10A0,
                  0xABBEBC, 0xABAD24, 0xAA0E28, 0xABAD1C, 0xABC2C8, 0xAA101C)
# Names/counts/cleanup describe the original calling convention, not results.
CALLS = {
    0x578E60: ('materialize', 2, 8), 0x579010: ('raise', 2, 8),
    0x579620: ('select', 2, 8), 0x579B70: ('mask', 1, 4),
    0x57B440: ('shared_stamp', 6, 24), 0x5A1350: ('variant', 1, 4),
    0x5A17F0: ('restamp', 0, 0), 0x5A19E0: ('jitter', 0, 0),
    0x5A6C10: ('unconditional_stamp', 2, 8),
    0x598030: ('scaled_range', 0, 0), 0x65C780: ('next', 0, 0),
    0x568300: ('playfield_admission', 1, 4),
    0x5657A0: ('cell_lookup', 1, 4), 0x481810: ('neighbor', 1, 4),
    0x486380: ('clear', 0, 0), 0x4863D0: ('special', 0, 0),
    0x4865B0: ('shore_family', 0, 0), 0x578D80: ('family_piece', 0, 0),
    0x5A00C0: ('get_region', 1, 4), 0x5A0090: ('set_region', 2, 8),
    0x58BDC0: ('scratch_constructor', 0, 0),
    0x4A91B0: ('placement', 2, 8), 0x4A8BF0: ('clear_placement', 1, 4),
    0x578350: ('iterator_reset', 0, 0), 0x578290: ('iterator_next', 0, 0),
}
# Existing PaletteReader/Lists supplies these CRT allocation/TLS boundaries.
# No Cell, Map, mask, placement, stamper, scaled arithmetic or RNG entry may
# return through a fixture hook. RawFile IO is allowed only during preparation.
TRANSPORT_RETURNS = frozenset((0x7C8E17, 0x7C9430, 0x7C8B3D, 0x7D140B))
TRANSPORT_REDIRECTS = frozenset((0x7C9430, 0x7C93E8))
CELL_RECEIVERS = frozenset((0x486380, 0x4863D0, 0x4865B0, 0x481810))
ORDERED_CALLS = frozenset(('materialize', 'raise', 'select', 'shared_stamp',
                           'variant', 'jitter', 'restamp', 'unconditional_stamp',
                           'scaled_range', 'next', 'placement'))
# Cell+38 tile (signed dword), +11A subtile, +11B level, +11C slope: the Cell
# fields RmgGrid models. Dummy+24/+26 hold the last stamped lookup coordinate.
CELL_FIELD_BYTES = frozenset(range(0x38, 0x3C)) | {0x11A, 0x11B, 0x11C}
DUMMY_COORD_BYTES = frozenset(range(0x24, 0x28))
REGION_BYTES = frozenset(range(0x38, 0x3C))
# Placement origin global (88095C) read by 57B440 for the selected TileType.
PLACEMENT_ORIGIN = MAP + 0x1174
SELECTED_TYPE = 0x880990


def cell(x, y, **values):
    return dict(coords=[x, y], **values)


def ring_scene(indices, *, center=CENTER, level=0, difference=4):
    """Declare neighbor heights. No expected native mask is calculated."""
    x, y = center
    return [cell(x, y, level=level)] + [
        cell(x + RING[n][0], y + RING[n][1], level=level + difference)
        for n in indices]


def plane_scene(kind):
    """Complete declared height inputs, independent of native output membership."""
    predicates = dict(east=lambda x, y: x > y, west=lambda x, y: y > x,
                      northwest=lambda x, y: x + y < 32,
                      plateau=lambda x, y: 14 <= x <= 18 and 14 <= y <= 18)
    return [cell(x, y, level=4) for y in range(32) for x in range(32)
            if predicates[kind](x, y)]


def controls():
    """Bounded, reviewable input matrix; none of these values is an output."""
    rows = [dict(id='mask_flat', kind='mask', cells=[])]
    rows += [dict(id=f'mask_neighbor_{n}', kind='mask', cells=ring_scene([n]))
             for n in range(8)]
    rows += [dict(id=f'mask_height_{delta}', kind='mask',
                  cells=ring_scene([1], difference=delta)) for delta in (3, 5)]
    rows += [dict(id='mask_disabled', kind='mask', cells=ring_scene([1]), disabled=[list(CENTER)]),
             dict(id='mask_signed_123_127', kind='mask', cells=ring_scene([1], level=123)),
             dict(id='mask_signed_124_minus128', kind='mask', cells=ring_scene([1], level=124)),
             dict(id='mask_signed_127_minus128', kind='mask',
                  cells=[cell(16, 16, level=127), cell(17, 16, level=-128)]),
             dict(id='mask_edge', kind='mask', target=[1, 16], cells=[]),
             dict(id='mask_missing_neighbor', kind='mask', cells=ring_scene([1]),
                  missing=[[17, 16]], allow_dummy=True, rust_grid_comparison=False),
             dict(id='mask_special_neighbor', kind='mask',
                  cells=ring_scene([1]) + [cell(17, 16, family='cliff', offset=0, level=4)])]
    # Same original family leaves, with physical native-published values and
    # explicit synthetic unset/overlap priors. No descriptor is needed by these
    # numeric admission leaves; no synthetic registration is used by stampers.
    for kind in ('family_broad', 'family_narrow'):
        for global_at in FAMILY_GLOBALS[:6]:
            for offset in (0, 1, 3):
                for subtile in (0, 1, 4):
                    rows.append(dict(id=f'{kind}_{global_at:08x}_{offset}_{subtile}',
                        kind=kind, family_global=global_at, offset=offset, subtile=subtile, cells=[]))
        rows += [dict(id=kind + '_unset', kind=kind, tile=-1, all_unset=True, cells=[]),
                 dict(id=kind + '_unset_zero', kind=kind, tile=0, all_unset=True, cells=[]),
                 dict(id=kind + '_unset_ffff', kind=kind, tile=0xFFFF, all_unset=True, cells=[]),
                 dict(id=kind + '_waterfall_overlap_early_false', kind=kind,
                      tile=200, subtile=0, all_unset=True,
                      family_globals={'00AA073C': 200, '00AA101C': 200}, cells=[])]
    rows.append(dict(id='family_narrow_full_index', kind='family_narrow',
                     tile=200, subtile=256, all_unset=True,
                     family_globals={'00AA073C': 200}, cells=[]))
    for n, indices in enumerate(([7], [5], [3], [1], [7, 5], [7, 0, 1], [5, 4, 3], [5, 6, 7])):
        rows.append(dict(id=f'select_corner_{n}', kind='select', cells=ring_scene(indices)))
    rows += [dict(id='select_flat', kind='select', cells=[]),
             dict(id='select_plateau', kind='select', cells=plane_scene('plateau'))]
    # All paths share a real sparse physical CliffSet[0] descriptor. The second
    # cell is reached after the first present slot in the native row-major loop.
    for name, values in (
            ('sparse_preserve_slope', {}), ('adopt_owner', {'region': 7}),
            ('foreign_owner', {'region': 8}), ('unowned_request', {'request_region': -1}),
            ('keep_flag', {'keep': True}), ('null_region', {'scratch': False}),
            ('soft_after_write', {'block': 'soft'}), ('hard_after_write', {'block': 'hard'}),
            ('missing_cell', {'missing': [[16, 17]], 'allow_dummy': True, 'rust_grid_comparison': False})):
        rows.append(dict(id='stamp_' + name, kind='stamp',
                         cells=[cell(16, 16, slope=13), cell(16, 17, slope=29)], **values))
    rows.append(dict(id='stamp_border', kind='stamp', target=[1, 16], cells=[]))
    # Native table dispatch is measured for all40 ordinary offsets, plus both
    # out-of-window directions. Raw inputs include the unsigned upper bound; the
    # constrained stream is independently verified by original Next's owner.
    for offset in range(-1, 41):
        for word in (0, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF):
            rows.append(dict(id=f'variant_{offset}_{word:08x}', kind='variant',
                             offset=offset, raw=[word, 0], cells=[]))
    for offset in (4, 5, 6, 34, 35, 36):
        rows.append(dict(id=f'variant_wrap_{offset}', kind='variant', offset=offset,
                         advance=249, cells=[]))
    rows.append(dict(id='variant_locked', kind='variant', offset=4, locked=True, cells=[]))
    for low, high in ((0, 1), (1, 2), (0, 5)):
        for word in (0, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF):
            rows.append(dict(id=f'range_{low}_{high}_{word:08x}', kind='range',
                             bounds=[low, high], raw=[word, 0], cells=[]))
    for plane in ('east', 'west', 'northwest'):
        for word in ((0, 0), (0x80000000, 0), (0xFFFFFFFF, 0)):
            rows.append(dict(id=f'jitter_{plane}_{word[0]:08x}', kind='jitter',
                             cells=plane_scene(plane), raw=list(word), tag_regions=True))
    # Supplied field controls use present physical subtile indices. They do
    # not claim complete template placement or native Generate provenance.
    for offset in (4, 8, 14, 22, 34):
        for layout in ('south', 'east', 'south_then_east'):
            current_subtile = {4: 3, 8: 2, 14: 3, 22: 1, 34: 1}[offset]
            members = [cell(16, 16, family='cliff', offset=offset,
                            subtile=current_subtile, level=9, slope=17)]
            if layout != 'east':
                members.append(cell(16, 17, family='cliff', offset=offset, subtile=0, level=7, slope=19))
            if layout != 'south':
                members.append(cell(17, 16, family='cliff', offset=offset, subtile=0, level=11, slope=23))
            rows.append(dict(id=f'restamp_{offset}_{layout}', kind='restamp', cells=members,
                             physical_valid_subtiles=True, tag_regions=True, raw=[0, 0]))
    rows += [dict(id='restamp_flat', kind='restamp', cells=[]),
             dict(id='sweep_flat_retained', kind='sweep', cells=[]),
             dict(id='sweep_plateau_retained', kind='sweep', cells=plane_scene('plateau')),
             dict(id='sweep_plateau_owned', kind='sweep', scratch=False, cells=plane_scene('plateau'))]
    # Restamp receives the actual original materializer's complete live output,
    # with no reseed/reset/VERA-prestage transport between these native phases.
    rows += [dict(id=f'joined_{plane}', kind='joined', cells=plane_scene(plane))
             for plane in ('plateau', 'east', 'west', 'northwest')]
    validate_controls(rows)
    return rows


def validate_controls(rows):
    ids = [row['id'] for row in rows]
    if len(ids) != len(set(ids)):
        raise ValueError('Control IDs must be unique')
    for row in rows:
        if row['kind'] not in ENTRY:
            raise ValueError('Unadmitted native entry')
        if row['kind'] == 'joined' and not row.get('scratch', True):
            raise ValueError('Joined materialize/restamp requires retained scratch')
        for item in row.get('cells', []):
            x, y = item['coords']
            if not (0 <= x < DOMAIN[2] and 0 <= y < DOMAIN[3]):
                raise ValueError('Scene cells must belong to the complete declared domain')
            if set(item) - {'coords', 'family', 'offset', 'subtile', 'level', 'slope', 'region'}:
                raise ValueError('Unknown scene field')
        for xy in row.get('missing', []):
            if not row.get('allow_dummy') or not (0 <= xy[0] < 32 and 0 <= xy[1] < 32):
                raise ValueError('A missing registration requires explicit Dummy scope')
        if 'raw' in row and (not row['raw'] or any(type(n) is not int or not 0 <= n < 1 << 32 for n in row['raw'])):
            raise ValueError('Raw controls must be unsigned original-Next input words')


class StatePool:
    """Intern observed bytes without calculating or discarding RNG state."""
    def __init__(self):
        self.states = []
        self.index = {}

    def refs(self, states):
        result = {}
        for name, raw in states.items():
            if raw not in self.index:
                self.index[raw] = len(self.states)
                self.states.append(raw)
            result[name] = self.index[raw]
        return result


def compact_calls(calls):
    """Keep return closure for every entered function, order for effects/RNG."""
    counts = Counter()
    ordered = []
    for call in calls:
        if 'return_pc' not in call:
            raise ValueError('Unreturned original entry: ' + call['name'])
        counts[(call['name'], call['entry'], call['return_pc'])] += 1
        if call['name'] in ORDERED_CALLS:
            ordered.append(call)
    return dict(counts=[dict(name=n, entry=e, return_pc=r, count=count)
                        for (n, e, r), count in sorted(counts.items())], ordered=ordered)


def require_projected(before, after, allowed, owner):
    """Refuse to drop a native byte change outside the consumed fields."""
    outside = sorted(n for n, (old, new) in enumerate(zip(before, after))
                     if old != new and n not in allowed)
    if len(before) != len(after) or outside:
        raise ValueError(f'{owner} changed unprojected bytes {[hex(n) for n in outside[:8]]}')


def cell_fields(raw):
    """Read the modeled fields from an observed Cell image; no Cell logic."""
    return dict(tile=struct.unpack_from('<i', raw, 0x38)[0], subtile=raw[0x11A],
                level=raw[0x11B], slope=raw[0x11C])


def dummy_fields(raw):
    return dict(cell_fields(raw), coords=list(struct.unpack_from('<hh', raw, 0x24)))


def region_changes(before, after):
    """Per-record region changes over the whole observed scratch block."""
    changes = []
    for index in range(len(before) // RECORD_BYTES):
        start = index * RECORD_BYTES
        old, new = before[start:start + RECORD_BYTES], after[start:start + RECORD_BYTES]
        if old == new:
            continue
        require_projected(old, new, REGION_BYTES, f'Scratch record {index}')
        changes.append(dict(coords=[index % STRIDE, index // STRIDE],
                            before=struct.unpack_from('<i', old, 0x38)[0],
                            after=struct.unpack_from('<i', new, 0x38)[0]))
    return changes


def stamp_results(calls):
    """Every observed 57B440 call in order: selected tile/origin, AL, out-bool."""
    return [dict(tile=call['tile'], origin=call['origin'], al=call['return_eax'] & 255,
                 outbool_before=call['outbool_before'], outbool=call['outbool'])
            for call in calls if call['name'] == 'shared_stamp']


class CliffFixture:
    """Compose one existing PaletteReader VM; no gameplay return substitutions."""
    def __init__(self):
        from tools import native_oracle as native
        from tools.projectile_oracle.bridge_render_inputs_palette import PaletteReader
        from tools.spatial_oracle import tmp_clear_render as tmp
        from tools.spatial_oracle.bridge_shadow_render import physical_assets
        from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
        from tools.procedural_drawing_oracle.shroud import _prepare_flat_cells
        from tools.rmg_oracle.gen_rng_vectors import code_guard, STRUCT_LEN
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW

        self.native, self.struct_len = native, STRUCT_LEN
        self.m = m = PaletteReader({})
        self.u = u = m.u
        u.reg_write(UC_X86_REG_FPCW, native.NATIVE_FPCW)
        self.code = code_guard(u)
        assets, sources = physical_assets(tile=True, tmp_names=('clear01.tem', 'cliff01.tem'))
        files = {s['name']: assets[s['name'].upper()] for s in sources}
        _map, authored = decode_cells(files['XMP03T4.MAP'])
        authored_cell = authored[(18, 69)]
        tiles, mapping = tmp.theater_tile_files(m, files['temperatmd.ini'], authored_cell['tile'], 'tem')
        self.bases = dict(clear=m.read32(0xAA10B0), cliff=m.read32(0xAA1020), shore=m.read32(0xABAD28))
        self.family_priors = {p: bytes(u.mem_read(p, 4)) for p in FAMILY_GLOBALS}
        assert self.bases['clear'] == 0, 'Existing flat-cell owner supplies tile0; changed native ClearTile is a prerequisite'
        assert mapping['base'] == self.bases['cliff'] and mapping['tiles_in_set'] == 40
        assert tiles[authored_cell['tile']] == 'cliff01.tem', 'Physical authored identity no longer selects this control TMP'
        ids = [self.bases['clear']] + list(range(self.bases['cliff'], self.bases['cliff'] + 40))
        # Shore's native42-member tables are reached by the shared stamper even
        # on cliff controls. Load physical descriptors rather than fake types.
        if self.bases['shore'] != 0xFFFFFFFF:
            ids += list(range(self.bases['shore'], self.bases['shore'] + 42))
        ids = sorted(set(ids))
        names = [tiles[n] for n in ids]
        assert len(names) == len(set(names)), 'Distinct native primary IDs alias a filename; registration needs review'
        assets, sources = physical_assets(tile=True, tmp_names=tuple(names))
        files = {s['name']: assets[s['name'].upper()] for s in sources}
        m.assets.update(assets)
        m.phase = 'cliff_tmp_preparation'
        heads, _file_io = tmp.load_tmp_files(m, {name: files[name] for name in names},
                                            tile_ids={tiles[n]: n for n in ids})
        registry = m.alloc(len(tiles) * 4)
        self.heads = {}
        for n in ids:
            head = heads[tiles[n]]
            assert m.read32(head + 0x294) == n, 'Original constructor did not retain its first argument'
            self.heads[n] = head
            u.mem_write(registry + n * 4, words(head))
        u.mem_write(0xA8ED2C, words(registry))
        u.mem_write(0xA8ED38, words(len(tiles)))
        m.invoke(0x579320, 0)  # Original CRT anchor initializer; BSS is not a file table.
        m.invoke(0x49F2F0, 0)  # Original neighbor direction initializer.
        self.table, pointers = _prepare_flat_cells(m, range(32), range(32))
        m.invoke(0x47BBF0, DUMMY)
        self.backing = {(x, y): pointers[y * 32 + x] for y in range(32) for x in range(32)}
        # Original admission supplies the component's lookup membership. This
        # is deliberately not a host diamond predicate or allocation-history
        # claim. Retain every admitted cell and all rejected backing storage.
        probe = m.alloc(4)
        admitted = []
        membership_trace = native.NativeCallTrace(u, m.read32)
        def observe_membership(machine, pc, _size, _data):
            sp = machine.reg_read(UC_X86_REG_ESP)
            membership_trace.returned(pc, sp)
            if pc in (0x568300, 0x5657A0):
                assert machine.reg_read(UC_X86_REG_EIP) == pc, 'Membership entry was substituted'
                membership_trace.entered(pc, sp, CALLS[pc])
        membership_hook = u.hook_add(UC_HOOK_CODE, observe_membership)
        try:
            for xy, pointer in self.backing.items():
                u.mem_write(probe, struct.pack('<hh', *xy))
                actual = m.invoke(0x568300, MAP, (probe,))
                membership_trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
                if actual & 255:
                    admitted.append(xy)
                else:
                    u.mem_write(self.table + (xy[1] * 512 + xy[0]) * 4, words(0))
            admitted_set = set(admitted)
            for xy, pointer in self.backing.items():
                u.mem_write(probe, struct.pack('<hh', *xy))
                actual = m.invoke(0x5657A0, MAP, (probe,))
                membership_trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
                assert actual == (pointer if xy in admitted_set else DUMMY), 'Original lookup membership readback differs'
                assert bytes(u.mem_read(actual + 0x24, 4)) == struct.pack('<hh', *xy), 'Original lookup coordinate readback differs'
            compact_calls(membership_trace.calls)  # Every membership entry returned.
            assert not membership_trace.pending
        finally:
            u.hook_del(membership_hook)
        self.cells = {xy: self.backing[xy] for xy in admitted}
        self.rejected_prior = {p: bytes(u.mem_read(p, CELL_BYTES))
                               for xy, p in self.backing.items() if xy not in admitted_set}
        m.invoke(0x47BBF0, DUMMY)
        self.pointer_coords = {p: xy for xy, p in self.cells.items()}
        self.scratch = m.alloc(STRIDE * STRIDE * RECORD_BYTES)
        for index in range(STRIDE * STRIDE):
            m.invoke(0x58BDC0, self.scratch + index * RECORD_BYTES)
        u.mem_write(SCRATCH_PTR, words(self.scratch))
        u.mem_write(STRIDE_AT, words(STRIDE))
        # Supplied component bounds mirror this declared diamond, not a claim
        # that a complete native Generate initialized the fixture.
        u.mem_write(0xABED04, words(16, 48))
        u.mem_write(MAP + 0x117C, words(0))
        u.mem_write(0x8A03F8, struct.pack('<hh', -32768, -32768))
        u.mem_write(0x88098C, words(0, 0))
        self.out = m.alloc(4)
        self.rngs = dict(scenario=m.read32(0xA8B230) + 0x218, main=0x886B88, mapgen=0xABE890)
        self.cell_prior = {xy: bytes(u.mem_read(p, CELL_BYTES)) for xy, p in self.cells.items()}
        self.scratch_prior = bytes(u.mem_read(self.scratch, STRIDE * STRIDE * RECORD_BYTES))
        self.map_prior = bytes(u.mem_read(MAP, 0x1200))
        self.dummy_prior = bytes(u.mem_read(DUMMY, CELL_BYTES))
        assert m.read32(DUMMY + 0x38) == 0xFFFF and u.mem_read(DUMMY + 0x11B, 1) == b'\0', f'Native Dummy prior differs: tile={m.read32(DUMMY + 0x38):08X}, level={u.mem_read(DUMMY + 0x11B, 1).hex()}'
        # Consumed inputs only. Provenance (theater, authored identity, file
        # transport, membership boundary) stays in the sidecar assumptions.
        # The supplied flat Cell prior is checked against every observed
        # changed Cell's native before-image by the Rust consumer.
        self.inputs = dict(
            bases=self.bases, family_globals={f'{p:08X}': m.read32(p) for p in FAMILY_GLOBALS},
            registered_tiles=[self.registered_input(n) for n in ids],
            scene=dict(map_size=list(MAP_SIZE), scratch_stride=STRIDE,
                       admitted_cells=[list(xy) for xy in admitted],
                       common=dict(tile=0, subtile=0, level=0, slope=0),
                       tagged_regions=[dict(coords=list(xy), region=1 + (xy[0] + xy[1]) % 3)
                                       for xy in self.cells]))
        self._guard_returns()

    def registered_input(self, tile):
        # Observe the actual original-loaded in-memory descriptor and sparse
        # header pointers consumed by57B440/5A6C10. This is not a file parser
        # or a Rust-supplied footprint. Height+28 is read at57B667; slope+2A
        # is read by the existing unconditional5A6C10 owner.
        m, u = self.m, self.u
        head = self.heads[tile]
        width, height = m.read32(head + 0x2E4), m.read32(head + 0x2E8)
        image = m.read32(head + 0xA4)
        subtiles = []
        for index in range(width * height):
            header = m.read32(image + 0x10 + index * 4)
            if not header:
                subtiles.append(None)
            else:
                level, terrain, slope = u.mem_read(header + 0x28, 3)
                subtiles.append(dict(height=level, terrain=terrain, slope=slope))
        return dict(tile=tile, width=width, height=height, subtiles=subtiles)

    def _guard_returns(self):
        from unicorn.x86_const import UC_X86_REG_EIP
        original = self.m.ret
        self.active = False
        def guarded(eax, cleanup=0):
            if self.active:
                pc = self.u.reg_read(UC_X86_REG_EIP)
                if pc not in TRANSPORT_RETURNS:
                    raise self.native.OracleError(f'Unadmitted hooked return at {pc:08X}')
            return original(eax, cleanup)
        self.m.ret = guarded

    def region_at(self, xy, scratch=None):
        return (self.scratch if scratch is None else scratch) + (xy[1] * STRIDE + xy[0]) * RECORD_BYTES + 0x38

    def states(self):
        return {name: bytes(self.u.mem_read(p, self.struct_len)).hex() for name, p in self.rngs.items()}

    def reset(self, row):
        u, m = self.u, self.m
        self.active = False
        m.phase = 'cliff_control'
        for xy, p in self.cells.items():
            u.mem_write(p, self.cell_prior[xy])
            u.mem_write(self.table + (xy[1] * 512 + xy[0]) * 4, words(p))
        u.mem_write(MAP, self.map_prior)
        u.mem_write(DUMMY, self.dummy_prior)
        u.mem_write(self.scratch, self.scratch_prior)
        u.mem_write(SCRATCH_PTR, words(self.scratch if row.get('scratch', True) else 0))
        u.mem_write(0x88098C, words(0, 0))
        for p, raw in self.family_priors.items():
            u.mem_write(p, words(-1) if row.get('all_unset') else raw)
        for p, value in row.get('family_globals', {}).items():
            assert int(p, 16) in FAMILY_GLOBALS
            u.mem_write(int(p, 16), words(value))
        for name, ptr in self.rngs.items():
            m.invoke(0x65C6D0, ptr, (row.get('seed', 11),))
        rng_input = None
        if 'raw' in row:
            rng_input = controlled_state(row.get('seed', 11), tuple(row['raw']))
            u.mem_write(self.rngs['mapgen'], bytes.fromhex(rng_input['before_state_hex']))
        for _ in range(row.get('advance', 0)):
            m.invoke(0x65C780, self.rngs['mapgen'])
        if row.get('locked'):
            u.mem_write(self.rngs['mapgen'], b'\1')
        if row.get('tag_regions'):
            for xy in self.cells:
                u.mem_write(self.region_at(xy), words(1 + (xy[0] + xy[1]) % 3))
        for item in row.get('cells', []):
            xy = tuple(item['coords'])
            if xy not in self.cells:
                # Complete plane inputs include backing cells rejected by the
                # original native admission, as RmgGrid get_mut also rejects.
                continue
            p = self.cells[xy]
            if 'family' in item:
                tile = (self.bases[item['family']] + item.get('offset', 0)) & 0xFFFFFFFF
                assert tile in self.heads, ('Unregistered input TileType', item)
                if row.get('physical_valid_subtiles'):
                    material = next(entry for entry in self.inputs['registered_tiles'] if entry['tile'] == tile)
                    sub = item.get('subtile', 0)
                    assert 0 <= sub < len(material['subtiles']) and material['subtiles'][sub] is not None, ('Input subtile is absent from original-loaded physical TMP', item)
                u.mem_write(p + 0x38, words(tile))
            for key, off in (('subtile', 0x11A), ('level', 0x11B), ('slope', 0x11C)):
                if key in item:
                    u.mem_write(p + off, bytes([item[key] & 255]))
            if 'region' in item:
                u.mem_write(self.region_at(xy), words(item['region']))
        for xy in row.get('disabled', []):
            u.mem_write(self.region_at(xy) - 0x38 + 0x4A, b'\0')
        for xy in row.get('missing', []):
            u.mem_write(self.table + (xy[1] * 512 + xy[0]) * 4, words(0))
        if row['kind'] == 'stamp':
            # Target controls supply actual cell/scratch priors; no admission
            # outcome is injected. Native shared stamping makes both decisions.
            if 'region' in row:
                for y in range(16, 19):
                    for x in range(16, 18):
                        u.mem_write(self.region_at((x, y)), words(row['region']))
            if row.get('block'):
                p = self.cells[(16, 17)]
                u.mem_write(p + 0x38, words(self.bases['cliff'] + 1))
                u.mem_write(self.region_at((16, 17)), words(99 if row['block'] == 'hard' else 7))
            u.mem_write(0x880990, words(self.heads[self.bases['cliff']]))
            u.mem_write(MAP + 0x1174, struct.pack('<hh', *row.get('target', CENTER)))
        if row['kind'].startswith('family_'):
            tile = row.get('tile')
            if tile is None:
                tile = (m.read32(row['family_global']) + row['offset']) & 0xFFFFFFFF
            u.mem_write(self.cells[CENTER] + 0x38, words(tile))
            u.mem_write(self.cells[CENTER] + 0x11A, bytes([row.get('subtile', 0) & 255]))
        u.mem_write(self.out, b'\1\0\0\0')
        return rng_input

    def run(self, row, pool):
        from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
        from unicorn.x86_const import (UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP,
                                      UC_X86_REG_ESI, UC_X86_REG_ESP)
        from tools.spatial_oracle.building_body_rules import SP
        from tools.rmg_oracle.gen_rng_vectors import check_code
        self.reset(row)
        u, m, native = self.u, self.m, self.native
        before_rng = pool.refs(self.states())
        before_cells = {xy: bytes(u.mem_read(p, CELL_BYTES)) for xy, p in self.cells.items()}
        before_scratch = bytes(u.mem_read(self.scratch, len(self.scratch_prior)))
        before_dummy = bytes(u.mem_read(DUMMY, CELL_BYTES))
        trace = native.NativeCallTrace(u, m.read32)
        reached, raw, writes, phases = set(), [], [], []
        scratch_observation = dict(pointer=self.scratch, before=before_scratch)
        def returned(pc, sp):
            # 57B440 reports through AL and the caller-owned out-bool; read
            # the out-bool byte at the actual return, before any caller use.
            for index in trace.returned(pc, sp):
                call = trace.calls[index]
                if call['name'] == 'shared_stamp':
                    call['outbool'] = u.mem_read(call['args'][4], 1)[0]
        def observe(machine, pc, _size, _data):
            sp = machine.reg_read(UC_X86_REG_ESP)
            returned(pc, sp)
            redirected = machine.reg_read(UC_X86_REG_EIP) != pc
            if redirected and pc not in TRANSPORT_RETURNS | TRANSPORT_REDIRECTS:
                raise native.OracleError(f'Inherited hook replaced original entry {pc:08X}')
            reached.add(pc)
            if pc == 0x578EC9 and not row.get('scratch', True):
                # After native allocation/58BDC0, before the all+4A pass.
                pointer = m.read32(SCRATCH_PTR)
                assert pointer and pointer != self.scratch, 'Owned scratch was not allocated'
                scratch_observation.update(pointer=pointer,
                    before=bytes(machine.mem_read(pointer, len(self.scratch_prior))))
            if pc == 0x578FEC:
                # Native cleanup is about to free its own scratch. Preserve
                # that actual storage, not the unused retained-fixture block.
                pointer = m.read32(SCRATCH_PTR)
                assert pointer == scratch_observation['pointer']
                scratch_observation['after'] = bytes(machine.mem_read(pointer, len(self.scratch_prior)))
            if pc in CALLS:
                assert not redirected, f'Original {pc:08X} entry was substituted'
                index = trace.entered(pc, sp, CALLS[pc])
                if pc == 0x57B440:
                    # The selected TileType and origin are 57B440's global
                    # inputs (57B440/57B4BC/57B4C4); read them at entry.
                    selected = m.read32(SELECTED_TYPE)
                    trace.calls[index].update(
                        tile=struct.unpack('<i', machine.mem_read(selected + 0x294, 4))[0] if selected else None,
                        origin=list(struct.unpack('<hh', machine.mem_read(PLACEMENT_ORIGIN, 4))),
                        outbool_before=machine.mem_read(trace.calls[index]['args'][4], 1)[0])
            if pc in CELL_RECEIVERS:
                receiver = machine.reg_read(UC_X86_REG_ECX)
                if receiver != DUMMY and receiver not in self.pointer_coords:
                    raise native.OracleError(f'Unenrolled Cell receiver {receiver:08X} at {pc:08X}')
            if pc == 0x579B70:
                receiver = m.read32(sp + 4)
                assert receiver in self.pointer_coords or receiver == DUMMY, 'Unenrolled mask receiver'
            if pc in (0x65C79D, 0x65C84B):
                owner = machine.reg_read(UC_X86_REG_ECX)
                assert owner in self.rngs.values(), f'Unknown RNG stream {owner:08X}'
                raw.append(dict(stream=next(name for name, p in self.rngs.items() if p == owner),
                                word=machine.reg_read(UC_X86_REG_ESI) & 0xFFFFFFFF))
        def memory(machine, _access, address, size, _value, _data):
            # Only Dummy write offsets are needed: the declared-scope check.
            if DUMMY <= address and address + size <= DUMMY + CELL_BYTES:
                writes.append(dict(pc=f'{machine.reg_read(UC_X86_REG_EIP):08X}',
                                   offset=address - DUMMY, size=size))
        code_hook = u.hook_add(UC_HOOK_CODE, observe)
        write_hook = u.hook_add(UC_HOOK_MEM_WRITE, memory)
        self.active = True
        try:
            function = ENTRY[row['kind']]
            target = tuple(row.get('target', CENTER))
            args, receiver, edx = (), MAP, 0
            if row['kind'] == 'mask': args = (self.cells[target],)
            elif row['kind'] == 'select': args = (self.cells[target], 7)
            elif row['kind'] == 'stamp': args = (0, 0, 0, row.get('request_region', 7), self.out, int(row.get('keep', False)))
            elif row['kind'] == 'variant':
                receiver, args = 0xABDFD8, ((self.bases['cliff'] + row['offset']) & 0xFFFFFFFF,)
            elif row['kind'] == 'range': receiver, edx = row['bounds']
            elif row['kind'] == 'restamp': receiver = 0xABDFD8
            elif row['kind'] in ('sweep', 'joined'): args = (0, -1)
            elif row['kind'] == 'family_broad': receiver = self.cells[CENTER]
            elif row['kind'] == 'family_narrow':
                receiver = m.read32(self.cells[CENTER] + 0x38)
                edx = row.get('subtile', 0)
            u.reg_write(UC_X86_REG_EDX, edx)
            result = m.invoke(function, receiver, args, timeout_us=60_000_000,
                              instruction_limit=20_000_000, context=dict(control=row['id']))
            returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            assert u.reg_read(UC_X86_REG_ESP) == SP + 4 + len(args) * 4, 'Unexpected native cleanup'
            assert function in reached, 'Original entry was not observed'
            compact_calls(trace.calls)  # Every entered original function returned.
            assert not trace.pending, 'Original call/return closure incomplete'
            check_code(u, self.code)
            assert all(bytes(u.mem_read(p, CELL_BYTES)) == prior
                       for p, prior in self.rejected_prior.items()), 'Native modified rejected backing storage'
            if row['kind'] in ('sweep', 'joined'):
                assert 0x579010 in reached and 0x578290 in reached, 'Whole original raise/iterator pass was not entered'
                assert m.read32(SELECTED_TYPE) == 0 and m.read32(0x88098C) == 0, 'Native selected/temporary cleanup incomplete'
                assert m.read32(SCRATCH_PTR) == (self.scratch if row.get('scratch', True) else 0), 'Native scratch ownership changed'
            if row['kind'] == 'joined':
                materialized_scratch = bytes(u.mem_read(self.scratch, len(self.scratch_prior)))
                phases.append(dict(name='after_materialize', rng=pool.refs(self.states()),
                    cells=[dict(coords=list(xy), **self.fields(xy, materialized_scratch))
                           for xy in self.cells]))
                result = m.invoke(0x5A17F0, 0xABDFD8, timeout_us=60_000_000,
                    instruction_limit=20_000_000, context=dict(control=row['id'], phase='restamp'))
                returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
                assert u.reg_read(UC_X86_REG_ESP) == SP + 4, 'Unexpected joined restamp cleanup'
                assert 0x5A17F0 in reached
                assert not trace.pending, 'Joined call/return closure incomplete'
                compact_calls(trace.calls)
                check_code(u, self.code)
                assert m.read32(SCRATCH_PTR) == self.scratch, 'Joined scratch owner changed'
                assert m.read32(SELECTED_TYPE) == 0 and m.read32(0x88098C) == 0
                assert all(bytes(u.mem_read(p, CELL_BYTES)) == prior
                           for p, prior in self.rejected_prior.items()), 'Joined restamp modified rejected backing storage'
            # Native may update Dummy coordinates during rejected lookups. A
            # positive material write to the shared Dummy requires named scope.
            if not row.get('allow_dummy'):
                assert not any(w['offset'] != 0x24 for w in writes), 'Undeclared positive Dummy effect'
        finally:
            self.active = False
            u.hook_del(code_hook)
            u.hook_del(write_hook)
        scratch_after = scratch_observation.get('after')
        if scratch_after is None:
            scratch_after = bytes(u.mem_read(scratch_observation['pointer'], len(self.scratch_prior)))
        changes = []
        for xy, p in self.cells.items():
            after = bytes(u.mem_read(p, CELL_BYTES))
            if after != before_cells[xy]:
                require_projected(before_cells[xy], after, CELL_FIELD_BYTES, f'Cell {xy}')
                changes.append(dict(coords=list(xy), before=cell_fields(before_cells[xy]),
                                    after=cell_fields(after)))
        after_dummy = bytes(u.mem_read(DUMMY, CELL_BYTES))
        require_projected(before_dummy, after_dummy, CELL_FIELD_BYTES | DUMMY_COORD_BYTES, 'Dummy')
        stamps = stamp_results(trace.calls)
        if row['kind'] == 'stamp':
            assert len(stamps) == 1 and (stamps[0]['al'], stamps[0]['outbool']) == (result & 255, u.mem_read(self.out, 1)[0])
        return dict(id=row['id'], input=row,
            before_rng=before_rng, after_rng=pool.refs(self.states()), raw_draws=raw,
            eax=result, al=result & 255, outbool=u.mem_read(self.out, 1)[0],
            changed_cells=changes,
            changed_regions=region_changes(scratch_observation['before'], scratch_after),
            scratch_final_pointer=m.read32(SCRATCH_PTR),
            dummy=dict(before=dummy_fields(before_dummy), after=dummy_fields(after_dummy)),
            phases=phases, stamps=stamps)

    def fields(self, xy, scratch):
        p = self.cells[xy]
        tile = struct.unpack('<i', self.u.mem_read(p + 0x38, 4))[0]
        sub, level, slope = self.u.mem_read(p + 0x11A, 3)
        region = struct.unpack_from('<i', scratch, (xy[1] * STRIDE + xy[0]) * RECORD_BYTES + 0x38)[0]
        return dict(tile=tile, subtile=sub, level=level, slope=slope, region=region)


def words(*values):
    return b''.join(struct.pack('<I', n & 0xFFFFFFFF) for n in values)


@lru_cache(maxsize=None)
def controlled_state(seed, words_):
    from tools.rmg_oracle.gen_rng_vectors import constrained_state
    return constrained_state(seed, dict(enumerate(words_)))


def generate():
    from tools import native_oracle as native
    fixture, pool = CliffFixture(), StatePool()
    rows = [fixture.run(row, pool) for row in controls()]
    return dict(schema=2, inputs=fixture.inputs,
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='Original post-connector cliff mask/selector/shared stamp, variant/restamp/jitter and complete materialization component controls',
        assumptions=[
            'Explicit complete32x32 native-constructed Cell backing, native MapSize16x16, fixed512 lookup and scratch stride33. Original568300 supplies admitted lookup membership;5657A0 reads back every supplied coordinate. Rejected slots areNULL with backing retained. Allocation history/native Generate/map bootstrap remain unproven.',
            'Native General/TileSet scalar/string reads and ordinal publication; existing supplied cumulative registration prefix, physical authored XMP03T4 identity and physical TMP descriptors. No Rust tile identity initializes native input.',
            'Original579320 cliff anchors and49F2F0 direction initialization execute. ABED10 alone owns scratch records, stride0x50 and region+38.',
            'Cell prior has no actors/occupation. Placement+117C is0, cursor coordinate−32768/−32768 cannot match this domain; selected temporary88098C startsNULL. Nonnull destructor and cursor/Tactical branches are excluded.',
            'Explicit RMG FPCW0E7F component prerequisite. All three streams seeded by original65C6D0; optional declared independent early XOR-lane constraints verified by original65C780, not native Generate-produced RNG history.',
            'No raster/frame/native wholeGenerate equivalence; consumers must compare only recorded controls and separately prove normal pipeline admission.'],
        substitutions=[
            'Existing PaletteReader bump allocation, no-op CRT free and TLS storage; original code bytes unchanged and every hooked return admitted only at those boundaries during controls.',
            'Existing NativeAudioPlatform immutable physical RawFile transport during TMP preparation only; no Map/Cell/mask/admission/placement/stamp/RNG substitute returns.'],
        entry_points={name: address for name, address in ENTRY.items()} | dict(
            raise_pass=0x579010, anchors=0x579320, unconditional_stamp=0x5A6C10,
            scratch_constructor=0x58BDC0, seed=0x65C6D0, raw=0x65C780))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
