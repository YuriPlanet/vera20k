"""Original ore queues and natural drivers over the shared native fixtures.

Run ``python -m tools.spatial_oracle.ore_queue --check`` (or ``--write``).
Prepared map/queue state is explicit; AddToGrowth/Spread and both rebuild bodies,
their CellIterator/predicates, heap writes and Scenario Random execute unchanged.
"""
from functools import lru_cache
import hashlib
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle.harvest_field import (
    ADD_GROWTH, ADD_SPREAD, FIELD, REDUCE, TIBS, fixture,
)
from tools.spatial_oracle.map_queries import DUMMY, dwords
from tools.spatial_oracle.refinery_dock import ACTOR, cell, cell_xy
from tools.spatial_oracle.unit_scatter_state import SP
from tools.spatial_oracle.unit_source_scatter import MAP, SCENARIO, TABLE

REBUILD_GROWTH, REBUILD_SPREAD, RANDOM = 0x7233A0, 0x7228B0, 0x65C780
RANGED = 0x65C7E0
GROWTH_PROCESSOR, GROW_CELL = 0x722F00, 0x483710
SPREAD_PROCESSOR, SPREAD_DRIVER, GROWTH_DRIVER = 0x722440, 0x7221B0, 0x722C40
SPREAD_CELL, CAN_PLACE, PLACE = 0x483780, 0x4838E0, 0x487190
SPREAD_GERMINATE, GROWTH_DENSITY_GUARD = 0x4818E0, 0x7235C1
ARENA, ARENA_SIZE = FIELD + 0x200000, 0x20000
FIELDS = {'growth': (0x10C, 0x110, 0x114, 0x118), 'spread': (0xF0, 0xF4, 0xF8, 0xFC)}
DEFAULT_CELLS = [
    dict(cell=[4, 4], type_id=0, variant=0, density=3, slope=0, occupied=False),
    dict(cell=[1, 4], type_id=0, variant=0, density=2, slope=0, occupied=False),
    dict(cell=[2, 4], type_id=0, variant=0, density=5, slope=0, occupied=False),
    dict(cell=[3, 3], type_id=1, variant=0, density=2, slope=0, occupied=False),
    dict(cell=[5, 3], type_id=0, variant=0, density=11, slope=0, occupied=False),
    dict(cell=[3, 5], type_id=0, variant=0, density=3, slope=1, occupied=False),
    dict(cell=[3, 4], type_id=0, variant=0, density=4, slope=0, occupied=True),
]


def native_timer_reader(name, values):
    """Original constructor member block and four native ReadINI stores.

    The shared cached-INI owner supplies lexical strings; no host scalar parser
    supplies constructor defaults, ReadInt values or ReadDouble rounding.
    """
    from tools.rules_oracle.bridge_landing_inputs import Landing
    from tools.rules_oracle.guided_controls import normalized_cache
    from tools.spatial_oracle.building_body_rules import INI, SP as READER_SP
    m = Landing()
    u, pointer = m.u, m.alloc(0x200)
    u.mem_write(pointer + 0x24, b'OracleTiberium\0')
    u.reg_write(UC_X86_REG_ESI, pointer)
    u.reg_write(UC_X86_REG_ESP, READER_SP)
    # AbstractType construction/name and later registry publication are outside
    # this selected initialization block; the original 0/0.1 defaults execute.
    run_checked(u, 0x7216CF, 0x7217A6, required_addresses=(0x7216FB, 0x721707))
    keys = dict(Growth=values.get('growth'), Spread=values.get('spread'),
                GrowthPercentage=values.get('growth_percentage'),
                SpreadPercentage=values.get('spread_percentage'))
    normalized = {k: v for k, v in keys.items() if v is not None}
    m.make_ini(normalized_cache({'OracleTiberium': dict(FixtureOnly='1', **normalized)}))
    u.reg_write(UC_X86_REG_ESI, pointer)
    u.reg_write(UC_X86_REG_EBX, INI)
    u.reg_write(UC_X86_REG_ESP, READER_SP)
    run_checked(u, 0x721A78, 0x721AFA, required_addresses=(0x5276D0, 0x5283D0, 0x721AA6, 0x721AE1))
    assert u.reg_read(UC_X86_REG_ESP) == READER_SP - 4
    signed = lambda offset: struct.unpack('<i', u.mem_read(pointer + offset, 4))[0]
    real_bits = lambda offset: f'{struct.unpack("<Q", u.mem_read(pointer + offset, 8))[0]:016x}'
    return dict(name=name, input=values,
                output=dict(growth=signed(0xA8), spread=signed(0x9C),
                            growth_percentage_bits=real_bits(0xB0),
                            spread_percentage_bits=real_bits(0xA0)))


@lru_cache(maxsize=1)
def timer_reader_cases():
    from tools.rules_oracle.bridge_child_sound import sections
    rows = []
    for name, raw in [('missing', None), ('empty', ''), ('malformed', 'junk'),
                      ('zero', '0'), ('positive', '2200'), ('negative', '-3'),
                      ('minimum', '-2147483648'), ('maximum', '2147483647'),
                      ('signed_overflow', '2147483648'), ('all_bits', '4294967295'),
                      ('hex_all_bits', '$FFFFFFFF'), ('trailing_text', '2200suffix')]:
        rows.append(native_timer_reader(name, dict(growth=raw, spread=raw)))
    raw = (Path(os.environ.get('VERA20K_TIBTRE_INI', 'ini')) / 'RULESMD.INI').read_bytes()
    physical = sections(raw)
    for name in ('Riparius', 'Cruentus', 'Vinifera', 'Aboreus'):
        # This shared extractor supplies cached lexical strings only. These
        # selected retail sections/keys are unique; native readers own parsing.
        keys = physical[name]
        values = {field: keys.get(key) for field, key in
                  [('growth', 'Growth'), ('spread', 'Spread'),
                   ('growth_percentage', 'GrowthPercentage'), ('spread_percentage', 'SpreadPercentage')]}
        row = native_timer_reader(f'retail_{name}', values)
        row['physical'] = dict(filename='RULESMD.INI', sha256=hashlib.sha256(raw).hexdigest(), section=name)
        rows.append(row)
    return rows


def prepared_case(name, queue, count, **overrides):
    case = dict(name=name, entry='enqueue', queue=queue, receiver=0, size=[4, 4],
                target=[4, 4], frame=100, seed=1, grows=True, spreads=True,
                growth_percentages=[0.125] * 4, spread_percentages=[0.125] * 4,
                pre_count=count, pre_cell=[3, 3], pre_priority_bits=0x42480000,
                pre_heap_indices=[0], pre_bitmap=[[3, 3]], cells=DEFAULT_CELLS)
    return dict(case, **overrides)


def changed_cells(**updates):
    return [dict(c, **updates) if c['cell'] == [4, 4] else dict(c) for c in DEFAULT_CELLS]


