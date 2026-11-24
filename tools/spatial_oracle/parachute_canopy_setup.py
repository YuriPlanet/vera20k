"""Native E1/PARACH admitted-component construction and drawing equipment.

This reuses the shared House/map/LightConvert and Surface owners. Full Paradrop
admission, owner descent and retained Display traversal are outside this fixture.
Sparse map, House, registry, admitted-owner and surface priors and OS/COM
transports are explicit. Original constructors, readers, attachment suffix,
AnimAI and drawing execute unchanged.
"""
from collections import deque
import hashlib
import struct

def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def physical_inputs(game):
    from tools.spatial_oracle.building_anim_translucency_setup import physical_inputs as power_inputs
    from tools.sidebar_oracle.stock import mix, mix_hash
    from tools.projectile_oracle.bridge_render_inputs import lexical
    assets, receipt = power_inputs(game)
    containers = []
    for top, children in (
        ('expandmd01.mix', ()),
        ('ra2md.mix', ('conqmd.mix', 'genermd.mix', 'localmd.mix')),
        ('ra2.mix', ('conquer.mix', 'generic.mix', 'cache.mix')),
    ):
        outer = mix((game / top).read_bytes())
        containers.append((top, outer))
        for child in children:
            raw = outer.get(mix_hash(child))
            if raw is not None:
                containers.append((top + '/' + child, mix(raw)))
    sources = []
    for name in ('PARACH.SHP', 'GI.SHP', 'GIDEPLOY.SHP', 'GI.TEM', 'RULESMD.INI'):
        hits = [(archive, entries[mix_hash(name)]) for archive, entries in containers
                if mix_hash(name) in entries]
        assets[name] = hits[0][1] if hits else None
        sources.append(dict(name=name, found=bool(hits),
            archive=hits[0][0] if hits else None,
            bytes=len(hits[0][1]) if hits else 0,
            sha256=sha(hits[0][1]) if hits else None))
    assert assets['PARACH.SHP'] and assets['GI.SHP'] and assets['RULESMD.INI']
    art, lines = lexical(assets['ARTMD.INI'], {'E1', 'GI', 'GISequence', 'PARACH'})
    assert art['PARACH']['ZAdjust'] == '-10'
    receipt['lexical_art'].update(art)
    rules, rule_lines = lexical(assets['RULESMD.INI'], {'General', 'E1'})
    assert rules['General']['Parachute'] == 'PARACH'
    assert rules['General']['AmerParaDropInf'] == 'E1'
    receipt['parachute'] = dict(sources=sources, art=art, art_lines=lines,
        general_parachute=rules['General']['Parachute'], e1=rules['E1'],
        selected_rule_lines=rule_lines,
        boundary='Enumerated immutable MIX resources and lexical caches, not native archive/INI file traversal. Full physical E1/PARACH sections remain unchanged.')
    return assets, receipt


def prepare_scene(assets, metadata):
    """Reuse the real House renderer VM; register E1 before House counters."""
    from tools.procedural_drawing_oracle import shroud
    from tools.spatial_oracle import tmp_clear_render as tmp
    from tools.spatial_oracle import building_anim_translucency_setup as power
    from tools.spatial_oracle.building_body_rules import dwords
    original = tmp.prepare
    receipt = {}

    def with_infantry_type(*args, **kwargs):
        prepared = original(*args, **kwargs)
        m, u = prepared[0], prepared[0].u
        header = 0xA8E348
        before = list(struct.unpack('<6I', u.mem_read(header, 24)))
        assert before[1] == before[2] == before[4] == 0, before
        # The existing Engineer/FootMissions type-owner convention: genuine
        # constructor/vtable, supplied empty registry capacity only.
        u.mem_write(header, dwords(0x7EB6D4, m.alloc(256), 64, 1, 0, 10))
        typ = m.alloc(0x1900)
        m.invoke(0x5236A0, typ, (m.cstring('E1'),))
        m.parach_e1_type = typ
        receipt.update(registry_before=before, registry_after=list(struct.unpack('<6I', u.mem_read(header, 24))),
            constructor='005236A0', type=f'{typ:08X}', vtable=f'{m.read32(typ):08X}',
            array_index=m.invoke(m.read32(m.read32(typ) + 0x40), typ),
            boundary='Original InfantryType constructor before actual House constructor; only empty native-layout registry storage/capacity is supplied.')
        return prepared

    tmp.prepare = with_infantry_type
    try:
        scene = power.prepare_power_plant_scene(shroud, assets, metadata)
    finally:
        tmp.prepare = original
    scene['infantry_type_startup'] = receipt
    return scene


