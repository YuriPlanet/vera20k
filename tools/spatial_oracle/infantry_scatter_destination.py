"""Original Walk Scatter and separate ordinary CLEG Cell destination controls."""
from pathlib import Path
import hashlib
import os
import struct
import sys
from tools.native_oracle import finish_vectors, provenance, image_bytes, image_sha256, file_span, NATIVE_SHA256
from tools.spatial_oracle.infantry_source_scatter import query


def generate():
    cases = [
        dict(process_probe=True), dict(power_off=True), dict(warp_in=True), dict(warp_out=True),
        dict(swap_active=True), dict(open_transport=True), dict(bunker=True), dict(mission=7),
        dict(head=[3200, 3200, 0], process_probe=True), dict(moving=True),
        dict(moving=True, power_off=True), dict(moving=True, raw=[[10, 10, 0x1C, 0]]),
        dict(moving=True, raw=[[10, 10, 0x20, 0]]),
        dict(moving=True, blocked_terrain=[[10, 10]]),
        dict(moving=True, cells=[[10, 10, 0, 0x100]], raw=[[10, 10, 0x20, 0]]),
        dict(moving=True, cells=[[10, 10, 0, 0x100]], raw=[[10, 10, 0, 0x20]]),
        dict(moving=True, swap_active=True),
        dict(cells=[[x, y, 0, 0x100] for x, y in
                    [(10, 9), (11, 9), (11, 10), (11, 11), (10, 11), (9, 11), (9, 10), (9, 9)]]),
    ]
    return [query(dict(case, live_entry=True, live_setter=True)) for case in cases]


def teleport_cases():
    """Two Cell requests before Process, including the retained-reservation Stop."""
    common = dict(teleport_destination=True, live_entry=True, live_setter=True,
                  actor=[2752, 2752, 0], doing=3, seed=31, human=True,
                  frame=100, game_speed=1)
    move_a = dict(kind='set_destination', cell=[12, 10], flag=1)
    move_b = dict(kind='set_destination', cell=[13, 10], flag=1)
    rows = [
        dict(name='guard_two_distinct_cells', mission=5, commands=[move_a, move_b]),
        dict(name='guard_same_cell_twice', mission=5, commands=[move_a, move_a]),
        dict(name='guard_move_stop_move', mission=5,
             commands=[move_a, dict(kind='stop_moving'), move_b]),
        dict(name='attack_same_cell_twice', mission=1, commands=[move_a, move_a]),
        dict(name='guard_queued_attack_same_cell_twice', mission=5, queued_mission=1,
             commands=[move_a, move_a]),
        dict(name='guard_queue_attack_between_same_cell_requests', mission=5,
             commands=[move_a, dict(kind='queue_mission', mission=1, start=0), move_a]),
        dict(name='guard_prone_two_cells', mission=5, prone=True, doing=6, commands=[move_a, move_b]),
    ]
    # Dependency controls: resolver deck choice uses physical Z and structural
    # destination flags, independently of the owner's retained OnBridge byte.
    bridge_cells = [[12, 10, 0, 0x100], [13, 10, 0, 0x100]]
    rows += [
        dict(name='bridge_threshold_equal_ground', mission=5, actor=[2752, 2752, 312],
             cells=bridge_cells, commands=[move_a, move_b]),
        dict(name='bridge_threshold_above_deck', mission=5, actor=[2752, 2752, 313],
             cells=bridge_cells, commands=[move_a, move_b]),
        dict(name='on_bridge_below_threshold_ground', mission=5, on_bridge=True,
             cells=bridge_cells, commands=[move_a, move_b]),
        dict(name='on_bridge_above_threshold_deck', mission=5, on_bridge=True,
             actor=[2752, 2752, 313], cells=bridge_cells, commands=[move_a, move_b]),
        dict(name='no_structural_bridge_above_threshold_ground', mission=5, on_bridge=True,
             actor=[2752, 2752, 313], commands=[move_a, move_b]),
    ]
    rows += [
        # The current Cell's supplied vehicle bit is retained through the
        # actual Infantry5217C0 initial mark; snapshots save the resulting raw.
        dict(name='current_vehicle_bit20_guard_two_cells', mission=5,
             raw=[[10, 10, 0x20, 0]], commands=[move_a, move_b]),
        dict(name='current_vehicle_bit20_attack_same_cell', mission=1,
             raw=[[10, 10, 0x20, 0]], commands=[move_a, move_a]),
        dict(name='human_prone_guard_same_reference_up7', mission=5,
             prone=True, doing=6, commands=[move_a, move_a]),
        dict(name='human_prone_attack_same_reference_up7', mission=1,
             prone=True, doing=6, commands=[move_a, move_a]),
        dict(name='destination_full_infantry_slots_refuses', mission=5,
             raw=[[12, 10, 0x1C, 0]], commands=[move_a, move_b]),
        dict(name='destination_vehicle_bit20_refuses', mission=5,
             raw=[[12, 10, 0x20, 0]], commands=[move_a, move_b]),
    ]
    rows += [dict(name=f'human_doing_{doing}_class_early_return', mission=5,
                  doing=doing, class_early_return=True, commands=[move_a, move_b])
             for doing in (27, 28, 29, 30)]
    return [{**common, **row} for row in rows]


