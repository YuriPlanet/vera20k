"""Original AI capture -> retained initial owner -> computer Sell_Back handoff.

Reuses EngineerJoinedFixture; only observation and explicit prepared priors are
added. See captured_building_sale.md for the joined/cropped evidence boundary.
"""
import hashlib
import os
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_ESP

from tools import native_oracle as native
from tools.spatial_oracle import building_construction as construction
from tools.spatial_oracle import engineer_repair_admission as engineer
from tools.spatial_oracle.engineer_capture_accounting import SOURCE_PATHS as INHERITED_SOURCES
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import place_building, cell

SOURCE_PATHS = ('tools/spatial_oracle/captured_building_sale.py', *INHERITED_SOURCES)
TAIL_BEGIN, TAIL_END = 0x4492D7, 0x44936E
BLD, HOUSE, SP = engineer.BLD, engineer.HOUSE, engineer.SP


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


class Fixture(engineer.EngineerJoinedFixture):
    def building(self, name, pointer, radio_items, coord=(6, 9), health=None):
        """Actual constructor followed by the inherited admitted-map prior.

        Unlike Construction's partial ctor prior, the entire43B740 establishes
        Techno+14C. Physical placement/HP/registry are explicitly supplied by
        the existing repair target helper, not claimed as original Unlimbo.
        """
        u = self.u
        u.mem_write(pointer, bytes(0x800))
        self.phase = 'setup_full_building_constructor'
        result = construction.invoke(u, 0x43B740, pointer, self.types[name], self.constructor_house)
        self.constructor = dict(entry='0x0043B740', initial_owner_store='0x006F3241',
            result_is_receiver=result == pointer, initial_owner=self.house_name(self.read32(pointer + 0x14C)))
        if self.read32(pointer + 0x14C) != self.constructor_house or 0x6F3241 not in self.executed:
            raise native.OracleError('Whole Building constructor did not establish initial owner')
        place_building(u, pointer, coord, constructor_abstract_flags=self.read32(pointer + 0x14))
        u.mem_write(pointer + 0x21C, dwords(self.constructor_house))
        u.mem_write(pointer + 0x520, dwords(self.types[name]))
        u.mem_write(pointer + 0x6C, dwords(self.read32(self.types[name] + 0xA0) if health is None else health))
        for offset, value in ((0x90, 1), (0x74, 1), (0x81, 0), (0x98, 1)):
            u.mem_write(pointer + offset, bytes([value]))
        u.mem_write(cell(*coord) + 0x12C, dwords(0x18))
        u.mem_write(pointer + 0x534, dwords(-1, -1))
        u.mem_write(pointer + 0x6E9, b'\1\1')
        u.mem_write(pointer + 0xE0, dwords(0x7E180C, radio_items, 1, 1))
        u.mem_write(radio_items, dwords(0))
        self.phase = 'setup'

    def house_name(self, pointer):
        return next((name for name, value in self.houses.items() if value == pointer), None)

    def type_name(self, pointer):
        return next((name for name, value in self.types.items() if value == pointer), None)

    def compact_state(self):
        u, read = self.u, self.read32
        return dict(owner=self.house_name(read(BLD + 0x21C)),
            initial_owner=self.house_name(read(BLD + 0x14C)),
            mission=signed(u, BLD + 0xAC), queued=signed(u, BLD + 0xB4),
            status=signed(u, BLD + 0xBC), buildup=bool(u.mem_read(BLD + 0x6E9, 1)[0]),
            c4=bool(u.mem_read(BLD + 0x6DF, 1)[0]),
            upgrade_count=struct.unpack('<b', u.mem_read(BLD + 0x702, 1))[0],
            upgrade_slots=[self.type_name(read(BLD + 0x5EC + i * 4)) for i in range(3)],
            credits=signed(u, HOUSE + 0x30C),
            power_dirty=bool(u.mem_read(HOUSE + 0x5778, 1)[0]),
            radar_dirty=bool(u.mem_read(HOUSE + 0x5779, 1)[0]))