def initialize_draw_startup(scene):
    from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EBX, UC_X86_REG_ESP
    from tools import native_oracle as native
    from tools.spatial_oracle.building_body_rules import SP, dwords
    from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry
    m, u = scene['m'], scene['u']
    geometry = initialize_discovery_geometry(m.invoke, m.read32, u)
    old = scene['tactical']
    tactical = m.alloc(0x1200)
    u.mem_write(tactical, bytes(u.mem_read(old, 0x200)))
    u.reg_write(UC_X86_REG_ESI, tactical)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESP, SP)
    native.run_checked(u, 0x6D1DC5, 0x6D1E24, count=1000)
    assert m.read32(0x887324) == tactical
    u.mem_write(tactical + 0xB0, dwords(-384, 380))
    scene['tactical'] = tactical
    registries = []
    for vector, begin, end, store in (
        (0xA8E9A8, 0x4E6D60, 0x4E6D96, 0x4E6D7D),
        (0xB0F698, 0x725850, 0x725886, 0x72586D),
        (0xA8E360, 0x4E7AE0, 0x4E7B16, 0x4E7AFD),
        (0xB0F720, 0x7252D0, 0x725306, 0x7252ED),
        (0xB0F670, 0x725350, 0x725386, 0x72536D),
        (0xB0F618, 0x7253D0, 0x725406, 0x7253ED),
    ):
        prior = bytes(u.mem_read(vector, 24))
        ran = m.read32(vector) == 0
        if ran:
            assert prior == bytes(24), ('non-pristine missing registry', hex(vector))
            u.mem_write(SP, dwords(native.RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            native.run_checked(u, begin, end, count=100, required_addresses=(store,))
            assert u.reg_read(UC_X86_REG_ESP) == SP - 4
        registries.append(dict(vector=f'{vector:08X}', before=list(struct.unpack('<6I', prior)),
            after=list(struct.unpack('<6I', u.mem_read(vector, 24))),
            executed_prefix=f'{begin:08X}..{end:08X}' if ran else None,
            boundary='Original pristine header initialization stops before CRT exit registration; live registries are retained.'))
    return dict(geometry=geometry, tactical_matrix_prefix=['006D1DC5', '006D1E24'],
        camera=list(struct.unpack('<2i', u.mem_read(tactical + 0xB0, 8))), registries=registries)



def normalize_scene_playfield(scene):
    """Original LocalSize normalization; stop before redraw/Techno traversal."""
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_INVALID
    from unicorn.x86_const import (
        UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
        UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_ESP,
        UC_X86_REG_EIP,
    )
    from tools import native_oracle as native
    from tools.spatial_oracle.building_body_rules import SP, dwords
    m, u = scene['m'], scene['u']
    map_pointer = 0x87F7E8
    supplied_local = [0, 0, 16, 16]
    argument = m.alloc(16)
    u.mem_write(argument, dwords(*supplied_local))
    registers = (
        ('EAX', UC_X86_REG_EAX), ('EBX', UC_X86_REG_EBX),
        ('ECX', UC_X86_REG_ECX), ('EDX', UC_X86_REG_EDX),
        ('ESI', UC_X86_REG_ESI), ('EDI', UC_X86_REG_EDI),
        ('EBP', UC_X86_REG_EBP), ('ESP', UC_X86_REG_ESP),
        ('EIP', UC_X86_REG_EIP),
    )
    def register_state():
        return {name: f'{u.reg_read(register):08X}' for name, register in registers}
    def map_state():
        return dict(map_rect_ec=list(struct.unpack('<4i', u.mem_read(map_pointer + 0xEC, 16))),
            normalized_local_fc=list(struct.unpack('<4i', u.mem_read(map_pointer + 0xFC, 16))),
            cell_table=f'{m.read32(map_pointer + 0x13C):08X}',
            cell_table_capacity=m.read32(map_pointer + 0x140))
    receipt = dict(entry='00567230', stop_before='005672D3', clip_rect='00421B60',
        map_pointer=f'{map_pointer:08X}', supplied_local=supplied_local,
        argument_pointer=f'{argument:08X}', before=map_state(), reached=[], fault=None,
        boundary='Declared sparse flat Map Size16x16 with full LocalSize input(0,0,16,16). Original567230 including421B60 writes normalized visible bounds; stop5672D3 before redraw/Techno traversal. Existing map size, cell table, cells and camera are unchanged. No playfield/admission answer is supplied.')
    m.parach_map_normalization = receipt
    assert receipt['before']['map_rect_ec'][2:] == [16, 16], receipt['before']
    original_text = bytes(u.mem_read(0x401000, 0x3E0000))
    def observe(_u, pc, _size, _data):
        if pc in (0x567230, 0x421B60, 0x5672CD, 0x5672D3):
            receipt['reached'].append(f'{pc:08X}')
        if pc == 0x567230:
            receipt['registers_at_entry'] = register_state()
    def invalid(_u, access, address, size, value, _data):
        receipt['fault'] = dict(access=access, address=f'{address:08X}', size=size,
            registers=register_state())
        return False
    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_INVALID, invalid)]
    try:
        u.mem_write(SP, dwords(native.RET_MAGIC, argument))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, map_pointer)
        receipt['registers_pre_call'] = register_state()
        native.run_checked(u, 0x567230, 0x5672D3, count=1000,
            required_addresses=(0x421B60, 0x5672CD))
        receipt['registers_at_stop'] = register_state()
        receipt['after'] = map_state()
        receipt['argument_after'] = list(struct.unpack('<4i', u.mem_read(argument, 16)))
        assert receipt['argument_after'] == supplied_local
        for key in ('map_rect_ec', 'cell_table', 'cell_table_capacity'):
            assert receipt['after'][key] == receipt['before'][key], (key, receipt)
        assert bytes(u.mem_read(0x401000, 0x3E0000)) == original_text
        receipt['original_text_unchanged'] = True
        return receipt
    except Exception as error:
        receipt['registers_at_failure'] = register_state()
        receipt['after'] = map_state()
        error.native_map_normalization_failure = receipt
        raise
    finally:
        for hook in hooks:
            u.hook_del(hook)


