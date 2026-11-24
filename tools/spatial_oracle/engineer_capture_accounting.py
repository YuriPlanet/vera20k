"""Bounded original capture accounting, composed from EngineerJoinedFixture.

Whole native callbacks produce every retained counter/score result. This adds
observation and supplied boundary controls, not another gameplay fixture. See
engineer_capture_accounting.md for reproduction and the inherited limits.
Use --entered-owner for the joined two-callback owner controls.
"""
import hashlib
import struct
import sys
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EIP

from tools import native_oracle as native
from tools.spatial_oracle import building_construction as construction
from tools.spatial_oracle import engineer_repair_admission as engineer
from tools.spatial_oracle.map_queries import dwords


SOURCE_PATHS = (
    'tools/spatial_oracle/engineer_capture_accounting.py',
    'tools/spatial_oracle/engineer_repair_admission.py',
    'tools/spatial_oracle/building_construction.py', 'tools/native_oracle.py',
    'tools/rules_oracle/bridge_anim_inputs.py',
    'tools/rules_oracle/bridge_child_sound.py',
    'tools/rules_oracle/bridge_anim_lists.py',
    'tools/projectile_oracle/flat_art.py',
    'tools/spatial_oracle/refinery_dock.py',
    'tools/spatial_oracle/track_destination.py',
    'tools/spatial_oracle/unit_entry.py',
    'tools/spatial_oracle/unit_scatter_state.py',
    'tools/spatial_oracle/unit_source_scatter.py',
    'tools/spatial_oracle/building_body_rules.py',
    'tools/spatial_oracle/map_queries.py',
    'tools/spatial_oracle/anim_bouncer_launch.py',
    'tools/spatial_oracle/building_slot_replacement.py',
    'tools/spatial_oracle/anytown_damage/mission.py',
    'tools/spatial_oracle/tag_lifecycle.py',
    'tools/spatial_oracle/anytown_damage/navigation.py',
    'tools/spatial_oracle/shrapnel_repair/zone_composition.py',
    'tools/spatial_oracle/shrapnel_repair/hierarchy_composition.py',
)

# These PCs identify native sites to observe, never answers or injected code.
ACCOUNTING_PCS = {
    0x702D40: 'record_the_kill', 0x702E54: 'record_kill_dont_score_gate',
    0x703045: 'record_kill_insignificant_gate',
    0x70305C: 'record_kill_building_loss',
    0x7014A0: 'change_owner', 0x7015A8: 'change_owner_null_callback',
    0x7015D0: 'change_owner_score_add', 0x7015D2: 'change_owner_score_store',
    0x7015DE: 'change_owner_remove_tracking',
    0x7015E6: 'change_owner_add_tracking',
    0x70164D: 'change_owner_kill_counter',
    0x701735: 'change_owner_owner_store',
    0x519F4F: 'arrival_capture_notification',
    0x4FB6B0: 'record_last_built',
    0x49FA00: 'counter_increment',
    0x49FA47: 'built_item_increment', 0x49FA50: 'built_total_increment',
}


def accounting_state(fixture, house):
    """Raw retained fields; the total-only Rust projection is a test concern."""
    read, u = fixture.read32, fixture.u
    return dict(
        pointer=house,
        building_losses=read(house + 0x5488),
        building_kills=[read(house + 0x5438 + i * 4) for i in range(20)],
        score=struct.unpack('<i', u.mem_read(house + 0x54E8, 4))[0],
        notification=u.mem_read(house + 0x244, 1)[0],
        owner=read(engineer.BLD + 0x21C),
        health=read(engineer.BLD + 0x6C),
    )