def teleport_overlay_cases():
    """Additive overlay controls; historical22 and Guard2 rows stay exact."""
    common = dict(teleport_destination=True, live_entry=True, live_setter=True,
                  actor=[2752, 2752, 0], doing=3, seed=31, human=True,
                  frame=100, game_speed=1, mission=5,
                  commands=[dict(kind='set_destination', cell=[12, 10], flag=1),
                            dict(kind='set_destination', cell=[13, 10], flag=1)])
    guards = [dict(common, name='guard_resource_overlay_two_cells',
                   overlays=[dict(cell=[10, 10], name='TIB01', data=0, owner=-1),
                             dict(cell=[12, 10], name='TIB01', data=0, owner=-1)]),
              dict(common, name='guard_wall_overlay_refuses_then_clear',
                   overlays=[dict(cell=[10, 10], name='TIB01', data=0, owner=-1),
                             dict(cell=[12, 10], name='GASAND', data=0, owner=-1)])]
    return guards + [dict(row, name=name, queued_mission=2)
                     for row, name in zip(guards, ('queued_move_resource_overlay_two_cells',
                                                  'queued_move_wall_overlay_refuses_then_clear'))]


def teleport_transport_cases():
    """Independent original object-NavCom resolver controls, before Process.

    Unit class/coordinate virtuals and Cell481180/Infantry51BF90 execute.
    The supplied live Unit and its Cell content are explicit prestates; no
    radio or passenger result is provided or claimed.
    """
    common = {'teleport_destination': True,
     'live_entry': True,
     'live_setter': True,
     'actor': [2752, 2752, 0],
     'doing': 3,
     'seed': 31,
     'human': True,
     'frame': 100,
     'game_speed': 1}
    rows = [{'name': 'enter_transport_center_vehicle20',
      'mission': 7,
      'transport': {'coords': [3200, 2688, 0], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3200, 2688, 0]}]},
     {'name': 'enter_transport_offset_vehicle20',
      'mission': 7,
      'transport': {'coords': [3264, 2752, 0], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3264, 2752, 0]}]},
     {'name': 'enter_transport_offset_requested_center',
      'mission': 7,
      'transport': {'coords': [3264, 2752, 0], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3200, 2688, 0]}]},
     {'name': 'enter_transport_other_requested_cell',
      'mission': 7,
      'transport': {'coords': [3200, 2688, 0], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3456, 2688, 0]}]},
     {'name': 'guard_transport_center_vehicle20',
      'mission': 5,
      'transport': {'coords': [3200, 2688, 0], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3200, 2688, 0]}]},
     {'name': 'guard_queued_enter_transport_center_vehicle20',
      'mission': 5,
      'queued_mission': 7,
      'transport': {'coords': [3200, 2688, 0], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3200, 2688, 0]}]},
     {'name': 'enter_transport_center_clear_raw',
      'mission': 7,
      'transport': {'coords': [3200, 2688, 0], 'cell_content': True, 'health': 1},
      'commands': [{'kind': 'resolve', 'coords': [3200, 2688, 0]}]},
     {'name': 'enter_transport_center_missing_content',
      'mission': 7,
      'transport': {'coords': [3200, 2688, 0], 'cell_content': False, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3200, 2688, 0]}]},
     {'name': 'enter_transport_offset_all_slots_and_vehicle',
      'mission': 7,
      'transport': {'coords': [3264, 2752, 0], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 60, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3264, 2752, 0]}]},
     {'mission': 7,
      'name': 'enter_transport_non_subcell_center_sector_z99',
      'transport': {'coords': [3234, 2703, 99], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3234, 2703, 99]}]},
     {'mission': 7,
      'name': 'enter_transport_non_subcell_outer_sector',
      'transport': {'coords': [3324, 2773, 99], 'cell_content': True, 'health': 1},
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3324, 2773, 99]}]},
     {'mission': 7,
      'name': 'enter_transport_level2_incoming_z999',
      'transport': {'coords': [3234, 2703, 999], 'cell_content': True, 'health': 1},
      'cells': [[12, 10, 2, 0]],
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3234, 2703, 999]}]},
     {'mission': 7,
      'name': 'enter_transport_level2_slope8',
      'transport': {'coords': [3324, 2773, 208], 'cell_content': True, 'health': 1},
      'cells': [[12, 10, 2, 0]],
      'slopes': [[12, 10, 8]],
      'raw': [[12, 10, 32, 0]],
      'commands': [{'kind': 'resolve', 'coords': [3324, 2773, 208]}]},
     {'actor': [2752, 2752, 624],
      'mission': 7,
      'name': 'enter_transport_bridge_deck',
      'on_bridge': True,
      'transport': {'coords': [3324, 2773, 624], 'cell_content': True, 'on_bridge': True, 'health': 1},
      'cells': [[10, 10, 2, 256], [12, 10, 2, 256]],
      'raw': [[12, 10, 0, 32]],
      'commands': [{'kind': 'resolve', 'coords': [3324, 2773, 624]}]}]
    return [{**common, **row} for row in rows]


