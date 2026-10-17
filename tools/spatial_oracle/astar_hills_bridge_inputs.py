"""Native Hills high-bridge Cell inputs and concrete MTNK admission.

Physical map fields, original scalar/type readers, Cell and IsoTileType
constructors, relocated physical TMPs and original Recalc establish the terrain.
The existing Overlay constructor owner executes original Mark and bridge
setters. Native archive/map loading, world-object placement, final map passes,
connectivity/hierarchy, ordinary Move and A* are excluded.
"""
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_ESI

from tools.native_oracle import NATIVE_SHA256, provenance, run_checked
from tools.projectile_oracle.bridge_render_inputs import assets_root
from tools.spatial_oracle import astar_mtnk_inputs, bridge_constructor, map_queries
from tools.spatial_oracle.astar_mtnk_inputs import RecordedInputs, type_state
from tools.spatial_oracle.anytown_damage import navigation, navigation_inputs, next_family_native
from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
from tools.spatial_oracle.shrapnel_repair import shrapnel_repair as sr


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def build():
    """Return the reusable native VM plus its independent physical producers."""
    map_file = assets_root() / 'Hills.map'
    raw = map_file.read_bytes()
    sections, physical = decode_cells(raw)
    fields = {k: v for k, v, _line in sections['Map']}
    size = [int(v) for v in fields['Size'].split(',')[2:]]
    local = [int(v) for v in fields['LocalSize'].split(',')]
    theater = next_family_native.theater()
    reader = RecordedInputs(theater, map_file)
    tiles, assets = navigation_inputs.extract_tiles(theater)
    case = dict(start=[75, 75], impact=[76, 75], cells=[], supplied_cells=[],
                size=size, local_size=local,
                bridge_base=theater['globals'][0xAA0E28],
                wood_base=theater['globals'][0xABAD1C],
                rim_keys={k: theater['globals'][a] for k, a in sr.GLOBALS.items()},
                supplied_rng={k: next_family_native.seed_state() for k in ('main', 'scenario', 'mapgen')},
                map_sha256=sha(raw))
    native_map = navigation.Navigation(reader, theater, tiles, case=case,
                                       create_actor=False, cell_inputs_only=True)
    native_map.physical_receipt = dict(file=map_file.name, sha256=sha(raw), bytes=len(raw),
                                       size=size, local_size=local,
                                       normalized_local_size=native_map.case['local_size'],
                                       physical_cells=len(physical))
    native_map.activity = 'overlay_inputs'
    u = native_map.uc
    bridge_constructor.OriginalBridgeConstructor.empty_registries(u, native_map.allocate)
    native_map.call(0x5FC310)
    native_map.call(0x49F2F0)
    u.mem_write(0x87F5D8, map_queries.dwords(0x7E17CC))
    u.mem_write(0xA8E9A0, b'\x01')
    return native_map, reader, theater, assets


def rng(m):
    return {name: sr.rng_state(m.uc, p) for name, p in m.rngs.items()}


def archive_activation_evidence(u):
    """Instruction/data evidence only; the archive constructor is not run."""
    string = lambda address: bytes(u.mem_read(address, 16)).split(b'\0')[0].decode('ascii')
    formats = [string(a) for a in (0x827D64, 0x827D58)]
    assert formats == ['%s.MIX', '%sMD.MIX']
    names = []
    for index, expected in ((0, ('TEMPERAT', 'TEM', 'ISOTEM', 'ISOTEMP')),
                            (1, ('SNOW', 'SNO', 'ISOSNO', 'ISOSNOW'))):
        values = tuple(string(a + index * 0x70) for a in (0x7E1BA8, 0x7E1BC6, 0x7E1BBC, 0x7E1BB2))
        assert values == expected
        names.append(dict(index=index, prefixes=list(values)))
    calls = []
    for pc in (0x534AC0, 0x534B00, 0x534B3C, 0x534B94, 0x534BD0):
        raw = bytes(u.mem_read(pc, 5))
        assert raw[0] == 0xE8 and pc + 5 + struct.unpack('<i', raw[1:])[0] == 0x5B3C20
        calls.append(dict(pc=f'{pc:08x}', bytes=raw.hex(), target='005b3c20'))
    return dict(evidence_level='original instruction and constant-table reading; activation not executed',
                body='005349c0', body_sha256=sha(bytes(u.mem_read(0x5349C0, 0x410))),
                table='007e1ba8', formats=formats, names=names, registered_constructor_calls=calls)