def relabel_canopy_draw_provenance(row):
    """Correct only the reused helper's slot-specific explanatory string."""
    old = ('Observed actual slot451890 palette/light stores and draw selector pointers; '
           '+CC is recorded only as attached-object state, not treated as palette ownership.')
    new = ('Observed actual ObjectParadrop5F5A9D..5F5B3E canopy attachment stores: '
           'owner remap to Anim+D4 and owner Cell+10A light to Anim+FC; '
           'the selected converter and draw intensity are read back at the real selector. '
           'No Building451890 slot producer is claimed.')
    changed = 0
    def visit(value):
        nonlocal changed
        if isinstance(value, dict):
            for key, item in value.items():
                if key == 'boundary' and item == old:
                    value[key] = new
                    changed += 1
                else:
                    visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)
    visit(row)
    row['canopy_provenance_strings_corrected'] = changed
    return row



def draw_canopy_depth_control(scene, anim, image, label, depth_prior):
    """Supply a complete scene-wide Z input before the unchanged whole draw."""
    from tools.projectile_oracle import bridge_render_shape as shape
    from tools.spatial_oracle.building_anim_translucency import draw_slot
    assert depth_prior in ('empty', 'left_half_zero')
    original = shape.prepare_surface
    setup_receipts = []
    expected_z = [0 if depth_prior == 'left_half_zero' and x < 80 else 65535
                  for y in range(160) for x in range(160)]
    expected_raw = struct.pack('<25600H', *expected_z)
    def prepare_surface_with_declared_depth(u, case, **kwargs):
        assert tuple(kwargs.get('size', ())) == (160, 160)
        assert not case.get('z_patches') and not case.get('a_patches')
        supplied = dict(case)
        if depth_prior == 'left_half_zero':
            supplied['z_patches'] = [(y * 160 + x, 0)
                                     for y in range(160) for x in range(80)]
        result = original(u, supplied, **kwargs)
        actual = bytes(u.mem_read(shape.Z, 160 * 160 * 2))
        assert actual == expected_raw
        setup_receipts.append(dict(size=[160,160], depth_baseline=case['depth_baseline'],
            color565=case['background'], alpha=case['a_value'],
            z_prior=depth_prior, readback_matches_declared_input=True,
            boundary='Complete viewport input chosen before drawing: x<80 has Z0 and x>=80 has Z65535 for left_half_zero; empty has Z65535 everywhere. No source frame, projected point, crop or measured pixel result is used to choose this prior.'))
        return result
    shape.prepare_surface = prepare_surface_with_declared_depth
    try:
        row = draw_slot(scene, anim, image, label,
            dict(initial_z=65535, a_value=127, background=0xBDF7))
    finally:
        shape.prepare_surface = original
    assert len(setup_receipts) == 1
    # The original whole-draw helper reads its complete before-plane after our
    # input transport and before any DrawIfVisible/DrawIt instruction executes.
    decoded = b''.join(struct.pack('<H', word) * count
                       for count, word in row['before_planes']['z_words_rle'])
    assert decoded == expected_raw
    row['surface_input'] = setup_receipts[0]
    row['underlay'] = ('Supplied uniform color565 BDF7/A127, empty Z65535' if depth_prior == 'empty'
        else 'Supplied uniform color565 BDF7/A127; scene-wide Z0 left half(x<80), Z65535 right half')
    row['source_boundary'] = ('Original ObjectParadrop suffix-created physical PARACH; unchanged whole422C70/422CA0/Surface selector and raster. '
        'Host wrapper changes only the supplied pre-draw Z surface through the existing prepare_surface transport; no original instruction, getter, draw flags or result is changed.')
    row['camera'] = list(struct.unpack('<2i', scene['u'].mem_read(scene['tactical'] + 0xB0, 8)))
    return relabel_canopy_draw_provenance(row)