def teleport_type_inputs(records, *, include_overlays=False):
    """Reuse Mission's constructor, physical cache and original GUID OS owner.

    Whole InfantryType5236A0/5240A0 execute here, before the separate selected
    command fixture. Mission.setup/GeneralRules and the full scenario loader
    are not needed. Asset and OS boundaries remain those of this existing
    controller; no selected command result or type scalar is supplied.
    """
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.spatial_oracle.anytown_damage import mtnk_attack as base
    from tools.spatial_oracle.building_body_rules import RULES, dwords
    mission = Mission()
    m, u = mission.m, mission.u
    art_path = Path(os.environ['VERA20K_SHRAPNEL_INPUTS']) / 'ARTMD.INI'
    art_raw = art_path.read_bytes()
    art, lines = base.lexical(art_raw, {'CLEG', 'ClegSequence'})
    m.make_ini(art)
    for registry in (0xA8E348, 0xA83DE8):
        u.mem_write(registry, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    typ = m.alloc(0x1900)

    def fields():
        return dict(name=m.string(typ + 0x24), image=m.string(typ + 0x1F8),
                    vtable=hex(m.read32(typ)), jumpjet=u.mem_read(typ + 0xD94, 1)[0],
                    movement_zone=base.i32(u, typ + 0x5B4), speed_type=base.i32(u, typ + 0x67C),
                    crawls=u.mem_read(typ + 0xEBD, 1)[0], fraidycat=u.mem_read(typ + 0xEBF, 1)[0],
                    deployer=u.mem_read(typ + 0xEAC, 1)[0], strength=base.i32(u, typ + 0xA0),
                    locomotor_guid=bytes(u.mem_read(typ + 0x34C, 16)).hex(),
                    fire_frames=list(struct.unpack('<4i', u.mem_read(typ + 0xE40, 16))))

    def read_records():
        ptr = m.read32(typ + 0xE3C)
        return [list(struct.unpack('<9i', u.mem_read(ptr + index * 36, 36)))
                for index in range(42)]

    def rng():
        return {name: bytes(u.mem_read(ptr, 1012)).hex()
                for name, ptr in mission.resident.rngs.items()}

    original = bytes(u.mem_read(0x401000, 0x3E0000))
    before_rng = rng()
    m.invoke(0x5236A0, typ, [m.cstring('CLEG')],
             context=dict(owner='infantry_scatter_destination', phase='CLEG type constructor'))
    constructor = dict(fields=fields(), records=read_records())
    layers = []
    for name, path in base.layers():
        if not path.exists():
            assert name == 'LANGRULE.INI'
            layers.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        sections, source_lines = base.lexical(raw, {'CLEG'})
        m.rules_cache(sections)
        before = fields()
        event_start, read_start = len(mission.events), len(m.reads)
        eax = m.invoke(0x5240A0, typ, [RULES],
                       context=dict(owner='infantry_scatter_destination', phase='CLEG type read',
                                    layer=name, input_sha256=hashlib.sha256(raw).hexdigest()))
        layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(), sections=sections,
                           source_lines=source_lines, before=before, after=fields(), eax=eax, al=eax & 255,
                           reads=m.reads[read_start:], setup_events=mission.events[event_start:]))
    actual_records = read_records()
    assert actual_records == records, 'Whole CLEG type reader disagrees with original sequence owner'
    assert original == bytes(u.mem_read(0x401000, 0x3E0000))
    result = dict(constructor=constructor, layers=layers, fields=fields(), records=actual_records,
                  art=dict(sha256=hashlib.sha256(art_raw).hexdigest(), sections=art, source_lines=lines),
                  inherited_setup=dict(inputs=mission.inputs, world=mission.world),
                  rng_before=before_rng, rng_after=rng(), code_unchanged=True,
                  limits=['Existing Mission.prepare/attach_world bootstrap, without Mission.setup or GeneralRules.',
                          'Original constructor/full reader execute against physical lexical signed-CRC caches; file/archive loading is outside this boundary.',
                          'Existing Mission owns Windows ASCII/CLSID callbacks and setup-only type visual asset returns. No original gameplay callable result is supplied.'])
    if not include_overlays:
        return result

    # Compose the existing Rules overlay owner and Mission weapon controller.
    # Capture the historical CLEG receipt first; later reader work cannot alter
    # any of its saved fields or the original22 independently initialized rows.
    from tools.spatial_oracle.shrapnel_repair.retail_inputs import Rules
    overlays = Rules()
    overlay_reader, overlay_u = overlays, overlays.u
    inherited_overlay_setup = overlays.snapshot()
    overlay_names = ('GASAND', 'TIB01')
    overlay_art, overlay_art_lines = base.lexical(art_raw, set(overlay_names) | {'CLEG', 'ClegSequence'})
    rules_raw = base.layers()[0][1].read_bytes()
    declarations, declaration_lines = base.lexical(rules_raw, {'OverlayTypes'})
    overlay_rng_before = rng()
    registry_rng_before = {name: bytes(overlay_u.mem_read(ptr, 1012)).hex()
                           for name, ptr in (('main', 0x886B88), ('mapgen', 0xABE890))}
    registry = []
    pointers = {}
    table = overlays.read32(0xA83D84)
    for index in range(overlays.read32(0xA83D90)):
        ptr = overlays.read32(table + index * 4)
        name = overlays.string(ptr + 0x24)
        registry.append(dict(index=index, name=name, stored_index=base.i32(overlay_u, ptr + 0x294)))
        if name in overlay_names:
            pointers[name] = ptr
    assert len(registry) == 250 and set(pointers) == set(overlay_names)

    def overlay_fields(ptr):
        return dict(name=overlay_reader.string(ptr + 0x24), image=overlay_reader.string(ptr + 0x1F8),
                    index=base.i32(overlay_u, ptr + 0x294), land=base.i32(overlay_u, ptr + 0x298),
                    armor=base.i32(overlay_u, ptr + 0x9C), damage_levels=base.i32(overlay_u, ptr + 0x2A0),
                    crushable=overlay_u.mem_read(ptr + 0x22D, 1)[0],
                    wall=overlay_u.mem_read(ptr + 0x2A8, 1)[0], tiberium=overlay_u.mem_read(ptr + 0x2A9, 1)[0],
                    crate=overlay_u.mem_read(ptr + 0x2AA, 1)[0], no_use_tile_land=overlay_u.mem_read(ptr + 0x2AC, 1)[0])

    constructors = {name: dict(fields=overlay_fields(ptr), class_hex=bytes(overlay_u.mem_read(ptr, 0x2C0)).hex())
                    for name, ptr in pointers.items()}
    # Mission already imports the same original250 constructor templates from
    # the Rules owner. Its Scenario/ART/assets close baseObject's NewTheater
    # reader prerequisite; do not add another mock to the registry controller.
    mission_table = m.read32(0xA83D84)
    mission_pointers = {row['name']: m.read32(mission_table + row['index'] * 4)
                        for row in registry if row['name'] in overlay_names}
    for name, ptr in mission_pointers.items():
        assert bytes(u.mem_read(ptr, 0x2C0)).hex() == constructors[name]['class_hex'], name
    overlay_reader, overlay_u, pointers = m, u, mission_pointers
    m.make_ini(overlay_art)
    reader_scenario = m.read32(0xA8B230)
    assert reader_scenario
    reader_context = dict(controller='Mission', scenario_pointer=hex(reader_scenario),
                          theater=base.i32(u, reader_scenario + 0x1258),
                          constructor_templates_match=True)
    # Primary is read through the real type slot. Original GetWeapon and
    # CanDestroyWall subsequently consume this physically read normal weapon.
    weapon = m.read32(typ + 0x898)
    assert weapon
    weapon_name = m.string(weapon + 0x24)

    def weapon_fields():
        warhead = m.read32(weapon + 0xAC)
        return dict(name=m.string(weapon + 0x24), damage=base.i32(u, weapon + 0xA4),
                    ambient_damage=base.i32(u, weapon + 0x98),
                    warhead=m.string(warhead + 0x24) if warhead else None,
                    warhead_wall=u.mem_read(warhead + 0x144, 1)[0] if warhead else None)

    weapon_constructor = weapon_fields()
    overlay_layers = []
    for name, path in base.layers():
        if not path.exists():
            assert name == 'LANGRULE.INI'
            overlay_layers.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        sections, source_lines = base.lexical(raw, set(overlay_names) | {weapon_name})
        m.rules_cache(sections)
        reads = []
        for overlay_name in overlay_names:
            ptr = pointers[overlay_name]
            before = overlay_fields(ptr)
            asset_start = len(m.asset_loaded)
            eax = m.invoke(0x5FE770, ptr, [RULES],
                           context=dict(owner='infantry_scatter_destination', phase='overlay type read',
                                        layer=name, type=overlay_name))
            reads.append(dict(name=overlay_name, before=before, after=overlay_fields(ptr), eax=eax, al=eax & 255,
                              asset_loads=m.asset_loaded[asset_start:]))
        before_weapon = weapon_fields()
        read_start, event_start = len(m.reads), len(mission.events)
        weapon_eax = m.invoke(0x772080, weapon, [RULES],
                              context=dict(owner='infantry_scatter_destination', phase='CLEG primary read', layer=name))
        warhead = m.read32(weapon + 0xAC)
        assert warhead
        warhead_name = m.string(warhead + 0x24)
        warhead_sections, warhead_lines = base.lexical(raw, {warhead_name})
        m.rules_cache(warhead_sections)
        warhead_eax = m.invoke(0x75D3A0, warhead, [RULES],
                               context=dict(owner='infantry_scatter_destination', phase='CLEG warhead read', layer=name))
        overlay_layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(),
                                   sections=sections, source_lines=source_lines, overlays=reads,
                                   weapon=dict(before=before_weapon, after=weapon_fields(),
                                               weapon_eax=weapon_eax, weapon_al=weapon_eax & 255,
                                               warhead_eax=warhead_eax, warhead_al=warhead_eax & 255,
                                               warhead_sections=warhead_sections, warhead_source_lines=warhead_lines,
                                               reads=m.reads[read_start:], setup_events=mission.events[event_start:])))
    warhead = m.read32(weapon + 0xAC)
    overlay_inputs = dict(registry=registry, declarations=declarations, declaration_source_lines=declaration_lines,
                          rules_sha256=hashlib.sha256(rules_raw).hexdigest(), constructors=constructors,
                          art=dict(sha256=hashlib.sha256(art_raw).hexdigest(), sections=overlay_art,
                                   source_lines=overlay_art_lines), layers=overlay_layers,
                          fields={name: overlay_fields(ptr) for name, ptr in pointers.items()},
                          class_hex={name: bytes(overlay_u.mem_read(ptr, 0x2C0)).hex() for name, ptr in pointers.items()},
                          weapon_constructor=weapon_constructor, weapon=weapon_fields(),
                          weapon_class_hex=bytes(u.mem_read(weapon, 0x160)).hex(),
                          warhead_class_hex=bytes(u.mem_read(warhead, 0x190)).hex(),
                          primary_slot_hex=bytes(u.mem_read(typ + 0x898, 28)).hex(),
                          weapon_count=base.i32(u, typ + 0x808), is_gattling=u.mem_read(typ + 0xCD5, 1)[0],
                          rng_before=overlay_rng_before, rng_after=rng(), code_unchanged=True,
                          inherited_overlay_setup=inherited_overlay_setup,
                          reader_context=reader_context,
                          registry_rng_before=registry_rng_before,
                          registry_rng_after={name: bytes(overlays.u.mem_read(ptr, 1012)).hex()
                                              for name, ptr in (('main', 0x886B88), ('mapgen', 0xABE890))},
                          registry_scenario_pointer=hex(overlays.read32(0xA8B230)),
                          limits=['Existing Rules controller runs its complete original250-name overlay enumeration/constructors with its established source-linked cache and unchanged6M/10s block.',
                                  'Its inherited Shrapnel low-bridge scalar reads do not read GASAND0/TIB01102, so these selected constructors remain fresh. Inherited reads are retained explicitly; no full Rules chronology claim.',
                                  'Mission resident templates must equal the independent original selected constructor bytes before full OverlayType5FE770 reads physical rules layers in order. Its existing Scenario+1258 and fixed ART/assets provide baseObject NewTheater context; native file/archive parsing/loading chronology is outside this boundary.',
                                  'Original full normal CLEG primary772080/warhead75D3A0 readers establish the IsArmed/CanDestroyWall inputs; projectile effects and complete weapon/type-load closure are outside this movement boundary.',
                                  'Existing sparse Mission/Reader asset callbacks record supplied missing SHPs; this establishes scalar/ART reads, not overlay pixels. The separate Rules registry VM has no Scenario object; its Main/MapGen buffers are retained, whereas Mission overlay/weapon and all command comparisons retain full3 RNG objects.'])
    image = image_bytes()
    overlay_inputs['native_spans'] = [dict(address=hex(address), bytes=length,
                                         sha256=hashlib.sha256(file_span(image, address, length)[1]).hexdigest())
                                     for address, length in ((0x668CE3, 0x51), (0x5FE250, 0x199),
                                                             (0x5FE770, 0x2B3), (0x701120, 0x1A),
                                                             (0x772080, 0x965), (0x75D3A0, 0xB1E),
                                                             (0x772AC0, 0x1A), (0x51C17C, 0xA9))]
    assert original == bytes(u.mem_read(0x401000, 0x3E0000))
    assert original == bytes(overlays.u.mem_read(0x401000, 0x3E0000))
    assert overlay_inputs['rng_before'] == overlay_inputs['rng_after']
    assert overlay_inputs['registry_rng_before'] == overlay_inputs['registry_rng_after']
    return dict(type_inputs=result, overlay_inputs=overlay_inputs)