def enqueue_cases():
    rows = []
    for queue, counts in [('growth', (53, 54, 55)), ('spread', (43, 44, 45))]:
        for count in counts:
            rows.append(prepared_case(f'{queue}_counter_{count}', queue, count))
        edge = counts[-1]
        rows += [prepared_case(f'{queue}_empty_heap_rebuild', queue, edge, pre_heap_indices=[]),
                 prepared_case(f'{queue}_late_heap_reference', queue, edge,
                               pre_heap_indices=[edge - 1]),
                 prepared_case(f'{queue}_seed_31', queue, edge, seed=31),
                 prepared_case(f'{queue}_seed_42', queue, edge, seed=42),
                 prepared_case(f'{queue}_negative_frame', queue, edge, frame=-100),
                 prepared_case(f'{queue}_large_frame', queue, edge, frame=16777217),
                 prepared_case(f'{queue}_chopped_f32_frame', queue, edge, frame=16777220),
                 prepared_case(f'{queue}_frame_wrap', queue, edge, frame=2147483647),
                 prepared_case(f'{queue}_frame_min', queue, edge, frame=-2147483648),
                 prepared_case(f'{queue}_crossclass_receiver', queue, edge, receiver=1),
                 prepared_case(f'{queue}_dormant_receiver', queue, edge, receiver=2)]
        for raw in [0, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF]:
            rows.append(prepared_case(f'{queue}_raw_{raw:08x}', queue, edge, next_raw=raw))
    rows += [
        prepared_case('growth_density_11_rejects_before_rebuild', 'growth', 55,
                      cells=changed_cells(density=11)),
        prepared_case('growth_density_10_admits', 'growth', 55,
                      cells=changed_cells(density=10)),
        prepared_case('growth_queued_bit_does_not_dedupe', 'growth', 55,
                      pre_bitmap=[[3, 3], [4, 4]]),
        prepared_case('growth_disabled_rebuild_then_append', 'growth', 55, grows=False),
        prepared_case('growth_zero_percentage_rebuild_then_append', 'growth', 55,
                      growth_percentages=[0.0] * 4),
        prepared_case('growth_sloped_request_rebuild_then_append', 'growth', 55,
                      cells=changed_cells(slope=1)),
        prepared_case('spread_queued_rejects_before_rebuild', 'spread', 45,
                      pre_bitmap=[[3, 3], [4, 4]]),
        prepared_case('spread_disabled_rejects_before_rebuild', 'spread', 45, spreads=False),
        prepared_case('spread_zero_percentage_rejects_before_rebuild', 'spread', 45,
                      spread_percentages=[0.0] * 4),
        prepared_case('spread_receiver_zero_own_percentage_admits', 'spread', 45,
                      receiver=1, spread_percentages=[0.125, 0.0, 0.125, 0.125]),
        prepared_case('spread_sloped_rejects_before_rebuild', 'spread', 45,
                      cells=changed_cells(slope=1)),
        prepared_case('spread_occupied_rejects_before_rebuild', 'spread', 45,
                      cells=changed_cells(occupied=True)),
        prepared_case('spread_density_zero_rejects_before_rebuild', 'spread', 45,
                      cells=changed_cells(density=0)),
        prepared_case('spread_gem_density_zero_rejects_before_rebuild', 'spread', 45,
                      cells=changed_cells(type_id=1, density=0)),
        prepared_case('spread_gem_density_one_admits', 'spread', 45,
                      cells=changed_cells(type_id=1, density=1)),
        prepared_case('spread_growth_flag_disabled_still_rebuilds', 'spread', 45, grows=False),
    ]
    # Complete Reduce_Tiberium full-removal route: the removed Gem receiver
    # rebuilds its own class, then appends surviving Ore neighbors in native
    # N..NW order. The old receiver bitmap is clear on these neighbors.
    ring = [(4, 3), (5, 3), (5, 4), (5, 5), (4, 5), (3, 5), (3, 4), (3, 3)]
    removed = dict(cell=[4, 4], type_id=1, variant=0, density=0, slope=0, occupied=False)
    neighbors = [dict(cell=list(c), type_id=0, variant=0, density=3, slope=0,
                      occupied=False) for c in ring]
    rows += [prepared_case('reduce_empty_gem_rebuilds_gem_receiver_with_ore_neighbors',
                           'spread', 44, entry='reduce', receiver=1,
                           pre_bitmap=[[4, 4]], cells=[removed, *neighbors]),
             prepared_case('reduce_empty_ore_rebuild_then_skip_reseeded_neighbors',
                           'spread', 44, entry='reduce', receiver=0,
                           pre_bitmap=[[4, 4]], cells=[dict(removed, type_id=0), *neighbors]),
             prepared_case('reduce_density11_growth_guard_precedes_partial_removal',
                           'growth', 55, entry='reduce', amount=1,
                           cells=changed_cells(density=11))]
    for frame in [100, 16777220, -2147483648]:
        rows.append(prepared_case(f'growth_processor_rebuilds_spread_frame_{frame}',
                                   'spread', 44, entry='growth_processor', frame=frame,
                                   pre_cell=[4, 4]))
    # The processor reinserts directly into its own growth array; AddToGrowth's
    # threshold is not a processor-array guard. The same turn can rebuild spread.
    rows.append(prepared_case('growth_processor_inline_reinsert_crosses_growth_counter',
                               'spread', 44, entry='growth_processor', pre_cell=[4, 4],
                               other_pre_counts={'growth': 55}))
    rows.append(prepared_case('growth_processor_negative_budget_drops_first_root',
                               'spread', 44, entry='growth_processor', pre_cell=[4, 4],
                               next_raw=0x80000000))
    return rows


def supplied_queue(kind, name, coordinates=(), priority_bits=0, bitmap=None):
    """Declare retained state; native heap mutation is never simulated here."""
    entries = [dict(cell=list(c), priority_bits=priority_bits) for c in coordinates]
    return dict(type_id=kind, queue=name, entries=entries,
                heap_indices=list(range(len(entries))),
                bitmap=[list(c) for c in (coordinates if bitmap is None else bitmap)])


def natural_case(name, **overrides):
    source = dict(cell=[4, 4], type_id=0, variant=0, density=3, slope=0, occupied=False)
    queues = [supplied_queue(k, q, [[4, 4]] if k == 0 and q == 'spread' else [])
              for k in range(4) for q in FIELDS]
    base = prepared_case(name, 'spread', 1, entry='spread_processor', pre_cell=[4, 4],
                         cells=[source], queue_states=queues,
                         growth_timers=[[-1, 0, 3]] * 4,
                         spread_timers=[[-1, 0, 0], [-1, 0, 5], [-1, 0, 7], [-1, 0, 11]],
                         growth_frames=[3, 5, 7, 11], spread_frames=[3, 5, 7, 11],
                         scenario_serial=0)
    return dict(base, **overrides)