class AccountingObservation:
    """Read-only hooks attached to the existing native fixture owner."""
    def __init__(self, fixture, houses):
        self.fixture = fixture
        self.rng_before = fixture.rng()
        self.runtime_globals = {f'0x{pc:08X}': fixture.read32(pc)
                                for pc in (0xAC13C8, 0xAC13BC)}
        self.trace_mark, self.event_mark = len(fixture.trace), len(fixture.events)
        self.draw_mark, self.advance_mark = len(fixture.draws), len(fixture.advances)
        self.visited, self.sequence, self.instructions, self.writes = set(), [], {}, []
        self.watched = {}
        for name, house in houses.items():
            self.watched.update({house + offset: name for offset in (
                0x244, 0x5488, 0x54E8, 0x55B0, 0x246, 0x1FC,
                *(0x5438 + i * 4 for i in range(20)),
            )})
        self.hooks = [
            fixture.u.hook_add(UC_HOOK_CODE, self.code),
            fixture.u.hook_add(UC_HOOK_MEM_WRITE, self.write),
        ]

    def code(self, u, pc, size, _data):
        self.visited.add(pc)
        self.sequence.append(pc)
        if pc in ACCOUNTING_PCS:
            self.instructions[f'0x{pc:08X}'] = dict(
                site=ACCOUNTING_PCS[pc], bytes=bytes(u.mem_read(pc, size)).hex())

    def write(self, u, _access, address, size, value, _data):
        if address in self.watched:
            self.writes.append(dict(
                house=self.watched[address], address=f'0x{address:08X}',
                pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}', size=size,
                before=bytes(u.mem_read(address, size)).hex(),
                value=value & ((1 << (8 * size)) - 1),
            ))

    def finish(self, required):
        f = self.fixture
        for hook in self.hooks:
            f.u.hook_del(hook)
        missing = set(required) - self.visited
        if missing:
            raise native.OracleError(f'Accounting route omitted required native PCs: {sorted(hex(pc) for pc in missing)}')
        if not f.code_unchanged() or f.pending:
            raise native.OracleError('Accounting route changed PE code or left an unfinished native RNG call')
        return dict(
            rng_before=self.rng_before, rng_after=f.rng(),
            native_runtime_globals=self.runtime_globals,
            native_draws=f.draws[self.draw_mark:],
            raw_rng_advances=f.advances[self.advance_mark:],
            trace=f.trace[self.trace_mark:], lifecycle=f.events[self.event_mark:],
            accounting_writes=self.writes, native_instructions=self.instructions,
            executed_instruction_count=len(self.sequence),
            executed_unique_instruction_count=len(self.visited),
            executed_instruction_pc_sha256=hashlib.sha256(dwords(*self.sequence)).hexdigest(),
            executed_unique_pc_sha256=hashlib.sha256(dwords(*sorted(self.visited))).hexdigest(),
            unchanged_executable_sections=True,
        )


def record_kill_controls(inputs):
    rows = []
    for name, dont_score, insignificant, sale, initial_loss, repetitions in (
        ('live_null', False, False, False, 0, 1),
        ('repeated_live_null', False, False, False, 0, 2),
        ('null_dont_score', True, False, False, 0, 1),
        ('null_insignificant', False, True, False, 0, 1),
        ('null_sale_loss_suppression', False, False, True, 0, 1),
        ('null_loss_wrap', False, False, False, 0xFFFFFFFF, 1),
    ):
        f = engineer.EngineerJoinedFixture(inputs)
        f.repair_target_prior()
        u, kind = f.u, f.types['GAPOWR']
        u.mem_write(kind + 0xC9F, bytes([dont_score]))
        u.mem_write(kind + 0x232, bytes([insignificant]))
        if sale:
            # Native sale44A1EF produces this before44A1F9. Its full producer
            # is excluded; this is a supplied loss-suppression gate control.
            u.mem_write(engineer.BLD + 0x53C, dwords(-1))
        u.mem_write(engineer.HOUSE + 0x5488, dwords(initial_loss))
        before = accounting_state(f, engineer.HOUSE)
        observation = AccountingObservation(f, {'owner': engineer.HOUSE})
        after = []
        for index in range(repetitions):
            f.phase = f'{name}_callback_{index}'
            construction.invoke(u, 0x702D40, engineer.BLD, 0)
            after.append(accounting_state(f, engineer.HOUSE))
        rows.append(dict(
            name=name,
            input=dict(dont_score=dont_score, insignificant=insignificant,
                       supplied_sale_53c=-1 if sale else None,
                       initial_loss=initial_loss, repetitions=repetitions),
            before=before, after=after,
            observation=observation.finish((0x702D40, 0x45EDD0)),
        ))
    return rows


