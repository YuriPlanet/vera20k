"""Original sale completion with live Tag callbacks, stopped before Limbo."""
import hashlib
import os
import struct
from pathlib import Path

from unicorn.x86_const import (UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                              UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP)

from tools import native_oracle as native
from tools.spatial_oracle import building_construction as construction
from tools.spatial_oracle import engineer_repair_admission as engineer
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.building_body_rules import INI
from tools.rules_oracle.bridge_child_sound import sections
from tools.spatial_oracle.refinery_dock import place_building, cell
from tools.spatial_oracle.tag_lifecycle import REGISTRIES, initialize_registries
from tools.spatial_oracle.unit_source_scatter import SCENARIO

BLD, HOUSE, SP = engineer.BLD, engineer.HOUSE, engineer.SP
BEGIN, END = 0x44A1E8, 0x44A22C
TYPE_REGISTRIES = (0x8B4178, 0xB0E780, 0xB0F1A0, 0xB0E658, 0xB0F658)


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


class Fixture(engineer.EngineerJoinedFixture):
    def __init__(self, inputs):
        self.sale_calls = []
        self.live_triggers = []
        self.houses = {'OLD': HOUSE}
        self.recording = False
        self.reached_presentation = set()
        super().__init__(inputs)
        u = self.u
        # Physical BuildingType pointers refer into this original static table;
        # its reader-VM bytes are not part of the cloned heap.
        construction.invoke(u, 0x45B1C0, 0)
        for address in TYPE_REGISTRIES:
            u.mem_write(address, bytes(inputs.u.mem_read(address, 24)))
        initialize_registries(u, {name: self.allocate(128 * 4) for name in
            ('tag', 'tag_trigger_base', 'trigger', 'logic_tags', 'map_tags', 'cell_tags')})
        self.checked_vtables = [(address, native.file_span(native.image_bytes(), address, length)[1])
            for address, length in ((0x7E3EBC, 0x500), (0x7EA8A0, 0x200))]
        self.phase = 'building_constructor'
        construction.invoke(u, 0x43B740, BLD, self.types['GAPOWR'], HOUSE)
        self.constructor_loss_field = signed(u, BLD + 0x53C)
        self.constructor_identity = self.read32(BLD + 0x10)
        for y in range(32):
            for x in range(32):
                u.mem_write(cell(x, y) + 0xE4, dwords(0))
                u.mem_write(cell(x, y) + 0x12C, dwords(0x18))
                u.mem_write(cell(x, y) + 0x48, dwords(-1))
        place_building(u, BLD, (10, 10),
                       constructor_abstract_flags=self.read32(BLD + 0x14))
        u.mem_write(BLD + 0x520, dwords(self.types['GAPOWR']))
        for offset, value in ((0x90, 1), (0x74, 1), (0x81, 0), (0x98, 1)):
            u.mem_write(BLD + offset, bytes([value]))
        for y in range(10, 12):
            for x in range(10, 12):
                u.mem_write(cell(x, y) + 0xE4, dwords(BLD))
        self.house_prior(HOUSE, (BLD,))
        self.owner_houses = [HOUSE]
        self.phase = 'grand_opening_prior'
        construction.invoke(u, construction.GRAND_OPENING, BLD, 0)
        assert u.mem_read(BLD + 0x6E4, 1)[0] == 1
        self.phase = 'prepared_completion'
        u.mem_write(BLD + 0xAC, dwords(19))
        u.mem_write(BLD + 0xBC, dwords(2))
        u.mem_write(BLD + 0x6C, dwords(self.read32(self.types['GAPOWR'] + 0xA0)))
        u.mem_write(HOUSE + 0x30C, dwords(1000))

    def owner_name(self, pointer):
        return next((name for name, value in self.houses.items() if value == pointer), None)

    def hook(self, u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        receiver = u.reg_read(UC_X86_REG_ECX)
        if pc == 0x725FA0:
            self.live_triggers.append(receiver)
        if self.recording:
            if pc in construction.PRESENTATION:
                self.reached_presentation.add(pc)
            row = None
            if pc == 0x6E53A0:
                row = dict(kind='tag_event', event=signed(u, sp + 4),
                           source_null=self.read32(sp + 20) == 0)
            elif pc == 0x6DD8B0:
                row = dict(kind='action', action=signed(u, receiver + 0x2C),
                           value=signed(u, receiver + 0x90))
            elif pc == 0x448260:
                row = dict(kind='change_owner', owner=self.owner_name(self.read32(sp + 4)))
            elif pc == 0x4F9950:
                row = dict(kind='add_credits', owner=self.owner_name(receiver),
                           amount=signed(u, sp + 4))
            elif pc == 0x70ADA0:
                row = dict(kind='refund', owner=self.owner_name(self.read32(BLD + 0x21C)))
            elif pc == 0x702D40:
                row = dict(kind='record_kill', source_null=self.read32(sp + 4) == 0,
                           loss_field=signed(u, BLD + 0x53C))
            elif pc in (0x6E2050, 0x442230, 0x5F65F0):
                row = dict(kind={0x6E2050:'destroy_attached', 0x442230:'receive_damage',
                                 0x5F65F0:'uninit'}[pc])
            if row is not None:
                self.sale_calls.append(dict(pc=f'0x{pc:08X}', **row))
        if pc == 0x6E53A0:
            # The parent fixture's contract is Tag=NULL. Only lift its guard:
            # leave EIP/registers/stack intact and execute the original body.
            self.executed.add(pc)
            return
        return super().hook(u, pc, size, data)

    def write(self, u, access, address, size, value, data):
        if self.recording and address == BLD + 0x53C:
            self.sale_calls.append(dict(pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',
                kind='loss_field_write', value=struct.unpack('<i', dwords(value))[0]))
        return super().write(u, access, address, size, value, data)

    def attach(self, typ):
        self.tag = construction.invoke(self.u, 0x6E52A0, typ)
        construction.invoke(self.u, 0x5F5B50, BLD, self.tag)

    def add_house(self, country):
        u = self.u
        other = self.allocate(0x6000)
        u.mem_write(other, bytes(u.mem_read(HOUSE, 0x6000)))
        u.mem_write(other + 0x30, dwords(1))
        u.mem_write(other + 0x34, dwords(country))
        u.mem_write(other + 0x1ED, b'\0')
        u.mem_write(other + 0x1EC, b'\1')
        u.mem_write(other + 0x30C, dwords(2000))
        u.mem_write(other + 0x5788, dwords(2))
        self.house_prior(other)
        items = self.allocate(8)
        u.mem_write(items, dwords(HOUSE, other))
        u.mem_write(0xA8022C, dwords(items))
        u.mem_write(0xA80238, dwords(2))
        self.houses['NEW'] = other
        self.owner_houses.append(other)

    def snapshot(self):
        u, read = self.u, self.read32
        slot3 = read(BLD + 0x55C + 3 * 4)
        return dict(owner=self.owner_name(read(BLD + 0x21C)),
            loss_field=signed(u, BLD + 0x53C), health=read(BLD + 0x6C),
            alive=bool(u.mem_read(BLD + 0x90, 1)[0]),
            tag_attached=bool(read(BLD + 0x34)),
            actually_placed=bool(u.mem_read(BLD+0x6E4,1)[0]),
            damaged_mode=bool(u.mem_read(BLD+0x6E6,1)[0]),
            slot3=self.anim_type_identity(read(slot3+0xC8))['type_name'] if slot3 else None,
            globals=[n for n in range(50) if u.mem_read(SCENARIO + 0x1CB0 + n * 41, 1)[0]],
            houses={name: dict(credits=signed(u, house + 0x30C),
                losses=read(house + 0x5488), score=signed(u, house + 0x54E8))
                for name, house in self.houses.items()},
            triggers=[dict(enabled=bool(u.mem_read(p + 0x44, 1)[0]),
                           completed=read(p + 0x40), pending=bool(u.mem_read(p + 0x30, 1)[0]))
                      for p in self.live_triggers])


def read_definitions(inputs, definitions):
    """Read supplied/physical strings through actual constructors and readers."""
    m, u = inputs, inputs.u
    for base in TYPE_REGISTRIES:
        u.mem_write(base, dwords(0x7EB6D4, m.alloc(128 * 4), 128, 1, 0, 10))
    triggers = {}
    for name in definitions.get('Triggers', {}):
        pointer = m.alloc(0x200)
        m.invoke(0x726C80, pointer, (m.cstring(name),))
        triggers[name] = pointer
    m.make_ini(definitions)
    for pointer in triggers.values():
        assert m.invoke(0x727240, pointer, (INI,)) & 255
    tags = {}
    for name in definitions.get('Tags', {}):
        pointer = m.alloc(0x100)
        m.invoke(0x6E5B60, pointer, (m.cstring(name),))
        assert m.invoke(m.read32(m.read32(pointer) + 0x64), pointer, (INI,)) & 255
        tags[name] = pointer
    return tags


def synthetic_definitions(name, events, *, repeat=2):
    ids = [f'{name}_{i}' for i in range(len(events))]
    return dict(Triggers={key: f'Americans,{ids[i+1] if i+1 < len(ids) else "<none>"},{key},0,1,1,1,0'
                          for i, key in enumerate(ids)},
        Events={key: f'1,{event},0,0' for key, (event, _kind, _value) in zip(ids, events)},
        Actions={key: f'1,{kind},0,{value},0,0,0,0,A'
                 for key, (_event, kind, value) in zip(ids, events)},
        Tags={name: f'{repeat},{name},{ids[0]}'})


def physical_all01_case():
    from tools.sidebar_oracle.stock import mix, mix_hash
    raw = mix((native.configured_gamemd().parent / 'mapsmd03.mix').read_bytes())[mix_hash('ALL01UMD.MAP')]
    parsed = sections(raw)
    ids = ('061FF03C', '0BE0B26C')
    definitions = {section: {key: parsed[section][key] for key in ids}
                   for section in ('Triggers', 'Events', 'Actions')}
    definitions['Tags'] = {'0BE0AB4C': parsed['Tags']['0BE0AB4C']}
    return dict(name='physical_all01_before_reshroud', stop_before_action=51,
                definitions=definitions, map='ALL01UMD.MAP', map_sha256=hashlib.sha256(raw).hexdigest())


def prepare_c4(inputs):
    if hasattr(inputs, 'sale_c4'):
        return
    physical = sections((engineer.JOINED_ROOT / 'RULESMD.INI').read_bytes())
    name = physical['CombatDamage']['C4Warhead']
    # The joined Engineer profile did not need Warhead registration. Supply
    # empty capacity, then let the actual factory/constructor enroll it.
    inputs.u.mem_write(0x8874C0, dwords(0x7EB6D4, inputs.alloc(128 * 4), 128, 1, 0, 10))
    inputs.make_ini({'CombatDamage': {'C4Warhead': name}, name: physical[name]})
    c4 = inputs.invoke(0x75E3B0, inputs.cstring(name))
    inputs.invoke(0x75D3A0, c4, (INI,))
    u = inputs.u
    u.mem_write(SP - 4, bytes(0x404))
    u.reg_write(UC_X86_REG_ESP, SP - 4)
    u.reg_write(UC_X86_REG_ESI, inputs.rules)
    u.reg_write(UC_X86_REG_EDI, INI)
    native.run_checked(u, 0x66C304, 0x66C34C)
    assert inputs.read32(inputs.rules + 0xFA8) == c4
    inputs.sale_c4 = dict(name=name, keys=physical[name])


def execute(inputs, case):
    if case.get('destroy_attached'):
        prepare_c4(inputs)
    if case.get('new_house') and not hasattr(inputs, 'sale_new_country'):
        inputs.sale_new_country = inputs.alloc(0x400)
        inputs.invoke(0x5113F0, inputs.sale_new_country, (inputs.cstring('SaleNewCountry'),))
    if case.get('new_house'):
        inputs.make_ini({'SaleNewCountry': {'CostBuildingsMult': '0.75'}})
        for register, value in ((UC_X86_REG_EBX, inputs.sale_new_country),
                (UC_X86_REG_ESI, INI), (UC_X86_REG_EDI, inputs.sale_new_country + 0x24),
                (UC_X86_REG_ESP, SP)):
            inputs.u.reg_write(register, value)
        native.run_checked(inputs.u, 0x511BC1, 0x511BE0)
    tags = read_definitions(inputs, case.get('definitions', {}))
    f = Fixture(inputs)
    if case.get('new_house'):
        f.add_house(inputs.sale_new_country)
    for offset, key in ((0xC9F, 'dont_score'), (0x232, 'insignificant')):
        if key in case:
            f.u.mem_write(f.types['GAPOWR'] + offset, bytes([case[key]]))
    if tags:
        f.attach(next(iter(tags.values())))
    if case.get('ordinary_record_kill'):
        f.u.mem_write(BLD + 0xAC, dwords(5))
    before, rng = f.snapshot(), f.rng()
    trace_start = len(f.trace)
    event_start = len(f.events)
    uid, allocations = f.read32(SCENARIO + 0x214), len(f.allocations)
    f.executed.clear()
    f.recording = True
    if case.get('ordinary_record_kill'):
        construction.invoke(f.u, 0x702D40, BLD, 0)
    else:
        f.u.mem_write(SP, bytes(0x1000))
        f.u.reg_write(UC_X86_REG_ESP, SP)
        f.u.reg_write(UC_X86_REG_EBP, BLD)
        native.run_checked(f.u, BEGIN, 0x6DD8B0 if case.get('stop_before_action') else END)
        if not case.get('stop_before_action'):
            assert f.u.reg_read(UC_X86_REG_ESP) == SP
        else:
            assert signed(f.u, f.u.reg_read(UC_X86_REG_ECX) + 0x2C) == case['stop_before_action']
    assert f.code_unchanged()
    assert all(bytes(f.u.mem_read(a, len(raw))) == raw for a, raw in f.checked_vtables)
    required = ((0x702D40, 0x70305C) if case.get('ordinary_record_kill') else
                (BEGIN, 0x44A1EF, 0x702D40, 0x6E53A0, 0x7264C0, 0x7265C0) if case.get('stop_before_action') else
                (BEGIN, 0x44A1EF, 0x702D40, 0x44A215, 0x44A21B, 0x4F9950))
    if case.get('new_house'):
        required += (0x6E0AA0, 0x448260, 0x7014A0)
    if case.get('destroy_attached'):
        required += (0x6E2050, 0x442230, 0x4415F0)
    assert set(required) <= f.executed
    return dict(input=case, before=before, after=f.snapshot(), calls=f.sale_calls,
        native_inputs=dict(type=dict(name='GAPOWR', strength=f.read32(f.types['GAPOWR'] + 0xA0),
            cost=f.read32(f.types['GAPOWR'] + 0x610)),
            game_mode=signed(f.u, 0xA8B238),
            houses={name:dict(human=bool(f.u.mem_read(p+0x1EC,1)[0]),
                player_control=bool(f.u.mem_read(p+0x1ED,1)[0]),
                country_index=signed(f.u, f.read32(p+0x34)+0xB8),
                country_cost_buildings_bits=bytes(f.u.mem_read(f.read32(p+0x34)+0x120,4)).hex(),
                country_keys={'CostBuildingsMult':'0.75'} if name=='NEW' else {})
                for name,p in f.houses.items()}),
        constructor_loss_field=f.constructor_loss_field,
        required_reached=[f'0x{p:08X}' for p in required],
        rng_unchanged=f.rng() == rng,
        identity_delta=f.read32(SCENARIO + 0x214) - uid,
        allocation_delta=len(f.allocations) - allocations,
        constructed_anims=[dict(name=event['type_name'], delay=event['delay'], flags=event['flags'])
            for event in f.events[event_start:] if event.get('event') == 'anim_ctor'],
        reached_presentation=[f'0x{p:08X}' for p in sorted(f.reached_presentation)],
        reached_platform=sorted({(entry['kind'], entry.get('pc'))
            for entry in f.trace[trace_start:] if entry['kind'].startswith('platform_')}),
        unchanged_executable_sections=True, unchanged_vtables=True)


def generate():
    inputs = engineer.prepare_joined_inputs(engineer.JOINED_ROOT)
    cases = [dict(name='no_tag'),
        dict(name='repeating_48_and_29', definitions=synthetic_definitions('REPEAT', [(48, 28, 7), (29, 28, 8), (7, 28, 9)])),
        dict(name='one_shot_48', definitions=synthetic_definitions('ONESHOT', [(48, 28, 7)], repeat=0)),
        dict(name='callback_changes_owner', new_house=True,
             definitions=synthetic_definitions('OWNER', [(48, 14, 1)])),
        dict(name='callback_destroys_attached', destroy_attached=True,
             rust_comparison='native_only_selling_death',
             definitions=synthetic_definitions('DESTROY', [(48, 32, 0)])),
        dict(name='ordinary_record_kill', ordinary_record_kill=True),
        dict(name='dont_score_callback', dont_score=True,
             definitions=synthetic_definitions('DONTSCORE', [(48, 28, 7)])),
        dict(name='insignificant_callback', insignificant=True,
             definitions=synthetic_definitions('INSIGNIFICANT', [(48, 28, 7)])),
        dict(physical_all01_case(), rust_comparison='before_authored_action')]
    for case in cases:
        case.setdefault('rust_comparison', 'sale_callbacks')
    art = sections((engineer.JOINED_ROOT / 'ARTMD.INI').read_bytes())
    return dict(schema_version=1, source='unicorn/gamemd.exe',
        art_input={name: art[name] for name in ('GAPOWR', 'GAPOWR_A', 'GAPOWR_AD')},
        native_anim_inputs=[{key:row[key] for key in ('name','raw_shp_frame_count')}
            for row in inputs.anim_rows if row['name'] in ('GAPOWR_A', 'GAPOWR_AD')],
        cases=[execute(inputs, case) for case in cases])


def metadata():
    result = native.provenance(
        scope='Original non-undeploy sale completion with live Tag callbacks, refund and current-owner payment; stop before Limbo. Separate ALL01 counted-reader/callback prefix stops before action51. Native-only action32 fatal Selling composition is retained without a Rust full-death comparison.',
        entry_points=dict(begin=BEGIN, end_before_limbo=END, building_constructor=0x43B740,
            foundation_initializer=0x45B1C0, grand_opening=0x445F80,
            record_kill=0x702D40, tag_event=0x6E53A0, trigger_register=0x7264C0,
            trigger_actions=0x7265C0, action_dispatch=0x6DD8B0,
            action_change_owner=0x6E0AA0, building_change_owner=0x448260,
            techno_change_owner=0x7014A0, refund=0x70ADA0, add_credits=0x4F9950,
            action_destroy_attached=0x6E2050, building_receive_damage=0x442230,
            destruction_effects=0x4415F0, country_cost_reader=0x511BC1),
        assumptions=[
            'Each row uses a fresh runtime VM from the reused physical Engineer joined-reader heap. Original Building construction and GrandOpening execute before the prepared Selling19/status2 boundary. Original45B1C0 initializes the referenced static foundation lists; Object CRT height initializers come from the shared Engineer fixture.',
            'Map/House admission is supplied: marked/alive/nonlimbo GAPOWR at10,10, clear32x32 cells, TileIndex=-1, declared visible flags/footprint, House building/tracking lists and empty FactoryPlant membership. Original50BF60 computes the neutral FactoryPlant factors. No full House constructor, map load, Unlimbo or preceding Selling frames are claimed.',
            'Actual constructors/counted readers create TriggerType/TagType/TEvent/TAction definitions. Original FindOrAllocateTag and ObjectReplaceTag attach them. Selected registry capacity excludes existing live Object/Techno arrays. A subclass lifts only the parent Tag=NULL assertion; it does not change registers, stack, EIP, vtables or any native callback result.',
            'OLD and NEW are human Houses in runtime mode1, OLD player-controlled and NEW not. NEW Country CostBuildingsMult=.75 is read by original511BC1..511BE0. Per-row native_inputs records the measured physical type and country fields. Whole owner mutation, nested RecordKill, Tag firing guards, live Tag reread, refund and AddCredits execute.',
            'ALL01 selected physical records are attached to the controlled GAPOWR and execution stops before6DD8B0 action51. This is not the complete physical GASPYSAT sale or its map effects. The Rust actual-map integration separately executes ordinary SELL through authored51/18/54.',
            'Action32 uses original dynamic Techno scan, physical C4/Super factory/read/binding, BuildingReceiveDamage and destruction effects. Before outer Limbo, HP is zero but the original Selling death timer retains Alive. Outer refund/payment continue. Its real debris Anim/RNG/identity effects are native-only evidence, not a claim of a Rust defect or full-death differential comparison.',
            'Complete Main/Scenario/Mapgen state equality is measured per row; identity/allocation deltas and actual Anim constructor names are observed. The death row intentionally advances RNG. Executable PE sections and declared Building/House vtable spans are compared against original bytes. Measured image digest is provenance, not a build hash gate.',
            'Static +53C census/save-load discussion is separate from execution: constructor0 and sale-1 execute here; InitManagers/damage-source writers and raw720-byte save/load persistence are static evidence, not a whole native roundtrip.'],
        substitutions=[
            'Existing allocator/free arena transport, TLS/interlocked, OS-audio/clock and COM platform interfaces are inherited. No RecordKill/Tag/Trigger/TAction/ChangeOwner/refund/AddCredits result is substituted.',
            'Construction presentation leaves451F60(ret8),452170(ret4),456FB0(ret4),705D70(ret0) retain existing declared boundaries. Per-row reached_presentation/reached_platform identify observed entries. Platform arena release retains bytes for bounded observation; no deferred queue drain runs after the sale boundary.'])
    result['checked_vtable_ranges'] = [dict(start=f'0x{a:08X}', bytes=n)
        for a,n in ((0x7E3EBC,0x500),(0x7EA8A0,0x200))]
    return result


if __name__ == '__main__':
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