def natural_cases():
    rows = [natural_case(f'spread_processor_seed_{seed}', seed=seed) for seed in (1, 31, 42)]
    rows += [natural_case('spread_processor_negative_budget_drops_first_root', next_raw=0x80000000),
             natural_case('spread_processor_spreads_disabled_still_visits_and_reinserts', spreads=False),
             natural_case('spread_processor_grows_disabled_direct_call', grows=False),
             natural_case('spread_processor_empty_queue_no_draw', queue_states=[
                 supplied_queue(k, q) for k in range(4) for q in FIELDS]),
             natural_case('spread_processor_zero_percentage_no_draw', spread_percentages=[0.0] * 4)]
    ring = [(4, 3), (5, 3), (5, 4), (5, 5), (4, 5), (3, 5), (3, 4), (3, 3)]
    for target_count in (0, 1, 2):
        rows.append(natural_case(f'spread_processor_{target_count}_targets', next_raw=0,
                                  blocked=[[*c, 'overlay'] for c in ring[target_count:]]))
    for gate in ('slope', 'bridge', 'land', 'building', 'tree', 'ordinary_tree'):
        rows.append(natural_case(f'spread_processor_target_{gate}', next_raw=0,
                                  blocked=[[*c, gate] for c in ring]))
    rows.append(natural_case('spread_processor_target_units_are_allowed', units=[list(c) for c in ring], next_raw=0))
    for option in ('building_invisible', 'building_invisible_in_game'):
        rows.append(natural_case(f'spread_processor_target_{option}', next_raw=0,
                                  blocked=[[*c, 'building'] for c in ring], **{option: True}))
    rows.append(natural_case('spread_processor_target_dead_building', next_raw=0,
                             blocked=[[*c, 'building'] for c in ring], building_health=0))
    for key, value in [('density', 0), ('slope', 1), ('occupied', True)]:
        c = dict(natural_case('unused')['cells'][0], **{key: value})
        rows.append(natural_case(f'spread_processor_stale_source_{key}', cells=[c], next_raw=0))
    c = dict(natural_case('unused')['cells'][0], overlay=-1, land=0, density=0)
    rows.append(natural_case('spread_processor_stale_source_empty_overlay', cells=[c], next_raw=0))
    c = dict(natural_case('unused')['cells'][0], cell=[1, 4])
    rows.append(natural_case('spread_processor_source_on_diamond_edge', cells=[c], next_raw=0,
                             queue_states=[supplied_queue(k, q, [[1, 4]] if k == 0 and q == 'spread' else [])
                                           for k in range(4) for q in FIELDS]))
    rows.append(natural_case('spread_processor_no_tile_admission', tile_allows_tiberium=False, next_raw=0))
    for frame in (16777220, 2147483647, -2147483648):
        rows.append(natural_case(f'spread_processor_new_cell_frame_{frame}', frame=frame, next_raw=0))
    rows.append(natural_case('spread_processor_crossclass_receiver_uses_source_type', receiver=1,
                             queue_states=[supplied_queue(k, q, [[4, 4]] if k == 1 and q == 'spread' else [])
                                           for k in range(4) for q in FIELDS], next_raw=0))
    c = dict(natural_case('unused')['cells'][0], type_id=1, density=1)
    rows.append(natural_case('spread_processor_crossclass_ore_receiver_gem_source', cells=[c], next_raw=0))
    queues = natural_case('unused')['queue_states']
    queues[1] = supplied_queue(0, 'spread', [[4, 4], [2, 3]], bitmap=[[4, 4], [2, 3]])
    c = dict(natural_case('unused')['cells'][0], cell=[2, 3])
    rows.append(natural_case('spread_processor_zero_target_root_does_not_spend_budget',
                             queue_states=queues, cells=[natural_case('unused')['cells'][0], c],
                             blocked=[[*p, 'overlay'] for p in ring], next_raw=0))
    # A drained append array can be large while its heap contains one root.
    # Conversely, only heap_count > capacity-20 triggers the processor rebuild.
    for count in (44, 45):
        queues = natural_case('unused')['queue_states']
        queues[1] = supplied_queue(0, 'spread', [[4, 4]] * count, bitmap=[[4, 4]])
        rows.append(natural_case(f'spread_processor_heap_count_{count}', queue_states=queues,
                                 next_raw=0, spreads=False))
    queues = natural_case('unused')['queue_states']
    queues[1] = supplied_queue(0, 'spread', [[4, 4]] * 45, bitmap=[[4, 4]])
    rows.append(natural_case('spread_processor_heap_rebuild_reseeds_live_source',
                             queue_states=queues, next_raw=0))
    queues = natural_case('unused')['queue_states']
    queues[0] = supplied_queue(0, 'growth', [[4, 4]] * 55, bitmap=[[4, 4]])
    queues[0]['heap_indices'] = [54]
    rows.append(natural_case('spread_processor_new_cell_rebuilds_growth_before_density_store',
                             queue_states=queues, next_raw=0))
    queues = natural_case('unused')['queue_states']
    queues[1] = supplied_queue(0, 'spread', [[4, 4]] * 55, bitmap=[[4, 4]])
    queues[1]['heap_indices'] = [54]
    rows.append(natural_case('spread_processor_large_array_small_heap_no_rebuild',
                             queue_states=queues, next_raw=0))
    for percentage in (1.0, 10.0):
        queues = natural_case('unused')['queue_states']
        queues[1] = supplied_queue(0, 'spread', [[4, 4]] * 6, bitmap=[[4, 4]])
        rows.append(natural_case(f'spread_processor_budget_percentage_{percentage:g}',
                                 queue_states=queues, spread_percentages=[percentage] * 4,
                                 spreads=False, next_raw=24))
    rows += [natural_case('spread_driver_spreads_disabled_timer_reload', entry='spread_driver', spreads=False),
             natural_case('spread_driver_grows_disabled_no_dispatch', entry='spread_driver', grows=False),
             natural_case('spread_driver_empty_queue_reloads_without_draw', entry='spread_driver',
                          queue_states=[supplied_queue(k, q) for k in range(4) for q in FIELDS]),
             natural_case('spread_driver_zero_percentage_reloads_without_draw', entry='spread_driver',
                          spread_percentages=[0.0] * 4),
             natural_case('spread_driver_three_frame_cadence', entry='spread_driver',
                          frames=[100, 101, 102, 103, 104, 106]),
             natural_case('spread_driver_all_classes_due_native_order', entry='spread_driver',
                          spread_timers=[[-1, 0, 0]] * 4,
                          queue_states=[supplied_queue(k, q, [[4, 4]] if q == 'spread' else [])
                                        for k in range(4) for q in FIELDS], spreads=False),
             natural_case('spread_driver_stock_timer_reload', entry='spread_driver',
                          growth_frames=[2200, 10000, 2200, 2200],
                          spread_frames=[2200, 10000, 2200, 2200],
                          spread_timers=[[-1, 0, 0]] * 4, spreads=False)]
    for timer_name, timer, frames in [
        ('running_before_due', [98, 0, 3], [100, 101, 102]),
        ('paused_remaining', [-1, 0, 3], [100, 1000]),
        ('paused_zero', [-1, 0, 0], [100]),
        ('zero_reload_every_frame', [-1, 0, 0], [100, 101, 102]),
        ('negative_reload_every_frame', [-1, 0, 0], [100, 101, 102]),
        ('signed_frame_wrap', [2147483647, 0, 1], [2147483647, -2147483648, -2147483647]),
    ]:
        raw = 0 if timer_name == 'zero_reload_every_frame' else -3 if timer_name == 'negative_reload_every_frame' else 3
        rows.append(natural_case(f'spread_driver_timer_{timer_name}', entry='spread_driver', frames=frames,
                                 spread_timers=[timer, [-1, 0, 5], [-1, 0, 7], [-1, 0, 11]],
                                 spread_frames=[raw, 5, 7, 11], spreads=False))
    queues = natural_case('unused')['queue_states']
    queues[0] = supplied_queue(0, 'growth', [[4, 4]])
    queues[1] = supplied_queue(0, 'spread')
    rows.append(natural_case('growth_then_spread_driver_same_frame_new_queue',
                             entry='growth_then_spread_driver', queue_states=queues,
                             growth_timers=[[-1, 0, 0], [-1, 0, 5], [-1, 0, 7], [-1, 0, 11]],
                             next_raw=0))
    retail = [row['output'] for row in timer_reader_cases() if row['name'].startswith('retail_')]
    retail_growth = [r['growth'] for r in retail]
    retail_spread = [r['spread'] for r in retail]
    retail_growth_pct = [struct.unpack('<d', struct.pack('<Q', int(r['growth_percentage_bits'], 16)))[0] for r in retail]
    retail_spread_pct = [struct.unpack('<d', struct.pack('<Q', int(r['spread_percentage_bits'], 16)))[0] for r in retail]
    rows.append(natural_case('spread_driver_original_retail_reader_values', entry='spread_driver',
                             growth_frames=retail_growth, spread_frames=retail_spread,
                             growth_percentages=retail_growth_pct, spread_percentages=retail_spread_pct,
                             spread_timers=[[-1, 0, 0]] * 4))
    rows.append(natural_case('growth_then_spread_driver_stock_fast_growth_reload',
                             entry='growth_then_spread_driver', fast_growth=True,
                             growth_frames=retail_growth, spread_frames=retail_spread,
                             growth_percentages=retail_growth_pct, spread_percentages=retail_spread_pct,
                             growth_timers=[[-1, 0, 0]] * 4, spread_timers=[[-1, 0, 0]] * 4,
                             queue_states=[supplied_queue(k, q) for k in range(4) for q in FIELDS]))
    for fast in (False, True):
        rows.append(natural_case(f'growth_then_spread_driver_negative_growth_fast_{fast}',
                                 entry='growth_then_spread_driver', fast_growth=fast,
                                 growth_frames=[-3, -10, -2147483648, 2147483647],
                                 growth_timers=[[-1, 0, 0]] * 4,
                                 frames=[100, 101, 102],
                                 queue_states=[supplied_queue(k, q) for k in range(4) for q in FIELDS]))
    rows += [natural_case('spread_processor_overlay_serial_wrap', scenario_serial=0xFFFFFFFF, next_raw=0),
             natural_case('spread_processor_refused_overlay_serial_wrap', scenario_serial=0xFFFFFFFF,
                          next_raw=0, blocked=[[*c, 'ordinary_tree'] for c in ring])]
    rows += [natural_case('spread_driver_overlay_lifecycle_deferred_drain', entry='spread_driver',
                          next_raw=0, drain_deferred=True),
             natural_case('spread_driver_refused_overlay_limbo_survives_drain', entry='spread_driver',
                          next_raw=0, drain_deferred=True,
                          blocked=[[*c, 'ordinary_tree'] for c in ring])]
    queues = natural_case('unused')['queue_states']
    queues[0] = supplied_queue(0, 'growth', [[4, 4]])
    queues[1] = supplied_queue(0, 'spread')
    rows.append(natural_case('growth_then_spread_existing_cell_consumes_no_overlay_serial',
                             entry='growth_then_spread_driver', queue_states=queues, spreads=False,
                             growth_timers=[[-1, 0, 0], [-1, 0, 5], [-1, 0, 7], [-1, 0, 11]],
                             next_raw=0))
    # The empty (5,4) hole and all eight neighbors are allocated interior
    # Cells. Block the source's other empty neighbors so the original scan
    # selects this hole; the variant/density/queue effects execute unchanged.
    hole_neighbors = [(5, 3), (6, 3), (6, 4), (6, 5),
                      (5, 5), (4, 5), (4, 4), (4, 3)]
    for suffix, replacement in [('eight_same_class', None),
                                ('seven_same_one_gem', 1),
                                ('seven_same_one_empty', -1)]:
        neighbors = [dict(cell=list(c), type_id=0, variant=0, density=3,
                          slope=0, occupied=False) for c in hole_neighbors]
        if replacement == 1:
            neighbors[1]['type_id'] = 1
        elif replacement == -1:
            del neighbors[1]
        rows.append(natural_case(f'spread_processor_interior_hole_{suffix}',
                                 cells=neighbors, next_raw=0,
                                 blocked=[[*c, 'overlay'] for c in [(3, 5), (3, 4), (3, 3)]]))
    # Processor target counting still traverses the native lookup owner before
    # an occupied source or the Scenario spread flag refuses the spread. Keep
    # these controls free of Mark, whose later lookups could mask that write.
    edge_source = dict(natural_case('unused')['cells'][0], cell=[1, 4])
    edge_queues = [supplied_queue(k, q, [[1, 4]] if k == 0 and q == 'spread' else [])
                   for k in range(4) for q in FIELDS]
    rows += [natural_case('spread_processor_source_on_diamond_edge_occupied',
                          cells=[dict(edge_source, occupied=True)], queue_states=edge_queues,
                          next_raw=0),
             natural_case('spread_processor_source_on_diamond_edge_spreads_disabled',
                          cells=[edge_source], queue_states=edge_queues, spreads=False,
                          next_raw=0)]
    return rows