def state(m, actor, anim=0):
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    from tools.spatial_oracle.building_anim_translucency import converter_identity
    u = m.u
    def coord(pointer):
        out = m.alloc(12)
        target = m.read32(m.read32(pointer) + 0x48)
        result = m.invoke(target, pointer, (out,))
        assert result == out
        return dict(entry=f'{target:08X}', value=list(struct.unpack('<3i', u.mem_read(out, 12))))
    def one(pointer):
        return dict(pointer=f'{pointer:08X}', vtable=f'{m.read32(pointer):08X}',
            native_id=m.read32(pointer + 8), stored_xyz=list(struct.unpack('<3i', u.mem_read(pointer + 0x9C, 12))),
            actual_get_coords=coord(pointer), alive=bool(u.mem_read(pointer + 0x90, 1)[0]),
            limbo=bool(u.mem_read(pointer + 0x81, 1)[0]), marked=bool(u.mem_read(pointer + 0x74, 1)[0]),
            retained_display_layer=m.read32(pointer + 0x94),
            actual_get_layer=dict(entry=f'{m.read32(m.read32(pointer) + 0x78):08X}',
                value=m.invoke(m.read32(m.read32(pointer) + 0x78), pointer)))
    result = dict(owner=one(actor))
    result['owner'].update(canopy=f'{m.read32(actor + 0x88):08X}',
        house=f'{m.read32(actor + 0x21C):08X}', type=f'{m.read32(actor + 0x6C0):08X}',
        health=struct.unpack('<i', u.mem_read(actor + 0x6C, 4))[0],
        is_parachuted=bool(u.mem_read(actor + 0x84, 1)[0]),
        in_air=bool(u.mem_read(actor + 0x8D, 1)[0]))
    if anim:
        typ = m.read32(anim + 0xC8)
        result['anim'] = dict(one(anim), constructor_state=constructor_state(u, anim),
            owner_object=f'{m.read32(anim + 0xCC):08X}',
            light_intensity_fc=struct.unpack('<i', u.mem_read(anim + 0xFC, 4))[0],
            runtime_z_adjust_100=struct.unpack('<i', u.mem_read(anim + 0x100, 4))[0],
            type_z_adjust_348=struct.unpack('<i', u.mem_read(typ + 0x348, 4))[0],
            type_y_draw_offset_344=struct.unpack('<i', u.mem_read(typ + 0x344, 4))[0],
            drawer_d4=converter_identity(m, m.read32(anim + 0xD4)))
    return result


def read_e1_weapon_dependencies(m, typ, physical_rule_layers):
    """Complete the actual E1 normal/elite child-reader prerequisite.

    Same reader order as FootMissions.weapon_reader_receipts, before Infantry
    construction. Native slot getters select identities; lexical reference
    names only delimit the unchanged physical cache, never initialize fields.
    """
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.spatial_oracle.building_body_rules import RULES
    u = m.u
    slots, pointers = [], []
    for tier, entry in (('normal', 0x7177C0), ('elite', 0x7177E0)):
        for index in (0, 1):
            slot = m.invoke(entry, typ, (index,))
            pointer = m.read32(slot)
            assert pointer, ('Physical E1 weapon slot missing', tier, index)
            slots.append(dict(tier=tier, index=index, getter=f'{entry:08X}',
                slot=f'{slot:08X}', pointer=f'{pointer:08X}', name=m.string(pointer + 0x24)))
            if pointer not in pointers:
                pointers.append(pointer)
    names = {m.string(pointer + 0x24) for pointer in pointers}
    weapon_names = set(names)
    for _name, _archive, raw in physical_rule_layers:
        if raw is None:
            continue
        weapons, _ = lexical(raw, weapon_names)
        for keys in weapons.values():
            for key in ('Projectile', 'Warhead'):
                if keys.get(key):
                    names.add(keys[key])
    result = dict(slots=slots, selected_cache_sections=sorted(names), layers=[],
        reused_owner='tools/spatial_oracle/anytown_damage/foot_missions.py::FootMissions.weapon_reader_receipts',
        boundary='Whole original selected E1 WeaponType772080, actual referenced Projectile46BEE0/Warhead75D3A0, then Weapon7729F0 postpass. This completes constructor prerequisites, not a FireAt or weapon simulation claim.')
    scenario = m.read32(0xA8B230)
    for name, archive, raw in physical_rule_layers:
        if raw is None:
            result['layers'].append(dict(file=name, archive=archive, absent=True))
            continue
        sections, lines = lexical(raw, names)
        m.rules_cache(sections)
        rng_before = bytes(u.mem_read(scenario + 0x218, 0x3F4))
        reads = []
        layer = dict(file=name, archive=archive, sha256=sha(raw), sections=sections,
            lexical_lines=lines, reads=reads)
        result['layers'].append(layer)
        # Keep the in-progress receipt attached so a new real dependency fault
        # does not erase which original child reader was being attempted.
        m.parach_weapon_dependencies = result
        for pointer in pointers:
            row = dict(weapon=dict(entry='00772080', pointer=f'{pointer:08X}',
                name=m.string(pointer + 0x24)))
            reads.append(row)
            row['weapon']['returned_al'] = m.invoke(0x772080, pointer, (RULES,)) & 255
            projectile, warhead = m.read32(pointer + 0xA0), m.read32(pointer + 0xAC)
            for key, child, entry in (('projectile', projectile, 0x46BEE0),
                                      ('warhead', warhead, 0x75D3A0)):
                row[key] = None if not child else dict(entry=f'{entry:08X}',
                    pointer=f'{child:08X}', name=m.string(child + 0x24))
                if child:
                    row[key]['returned_al'] = m.invoke(entry, child, (RULES,)) & 255
            row['postpass'] = dict(entry='007729F0')
            m.invoke(0x7729F0, pointer)
            row['postpass']['completed'] = True
            assert projectile and warhead, ('Incomplete physical E1 weapon children', row)
            row['after'] = dict(projectile=f'{m.read32(pointer + 0xA0):08X}',
                warhead=f'{m.read32(pointer + 0xAC):08X}',
                warhead_byte_155=u.mem_read(warhead + 0x155, 1)[0],
                range_leptons=struct.unpack('<i', u.mem_read(pointer + 0xB4, 4))[0],
                damage=struct.unpack('<i', u.mem_read(pointer + 0xA4, 4))[0])
        rng_after = bytes(u.mem_read(scenario + 0x218, 0x3F4))
        layer.update(scenario_rng_before_sha256=sha(rng_before),
            scenario_rng_after_sha256=sha(rng_after), rng_unchanged=rng_before == rng_after)
        assert rng_before == rng_after
    return result