def capture_controls(inputs):
    rows = []
    for name, dont_score, insignificant, overflow in (
        ('capture_stock', False, False, False),
        ('capture_dont_score', True, False, False),
        ('capture_insignificant', False, True, False),
        ('capture_wrap', False, False, True),
    ):
        f = engineer.EngineerJoinedFixture(inputs)
        f.repair_target_prior()
        actor = f.actor((9, 10))
        command = f.resolved_command(actor, f.ordinary_action(actor))
        old = f.second_house_prior(allied=False)
        f.owner_houses.append(old)
        u, kind = f.u, f.types['GAPOWR']
        u.mem_write(kind + 0xC9F, bytes([dont_score]))
        u.mem_write(kind + 0x232, bytes([insignificant]))
        if overflow:
            u.mem_write(old + 0x5488, dwords(0xFFFFFFFF))
            u.mem_write(engineer.HOUSE + 0x54E8, dwords(0x7FFFFFFF))
            # The existing fixture supplies old House ArrayIndex1. The
            # original ChangeOwner chooses the slot from that admitted prior.
            u.mem_write(engineer.HOUSE + 0x5438 + 4, dwords(0xFFFFFFFF))
        houses = {'old': old, 'new': engineer.HOUSE}
        before = {name: accounting_state(f, house) for name, house in houses.items()}
        observation = AccountingObservation(f, houses)
        f.phase = name
        f.walk_arrival(actor, 10)
        after = {name: accounting_state(f, house) for name, house in houses.items()}
        required = (0x75AEC0, 0x519630, 0x448260, 0x7014A0,
                    0x702D40, 0x7015D0, 0x701735)
        if not dont_score:
            required += (0x70164D,)
        rows.append(dict(
            name=name,
            input=dict(dont_score=dont_score, insignificant=insignificant,
                       supplied_overflow_counters=overflow),
            command=command, before=before, after=after,
            observation=observation.finish(required),
        ))
    return rows


def built_controls(inputs):
    rows = []
    for name, prior in (('built_zero', 0), ('built_wrap', 0xFFFFFFFF)):
        f = engineer.EngineerJoinedFixture(inputs)
        f.repair_target_prior()
        counter = engineer.HOUSE + 0x55A0
        slot = f.heap
        f.heap += 0x100
        # Valid capacity-one prior Counter storage. Original49FA00's
        # existing-capacity branch does not query a Counter vtable.
        f.u.mem_write(counter + 4, dwords(slot, 1))
        f.u.mem_write(slot, dwords(prior))
        f.u.mem_write(counter + 0x10, dwords(prior))
        before = dict(item=f.read32(slot), total=f.read32(counter + 0x10))
        observation = AccountingObservation(f, {'owner': engineer.HOUSE})
        f.phase = name
        construction.invoke(f.u, 0x4FB6B0, engineer.HOUSE, engineer.BLD)
        after = dict(item=f.read32(slot), total=f.read32(counter + 0x10))
        rows.append(dict(
            name=name, input=dict(initial_item_and_total=prior, supplied_capacity=1),
            before=before, after=after,
            observation=observation.finish((0x4FB6B0, 0x49FA00, 0x49FA47, 0x49FA50)),
        ))
    return rows