def establish():
    """Return the established native VM and its receipt for downstream callers."""
    m, reader, theater, assets = build()
    u = m.uc
    selected = [(x, y) for y in range(73, 78) for x in range(73, 82)]
    def cell(p):
        return dict(m.snapshot(p), ground_bits=sr.u32(u, p + 0x124),
                    deck_bits=sr.u32(u, p + 0x128),
                    ground_owner=sr.i32(u, p + 0x54), deck_owner=sr.i32(u, p + 0x58),
                    deck_head=sr.u32(u, p + 0xE8))
    snapshot = lambda: [cell(m.ptrs[c]) for c in selected]
    before = snapshot()
    events = []
    active = None

    def observe(_u, pc, _size, _data):
        if pc not in (0x5FC380, 0x5FC570, 0x47E040, 0x47E470, 0x47D2B0):
            return
        this = u.reg_read(UC_X86_REG_ECX)
        sp = u.reg_read(UC_X86_REG_ESP)
        row = dict(source_coord=active, pc=f'{pc:08x}')
        if pc in (0x47E040, 0x47E470, 0x47D2B0):
            row['coord'] = m.coord(this)
            row['arg1'] = sr.i32(u, sp + 4)
        if pc in (0x47E040, 0x47E470):
            row['set'] = sr.u32(u, sp + 8)
        events.append(row)

    u.hook_add(UC_HOOK_CODE, observe)
    rng_before = rng(m)
    stamp_receipts = []
    # The original reader traverses overlay source cells y-major then x-major.
    # Reuse that order for every high bridge source in the physical map. This
    # does not execute the packed overlay reader or its membership admission.
    sources = sorted((c for c, v in m.physical.items() if v['overlay'] in (24, 25, 237, 238)),
                     key=lambda c: (c[1], c[0]))
    for c in sources:
        active = list(c)
        physical = m.physical[c]
        p = m.ptrs[c]
        fixture = bridge_constructor.OriginalBridgeConstructor.on_map(m, reader.overlay_ptrs[physical['overlay']], c)
        old = cell(p)
        mark = len(events)
        fixture.construct()
        # Original ReadMapOverlayPacks5FD4F3..5FD502 restores the retained
        # authored frame. Supply only its admitted caller frame and BL byte;
        # original Map lookup and Cell store execute.
        bridge_constructor.OriginalBridgeConstructor.restore_bridge_data(m,c,physical['frame'])
        stamp_receipts.append(dict(physical=dict(coord=list(c), **physical),
                                   before=old, after=cell(p), events=events[mark:]))
    after = snapshot()
    rng_after = rng(m)
    assert rng_before == rng_after
    assert sha(bytes(u.mem_read(0x401000, 0x3E0000))) == m.code_hash
    m.activity = 'actor_inputs'
    m.actor_coord, m.actor_height = (75, 75), 6
    actor_rng = rng(m)
    m.build_actor(reader.mtnk)
    after_actor = rng(m)
    # Execute the original admitted Unlimbo coefficient-copy slice used by the
    # MTNK prerequisite receipt, rather than supplying the scalar to Foot.
    u.reg_write(UC_X86_REG_ESP, navigation.STACK_BASE + navigation.STACK_SIZE - 0x1000)
    u.reg_write(UC_X86_REG_ESI, m.actor)
    run_checked(u, 0x4D72E0, 0x4D72FA, count=1000,
                required_addresses=(0x6F3270, 0x4D72EA, 0x4D72F4))
    assert bytes(u.mem_read(m.actor + 0x530, 8)) == bytes(u.mem_read(reader.mtnk + 0x2F0, 8))
    admissions = []
    fn = sr.u32(u, sr.u32(u, m.actor) + 0x1AC)
    assert fn == 0x73F0A0
    for c in ((74, 75), (75, 75), (76, 75), (77, 75), (78, 75), (79, 75), (80, 75), (78, 74), (78, 76)):
        for height in (2, 6):
            returned = m.call(fn, this=m.actor, args=(m.ptrs[c], 2, height, 0, 1), count=300000)
            admissions.append(dict(coord=list(c), direction=2, height=height,
                                   previous_cell=None, arg5=1, result=returned))
    assert after_actor == rng(m)
    assert sha(bytes(u.mem_read(0x401000, 0x3E0000))) == m.code_hash
    lookups = []
    for c in ((75, 75), (76, 75), (78, 75), (-437, 76), (-1, 75), (511, 511)):
        u.mem_write(navigation.COORD, map_queries.packed(*c))
        pointer = m.call(0x5657A0, args=(navigation.COORD,), count=1000)
        actual = next((list(coord) for coord, p in m.ptrs.items() if p == pointer), None)
        if c in m.ptrs:
            assert pointer == m.ptrs[c]
        lookups.append(dict(requested=list(c), real_cell=actual,
                            returns_dummy=pointer == navigation.DUMMY,
                            retained_coord=m.coord(pointer)))
    assert after_actor == rng(m)
    receipt = dict(schema_version=1, native_sha256=NATIVE_SHA256, scope=__doc__,
                physical=m.physical_receipt, theater=theater, assets=assets,
                archive_activation_evidence=archive_activation_evidence(u), native_map_lookups=lookups,
                native_type=type_state(reader), scalar_layers=reader.receipts,
                cell_startup=m.cell_startup, bootstrap=m.bootstrap,
                initial_recalc=m.sweeps, initial_recalc_rng=m.cell_inputs_rng,
                before_overlay=before, after_overlay=after, stamps=stamp_receipts,
                rng_before_overlay=rng_before, rng_after_overlay=rng_after,
                rng_before_actor=actor_rng, rng_after_actor=after_actor,
                actor_coefficient_f64_bits=bytes(u.mem_read(m.actor + 0x530, 8)).hex(),
                movement_admissions=admissions,
                text_sha256=m.code_hash, code_unchanged=True,
                native_reached=dict(m.counters))
    return m, receipt