def teleport_refused_enter_cases():
    """Execute the original denial tail from a declared warp-in arrival state.

    This is not a RadioCanEnter substitution: execution starts after refusal
    at 51A488 with the original PerCell saved-register/local frame supplied.
    """
    common = dict(teleport_destination=True, live_entry=True, live_setter=True,
                  refused_enter=True, warping_in=True, teleport_timer=[100, 0, 30],
                  doing=3, human=True, frame=100, game_speed=1, mission=7,
                  transport=dict(coords=[3200, 2688, 0], cell_content=True, health=1),
                  raw=[[12, 10, 0x20, 0]], commands=[dict(kind='refused_enter')])
    return [dict(common, name='refused_transport_warp_in_seed31',
                 actor=[3200, 2688, 0], seed=31),
            dict(common, name='refused_transport_warp_in_seed42',
                 actor=[3264, 2752, 0], seed=42)]


def chrono_timer_cases():
    """Direct TimerCheck uses the supplied epoch, not a process-call counter."""
    common = dict(teleport_destination=True, live_entry=True, live_setter=True,
                  chrono_timer=True, warping_in=True, teleport_timer=[100, 0, 30],
                  timer_state=0, actor=[3200, 2688, 0], doing=3, seed=31,
                  human=True, game_speed=1, mission=7,
                  transport=dict(coords=[3200, 2688, 0], cell_content=True, health=1),
                  raw=[[12, 10, 0x20, 0]], commands=[dict(kind='timer_check')])
    return [dict(common, name=f'chrono_timer_epoch100_frame{frame}', frame=frame)
            for frame in (129, 130, 131)] + [
                dict(common, name='chrono_timer_frame_wrap_expired', frame=14,
                     teleport_timer=[-16, 0, 30])]