def generate():
    root = engineer.JOINED_ROOT
    inputs = engineer.prepare_joined_inputs(root)
    pointer = inputs.types['GAPOWR']
    retail_files = [dict(name=p.name, size=p.stat().st_size,
                         sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                    for p in sorted(root.iterdir()) if p.is_file() and p.suffix.lower() != '.json']
    return dict(
        schema_version=1, kind='bounded-original-engineer-capture-accounting',
        source='unicorn/gamemd.exe',
        retail_files=retail_files,
        rules_building_layers=inputs.layers, engineer_layers=inputs.engineer_layers,
        native_building_entry_flags=inputs.building_entry,
        native_type=dict(name=inputs.string(pointer + 0x24),
                         strength=inputs.read32(pointer + 0xA0)),
        original_executable_sections=inputs.code_identity,
        record_kill_controls=record_kill_controls(inputs),
        capture_controls=capture_controls(inputs), built_controls=built_controls(inputs),
    )


def entered_owner_control(inputs, first_change, second_change):
    """Original two Entered callbacks, Action36, capture and deferred drain.

    Definitions and admitted House/map priors are supplied. All live predicate,
    action, ChangeOwner, navigation and cleanup decisions execute original code.
    """
    from tools.spatial_oracle import tag_lifecycle
    from tools.spatial_oracle.anytown_damage.navigation import Navigation, MAP

    f = engineer.EngineerJoinedFixture(inputs, live_tags=True, arena_size=0x1000000)
    # The inherited fixture's runtime mode1 is a supplied prior. Override it
    # before target setup, actor construction and command delivery to Skirmish5.
    f.u.mem_write(0xA8B238, dwords(5))
    infantry_vtable = bytes(f.u.mem_read(engineer.INF_VTABLE, 0x55C))
    f.repair_target_prior()
    u = f.u
    houses = [engineer.HOUSE]
    for index in range(1, 4):
        house = f.allocate(0x6000)
        u.mem_write(house, bytes(u.mem_read(engineer.HOUSE, 0x6000)))
        u.mem_write(house + 0x30, dwords(index))
        u.mem_write(house + 0x1EC, b'\1\1')
        u.mem_write(house + 0x5788, dwords(1 << index))
        houses.append(house)
    countries = []
    for index, house in enumerate(houses):
        country = f.allocate(0x200)
        u.mem_write(country, bytes(u.mem_read(inputs.country, 0x200)))
        #7265F7 reads+B4;502D4A compares+B8. Both are explicit identities,
        # not a claim that arbitrary Country constructors make them equal.
        identity = (0, 7, 2, 3)[index]
        u.mem_write(country + 0xB4, dwords(identity, identity))
        u.mem_write(house + 0x34, dwords(country))
        countries.append(country)
        f.house_prior(house, (engineer.BLD,) if index == 2 else ())
        for offset in (0x5528, 0x5578):
            u.mem_write(house + offset, dwords(0x7E449C, f.allocate(128 * 4), 128, 1, 1, 10))
    actor = f.actor((9, 10))
    f.resolved_command(actor, f.ordinary_action(actor))
    # Same delivered-friendly-order/enroute-owner race as the existing corpus.
    u.mem_write(engineer.BLD + 0x21C, dwords(houses[2]))
    items = f.allocate(4 * len(houses))
    u.mem_write(items, dwords(*houses))
    u.mem_write(0xA8022C, dwords(items))
    u.mem_write(0xA80238, dwords(len(houses)))
    f.owner_houses = houses
    chosen = ('tag', 'tag_trigger_base', 'trigger', 'logic_tags', 'map_tags', 'cell_tags')
    tag_lifecycle.initialize_registries(u, {name: f.allocate(128 * 4) for name in chosen})
    tag_type = f.allocate(0xA4)
    trigger_types = [f.allocate(0xC0) for _ in range(2)]
    # Constructor6E4DE0 pushes front: TypeT0->T1 becomes liveT1->T0.
    u.mem_write(tag_type + 0x9C, dwords(2, trigger_types[0]))
    for branch, typ in enumerate(trigger_types):
        source = 0 if branch == 0 or not first_change else 1
        event, action = f.allocate(0xC0), f.allocate(0x140)
        u.mem_write(typ + 0x9C, b'\1\1\1\1')
        u.mem_write(typ + 0xA4, dwords(countries[source],
                    trigger_types[1] if branch == 0 else 0, event, action))
        u.mem_write(event + 0x28, dwords(event + 0x60, 1, 0, (0, 7, 2, 3)[source]))
        u.mem_write(event + 0x88, dwords(0, 28 if branch == 0 else 27, 0, 11))
        mutate = first_change if branch == 0 else second_change
        if mutate:
            u.mem_write(action + 0x28, dwords(action + 0xA0, 36))
            u.mem_write(action + 0x90, dwords(7 if branch == 0 else 3))
            action += 0xA0
        u.mem_write(action + 0x28, dwords(0, 28))
        u.mem_write(action + 0x90, dwords(11 if branch == 0 else 13))
    tag = f.allocate(0x38)
    construction.invoke(u, 0x6E4DE0, tag, tag_type)
    construction.invoke(u, 0x5F5B50, engineer.BLD, tag)

    # Action36 clears NavCom. Capture evacuation then executes Scatter and
    # pathfinding, so install the existing native workspace/graph producer.
    u.mem_write(MAP + 0xEC, dwords(0, 0, 16, 16))
    u.mem_write(MAP + 0xFC, dwords(0, 0, 32, 32))
    plane = bytearray(b'\x07\0\0\0' * (33 * 33))
    for y in range(1, 32):
        for x in range(1, 32):
            struct.pack_into('<BBH', plane, (y * 33 + x) * 4, 0, 0, 0)
    f.phase = 'setup_flat_connectivity'
    nav = Navigation.on_existing_map(f, size=(16, 16), class_height_plane=bytes(plane))
    nav.finish_graphs()
    Navigation.setup_pathfinder(nav.call)
    observation = AccountingObservation(f, {str(i): house for i, house in enumerate(houses)})

    def observe(vm, pc, _size, _data):
        sp = vm.reg_read(engineer.UC_X86_REG_ESP)
        if pc == 0x6E53A0 and f.read32(sp + 4) == 1:
            assert f.read32(sp + 8) == actor

    hook = u.hook_add(UC_HOOK_CODE, observe)
    f.phase = 'tagged_capture'
    f.walk_arrival(actor, 10)
    f.phase = 'tagged_capture_deferred_drain'
    construction.invoke(u, 0x725C70, 0)
    u.hook_del(hook)
    runtime_game_mode = f.read32(0xA8B238)
    if runtime_game_mode != 5:
        raise native.OracleError('Tagged capture did not retain Skirmish runtime mode5')
    # Keep original-code, route and RNG-call integrity checks internal;
    # the fixture stores only the semantic fields the Rust regression reads.
    observation.finish((0x519630, 0x519B53, 0x519F44, 0x519F71,
                        0x6E53A0, 0x71E940, 0x7265C0, 0x448260,
                        0x7014A0, 0x725C70, 0x517D90))
    if bytes(u.mem_read(engineer.INF_VTABLE, 0x55C)) != infantry_vtable:
        raise native.OracleError('Tagged capture changed the Infantry vtable')
    return dict(first_change=first_change, second_change=second_change,
                runtime_game_mode=runtime_game_mode,
                owner=houses.index(f.read32(engineer.BLD + 0x21C)))


def entered_owner_generate():
    inputs = engineer.prepare_joined_inputs(engineer.JOINED_ROOT)
    return dict(source='unicorn/gamemd.exe',
        controls=[entered_owner_control(inputs, first, second)
                  for first, second in ((False, False), (True, False), (False, True), (True, True))])


def entered_owner_metadata():
    return dict(
        schema_version=1,
        scope='Original Engineer two Entered callbacks, current-owner capture and deferred destruction; supplied admitted target, House/map priors and final arrival head. See engineer_capture_accounting.md for boundaries.')


def metadata():
    result = native.provenance(
        scope='Bounded whole RecordTheKill702D40(NULL), resolved Engineer Walk/PerCell capture through448260/7014A0, and whole Record_Last_Built4FB6B0/Counter49FA00 accounting controls; supplied explicit flags/counters, not whole death/sale/House/score equivalence.',
        assumptions=[
            'Reuses the existing EngineerJoinedFixture without a second map, rules, lifecycle, RNG, audio, or native gameplay owner. Its startup, active-retail readers, target admission prior, physical cells, native vtables, platform/presentation boundaries and complete original-code checks are retained in inherited_fixture.',
            'The target is already admitted untagged damaged GAPOWR with positive actual HP374, estimated611 and sampled374. Direct NULL controls enter whole702D40 with sourceNULL. Capture controls deliver the original resolved repair order, then externally change target ownership before whole Walk/PerCell arrival. Travel/path payment remains supplied prior.',
            'DontScore/Insignificant overrides, sale Building+53C=-1 and maximum counters are explicit boundary inputs. Stock fields and CostOf inputs come from original constructors/readers. The sale datum is produced by native44A1EF before its44A1F9 callback; the producer itself is not executed here.',
            'Built controls supply valid capacity-one Counter storage for native-produced GAPOWR ArrayIndex0 and enter whole4FB6B0. Counter allocation/growth and full PLACE are excluded. The original existing-capacity49FA00 body owns item/total increments.',
            'Every tested route records unchanged complete PE executable sections, exact observed original instruction bytes, PC-sequence hashes, raw account stores, full Main/Scenario/MapGen RNG states and native requested/raw draw history. Retained per-victim-house tables are raw fields; Rust tests compare only the existing total-only projection.',
        ],
        substitutions=[
            'Only observation hooks and explicit data controls are added. Existing fixture platform, allocator and presentation interfaces are inherited; no native accounting, CostOf, capture, counter, RNG or lifecycle decision is answered or instruction patched.',
            'This does not certify complete lethal/sale/live Tag/TEvent polling, radar redraw, native House checksum, score-screen aggregation or the harvested/object score split at arbitrary overflow. Full House/Logic scheduling and ordinary travel remain outside the comparison.',
        ],
        entry_points={
            'record_the_kill': 0x702D40, 'building_cost_of': 0x45EDD0,
            'type_cost_of': 0x711F00, 'walk_process': 0x75AEC0,
            'infantry_per_cell': 0x519630, 'building_change_owner': 0x448260,
            'techno_change_owner': 0x7014A0,
            'capture_notification_store': 0x519F4F,
            'change_owner_null_callback': 0x7015A8,
            'change_owner_score_add': 0x7015D0,
            'change_owner_kill_count': 0x70164D,
            'record_last_built': 0x4FB6B0, 'counter_increment': 0x49FA00,
            'built_item_increment': 0x49FA47, 'built_total_increment': 0x49FA50,
        },
    )
    result['inherited_fixture'] = engineer.joined_metadata()
    return result


def main(argv=None):
    root = Path(__file__).resolve().parents[2]
    argv = list(sys.argv[1:] if argv is None else argv)
    entered = '--entered-owner' in argv
    if entered:
        argv.remove('--entered-owner')
    output = Path(__file__).with_name('engineer_capture_entered_owner.json') if entered else Path(__file__).with_suffix('.json')
    native.finish_vectors(entered_owner_generate if entered else generate, output,
                          provenance=entered_owner_metadata if entered else metadata, argv=argv,
                          source_paths={name: root / name for name in SOURCE_PATHS})


if __name__ == '__main__':
    main()