def prepare_fixture(inputs, case):
    f = Fixture(inputs)
    u = f.u
    # Original vtables are data rather than executable sections. Retain the
    # primary Building/Infantry tables separately from the inherited code guard.
    f.checked_vtables = [(address, native.file_span(native.image_bytes(), address, length)[1])
        for address, length in ((0x7E3EBC, 0x500), (0x7EB058, 0x600))]
    if any(bytes(u.mem_read(address, len(raw))) != raw for address, raw in f.checked_vtables):
        raise native.OracleError('Native fixture replaced a selected class vtable')
    # Real constructor's native global list storage, already initialized in
    # the shared input VM. This is the same prerequisite as building_death_anims.
    for address in (0x8B4120, 0x8874C0, 0x887568, 0xA83CE0, 0xB054D0):
        u.mem_write(address, bytes(inputs.u.mem_read(address, 24)))
    old = f.second_house_prior()
    f.houses = {'NEW': HOUSE, 'INITIAL': old}
    if case['current_owner'] == 'PREVIOUS':
        previous = f.allocate(0x6000)
        u.mem_write(previous, bytes(u.mem_read(old, 0x6000)))
        u.mem_write(previous + 0x30, dwords(2))
        f.houses['PREVIOUS'] = previous
    f.constructor_house = f.houses[case['initial_owner']]
    f.repair_target_prior()
    for label, house in f.houses.items():
        f.house_prior(house, (BLD,) if label == case['current_owner'] else ())
    u.mem_write(BLD + 0x21C, dwords(f.houses[case['current_owner']]))
    # Each house's Country object is a supplied copy of the native-created
    # Americans type. Only the explicit passive gate varies by row.
    for label, house in f.houses.items():
        country = f.allocate(0x400)
        u.mem_write(country, bytes(u.mem_read(inputs.country, 0x400)))
        u.mem_write(house + 0x34, dwords(country))
        u.mem_write(country + 0x1A6, bytes([case['initial_owner_passive'] and label == case['initial_owner']]))
    u.mem_write(HOUSE + 0x1EC, bytes([case['new_owner_human']]))
    u.mem_write(HOUSE + 0x1ED, bytes([case['new_owner_player']]))
    u.mem_write(HOUSE + 0x30C, dwords(case['credits']))
    u.mem_write(0xA8B238, dwords(case['game_mode']))
    for offset, value in ((0xAC, case['mission']), (0xB4, case['queued']), (0xBC, 0)):
        u.mem_write(BLD + offset, dwords(value))
    u.mem_write(BLD + 0x6E9, bytes([case['buildup']]))
    u.mem_write(BLD + 0x6DF, bytes([case['c4']]))
    u.mem_write(BLD + 0x702, bytes([len(case['upgrades'])]))
    for index, upgrade in enumerate(case['upgrades']):
        typ = f.types[upgrade['name']]
        if f.read32(typ + 0x610) != upgrade['cost']:
            raise native.OracleError('Upgrade cost differs from original reader')
        u.mem_write(BLD + 0x5EC + index * 4, dwords(typ))
    u.mem_write(HOUSE + 0x5778, b'\0\0')
    return f