def teleport_refused_enter_reference(result):
    """Retain only boundary inputs, native outputs and compact call order."""
    trace_fields = {'kind', 'call', 'pc', 'name', 'eax', 'args', 'xyz', 'cell',
                    'seed_cell', 'result_cell', 'requested_xyz', 'output_xyz',
                    'priority', 'deck', 'use_cell_coords'}
    return dict(input=result['input'], before=result['before'],
                human_input=result['human_input'], after=result['after'],
                rng_before=result['rng_before'], rng_after=result['rng_after'],
                transport_input={key: result['transport_input'][key]
                                 for key in ('rtti', 'mapcoords', 'navcoords')},
                code_unchanged=result['code_unchanged'],
                boundaries=[dict(input=boundary['input'], eax=boundary['eax'],
                                 trace=[{key: value for key, value in row.items()
                                         if key in trace_fields} for row in boundary['trace']])
                            for boundary in result['boundaries']])


def teleport_transport_reference(result):
    """Keep the new Rust-facing inputs/goldens, without repeated diagnostics.

    The existing controller still executes and observes every original call.
    Historical Cell/overlay rows retain their full schema unchanged.
    """
    trace_fields = {'kind', 'call', 'pc', 'name', 'eax', 'xyz', 'cell',
                    'requested_xyz', 'output_xyz', 'priority', 'deck', 'use_cell_coords'}
    selected_calls = {'place_in_cell', 'ground_height', 'can_enter'}
    return dict(
        input=result['input'], before=result['before'], human_input=result['human_input'],
        after=dict(actor=dict(mission=result['after']['actor']['mission']),
                   locomotor=dict(resolved_xyz=result['after']['locomotor']['resolved_xyz'])),
        rng_before=result['rng_before'], rng_after=result['rng_after'],
        transport_input={key: result['transport_input'][key]
                         for key in ('rtti', 'mapcoords', 'navcoords')},
        code_unchanged=result['code_unchanged'],
        boundaries=[dict(input=boundary['input'], eax=boundary['eax'],
                         trace=[{key: value for key, value in row.items() if key in trace_fields}
                                for row in boundary['trace'] if row['name'] in selected_calls])
                    for boundary in result['boundaries']])