def execute(assets, metadata, stage, attachment, owner_height=104):
    from types import SimpleNamespace
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_INVALID
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_EIP
    from tools import native_oracle as native
    from tools.spatial_oracle.building_body_rules import INI, RULES, SP, dwords
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.spatial_oracle.infantry_deploy_action import Fixture as Atomics
    from tools.spatial_oracle.building_anim_translucency import prepare_performance, draw_slot

    assert owner_height in (104, 232)
    assert stage != 'draw-controls' or attachment == 'suffix'
    scene = prepare_scene(assets, metadata)
    m, u = scene['m'], scene['u']
    startup = initialize_draw_startup(scene)
    map_normalization = normalize_scene_playfield(scene)
    # Match the successful component draw owner's declared live-game clock.
    # No global Main_Tick or frame scheduler is claimed by these input stores.
    u.mem_write(0xA8E9A0, b'\1')
    u.mem_write(0xA8E7AC, dwords(0))
    u.mem_write(0xA8ED84, dwords(1000))
    u.mem_write(0xA8EB60, dwords(4))
    typ, house = m.parach_e1_type, scene['house']
    code_before = bytes(u.mem_read(0x401000, 0x3E0000))
    vtables = {p:bytes(u.mem_read(p, size)) for p,size in (
        (0x7EB058,0x600),(0x7EB610,0xB0),(0x7E3354,0x210),
        (0x7E3608,0xA0),(0x7F4348,0x100),(0x7E2070,0x100))}
    phase, recent, calls, platform, sinks = 'type_readers', deque(maxlen=64), [], [], []
    fault, rows, layers, actor, anim = None, [], [], 0, 0
    playfield_returns = []
    ai_history = []
    ai_visits = 0
    original_atomics = bytes(u.mem_read(0x7E11C8, 8))
    atomic = SimpleNamespace(u=u, imports=[m.alloc(16),m.alloc(16)], read=m.read32)
    u.mem_write(0x7E11C8, dwords(*atomic.imports))
    watched = {0x5236A0,0x5240A0,0x517A50,0x6C4790,0x75AA90,
        0x7177C0,0x7177E0,0x772080,0x46BEE0,0x75D3A0,0x7729F0,
        0x5785F0,0x578460,0x51DFF0,0x4DB810,
        0x5F5940,0x5F5A9D,0x421EA0,0x424B50,0x422BE0,0x41BE00,
        0x423AC0,0x422C70,0x5F4B10,0x422CA0,0x4AED70}
    def observe(_u, pc, _size, _data):
        recent.append(f'{pc:08X}')
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x5F5957:
            playfield_returns.append(dict(pc='005F5957',
                returned_eax=u.reg_read(UC_X86_REG_EAX),
                returned_al=u.reg_read(UC_X86_REG_EAX) & 255,
                map_visible_rect=list(struct.unpack('<4i', u.mem_read(0x87F7E8 + 0xFC, 16)))))
        if pc in watched:
            row = dict(phase=phase, pc=f'{pc:08X}', this=f'{u.reg_read(UC_X86_REG_ECX):08X}', caller=f'{m.read32(sp):08X}')
            if pc == 0x421EA0:
                args = list(struct.unpack('<7I', u.mem_read(sp + 4, 28)))
                row.update(args=args, constructor_coord=list(struct.unpack('<3i', u.mem_read(args[1], 12))),
                    constructor_z_adjust=args[5], constructor_draw_flags=args[4])
            calls.append(row)
        if pc in atomic.imports:
            platform.append(dict(phase=phase, kind='OS_Interlocked', caller=f'{m.read32(sp):08X}'))
            Atomics.observe(atomic, _u, pc, _size, _data)
            return
        if m.guid_transport(u, pc, sp, platform):
            return
        if pc == 0x517B74:
            clsid, outer, context, iid, output = struct.unpack('<5I', u.mem_read(sp, 20))
            assert (outer, context) == (0, 7)
            assert bytes(u.mem_read(clsid, 16)) == bytes(u.mem_read(typ + 0x34C, 16))
            platform.append(dict(phase=phase, kind='COM_to_original_Walk_factory', factory='006C4790',
                clsid=bytes(u.mem_read(clsid,16)).hex(), iid=bytes(u.mem_read(iid,16)).hex()))
            u.mem_write(sp,dwords(pc+6,0,outer,iid,output))
            u.reg_write(UC_X86_REG_EIP,0x6C4790)
            return
        if pc == 0x517B83:
            platform.append(dict(phase=phase,kind='OS_OleRun_success'))
            u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,pc+6)
            return
        if pc in (0x7C978A,0x6C8C40) and phase in ('type_readers','infantry_constructor'):
            platform.append(dict(phase=phase,kind='CRT_exit_registration' if pc==0x7C978A else 'setup_wall_clock',caller=f'{m.read32(sp):08X}'))
            m.ret(0)
            return
        if pc in (0x5F5850,0x4A9720):
            sinks.append(dict(phase=phase,entry=f'{pc:08X}',caller=f'{m.read32(sp):08X}',this=f'{u.reg_read(UC_X86_REG_ECX):08X}',argument=m.read32(sp+4)))
            m.ret(1,4)
    def invalid(_u, access, address, size, value, _data):
        nonlocal fault
        fault=dict(phase=phase,access=access,address=f'{address:08X}',size=size,
            eip=f'{u.reg_read(UC_X86_REG_EIP):08X}',recent=list(recent),
            registers={name:f'{u.reg_read(register):08X}' for name,register in (
                ('EAX',UC_X86_REG_EAX),('EBX',UC_X86_REG_EBX),('ECX',UC_X86_REG_ECX),
                ('EDX',UC_X86_REG_EDX),('ESI',UC_X86_REG_ESI),('EDI',UC_X86_REG_EDI),
                ('EBP',UC_X86_REG_EBP),('ESP',UC_X86_REG_ESP))})
        return False
    hooks=[u.hook_add(UC_HOOK_CODE,observe),u.hook_add(UC_HOOK_MEM_INVALID,invalid)]
    try:
        m.make_ini(metadata['lexical_art'])
        physical_rule_layers = physical_layers()
        for name, archive, raw in physical_rule_layers:
            if raw is None:
                layers.append(dict(file=name,archive=archive,absent=True));continue
            selected,_=lexical(raw,{'E1'})
            m.rules_cache(selected)
            mark=len(m.asset_loaded)
            admitted=m.invoke(0x5240A0,typ,(RULES,)) & 255
            layers.append(dict(file=name,archive=archive,sha256=sha(raw),read_admitted=bool(admitted),
                e1_keys=selected.get('E1'),image=m.string(typ+0x1F8),strength=m.read32(typ+0xA0),
                locomotor_guid=bytes(u.mem_read(typ+0x34C,16)).hex(),loads=m.asset_loaded[mark:]))
        weapon_dependencies = read_e1_weapon_dependencies(m, typ, physical_rule_layers)
        # Exact General Parachute ReadString/FindOrAllocate/store block. Its
        # earlier unrelated read and wider General initialization are omitted.
        m.rules_cache({'General':{'Parachute':metadata['parachute']['general_parachute']}})
        u.mem_write(SP-4,dwords(0x80))
        for reg,value in ((UC_X86_REG_ESP,SP-4),(UC_X86_REG_ESI,m.rules),(UC_X86_REG_EDI,RULES),(UC_X86_REG_EDX,SP+0x50)):
            u.reg_write(reg,value)
        native.run_checked(u,0x66E561,0x66E59F,count=100000,
            required_addresses=(0x528A10,0x428B80,0x66E599))
        assert u.reg_read(UC_X86_REG_ESP)==SP-4
        anim_type=m.read32(m.rules+0xBBC)
        assert m.string(anim_type+0x24)=='PARACH'
        mark=len(m.asset_loaded)
        admitted=m.invoke(0x427D00,anim_type,(INI,))
        anim_reader=m.result('PARACH',anim_type,admitted)
        anim_reader.update(physical_art_keys=metadata['lexical_art']['PARACH'],loads=m.asset_loaded[mark:],
            type_z_adjust_348=struct.unpack('<i',u.mem_read(anim_type+0x348,4))[0],
            type_y_draw_offset_344=struct.unpack('<i',u.mem_read(anim_type+0x344,4))[0])
        image=m.read32(anim_type+0xA4)
        assert image and bytes(u.mem_read(image,len(assets['PARACH.SHP'])))==assets['PARACH.SHP']
        assert not anim_reader['shadow']
        header=0xA83DE8
        prior=list(struct.unpack('<6I',u.mem_read(header,24)))
        assert prior[1]==prior[2]==prior[4]==0,prior
        u.mem_write(header,dwords(0x7EB6D4,m.alloc(256),64,1,0,10))
        phase='infantry_constructor'
        actor=m.alloc(0x1000)
        m.invoke(0x517A50,actor,(typ,house),instruction_limit=4000000)
        assert m.read32(actor)==0x7EB058
        assert m.read32(actor+0x21C)==house and m.read32(actor+0x674)
        coord=m.alloc(12);u.mem_write(coord,dwords(2688,5248,owner_height))
        original_ctor=state(m,actor)
        phase='attachment'
        if attachment=='whole':
            result=m.invoke(0x5F5940,actor,(coord,),instruction_limit=10000000,timeout_us=120000000)
            assert result & 255, 'Original whole Paradrop admission refused; do not force the suffix'
            attachment_boundary='Whole ObjectParadrop5F5940 on real constructed E1; upstream Aircraft/InfantrySpawnParachuted omitted.'
        else:
            # Explicit already-admitted prior. Do not call this an observed
            # Unlimbo/SetLocation result. Owner virtuals remain original.
            u.mem_write(actor+0x9C,bytes(u.mem_read(coord,12)))
            u.mem_write(actor+0x74,b'\1');u.mem_write(actor+0x80,b'\1\0')
            u.mem_write(actor+0x8D,b'\1')
            u.mem_write(SP,dwords(0,0,0,0,0,0,native.RET_MAGIC,coord))
            for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,actor),(UC_X86_REG_EDI,coord)):
                u.reg_write(reg,value)
            native.run_checked(u,0x5F5A9D,native.RET_MAGIC,count=4000000,timeout_us=120000000,
                required_addresses=(0x421EA0,0x424B50,0x5F5B13,0x5F5B23))
            assert u.reg_read(UC_X86_REG_ESP)==SP+32
            attachment_boundary=f'Supplied successful airborne placement prior: Location(2688,5248,{owner_height}), marked/isDown, nonlimbo and inAir+8D. Exact original5F5A9D..5F5B3E suffix and saved-register/local stack; full5F5940 map/Unlimbo/SetLocation prefix omitted. Height is a declared fresh-VM input, not observed owner-AI descent.'
        anim=m.read32(actor+0x88)
        assert anim and m.read32(anim)==0x7E3354 and m.read32(anim+0xC8)==anim_type
        after_attach=state(m,actor,anim)
        assert after_attach['anim']['constructor_state']['runtime']['current_frame']==0
        owner_virtuals={f'{off:03X}':f'{m.read32(m.read32(actor)+off):08X}'
                        for off in (0x48,0x78,0xD8,0x1B4,0x1BC,0x1E4)}
        cell=m.invoke(m.read32(m.read32(actor)+0x1BC),actor)
        cell_light=struct.unpack('<h',u.mem_read(cell+0x10A,2))[0]
        performance=None
        if stage in ('first-draw', 'draw-controls'):
            phase='performance_setup'
            # This helper owns its own caller-checked atexit/time transport;
            # avoid the broad setup-only handler above during this phase.
            performance=prepare_performance(m)
            if stage == 'first-draw':
                phase='first_draw'
                row=draw_slot(scene,anim,image,f'frame0_height{owner_height}_empty_z',
                    dict(initial_z=65535,a_value=127,background=0xBDF7))
                row['camera']=list(struct.unpack('<2i',u.mem_read(scene['tactical']+0xB0,8)))
                row['source_boundary']='Generic helper whole422C70/Surface raster observation reused for physical PARACH; no451890 slot producer is claimed.'
                rows.append(relabel_canopy_draw_provenance(row))
            else:
                from tools.spatial_oracle.anim_bouncer_launch import constructor_state
                scenario = m.read32(0xA8B230)
                def ai_state():
                    snapshot = constructor_state(u, anim)
                    return dict(runtime=snapshot['runtime'], alive=snapshot['alive'],
                        limbo=snapshot['limbo'], marked=snapshot['marked'],
                        owner_object=f'{m.read32(anim + 0xCC):08X}',
                        owner_stored_xyz=list(struct.unpack('<3i',u.mem_read(actor + 0x9C,12))),
                        anim_stored_xyz=list(struct.unpack('<3i',u.mem_read(anim + 0x9C,12))),
                        identity_cursor=m.read32(scenario + 0x214),
                        scenario_rng_sha256=sha(u.mem_read(scenario + 0x218,0x3F4)))
                start_clock = m.read32(0xA8ED84)
                clock = start_clock
                for target_frame in (0, 20):
                    phase=f'original_anim_ai_to_frame{target_frame}'
                    while m.read32(anim + 0xAC) != target_frame:
                        assert ai_visits < 512, ('Natural PARACH frame not reached', target_frame, ai_state())
                        clock += 1
                        u.mem_write(0xA8ED84, dwords(clock))
                        visit=dict(visit=ai_visits+1,clock=clock,target_frame=target_frame,before=ai_state())
                        ai_history.append(visit)
                        m.invoke(0x423AC0,anim,instruction_limit=4000000,timeout_us=120000000)
                        ai_visits += 1
                        visit['after']=ai_state()
                        assert visit['after']['alive'] and not visit['after']['runtime']['inactive'], visit
                        assert visit['after']['owner_object']==f'{actor:08X}',visit
                        assert visit['after']['owner_stored_xyz']==[2688,5248,owner_height],visit
                    for depth_prior in ('empty', 'left_half_zero'):
                        phase=f'draw_frame{target_frame}_height{owner_height}_{depth_prior}'
                        before=state(m,actor,anim)
                        row=draw_canopy_depth_control(scene,anim,image,
                            f'frame{target_frame}_height{owner_height}_{depth_prior}',depth_prior)
                        row.update(clock=clock,ai_visits=ai_visits,requested_frame=target_frame,
                            state_before=before,state_after=state(m,actor,anim),
                            supplied_owner_height=owner_height,
                            owner_motion_boundary='Owner Location is fixed at this fresh-VM admitted input; only original AnimAI runs between frame0 and frame20. No owner descent, body sequence or MainTick is executed.')
                        rows.append(row)
                    assert m.read32(anim + 0xAC)==target_frame
                assert len(rows)==4
        phase='final_observation'
        after_draw=state(m,actor,anim)
        assert code_before==bytes(u.mem_read(0x401000,0x3E0000))
        assert all(raw==bytes(u.mem_read(ptr,len(raw))) for ptr,raw in vtables.items())
        return dict(stage=stage,attachment=attachment,attachment_boundary=attachment_boundary,
            supplied_owner_height=owner_height,ai_history=ai_history,ai_visits=ai_visits,
            physical_inputs=metadata,building_dependencies=scene['building_dependencies'],
            infantry_type_startup=scene['infantry_type_startup'],e1_layers=layers,
            weapon_dependencies=weapon_dependencies,
            e1_runtime_registry_prior=prior,draw_startup=startup,
            map_normalization=map_normalization,playfield_returns=playfield_returns,
            rules_parachute_reader=['0066E561','0066E59F'],anim_reader=anim_reader,
            original_infantry_constructor=original_ctor,after_attachment=after_attach,
            after_draw=after_draw,owner_virtuals=owner_virtuals,
            observed_owner_cell=f'{cell:08X}',observed_cell_light_10a=cell_light,
            performance=performance,draws=rows,calls=calls,platform_boundaries=platform,
            presentation_sinks=sinks,format_mask_startup=m.format_mask_startup,
            checked_original_text_and_selected_vtables_unchanged=True,
            boundary=('Suffix-only draw-controls: each supplied owner height uses a fresh VM. Original AnimAI reaches actual frame20 from constructorframe0 through only global clock advances, bounded512 visits; no frame/timer/rate writes. Each sampled frame receives empty and complete scene-wide split-Z inputs with full before/after planes. Owner height is supplied and fixed, not native descent. ' if stage=='draw-controls' else 'Constructor frame0 only at the declared owner height; no natural loop claim. ') + 'Real owner GetCoords/GetLayer/GetRemap/GetCell and full Anim draw run. Mark/Display registration remain declared presentation sinks, so membership/order are excluded. Unrelated GAPOWR is inherited House/map/drawing setup only and is never drawn. Whole-Paradrop V3 stopped at missing Map base-zone metadata; this suffix draw does not repair or bypass that failure and does not claim full placement.')
    except Exception as error:
        error.native_probe_failure=dict(phase=phase,fault=fault,recent=list(recent),
            actor=f'{actor:08X}',anim=f'{anim:08X}',layers=layers,calls=calls,
            weapon_dependencies=getattr(m,'parach_weapon_dependencies',None),
            platform=platform,presentation_sinks=sinks,completed_draws=rows,
            nested_draw=getattr(error,'native_draw_failure',None),
            nested_performance=getattr(error,'native_performance_failure',None),
            map_normalization=map_normalization,playfield_returns=playfield_returns,
            supplied_owner_height=owner_height,ai_history=ai_history,ai_visits=ai_visits)
        raise
    finally:
        u.mem_write(0x7E11C8,original_atomics)
        for hook in hooks:u.hook_del(hook)