def cases():
    return enqueue_cases() + natural_cases()


def queue_address(kind, name):
    return ARENA + kind * 0x8000 + (0x4000 if name == 'growth' else 0)


def read_queue(u, read32, kind, name, capacity, bitmap_coordinates, *, tiberium_pointer=None):
    counter, queue_field, bitmap_field, entries_field = FIELDS[name]
    tib = TIBS + kind * 0x200 if tiberium_pointer is None else tiberium_pointer
    q, array = read32(tib + queue_field), read32(tib + entries_field)
    heap_table, bitmap_pointer = read32(q + 8), read32(tib + bitmap_field)
    count, heap_count = read32(tib + counter), read32(q)
    assert count <= capacity + 1 and heap_count < capacity
    entries = [dict(cell=list(struct.unpack('<hh', u.mem_read(array + i * 8, 4))),
                    priority_bits=read32(array + i * 8 + 4)) for i in range(count)]
    heap = []
    for slot in range(1, heap_count + 1):
        delta = read32(heap_table + slot * 4) - array
        assert delta >= 0 and delta % 8 == 0 and delta // 8 < count
        heap.append(delta // 8)
    bitmap = [i for i, value in enumerate(u.mem_read(bitmap_pointer, capacity)) if value]
    assert all(i in bitmap_coordinates for i in bitmap)
    bitmap_cells = sorted([bitmap_coordinates[i] for i in bitmap])
    return dict(array_count=count, entries=entries, heap_indices=heap, bitmap_indices=bitmap,
                bitmap_cells=bitmap_cells)


def output_state(u, read32, capacity, bitmap_coordinates, lifecycle=False):
    classes = []
    for kind in range(4):
        tib = TIBS + kind * 0x200
        classes.append(dict(type_id=kind,
                            growth=read_queue(u, read32, kind, 'growth', capacity, bitmap_coordinates),
                            spread=read_queue(u, read32, kind, 'spread', capacity, bitmap_coordinates),
                            growth_timer=list(struct.unpack('<3i', u.mem_read(tib + 0x11C, 12))),
                            spread_timer=list(struct.unpack('<3i', u.mem_read(tib + 0x100, 12)))))
    rng = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4))
    result = dict(classes=classes, rng_indices=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)],
                  rng_sha256=hashlib.sha256(rng).hexdigest())
    if lifecycle:
        result.update(scenario_serial=read32(SCENARIO + 0x214),
                      overlay_registrations=read32(0xA8EC60), pending_deletes=read32(0xB0F6A8))
        # Snapshot the actual resident Cell without calling a map lookup:
        # observing its coordinates must not itself stamp the shared dummy.
        result['dummy'] = dict(cell=list(struct.unpack('<hh', u.mem_read(DUMMY + 0x24, 4))),
                               overlay=struct.unpack('<i', u.mem_read(DUMMY + 0x44, 4))[0],
                               density=u.mem_read(DUMMY + 0x11E, 1)[0],
                               land=struct.unpack('<i', u.mem_read(DUMMY + 0xEC, 4))[0],
                               level=struct.unpack('<b', u.mem_read(DUMMY + 0x11B, 1))[0],
                               slope=u.mem_read(DUMMY + 0x11C, 1)[0], flags=read32(DUMMY + 0x140))
    return result