def generate_teleport(cases=None):
    from tools.spatial_oracle.jumpjet_infantry_actions import default_motion_records
    records, receipts, read = default_motion_records()
    prerequisites = teleport_type_inputs(records, include_overlays=True)
    type_inputs = prerequisites['type_inputs']
    overlay_inputs = prerequisites['overlay_inputs']
    image = image_bytes()
    spans = [(0x51AA40, 0x7A1), (0x4D94B0, 0x274), (0x718000, 0x80),
             (0x55A710, 0x60), (0x718080, 0x20), (0x718100, 0x123),
             (0x718230, 0x2D), (0x718B70, 1870), (0x481180, 795),
             (0x51DAF0, 0xDF), (0x51D6F0, 0x400),
             (0x5217C0, 0x8F), (0x521850, 0x89), (0x51BF90, 0xAE8),
             (0x50B730, 0x23), (0x5236A0, 0x2D7), (0x5240A0, 0x6A9)]
    identity = dict(native_sha256=NATIVE_SHA256,
                    spans=[dict(address=hex(address), bytes=length,
                                sha256=hashlib.sha256(file_span(image, address, length)[1]).hexdigest())
                           for address, length in spans],
                    limits='File-backed byte identity for explicit spans; sizes do not assert exhaustive function or caller coverage.')
    return dict(records=records, retail_inputs=receipts, sequence_reader=read,
                type_inputs=type_inputs, native_identity=identity,
                rows=[query(case, teleport_records=records, teleport_type_inputs=type_inputs)
                      for case in (teleport_cases() if cases is None else cases)],
                transport_native_identity=dict(native_sha256=image_sha256(image),
                    spans=[dict(address=hex(address), bytes=length,
                                sha256=hashlib.sha256(file_span(image, address, length)[1]).hexdigest())
                           for address, length in ((0x746E20, 6), (0x41BEA0, 0x3D),
                                                   (0x4DBDF0, 0x13C), (0x75AA90, 0x120),
                                                   (0x7F5C70, 0x1BC))]),
                transport_rows=[teleport_transport_reference(query(
                                    case, teleport_records=records, teleport_type_inputs=type_inputs))
                                for case in teleport_transport_cases()],
                refused_enter_native_identity=dict(native_sha256=image_sha256(image),
                    spans=[dict(address=hex(address), bytes=length,
                                sha256=hashlib.sha256(file_span(image, address, length)[1]).hexdigest())
                           for address, length in ((0x51A488, 0x35), (0x51D0D0, 0xBD4),
                                                   (0x56DC20, 0x958), (0x7192F0, 0x8ED),
                                                   (0x719BF0, 0x83))]),
                chrono_timer_rows=[teleport_refused_enter_reference(query(
                                       case, teleport_records=records, teleport_type_inputs=type_inputs))
                                   for case in chrono_timer_cases()],
                refused_enter_rows=[teleport_refused_enter_reference(query(
                                        case, teleport_records=records, teleport_type_inputs=type_inputs))
                                    for case in teleport_refused_enter_cases()],
                overlay_inputs=overlay_inputs,
                overlay_rows=[query(case, teleport_records=records, teleport_type_inputs=type_inputs,
                                    teleport_overlay_inputs=overlay_inputs)
                              for case in teleport_overlay_cases()])