class Observation:
    def __init__(self, f):
        self.f, self.calls, self.visited, self.owner_writes = f, [], set(), []
        self.rng = f.rng()
        self.counter = f.read32(f.read32(0xA8B230) + 0x214)
        self.initial_owner = f.read32(BLD + 0x14C)
        self.hooks = [f.u.hook_add(UC_HOOK_CODE, self.code), f.u.hook_add(UC_HOOK_MEM_WRITE, self.write)]

    def code(self, u, pc, size, _data):
        self.visited.add(pc)
        f, read = self.f, self.f.read32
        sp, this = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        if pc == 0x447110:
            self.calls.append(dict(op='sell_back', control=signed(u, sp + 4)))
        elif pc == 0x711F60:
            self.calls.append(dict(op='get_refund', type=f.type_name(this),
                owner=f.house_name(read(sp + 4)), full=bool(read(sp + 8))))
        elif pc == 0x4F9950:
            self.calls.append(dict(op='add_credits', owner=f.house_name(this), amount=signed(u, sp + 4)))
        elif pc == 0x451690:
            self.calls.append(dict(op='remove_upgrade'))
        elif pc == 0x5B35E0 and this == BLD:
            self.calls.append(dict(op='queue', mission=signed(u, sp + 4), force=bool(read(sp + 8))))
        elif pc == 0x5B3570 and this == BLD:
            self.calls.append(dict(op='commence'))

    def write(self, u, _access, address, size, value, _data):
        if address <= BLD + 0x14C < address + size:
            self.owner_writes.append(dict(pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}', size=size, value=value))

    def finish(self, required):
        f = self.f
        for hook in self.hooks:
            f.u.hook_del(hook)
        missing = set(required) - self.visited
        tables_unchanged = all(bytes(f.u.mem_read(address, len(raw))) == raw
            for address, raw in f.checked_vtables)
        if missing or not f.code_unchanged() or not tables_unchanged or f.pending:
            raise native.OracleError(f'Incomplete/modified original capture-sale execution: {missing}')
        if self.owner_writes:
            raise native.OracleError(f'Capture-sale changed constructor owner: {self.owner_writes}')
        return dict(calls=self.calls, rng_unchanged=self.rng == f.rng(),
            initial_owner_unchanged=self.initial_owner == f.read32(BLD + 0x14C),
            object_id_counter_delta=f.read32(f.read32(0xA8B230) + 0x214) - self.counter,
            required_reached=[f'0x{pc:08X}' for pc in required], unchanged_executable_sections=True,
            unchanged_class_vtables=True)


def case(name, **changes):
    return dict(dict(name=name, game_mode=3, new_owner_human=False, new_owner_player=False,
        initial_owner='INITIAL', initial_owner_passive=False, current_owner='INITIAL',
        buildup=True, c4=False, mission=5, queued=-1, upgrades=[], credits=1000), **changes)


JOINED_CASES = (
    case('ai_capture'), case('human_capture', new_owner_human=True, new_owner_player=True),
    case('campaign_capture', game_mode=0), case('initial_owner_recapture', initial_owner='NEW'),
    case('passive_initial_owner', initial_owner_passive=True),
    case('third_owner_capture', current_owner='PREVIOUS'),
)
TAIL_CASES = (
    case('computer_c4_refusal', c4=True), case('no_buildup_refusal', buildup=False),
    case('already_selling_refusal', mission=19),
    case('nonlocal_human_refusal', new_owner_human=True),
    case('local_computer_is_still_computer', new_owner_player=True),
    case('upgrade_full_refund_then_sale', upgrades=[dict(name='GAPOWR', cost=800), dict(name='GAPILE', cost=500)]),
    case('upgrade_full_refund_before_c4_refusal', c4=True, upgrades=[dict(name='GAPILE', cost=500)]),
)


def joined_row(inputs, item):
    f = prepare_fixture(inputs, item)
    u, read = f.u, f.read32
    actor = f.actor((9, 10))
    f.phase = 'supplied_ai_capture_megamission'
    construction.invoke(u, 0x6FFBE0, actor, 8, BLD, BLD, 0)
    event = 0xA802D4 + (read(0xA802C8) - 1) * 0x6F
    construction.invoke(u, 0x4C6CB0, event)
    construction.invoke(u, 0x5B3570, actor)
    if (read(actor + 0xAC), read(actor + 0x2B4), read(actor + 0x5A4)) != (8, BLD, BLD):
        raise native.OracleError('Original AI order delivery did not retain Capture target/destination')
    before = f.compact_state()
    observation = Observation(f)
    f.phase = 'walk_arrival_capture_sale_handoff'
    f.walk_arrival(actor)
    return dict(input=item, constructor=f.constructor,
        command=dict(mission=8, target='building', destination='building'), before=before,
        after=f.compact_state(), engineer_after=dict(alive=bool(u.mem_read(actor + 0x90, 1)[0]),
            limbo=bool(u.mem_read(actor + 0x81, 1)[0])),
        **observation.finish((0x75AEC0, 0x519630, 0x448260, 0x7014A0, TAIL_BEGIN)))


def tail_row(inputs, item):
    f = prepare_fixture(inputs, item)
    u = f.u
    # Explicit post-ChangeOwner boundary. Constructor identity remains actual;
    # only current owner has already transferred when4492D7 is entered.
    u.mem_write(BLD + 0x21C, dwords(HOUSE))
    before = f.compact_state()
    observation = Observation(f)
    f.phase = 'supplied_post_change_owner_sale_tail'
    for register, value in ((UC_X86_REG_ESI, BLD), (UC_X86_REG_EBP, HOUSE), (UC_X86_REG_ESP, SP)):
        u.reg_write(register, value)
    native.run_checked(u, TAIL_BEGIN, TAIL_END)
    return dict(input=item, constructor=f.constructor, before=before, after=f.compact_state(),
                **observation.finish((TAIL_BEGIN,)))


def generate():
    root = Path(os.environ.get('VERA20K_CAPTURED_BUILDING_SALE_ASSETS', str(engineer.JOINED_ROOT)))
    inputs = engineer.prepare_joined_inputs(root)
    typ = inputs.types['GAPOWR']
    result = dict(schema=1, source='unicorn/gamemd.exe',
        native_type=dict(name='GAPOWR', strength=inputs.read32(typ + 0xA0), cost=inputs.read32(typ + 0x610)),
        upgrade_types=[dict(name=name, cost=inputs.read32(inputs.types[name] + 0x610),
            super_weapon=signed(inputs.u, inputs.types[name] + 0x16F0),
            turret=bool(inputs.u.mem_read(inputs.types[name] + 0xCA1, 1)[0])) for name in ('GAPOWR', 'GAPILE')],
        retail_files=[dict(name=p.name, size=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
            for p in sorted(root.iterdir()) if p.is_file() and p.suffix.lower() != '.json'],
        joined_cases=[], tail_cases=[])
    for item in JOINED_CASES:
        result['joined_cases'].append(joined_row(inputs, item))
    for item in TAIL_CASES:
        result['tail_cases'].append(tail_row(inputs, item))
    return result


def metadata():
    result = native.provenance(
        scope='Original joined Engineer Capture event/Walk/PerCell/ChangeOwner handoff to Sell_Back(1), plus cropped4492D7..44936E predicate/upgrade controls. Stops before Selling mission visits.',
        assumptions=[
            'EngineerJoinedFixture supplies the existing active-retail rules/art/sound/map/allocator platform. Whole Building43B740 establishes Techno+14C at6F3241 before the explicit already-admitted damaged GAPOWR map prior. No initial-owner value is supplied after constructor execution.',
            'The issued AI order boundary supplies Capture8 with both target and destination naming the building, as original Engineer Hunt callers do. Original6FFBE0/event delivery/Commence execute. Whole Team/Hunt selection, cursor/UI, distant path search/payment and autonomous AI planning are not executed.',
            'Walk starts16leptons before an already-paid head, as the shared Engineer oracle. Whole75AEC0/519630/448260/7014A0 execute; live untagged owner state and ordinary building admission are supplied. Stops after capture returns, before the existing building_sale oracle owned Selling continuation.',
            'Tail controls enter original4492D7 with ESI=building and EBP=newowner, currentowner already transferred. Native IsControlledByHuman, GetRefund(owner,true), AddCredits, RemoveLastUpgrade, Sell_Back(1), QueueMission and Commence remain live. GAPOWR/GAPILE installed upgrades are explicit synthetic input slots, not a stock powerplant-upgrade claim.',
            'House/Country copies, passive/human/player flags, game mode, C4, buildup, missions, credits and installed upgrade slots are explicit prior inputs. Constructor provenance survives the supplied current-owner changes. Pointer identity labels are fixture-relative.',
        ],
        substitutions=[
            'Only read-only observation hooks are added; inherited platform, allocator, presentation and file transport leaves are disclosed in inherited_fixture. No capture, sale, refund, upgrade-removal or mission decision is answered or original instruction/vtable patched.',
            'Static lifecycle evidence separately identifies constructor6F3241, Techno Save70C250 via Radio65AC40/Abstract raw-save, and Load70BFA0..70BFAC swizzle registration of+14C. The literal displacement writer census does not prove absence of alias/bulk writes. This corpus does not execute original save/load or whole House teardown.',
        ],
        entry_points=dict(building_constructor=0x43B740, initial_owner_store=0x6F3241,
            clicked_mission=0x6FFBE0, event_delivery=0x4C6CB0, walk=0x75AEC0,
            per_cell=0x519630, building_change_owner=0x448260, techno_change_owner=0x7014A0,
            capture_sale_tail=TAIL_BEGIN, tail_end=TAIL_END, sell_back=0x447110,
            refund=0x711F60, add_credits=0x4F9950, remove_upgrade=0x451690))
    result['inherited_fixture'] = engineer.joined_metadata()
    return result


def main(argv=None):
    root = Path(__file__).resolve().parents[2]
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
        argv=argv, source_paths={name: root / name for name in SOURCE_PATHS})


if __name__ == '__main__':
    main()