def execute(case):
    inherited = dict(case, west_building=False,
                     ore=[[*c['cell'], c['type_id'], c['variant'], c['density']]
                          for c in case['cells']])
    natural = case['entry'] in ('spread_processor', 'spread_driver', 'growth_then_spread_driver')
    if natural:
        # Reuse TIBTRE's real Overlay constructor/Mark/recalc ownership. Its
        # supplied tree at (16,16) is outside this diamond, available only for
        # declared blocked-target object controls.
        from tools.spatial_oracle.tibtre import fixture as tibtre_fixture
        u, call, read32, _coords, _events = tibtre_fixture(inherited)
        if case.get('drain_deferred'):
            from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor
            OriginalBridgeConstructor.prepare_deferred_services(u)
    else:
        u, call, read32, _events, _unused = fixture(inherited)
    u.mem_map(ARENA, ARENA_SIZE)
    width, height = case['size']
    u.mem_write(MAP + 0xF4, dwords(width, height))
    call(0x42B1F0, 0, [])
    capacity = u.reg_read(UC_X86_REG_EAX)
    assert capacity == (height + 4) * width * 2
    # Reuse the shared real Cell vtables and backing array, supplying the
    # successful Resize diamond. The original iterator stops at its first null.
    u.mem_write(TABLE, bytes(0x100000))
    bitmap_coordinates, bitmap_ordinals = {}, {}
    for y in range(32):
        for x in range(32):
            if width < x + y <= width + 2 * height and abs(x - y) < width:
                u.mem_write(TABLE + (y * 512 + x) * 4, dwords(cell(x, y)))
                call(0x42B1C0, cell(x, y) + 0x24, [])
                ordinal = u.reg_read(UC_X86_REG_EAX)
                assert ordinal < capacity and ordinal not in bitmap_coordinates
                bitmap_coordinates[ordinal] = [x, y]
                bitmap_ordinals[(x, y)] = ordinal
    u.mem_write(SCENARIO + 0x34A6, bytes([case['grows']]))
    for c in case['cells']:
        u.mem_write(cell(*c['cell']) + 0x11C, bytes([c['slope']]))
        u.mem_write(cell(*c['cell']) + 0xE4, dwords((ACTOR if natural else 1) if c['occupied'] else 0))
        if 'overlay' in c:
            u.mem_write(cell(*c['cell']) + 0x44, dwords(c['overlay']))
        if 'land' in c:
            u.mem_write(cell(*c['cell']) + 0xEC, dwords(c['land']))
    if natural:
        flags = (0x80 if case['spreads'] else 0) | (0x40 if case.get('fast_growth', False) else 0)
        u.mem_write(SCENARIO, dwords(flags))
        u.mem_write(SCENARIO + 0x214, dwords(case.get('scenario_serial', 0)))
    for kind in range(4):
        tib = TIBS + kind * 0x200
        u.mem_write(tib + 0xA0, struct.pack('<d', case['spread_percentages'][kind]))
        u.mem_write(tib + 0xB0, struct.pack('<d', case['growth_percentages'][kind]))
        u.mem_write(tib + 0xE4, dwords(12))
        growth_timer = case.get('growth_timers', [[21 + k, 0, 300 + k] for k in range(4)])[kind]
        spread_timer = case.get('spread_timers', [[11 + k, 0, 200 + k] for k in range(4)])[kind]
        u.mem_write(tib + 0x100, struct.pack('<3i', *spread_timer))
        u.mem_write(tib + 0x11C, struct.pack('<3i', *growth_timer))
        if natural:
            u.mem_write(tib + 0x9C, dwords(case['spread_frames'][kind]))
            u.mem_write(tib + 0xA8, dwords(case['growth_frames'][kind]))
        for name, fields in FIELDS.items():
            q = queue_address(kind, name)
            selected = kind == case['receiver'] and name == case['queue']
            count = case['pre_count'] if selected else case.get('other_pre_counts', {}).get(name, 1)
            if kind != case['receiver']:
                count = 1
            heap = case['pre_heap_indices'] if selected else [0]
            bitmaps = case['pre_bitmap'] if selected else [[2, 3]]
            supplied = next((state for state in case.get('queue_states', [])
                             if state['type_id'] == kind and state['queue'] == name), None)
            if supplied is not None:
                count, heap, bitmaps = len(supplied['entries']), supplied['heap_indices'], supplied['bitmap']
            u.mem_write(tib + fields[0], dwords(count, q, q + 0x2000, q + 0x100))
            u.mem_write(q, dwords(len(heap), capacity, q + 0x1000, 0, 0xFFFFFFFF))
            packed = struct.pack('<hhI', *case['pre_cell'], case['pre_priority_bits'])
            if supplied is None:
                u.mem_write(q + 0x100, packed * count)
            else:
                for index, entry in enumerate(supplied['entries']):
                    u.mem_write(q + 0x100 + index * 8,
                                struct.pack('<hhI', *entry['cell'], entry['priority_bits']))
            for slot, index in enumerate(heap, 1):
                assert 0 <= index < count
                u.mem_write(q + 0x1000 + slot * 4, dwords(q + 0x100 + index * 8))
            for x, y in bitmaps:
                ordinal = bitmap_ordinals[(x, y)]
                assert 0 <= ordinal < capacity
                u.mem_write(q + 0x2000 + ordinal, b'\x01')
    if 'next_raw' in case:
        # Same supplied native-state boundary as tibtre's probability controls;
        # the original Random::Next XOR/store/indices still execute unchanged.
        first, second = read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)
        a, b = SCENARIO + 0x224 + first * 4, SCENARIO + 0x224 + second * 4
        u.mem_write(a, dwords(case['next_raw'] ^ read32(b)))
    before = output_state(u, read32, capacity, bitmap_coordinates, lifecycle=natural)
    events, pending_random, pending_ranged, pending_can_place = [], [], [], []
    ranged_draw_index = None
    pending_constructor, pending_serial = [], []
    pending_growth_guard, pending_germinate = [], []
    constructed = {}

    def observe(_u, address, _size, _data):
        nonlocal ranged_draw_index
        sp, this = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        event_start = len(events)
        if pending_random and address == pending_random[-1][0]:
            _, index = pending_random.pop()
            events[index]['raw'] = u.reg_read(UC_X86_REG_EAX)
            events[index]['rng_after'] = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
        if pending_ranged and address == pending_ranged[-1][0]:
            _, index = pending_ranged.pop()
            events[index]['result'] = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
        if pending_can_place and address == pending_can_place[-1][0]:
            _, index = pending_can_place.pop()
            events[index]['result'] = bool(u.reg_read(UC_X86_REG_EAX) & 0xFF)
        if pending_constructor and address == pending_constructor[-1][0]:
            _, index, obj = pending_constructor.pop()
            assert u.reg_read(UC_X86_REG_EAX) == obj
            events[index].update(native_id=read32(obj + 0x10),
                                 scenario_serial=read32(SCENARIO + 0x214),
                                 alive=bool(u.mem_read(obj + 0x90, 1)[0]),
                                 limbo=bool(u.mem_read(obj + 0x81, 1)[0]),
                                 on_map=bool(u.mem_read(obj + 0x74, 1)[0]))
        if pending_serial and address == pending_serial[-1][0]:
            _, index = pending_serial.pop()
            events[index].update(result=u.reg_read(UC_X86_REG_EAX),
                                 serial_after=read32(SCENARIO + 0x214))
        if pending_germinate and address == pending_germinate[-1][0]:
            _, index, obj = pending_germinate.pop()
            events[index].update(density=u.mem_read(obj + 0x11E, 1)[0],
                                 result=u.reg_read(UC_X86_REG_EAX))
        if address == RANDOM:
            events.append(dict(kind='random', rng_before=[read32(SCENARIO + 0x21C),
                                                         read32(SCENARIO + 0x220)]))
            pending_random.append((read32(sp), len(events) - 1))
        elif address in (ADD_GROWTH, ADD_SPREAD):
            events.append(dict(kind='enqueue_growth' if address == ADD_GROWTH else 'enqueue_spread',
                               receiver=(this - TIBS) // 0x200,
                               cell=list(struct.unpack('<hh', u.mem_read(read32(sp + 4), 4)))))
            if natural and address == ADD_GROWTH:
                pending_growth_guard.append(len(events) - 1)
        elif natural and address == GROWTH_DENSITY_GUARD:
            # EAX is the original Get_CellClass result used by this CMP,
            # including any resident dummy. This is the density admission
            # sees after Mark, before Place's final amount store.
            index = pending_growth_guard.pop()
            obj = u.reg_read(UC_X86_REG_EAX)
            assert list(struct.unpack('<hh', u.mem_read(obj + 0x24, 4))) == events[index]['cell']
            events[index].update(density=u.mem_read(obj + 0x11E, 1)[0],
                                 overlay=struct.unpack('<i', u.mem_read(obj + 0x44, 4))[0])
        elif natural and address == SPREAD_GERMINATE:
            events.append(dict(kind='germinate_cell',
                               cell=list(struct.unpack('<hh', u.mem_read(this + 0x24, 4))),
                               caller=f'{read32(sp):08x}', randomize=bool(read32(sp + 4) & 0xFF),
                               before_density=u.mem_read(this + 0x11E, 1)[0],
                               overlay=struct.unpack('<i', u.mem_read(this + 0x44, 4))[0]))
            pending_germinate.append((read32(sp), len(events) - 1, this))
        elif natural and address == 0x4819CA:
            # The original IDIV consumes the completed matching-class count
            # in EAX and the native class's MaxDensity through ECX.
            events[pending_germinate[-1][1]].update(
                matching_neighbors=u.reg_read(UC_X86_REG_EAX), max_density=read32(this + 0xE4))
        elif address in (REBUILD_GROWTH, REBUILD_SPREAD):
            events.append(dict(kind='rebuild_growth' if address == REBUILD_GROWTH else 'rebuild_spread',
                               receiver=(this - TIBS) // 0x200))
        elif address == GROW_CELL:
            events.append(dict(kind='grow_cell', cell=cell_xy(this)))
        elif address == GROWTH_PROCESSOR:
            events.append(dict(kind='growth_processor', receiver=(this - TIBS) // 0x200))
        elif natural and address == SPREAD_PROCESSOR:
            events.append(dict(kind='spread_processor', receiver=(this - TIBS) // 0x200))
        elif natural and address in (SPREAD_DRIVER, GROWTH_DRIVER):
            events.append(dict(kind='spread_driver' if address == SPREAD_DRIVER else 'growth_driver'))
        elif natural and address == SPREAD_CELL:
            events.append(dict(kind='spread_cell', cell=cell_xy(this), force=read32(sp + 4)))
        elif natural and address == PLACE:
            events.append(dict(kind='place', cell=cell_xy(this), type_id=read32(sp + 4),
                               amount=read32(sp + 8)))
        elif natural and address == CAN_PLACE:
            # Read the original Cell coordinate so the resident shared Dummy
            # Cell is observable when neighbor lookup leaves the diamond.
            events.append(dict(kind='can_place', cell=list(struct.unpack('<hh', u.mem_read(this + 0x24, 4))),
                               caller=f'{read32(sp):08x}', argument=read32(sp + 4)))
            pending_can_place.append((read32(sp), len(events) - 1))
        elif natural and address == 0x5FC380:
            assert this not in constructed
            constructed[this] = len(constructed)
            events.append(dict(kind='overlay_constructor',
                               cell=list(struct.unpack('<hh', u.mem_read(read32(sp + 8), 4))),
                               overlay=read32(read32(sp + 4) + 0x294), object_index=constructed[this]))
            pending_constructor.append((read32(sp), len(events) - 1, this))
        elif natural and address == 0x410230:
            events.append(dict(kind='create_id', object_index=constructed[read32(sp + 4) - 4],
                               serial_before=read32(SCENARIO + 0x214)))
        elif natural and address == 0x68BCB0:
            assert this == SCENARIO
            events.append(dict(kind='next_unique_id', serial_before=read32(SCENARIO + 0x214)))
            pending_serial.append((read32(sp), len(events) - 1))
        elif natural and address in (0x5FC570, 0x5F65F0, 0x7258D0, 0x5FDF70, 0x5F3B80):
            if this in constructed:
                events.append(dict(kind={0x5FC570: 'overlay_mark', 0x5F65F0: 'object_uninit',
                                         0x7258D0: 'announce_expired', 0x5FDF70: 'overlay_destructor',
                                         0x5F3B80: 'object_destructor'}[address],
                                   object_index=constructed[this], native_id=read32(this + 0x10),
                                   pending_deletes=read32(0xB0F6A8)))
        elif natural and address == 0x725C70:
            events.append(dict(kind='drain_deferred', pending_deletes=read32(0xB0F6A8)))
        elif natural and address == RANGED:
            events.append(dict(kind='ranged', lo=read32(sp + 4), hi=read32(sp + 8)))
            pending_ranged.append((read32(sp), len(events) - 1))
        elif natural and address == 0x65C837:
            # RandomRanged owns an inline Random::Next copy and may reject
            # several words. Observe each original draw, never infer a count
            # from the final selection or substitute its result.
            events.append(dict(kind='random', ranged=True,
                               rng_before=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]))
            ranged_draw_index = len(events) - 1
        elif natural and address == 0x65C87E and ranged_draw_index is not None:
            events[ranged_draw_index]['raw'] = u.reg_read(UC_X86_REG_EAX)
            events[ranged_draw_index]['rng_after'] = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
            ranged_draw_index = None
        if natural:
            for event in events[event_start:]:
                event['address'] = f'{address:08x}'

    observed_coordinates = sorted(map(tuple, bitmap_coordinates.values())) if natural else [tuple(c['cell']) for c in case['cells']]

    def cell_state():
        result = [dict(cell=list(c), overlay=struct.unpack('<i', u.mem_read(cell(*c) + 0x44, 4))[0],
                       density=u.mem_read(cell(*c) + 0x11E, 1)[0]) for c in observed_coordinates]
        if natural:
            for row in result:
                row['land'] = struct.unpack('<i', u.mem_read(cell(*row['cell']) + 0xEC, 4))[0]
        return result

    u.hook_add(UC_HOOK_CODE, observe)
    steps, after_drains = [], []
    if natural:
        for frame in case.get('frames', [case['frame']]):
            first = len(events)
            u.mem_write(0xA8ED84, dwords(frame))
            # Driver timer+4 is an uninitialized caller-local padding word;
            # explicitly provide zero, as the shared TIBTRE timeline does.
            u.mem_write(SP - 0x100, bytes(0x100))
            if case['entry'] == 'growth_then_spread_driver':
                call(GROWTH_DRIVER, 0, [])
            if case['entry'] == 'spread_processor':
                call(SPREAD_PROCESSOR, TIBS + case['receiver'] * 0x200, [])
            else:
                u.mem_write(SP - 0x100, bytes(0x100))
                call(SPREAD_DRIVER, 0, [])
            assert not pending_random and not pending_ranged and not pending_can_place and ranged_draw_index is None
            assert not pending_constructor and not pending_serial
            assert not pending_growth_guard and not pending_germinate
            step_events = events[first:]
            steps.append(dict(frame=frame, state=output_state(u, read32, capacity, bitmap_coordinates, lifecycle=True),
                              cells=cell_state(), events=step_events,
                              draw_count=sum(e['kind'] == 'random' for e in step_events)))
            if case.get('drain_deferred'):
                # Original MainTick advances the frame before725C70. The
                # surrounding callbacks are excluded from this explicit seam.
                drain_frame = struct.unpack('<i', dwords(frame + 1))[0]
                u.mem_write(0xA8ED84, dwords(drain_frame))
                first = len(events)
                call(0x725C70, 0, [])
                assert not pending_constructor and not pending_serial
                after_drains.append(dict(frame=drain_frame,
                                         state=output_state(u, read32, capacity, bitmap_coordinates, lifecycle=True),
                                         cells=cell_state(), events=events[first:]))
        returned = None
    elif case['entry'] == 'reduce':
        call(REDUCE, cell(*case['target']), [case.get('amount', 1)])
        returned = u.reg_read(UC_X86_REG_EAX)
    elif case['entry'] == 'growth_processor':
        call(GROWTH_PROCESSOR, TIBS + case['receiver'] * 0x200, [])
        returned = None
    else:
        entry = ADD_GROWTH if case['queue'] == 'growth' else ADD_SPREAD
        call(entry, TIBS + case['receiver'] * 0x200, [cell(*case['target']) + 0x24])
        returned = None
    assert not pending_random
    final = output_state(u, read32, capacity, bitmap_coordinates, lifecycle=natural)
    observed_events = list(events)
    next_random = []
    for _ in range(4):
        call(RANDOM, SCENARIO + 0x218, [])
        next_random.append(u.reg_read(UC_X86_REG_EAX))
    result = dict(input=case, capacity=capacity, before=before, state=final, events=observed_events,
                bitmap_coordinate_map=[dict(index=index, cell=coord)
                                       for index, coord in sorted(bitmap_coordinates.items())],
                draw_count=sum(e['kind'] == 'random' for e in observed_events),
                next_random=next_random, returned=returned, cells=cell_state())
    if natural:
        result['steps'] = steps
        if after_drains:
            result['after_drains'] = after_drains
    return result


def generate():
    return dict(source='unicorn/gamemd.exe', cases=[execute(case) for case in cases()],
                timer_reader_cases=timer_reader_cases())


def generate_authored_init():
    """Original queue allocation/rebuild on independently physical authored cells.

    The full-map owner provides its existing pre-final Terrain phase. This
    composes that owner with these queue bodies; it creates no VM, heap, RNG,
    parser, Overlay implementation or expected queue on the host.
    """
    from collections import Counter
    from tools.native_oracle import image_sha256
    from tools.spatial_oracle.anytown_damage.navigation_inputs import (
        Inputs, identity, ASSETS, extract_tiles,
    )
    from tools.spatial_oracle.anytown_damage.navigation import Navigation, sr
    from tools.spatial_oracle.bridge_shadow_render import physical_assets

    theater = identity.theater()
    reader = Inputs(theater, map_file=ASSETS / 'XMP03T4.MAP', damage_overlays=(),
                    tiberium_timer_readers=True)
    effective_types = reader.tiberium_queue_inputs()
    scenario_theater = reader.read_map_theater()
    gates = reader.read_map_tiberium_gates()
    # The shared extraction/archive owner establishes these actual physical
    # payloads. No second archive reader or hand-authored winner table is used.
    ore_assets, ore_sources = physical_assets(ore=True)
    tile_assets, tile_sources = physical_assets(tile=True)
    physical_assets_by_name = ore_assets | tile_assets
    physical_sources = {row['name'].upper(): row for row in ore_sources + tile_sources}
    source_files = [('RULESMD.INI', identity.ri.ASSETS / 'RULESMD.INI'),
                    ('ARTMD.INI', identity.ri.ASSETS / 'ARTMD.INI'),
                    ('MPBATTLEMD.INI', identity.ri.ASSETS / 'MPBattleMD.ini'),
                    ('TEMPERATMD.INI', identity.ri.ASSETS / 'TEMPERATMD.INI'),
                    ('XMP03T4.MAP', reader.map_file)]
    manifest = []
    for name, path in source_files:
        assert path.read_bytes() == physical_assets_by_name[name], name
        manifest.append(physical_sources[name])
    manifest.append(dict(name='LANGRULE.INI', absent=not (identity.ri.ASSETS / 'LANGRULE.INI').exists(),
                         boundary='Selected physical extraction bundle; production consumer also checks its actual asset source.'))
    tiles, tile_receipt = extract_tiles(theater)
    native_map = Navigation(reader, theater, tiles, create_actor=False,
                            scenario_theater=scenario_theater, terrain_inputs_only=True)
    u = native_map.uc
    read32 = lambda pointer: sr.u32(u, pointer)
    signed = lambda value: struct.unpack('<i', dwords(value))[0]
    scenario = read32(0xA8B230)
    u.mem_write(scenario, dwords(gates['flags']))
    u.mem_write(scenario + 0x34A6, bytes([gates['basic_growth_enabled']]))
    pointers = [read32(read32(0xB0F4EC) + i * 4) for i in range(read32(0xB0F4F8))]
    capacity = native_map.call(0x42B1F0, this=0)
    bitmap_coordinates = {}
    iterator = []
    native_map.call(0x578350)
    while True:
        pointer = native_map.call(0x578290)
        if not pointer:
            break
        xy = native_map.coord(pointer)
        ordinal = native_map.call(0x42B1C0, this=pointer + 0x24)
        assert ordinal < capacity and ordinal not in bitmap_coordinates
        bitmap_coordinates[ordinal] = xy
        iterator.append(xy)
    assert set(map(tuple, iterator)) == set(native_map.ptrs)

    def rng_bytes():
        return {name: bytes(u.mem_read(pointer, 1012)) for name, pointer in native_map.rngs.items()}

    def rng_hashes(states):
        return {name: hashlib.sha256(raw).hexdigest() for name, raw in states.items()}

    def state():
        classes = []
        for kind, pointer in enumerate(pointers):
            assert read32(pointer + 0x98) == kind
            classes.append(dict(type_id=kind,
                growth=read_queue(u, read32, kind, 'growth', capacity, bitmap_coordinates,
                                  tiberium_pointer=pointer),
                spread=read_queue(u, read32, kind, 'spread', capacity, bitmap_coordinates,
                                  tiberium_pointer=pointer),
                growth_timer=list(struct.unpack('<3i', u.mem_read(pointer + 0x11C, 12))),
                spread_timer=list(struct.unpack('<3i', u.mem_read(pointer + 0x100, 12)))))
        return dict(classes=classes)

    def cell_state(xy):
        pointer = native_map.ptrs[tuple(xy)]
        return dict(cell=list(xy), overlay=sr.i32(u, pointer + 0x44),
            density=u.mem_read(pointer + 0x11E, 1)[0], tile=sr.i32(u, pointer + 0x38),
            subtile=u.mem_read(pointer + 0x11A, 1)[0], level=u.mem_read(pointer + 0x11B, 1)[0],
            slope=u.mem_read(pointer + 0x11C, 1)[0], land=sr.i32(u, pointer + 0xEC),
            flags=read32(pointer + 0x140), has_ground_object=bool(read32(pointer + 0xE4)),
            type_id=signed(native_map.call(0x485010, this=pointer)),
            value=signed(native_map.call(0x485020, this=pointer)),
            bitmap_ordinal=native_map.call(0x42B1C0, this=pointer + 0x24))

    # Only compact summaries and selected phase controls are retained. The
    # original full native source plane remains local to its established owner.
    resources = []
    for xy in iterator:
        pointer = native_map.ptrs[tuple(xy)]
        if sr.i32(u, pointer + 0x44) >= 0 and signed(native_map.call(0x485010, this=pointer)) >= 0:
            resources.append(cell_state(xy))
    summaries = []
    for kind in range(len(pointers)):
        cells = [row for row in resources if row['type_id'] == kind]
        summaries.append(dict(type_id=kind, resource_cells=len(cells),
            density_histogram=dict(sorted(Counter(row['density'] for row in cells).items())),
            flat_cells=sum(row['slope'] == 0 for row in cells),
            occupied_cells=sum(row['has_ground_object'] for row in cells)))
    timers_before = [dict(type_id=kind,
        growth_timer=list(struct.unpack('<3i', u.mem_read(pointer + 0x11C, 12))),
        spread_timer=list(struct.unpack('<3i', u.mem_read(pointer + 0x100, 12))))
        for kind, pointer in enumerate(pointers)]
    rng_before = rng_bytes()
    order = []

    def observe(_u, pc, _n, _d):
        if pc in (REBUILD_GROWTH, REBUILD_SPREAD):
            order.append(dict(entry=f'{pc:08X}', type_id=read32(u.reg_read(UC_X86_REG_ECX) + 0x98)))

    hook = u.hook_add(UC_HOOK_CODE, observe)
    native_map.call(0x722D00, this=0, count=100000000)
    native_map.call(0x722240, this=0, count=100000000)
    u.hook_del(hook)
    initialized = state()
    rng_after_init = rng_bytes()
    assert rng_before == rng_after_init
    assert all(before['growth_timer'] == after['growth_timer'] and before['spread_timer'] == after['spread_timer']
               for before, after in zip(timers_before, initialized['classes']))
    # Actual final load sweep follows this earlier initialization. Do not
    # seed expected queues from a later production/Rust cell snapshot.
    total_value = native_map.call(0x568BB0, args=(0,), count=100000000)
    final = state()
    rng_after_final = rng_bytes()
    assert initialized == final and rng_after_init == rng_after_final
    assert resources == [cell_state(row['cell']) for row in resources]
    # These declared patch queries exercise the independently native-read
    # selected AllowTiberium/Buildable/playfield prerequisites. No driver,
    # direction selection or startup RNG claim is added by these pure calls.
    u.mem_write(0xA8E9A0, b'\1')
    patch = []
    for xy in ((72, 25), (71, 24), (72, 24), (73, 24), (71, 25), (71, 26)):
        row = cell_state(xy)
        pointer = native_map.ptrs[xy]
        properties = native_map.tile_properties[row['tile']]
        head = read32(read32(0xA8ED2C) + row['tile'] * 4)
        row.update(tile_file=theater['tiles'][row['tile']],
            tile_allow_tiberium=bool(u.mem_read(head + 0x306, 1)[0]),
            tile_allow_reader=dict(file=reader.theater_file, section=f"TileSet{properties['ordinal']:04d}",
                key='AllowTiberium', default=False, entry='00546187..0054619B', store='0054646D'),
            land_buildable=bool(u.mem_read(0x89EA60 + row['land'] * 36, 1)[0]),
            playfield=bool(native_map.call(0x578460, args=(pointer + 0x24, 0)) & 255),
            can_place_riparius=bool(native_map.call(CAN_PLACE, this=pointer, args=(0,)) & 255))
        patch.append(row)
    assert rng_bytes() == rng_after_final
    flat_overlays = []
    for index in range(effective_types[0]['base_overlay'],
                       effective_types[0]['base_overlay'] + effective_types[0]['images']):
        pointer = reader.overlay_ptrs[index]
        flat_overlays.append(dict(index=index, name=reader.names[index],
                                  cell_anim_null=reader.read32(pointer + 0x29C) == 0))
    return dict(source='unicorn/gamemd.exe', native_sha256=image_sha256(),
        native_rect=native_map.case['size'], capacity=capacity,
        inputs=dict(manifest=manifest, type_readers=reader.tiberium_timer_layers,
            effective_types=effective_types, gates=gates, scenario_theater=scenario_theater,
            native_iterator=dict(count=len(iterator), first=iterator[0], last=iterator[-1],
                order_sha256=hashlib.sha256(b''.join(struct.pack('<hh', *xy) for xy in iterator)).hexdigest()),
            source_admission_summary=summaries, flat_overlay_types=flat_overlays,
            physical_tmp_archives=dict(outer_sha256=tile_receipt['outer_sha256'],
                                      inner_sha256=tile_receipt['inner_sha256']),
            admission_game_active=1),
        timers_before=timers_before, rng_before=rng_hashes(rng_before),
        init_order=order, state=initialized, rng_after_init=rng_hashes(rng_after_init),
        final_sweep=dict(entry='00568BB0', argument=0, queues_timers_unchanged=initialized == final,
            source_records_unchanged=True, tiberium_value=total_value,
            rng_after=rng_hashes(rng_after_final)), patch=patch)


def authored_init_metadata():
    return provenance(scope='Original authored XMP03T4 Riparius queue allocation and rebuild after native Cell/first Recalc/Terrain placement, before final568BB0; selected native target admission prerequisites. No natural driver or full startup claim.',
        assumptions=[
            'Existing Inputs physical RULESMD/optional LANGRULE/Battle/map layer walker and original Tiberium constructors/Image/Value/Overlay/Land readers; opt-in original721A78..721AFA timer reads share those exact passes/current-field defaults. Physical archive/member bytes are checked by the existing archive owner. Cached lexical strings and registry declaration-order transport remain explicit inputs, not whole native INI loading/registry enumeration.',
            'Existing Navigation/Geometry VM, actual original Cell constructors, physical primary TMPs, first Recalc and original Terrain constructor/direct Place phase. Whole Terrain Unlimbo/discovery and authored OverlayPack constructor/ID lifetime are excluded. No graph/actor setup, second VM/heap/parser/RNG or host expected queue is introduced.',
            'Original pre-init6B8CA0 physical SpecialFlags and Basic683848/68A571..68A58F read into temporary declared Scenario member storage on the same reader VM; results copy to the prepared map Scenario. Full_Init687853/689EAB reads these before687A85/687A8A queue initialization; GameMode1 session overwrite687C23 is later and is not substituted as the pre-init flags.',
            'Original InitGrowth722D00 then InitSpread722240 allocate/reset/rebuild all types; actual578350/578290 traversal,485010/483620/483690 admissions, zero-priority stores, x87 heap operations and42B1C0 bitmap projection execute. Output follows native member pointers. Full native Main/Scenario/MapGen states from existing seeded0 map fixture are checked unchanged across init/final sweep; no Rust startup RNG/queues become golden inputs.',
            'Final568BB0(0) executes on the same map and retains resource records/queues/timers/RNG. Only compact summaries and selected patch records are retained; no full cell-plane replay. Frame0 ctor timer priors remain unchanged, not reloaded by init.',
            'Opt-in existing TileSet scalar loop reads AllowTiberium through5295F0/defaultfalse and publishes the native result to existing IsoTile+306, matching original54646D/546AB5 store roles. Selected patch pure4838E0 controls use declared active-game1, actual native Buildable/playfield/Cell inputs. No natural direction/variant draw, driver continuation, complete startup, arbitrary map/type or cache lifetime is claimed.',
        ], substitutions=['Existing shared successful allocator/CRT/TLS/file-cache and presentation sinks; actual original Cell/rebuild/heap/admission/reader instructions are not replaced.'],
        entry_points=dict(init_growth=0x722D00, init_spread=0x722240,
            rebuild_growth=REBUILD_GROWTH, rebuild_spread=REBUILD_SPREAD,
            iterator_init=0x578350, iterator_next=0x578290, capacity=0x42B1F0,
            bitmap_ordinal=0x42B1C0, can_grow=0x483620, can_spread=0x483690,
            can_place=CAN_PLACE, type_reader=0x721A78, special_flags=0x6B8CA0,
            basic_growth=0x68A571, final_sweep=0x568BB0, tile_allow_reader=0x546187))


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--authored-init', action='store_true')
    selected, remaining = parser.parse_known_args()
    if selected.authored_init:
        finish_vectors(generate_authored_init, Path(__file__).with_name('ore_authored_init.json'),
                       provenance=authored_init_metadata, argv=remaining)
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Bounded original AddToGrowth7235A0/AddToSpread722AF0 enqueue-counter rebuilds, '
              'full RebuildGrowth7233A0/RebuildSpread7228B0, gates, reseeding order, duplicate '
              'append, cross-class receivers, timer retention and Scenario RNG continuation; '
              'three full Reduce_Tiberium480A80 caller controls and five complete '
              'GrowthProcessor722F00 calls through GrowTiberium483710/PlaceTiberium487190 '
              'and spread-counter rebuild admission. Full natural SpreadProcessor722440, '
              'SpreadDriver7221B0, ordered GrowthDriver722C40-before-SpreadDriver histories, '
              'active-match target selection/Overlay constructor and timer cadence; selected '
              'original Tiberium constructor defaults and ReadINI timer/percentage stores. '
              'Original Mark SpreadCellGerminate density before growth admission, including '
              'eight same-class versus seven matching neighbors in an interior empty hole. '
              'Retained shared Dummy state, including edge neighbor lookups before occupied '
              'or Scenario-disabled source refusal. '
              'Natural rows observe original Scenario serial allocation, ObjectUnInit/announce '
              'and pending Overlay state; two explicit full deferred drains preserve serial/cell '
              'effects while finalizing successful transient Overlays and retaining Terrain-refused limbo.',
        entry_points={'add_growth': ADD_GROWTH, 'add_spread': ADD_SPREAD,
                      'rebuild_growth': REBUILD_GROWTH, 'rebuild_spread': REBUILD_SPREAD,
                      'can_grow': 0x483620, 'can_spread': 0x483690,
                      'iterator_init': 0x578350, 'iterator_next': 0x578290,
                      'capacity': 0x42B1F0, 'bitmap_ordinal': 0x42B1C0,
                      'reduce_tiberium': REDUCE, 'random': RANDOM,
                      'growth_processor': GROWTH_PROCESSOR, 'grow_tiberium': GROW_CELL,
                      'place_tiberium': PLACE, 'can_place_tiberium': CAN_PLACE,
                      'spread_tiberium': SPREAD_CELL, 'spread_processor': SPREAD_PROCESSOR,
                      'neighbor_cell': 0x481810, 'packed_cell_lookup': 0x5657A0,
                      'spread_driver': SPREAD_DRIVER, 'growth_driver': GROWTH_DRIVER,
                      'ranged': RANGED, 'ranged_inline_draw': 0x65C837,
                      'overlay_constructor': 0x5FC380, 'overlay_mark': 0x5FC570,
                      'spread_cell_germinate': SPREAD_GERMINATE,
                      'growth_density_guard': GROWTH_DENSITY_GUARD,
                      'create_id': 0x410230, 'next_unique_id': 0x68BCB0,
                      'object_uninit': 0x5F65F0, 'announce_expired': 0x7258D0,
                      'deferred_drain': 0x725C70, 'overlay_destructor': 0x5FDF70,
                      'object_destructor': 0x5F3B80,
                      'tiberium_constructor_members': 0x7216CF,
                      'tiberium_timer_readers': 0x721A78},
        assumptions=['Shared harvest_field/refinery_dock fixture: original Cell/Unit vtables, '
                     'original Scenario seeder, declared successful Size4x4 diamond over '
                     'the existing 32x32 backing table; CellIterator executes until first null.',
                     'Four synthetic TiberiumClass instances retain the shared fixture overlay '
                     'registry (Riparius102..121, Cruentus27..38); MaxDensity12 and per-case '
                     'percentage/Scenario gates, cell density/slope/occupancy are supplied data.',
                     'Adequate external entries/heap/bitmap stores, explicit append count and '
                     'valid heap references represent prior queue state; initialization/allocator '
                     'failure, whole Logic/map startup and actual long-session history are excluded.',
                     'Natural spread rows reuse the existing TIBTRE fixture with GameActive1, '
                     'real Unit/Building/Terrain and Overlay vtables, resident1x1 TMP/tile and '
                     'Buildable land rows, initialized empty listener/Overlay registries. '
                     'Declared source/target objects, tile/land/slope/structural flags and '
                     'adequate allocator storage are prepared state; original admission, '
                     'Overlay constructor/Mark/Recalc and target placement execute. '
                     'Natural germination events read the original matching-count IDIV inputs '
                     'and return density; enqueue_growth density/overlay are read from the '
                     'original Get_CellClass EAX pointer at7235C1 before its admission compare. '
                     'Every natural before/step/final/after_drains state reads the resident '
                     'DummyABDC50 coordinate, overlay/density, land, level/slope and raw flags '
                     'without invoking a lookup. Its retained fields come from the existing '
                     'prepared shared fixture, not a full MapClass/CellClass startup claim. '
                     'The existing tree at16,16 is outside this Size4x4 diamond and is used '
                     'only as the real object identity of declared spawner-blocked targets.',
                     'Driver rows invoke the complete original functions per declared frame. '
                     'Growth-before-spread composition supplies their observed Logic caller '
                     'order and excludes unrelated surrounding Logic callbacks. Signed timer '
                     'anchors/raw durations execute; each opaque timer+4 padding input is '
                     'explicitly zero, with no gameplay meaning inferred from that word.',
                     'Selected Tiberium constructor7216CF..7217A6 and ReadINI721A78..721AFA '
                     'execute original zero/double defaults and ReadInt/ReadDouble stores '
                     'over shared cached lexical INI indexes. AbstractType construction, '
                     'full type registry discovery and physical INI file loading are excluded. '
                     'Four retail RULESMD sections carry physical file identity and original '
                     'reader-established timer/percentage outputs used by selected driver rows.',
                     'Natural Scenario+214 serial is explicitly supplied before dispatch, normally0 '
                     'with twoFFFFFFFF wrapping controls. Original410230/68BCB0 assign the actual '
                     'stored Object+10 values. Registry/pending counts are read from native vectors; '
                     'constructor return alive/limbo/on-map flags are native fields, never inferred '
                     'from cell installation. Two drain_deferred rows invoke original725C70 at '
                     'frame+1 matching MainTick ordering, with predrain steps and after_drains '
                     'state. Surrounding MainTick callbacks, populated external observers, broader '
                     'limbo-survivor persistence and full native save/load are excluded.',
                     'Timer+4 padding is supplied zero; sentinel frame/duration values are compared '
                     'for every class. X87 control0x0E7F comes from the shared fixture; signed frame '
                     'wrap and >2^24 chopped FILD/FSTP boundaries execute. Four next_raw controls '
                     'replace one seeded RNG data word before original Random::Next, including '
                     'INT_MIN signed-absolute overflow; no draw return is hooked.'],
        substitutions=['Shared fixture OS Interlocked imports and unused refinery presentation '
                       'sinks remain. Reduce_Tiberium observers supply empty tactical rectangles, '
                       'RecalcAttributes bare-land publication and radar/tactical dirtiness sinks. '
                       'Natural spread rows inherit the existing TIBTRE allocation/free storage '
                       'service hooks and empty tactical/radar dirty sinks; original Overlay '
                       'attribute recalculation executes. No ore admission, queue, rebuild, '
                       'iterator, timer, direction/variant choice or RNG return is substituted. '
                       'The two explicit drains reuse OriginalBridgeConstructor Windows SEH '
                       'empty-chain and IsBadReadPtr storage services; every supplied successful '
                       'probe validates mapped memory. Original queue/drain/destructor/announce '
                       'instructions execute, with existing allocation/free services as storage sinks.']),
        source_paths={'producer': Path(__file__), 'harvest_field': Path(__file__).with_name('harvest_field.py'),
                      'refinery_dock': Path(__file__).with_name('refinery_dock.py'),
                      'tibtre': Path(__file__).with_name('tibtre.py'),
                      'unit_scatter_state': Path(__file__).with_name('unit_scatter_state.py'),
                      'unit_source_scatter': Path(__file__).with_name('unit_source_scatter.py'),
                      'map_queries': Path(__file__).with_name('map_queries.py'),
                      'building_body_rules': Path(__file__).with_name('building_body_rules.py'),
                      'bridge_constructor': Path(__file__).with_name('bridge_constructor.py'),
                      'bridge_landing_inputs': Path('tools/rules_oracle/bridge_landing_inputs.py'),
                      'bridge_child_sound': Path('tools/rules_oracle/bridge_child_sound.py'),
                      'guided_controls': Path('tools/rules_oracle/guided_controls.py')})