def teleport_metadata():
    return provenance(
        scope='Original active ordinary CLEG Cell SetDestination51AA40 and separate Unit NavCom resolver718B70 controls through actual Cell481180/Infantry51BF90/raw receivers; repeated requests, Stop and declared Enter7/Guard5 inputs before Process; separate declared-refusal mobile PerCell51A488 tail controls during warp-in.',
        entry_points=dict(infantry_destination=0x51AA40, foot_destination=0x4D94B0,
                          teleport_constructor=0x718000, link=0x55A710,
                          teleport_is_moving=0x718080, teleport_move_to=0x718100,
                          teleport_stop=0x718230, teleport_resolver=0x718B70,
                          cell_placement=0x481180, infantry_entry=0x51BF90,
                          raw_put=0x5217C0, raw_remove=0x521850,
                          stop_driver=0x51DAF0, do_action=0x51D6F0,
                          infantry_type_constructor=0x5236A0, infantry_type_reader=0x5240A0,
                          human_predicate=0x50B730, infantry_ready=0x521B60, commence=0x5B3570,
                          megamission_queue=0x4C73B9, queue_mission=0x5B35E0,
                          subcell_startup=0x48E480, teleport_height_startup=0x717EC0,
                          unit_rtti=0x746E20, object_mapcoords=0x41BEA0,
                          foot_navigation_coord=0x4DBDF0, mobile_enter_refusal_tail=0x51A488,
                          infantry_scatter=0x51D0D0, fnpc=0x56DC20,
                          teleport_process=0x7192F0, teleport_timer_check=0x719BF0),
        assumptions=[
            'Single infantry_source_scatter command controller: original Infantry/Cell/ILocomotion vtables, Teleport constructor718000 and Link55A710, valid refcount1 and42 ClegSequence records. Independent original InfantryType5236A0 and whole5240A0 execute through the existing Mission reader/controller on physical RULESMD, absent LANGRULE, Battle and AnyTown CLEG sections, with fixed ARTMD. Actual JumpJet+D94 byte0 and MovementZone+5B4 Infantry7 initialize the already-live command fixture with the other retained scalar fields; its record bank must equal the existing original523D00 owner exactly. The corpus retains full inherited reader setup/asset inputs. Whole Infantry/House/scenario construction is outside the command boundary.',
            'Human House fields are explicit supplied inputs: index0, +1EC1, +1ED0 and image-default GameMode0. Original50B730 executes and saves its actual return and complete RNG before/after, including the per-command predicates. This establishes the predicate from these declared fields; it does not claim the full House-loading lifecycle.',
            'The command fixture explicitly supplies blocked-delay Rules+1768=22 through global8871E0, with frame100 and game speed1. This is a test input, not a physical retail GeneralRules result; Mission.setup/GeneralRules is excluded from the type prerequisite. Saved blocked-timer writes consume this supplied value.',
            '32x32 original Cells with widened supplied playfield, empty content/overlay lists, per-row declared raw occupation, flat level0 and nonzero land0/zero land1 speed-table fixture inputs. Original Cell/Infantry/Teleport CRT tables and subcell48E480 producer execute; cold geometry values are not supplied. Current vehicle20 inputs retain original Infantry initial-mark writes, and full-slot/vehicle destination refusals execute recursive classNULL without FNPC.',
            'Class51AA40 calls reach their real RET8. Original CanEnter51BF90, StopDriver51DAF0, DoAction51D6F0, MoveTo718100, resolver718B70, Cell481180 and5217C0/521850 execute. Full three1012-byte seeded RNG objects, callback/preflight/return order, raw planes, armed+1C and retained+28 XYZ are saved.',
            'Commands are direct original class calls at frame100, before any Process. Resolver Ready521B60/Commence5B3570 execute in original order, including queuedAttack continuation. Pure Cell NavCom selects priority0. Separate transport_rows execute the Unit NavCom Enter7 gate; Eaten8/Capture9/Patrol25 and other object classes remain outside executable coverage. Complete EventClass/command batch, FNPC/object destinations, Chronosphere and the whole Process/request lifetime are outside this corpus. Any FNPC56DC20 or TeleportProcess7192F0 visit fails these controls.',
            'Separate fourteen transport_rows directly invoke resolver718B70 with supplied NavCom pointing to a live Unit object using untouched7F5C70 vtable. Original746E20 returns RTTI1;41BEA0 computes its map coordinates and4DBDF0 returns its navigation XYZ through an original Walk constructor75AA90/Link55A710 with inactive head. Fields +9C coordinates, +6C health1, +74 active, +21C House, +684=-1, optional current Cell +E4 ground or +E8 bridge list head and raw occupation are explicit inputs. Unit/Infantry/House constructor lifecycles, Unit type loading, class51AA40 object admission, radio negotiation, actual passenger attachment and Teleport Process remain excluded. Original Ready/Commence still execute, including queuedEnter promotion. Cases compare Enter against Guard, matching against distinct requested Cells, center against noncenter XYZ, raw20 and3C, absent Cell content, arbitrary non-subcell points, nonzero incoming Z, level2/slope8 and structural bridge deck. No selected original gameplay return is supplied. Full three RNG objects and placement priority are saved.',
            'Four separate chrono_timer_rows directly execute original719BF0 through RET4 on supplied warp-in1, request0, timer-state0, complete timer epoch/pad/duration, frame and a nonnull actor target+2B4 pointing to the live fixture Unit. The target suppresses the no-target passive-acquisition callback branch without replacing a callable answer. Epoch100/duration30 at frames129,130,131 establishes expiry at frame130, including elapsed hidden frames without intervening Process calls. Signed epoch-16/frame14 checks the32-bit frame-counter wrap. All three full RNG objects and original timer/warp flag states are retained. Timer construction, Unlimbo/limbo scheduling, nonzero timer-state progression and the no-target expiry callback are excluded; Process7194AC..719579 timer publication is instruction evidence, not executed by these direct controls.',
            'Two separate refused_enter_rows start at original51A488 after an explicitly declared negative RadioCanEnter condition; the preceding Radio call and whole519630 PerCell are not executed or claimed. The original519630 saved-register/local frame is supplied with this=actor in ESI, target in EDI, other saved registers/locals zero, reason2 and a RET4 sentinel. Original classNULL, QueueGuard5(start0), Scatter51D0D0(NULL,1,1), FNPC56DC20, TeleportMoveTo718100, Process7192F0 and TimerCheck719BF0 execute untouched through the actual tail RET4. Arrival prestates explicitly supply actor warp-in+271=1 and locomotor timer start100/pad0/duration30 at frame100; they do not establish timer construction or absolute-versus-relative lifetime equivalence. Both controls preserve Enter7, queue Guard5, clear NavCom/armed/resolved XYZ, retain warp-in and write movement/blocked timers from the fixture Rules22. Scenario seed31 centre performs one range call and one basic draw; seed42 noncentral performs one range call with two basic draws due to rejection. Complete three RNG objects and call order are retained. Non-warp refusal reaches an unclosed animation/CRT prerequisite and is excluded; no non-warp/full lifecycle parity is claimed.',
            'Separate overlay_rows add only TIB01 resource and GASAND wall contexts to this pre-Process Cell chain. Current Cell10,10 carries TIB01 in each row; Cell12,10 carries TIB01 or GASAND, Cell13,10 remains clear. Cell overlay data0 and owner-1 are supplied. Original250-name OverlayTypes enumeration/constructors and full selected5FE770 readers establish registry IDs and flags across physical rules layers with fixed ART. The retained CLEG normal primary/warhead is physically read through original772080/75D3A0 for actual IsArmed701120/CanDestroyWall772AC0; no overlay or weapon gameplay result is substituted. Historical22 rows retain their original no-overlay/partial weapon context unchanged.',
            'Two additional queued_move overlay rows supply currentGuard5/queuedMove2 before the first class setter. Original Event4C73B9 calls Infantry Queue5B35E0(request,0) before target4C7467 and destination4C747C; flag0 writes queued+B4 without current+AC promotion. The selected resolver Ready/Commence promotes queuedMove itself. The second setter sees the retained current/queued result; same-currentMove with queuedNone is an idempotent Queue input5B3601..12. These are already-queued class boundaries, not whole EventClass execution. The original two Guard overlay rows remain unchanged.',
        ],
        substitutions=['Command controls substitute only OS InterlockedIncrement/Decrement imports with their pointed-count stdcall operations. Type prerequisite reads inherit existing Mission/Reader bounded allocation/cache/TLS, original GUID Windows OS callbacks and setup-only visual-asset boundaries. Additive overlay inputs compose the existing Rules/Sound original250-registry constructor and source-order cache owner, retaining its inherited scalar setup separately. No original selected gameplay callable return is supplied.'])