def generate():
    _m, receipt = establish()
    return receipt


def metadata():
    result = provenance(scope=__doc__, entry_points={
        'cell_ctor': 0x47BBF0, 'tile_ctor': 0x5447C0, 'recalc': 0x47D2B0,
        'overlay_ctor': 0x5FC380, 'overlay_mark': 0x5FC570,
        'concrete_setter': 0x47E040, 'wood_setter': 0x47E470,
        'bridge_frame_restore': 0x5FD4F3, 'unit_can_enter': 0x73F0A0,
        'theater_activation_reading': 0x5349C0, 'map_lookup': 0x5657A0},
        assumptions=[
            'Physical Hills MAP tile/subtile/level/ice/overlay/frame and source overlay order are host-decoded; native map/archive loading and OverlayPack membership admission are excluded.',
            'Existing Navigation cell_inputs_only producer executes original Cell/IsoTileType construction and full Recalc sweep against authentic relocated TMPs and native-read rules/type/theater fields. Registered long/short and isometric theater archives are checked for unique physical winners; primary TMP absence reached by Recalc fails closed. Terrain/world objects, final initialization, navigation graphs and actor placement are excluded.',
            'Existing OriginalBridgeConstructor owner is bound to the native map VM, with native-read OverlayTypes and supplied empty registry capacity. Every authored high bridge executes original construction, Mark and its chosen setter. The original loader-side authored frame restoration slice executes on a supplied admitted frame.',
            'Original Unit/Foot constructor prefix and Drive constructor create the MTNK actor; Navigation supplies its admitted owner/live/movement state and coordinates. The original Unlimbo coefficient copy executes; whole Unlimbo and ordinary Move/A* are excluded.',
            'Main, Scenario and MapGen start from independently native-seeded0 full states. Complete states are preserved before/after Recalc, bridge stamping and actor construction; this is not a scenario RNG initialization claim.'],
        substitutions=[
            'Inherited bounded successful allocation/free, CRT TLS and atexit; physical lexical INI preparation and archive IO are host supplied.',
            'Inherited screen/radar/dirty-rectangle sinks and a recorded waterfall animation constructor boundary; native Cell Recalc, bridge stamping, scalar readers, MTNK constructors and CanEnter returns are not replaced.'])
    return result