def teleport_source_paths():
    # Final publication binds this existing controller. The native run executes
    # no Rust; the Rust consumers' own tests compare them.
    from tools.spatial_oracle.anytown_damage import foot_missions
    root = Path(__file__).resolve().parents[2]
    # One existing inventory owns the Mission reader/controller dependency
    # closure. Keep its historical receipt, adding current mechanism owners.
    paths = {name: path for name, path in foot_missions.source_paths().items()
             if path.suffix == '.py'}
    names = [
        'tools/native_oracle.py', 'tools/native_inspect.py',
        'tools/spatial_oracle/infantry_scatter_destination.py',
        'tools/spatial_oracle/infantry_source_scatter.py',
        'tools/spatial_oracle/jumpjet_infantry_actions.py',
        'tools/spatial_oracle/infantry_sequence_rules.py',
        'tools/spatial_oracle/infantry_entry_raw.py',
        'tools/projectile_oracle/bridge_render_inputs.py',
    ]
    paths.update({name: root / name for name in names})
    return paths


if __name__ == '__main__' and '--teleport-cell' in sys.argv:
    args = [arg for arg in sys.argv[1:] if arg != '--teleport-cell']
    finish_vectors(generate_teleport, Path(__file__).with_name('infantry_teleport_destination.json'),
                   provenance=teleport_metadata, source_paths=teleport_source_paths(), argv=args)
elif __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Actual source-aware Infantry Scatter, CanEnter, QueueMission, Infantry/Foot destination setter, Walk MoveTo/Stop; supplied ordinary non-JumpJet Fraidycat. Optional separate next-Process probes stop at first FindPath or paid-step boundary.',
        entry_points={'scatter': 0x51D0D0, 'entry': 0x51BF90, 'infantry_set_destination': 0x51AA40,
                      'foot_set_destination': 0x4D94B0, 'walk_constructor': 0x75AA90,
                      'walk_move_to': 0x75ACB0, 'failed_path_receiver': 0x51DAF0,
                      'clear_cell': 0x4834A0, 'walk_stop': 0x75ADA0, 'walk_process': 0x75AEC0},
        assumptions=[
            'Inherits infantry_source_scatter live-entry fixture: widened synthetic playfield, 32x32 allocated original Cell vtables, empty lists/overlays, supplied raw occupation and nonzero land0/zero land1 speed rows. Original heading startup and seeded ScenarioRandom execute.',
            'Original Walk constructor and actual Infantry/Foot/Cell vtables. Linked owner and reference count1 supplied. No JumpJet, reciprocal Rocker/lift links, retained fire particles, EMP or +6AC suppression latch. Nonhuman Fraidycat avoids the unrelated human prone DoAction7 branch.',
            'Frame100 and blocked delay22; initial movement timer50/5, blocked timer40/6, retry count7, path backing2/3/4/5, reference9/8, null NavCom and aux123. Foot gate/warp bytes, optional paid head, power and moving prestates are supplied. Moving rows retain an old destination7808,2688,0.',
            'Failed-path receiver executes actual DoAction with supplied zero sequence counts (no animation acceptance), actual CanEnter and Stop. Initial +6DC is1 so receiver writes are observable. No invented result substituted for any gameplay callable.',
            'Source-aware Scatter never calls Process. Two explicit later probes stop at first FindPath or paid numeric approach; pathfinder core and complete movement are outside this native corpus.',
            'Original 6D1830/6D18C0/6D1BF0 initialize translation scale with FPCW0E7F, including actual structural-bridge destination rise414.',
        ],
        substitutions=['Only OS InterlockedIncrement/Decrement imports implement their pointed-count stdcall operations. No gameplay callable substituted.']))
