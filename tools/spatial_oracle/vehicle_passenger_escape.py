"""Original Unit passengers: death admission, full Unlimbo and virtual Scatter.

The supplied parent has already been removed from the map. Each selected
passenger enters the original737FE3 caller; successful rows continue through
its human-owner/selection tail and scope decrement. Refused rows stop before
RecordKill. No admission, placement, search, setter or movement answer is supplied.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_EDI, UC_X86_REG_EIP,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle.track_destination import make_destination_fixture
from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, LOCO, SP
from tools.spatial_oracle.unit_source_scatter import SCENARIO
from tools.spatial_oracle.unit_entry import HOUSE, EXTRA
from tools.spatial_oracle.refinery_dock import cell
from tools.spatial_oracle.map_queries import dwords

MAP, HEAP = 0x87F7E8, 0x24000000
TRANSPORT, TRANSPORT_TYPE = HEAP + 0x1C0000, HEAP + 0x1C2000
BEGIN, SUCCESS, REFUSED = 0x737FE3, 0x7381AE, 0x738188
UNLIMBO_ENTRIES = (0x737BA0, 0x4D7170, 0x6F6CA0, 0x5F4EC0,
                  0x4D3780, 0x5683C0, 0x47E8A0, 0x7441B0, 0x55BAA0, 0x4A9720)
SCATTER_ENTRIES = (0x738130, 0x73813D, 0x743A50)
DESTINATION_ENTRIES = (0x56DC20, 0x741970, 0x4D94B0, 0x4AFD40)
COMMON_REQUIRED = (BEGIN, 0x73F0A0, 0x738021, 0x738030)
SUCCESS_REQUIRED = UNLIMBO_ENTRIES + SCATTER_ENTRIES + (
    0x7380EC, 0x7380F2, 0x7380FA, 0x50B730, 0x7381A8)
WATCHED = set(COMMON_REQUIRED + SUCCESS_REQUIRED + DESTINATION_ENTRIES + (0x738143,))
TYPE_INPUT = dict(strength=300, sight=8, crusher=True, speed=7, speed_type='Track',
                  movement_zone='Normal', locomotor='Drive', rot=5, armed=False,
                  native_logic_default=True, native_cell_occupier_default=True)
MAP_INPUT = dict(size=[16, 16], allocated_cells=[32, 32], local_size=[0, 0, 32, 32],
                 level=0, land=0, clear_speed_rows=[1.0] * 9)


def signed(u, pointer):
    return struct.unpack('<i', u.mem_read(pointer, 4))[0]


class Fixture:
    def __init__(self, case):
        self.case = case
        self.u, self.call, self.read = make_destination_fixture(dict(
            family='drive', head=[0, 0, 0], prior=[0, 0, 0],
            seed=case['seed'], frame=case['frame']))
        self.u.mem_map(HEAP, 0x200000)
        self.cursor, self.phase = HEAP, 'setup'
        self.platform = []
        self.events, self.reached, self.snapshots = [], set(), {}
        self.current = None
        self.text_before = bytes(self.u.mem_read(0x401000, 0x3E0000))
        self.u.hook_add(UC_HOOK_CODE, self.observe)
        self.setup()

    def ret(self, value=0):
        u, sp = self.u, self.u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, self.read(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4)

    def observe(self, u, pc, size, data):
        if pc in (0x7C978A, 0x6C8C40):
            assert pc == 0x7C978A or self.phase == 'setup', 'runtime clock dependency'
            self.platform.append(['atexit' if pc == 0x7C978A else 'setup_clock', self.phase])
            self.ret(0)
            return
        if pc == 0x7C8E17:
            size = self.read(u.reg_read(UC_X86_REG_ESP) + 4)
            pointer = self.cursor
            self.cursor = (self.cursor + size + 15) & ~15
            assert self.cursor < HEAP + 0x100000
            self.platform.append(['heap', self.phase, size])
            self.ret(pointer)
            return
        if self.phase != 'escape':
            return
        assert pc not in (0x4B0500, 0x4D3920), 'escape advanced Drive Process/Find_Path'
        self.reached.add(pc)
        if pc not in WATCHED:
            return
        sp, read = u.reg_read(UC_X86_REG_ESP), self.read
        event = dict(pc=f'0x{pc:08X}')
        if pc == 0x73F0A0:
            args = [read(sp + 4 + i * 4) for i in range(5)]
            assert args == [cell(*self.case['source_cell']), 0xFFFFFFFF, 0xFFFFFFFF, 0, 1]
        elif pc == 0x738021:
            self.admission = u.reg_read(UC_X86_REG_EAX)
            event['admission'] = self.admission
        elif pc == 0x737BA0:
            event['facing'] = read(sp + 8)
        elif pc == 0x7380F2:
            self.unlimbo_al = u.reg_read(UC_X86_REG_EAX) & 255
            event['unlimbo_al'] = self.unlimbo_al
            self.snapshots['after_unlimbo'] = self.state(self.current)
        elif pc == 0x738130:
            self.snapshots['before_scatter'] = self.state(self.current)
        elif pc == 0x743A50:
            assert [read(sp + 4), read(sp + 8), read(sp + 12)] == [0xB1CFE8, 1, 0]
        elif pc == 0x738143:
            self.snapshots['after_scatter'] = self.state(self.current)
        self.events.append(event)

    def setup(self):
        u, read, call = self.u, self.read, self.call
        # Resolved ordinary human owner; no native House/Country construction.
        u.mem_write(HOUSE + 0x34, dwords(EXTRA + 0x2E000))
        u.mem_write(HOUSE + 0x1EC, b'\1')
        u.mem_write(0xA83D4C, dwords(HOUSE))
        u.mem_write(ACTOR + 0x6B6, b'\1')  # original Foot ctor4D344A.
        u.mem_write(ACTOR + 0x14, dwords(7))
        u.mem_write(ACTOR + 0x520, dwords(-1))  # Foot ctor4D31F1.
        u.mem_write(ACTOR + 0x94, dwords(-1))
        u.mem_write(ACTOR + 0x90, b'\1')
        u.mem_write(ACTOR + 0x81, b'\1')
        u.mem_write(ACTOR + 0x6C, dwords(TYPE_INPUT['strength']))
        u.mem_write(ACTOR + 0x11C, dwords(TRANSPORT))
        u.mem_write(TYPE + 0xA0, dwords(TYPE_INPUT['strength']))
        # ObjectType5F716E and TechnoType7116AD constructor defaults.
        u.mem_write(TYPE + 0x234, b'\1\1')
        u.mem_write(TYPE + 0x5E8, dwords(TYPE_INPUT['sight']))
        u.mem_write(TYPE + 0xD28, b'\1')
        u.mem_write(TYPE + 0x71C, dwords(TYPE_INPUT['rot']))
        # The original scalar conversion, after the caller's supplied Speed=7.
        # No INI reader or movement step is implied by this small setup slice.
        u.reg_write(UC_X86_REG_EAX, TYPE_INPUT['speed'])
        u.reg_write(UC_X86_REG_EBP, TYPE)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x714664, 0x71469F, count=1000,
                    required_addresses=(0x714699,))
        assert read(TYPE + 0x678) == 17
        u.mem_write(read(0x8871E0) + 0x16BC, dwords(104))
        u.mem_write(0xA8E7AC, dwords(0))
        u.mem_write(0xA8B238, dwords(5))
        u.mem_write(0xA8E9A0, b'\1')
        # Original geometry and display owners. No gameplay query is answered.
        startup = list(struct.unpack('<14I', u.mem_read(0x8150B8, 56))) + [
            0x4A8630, 0x40CB80, 0x561710, 0x5617A0, 0x5617C0, 0x5617E0,
            0x735180, 0x735210, 0x735230, 0x735250, 0x7352F0]
        for pc in startup:
            call(pc, 0, [])
        call(0x6D1C20, HEAP + 0x180000, [])
        u.mem_write(HEAP + 0x182000, dwords(0x7E2070, 640, 480, 0, 2, 0, 0, 0))
        u.mem_write(0x880A04, dwords(HEAP + 0x182000))
        call(0x49F9B0, HOUSE + 0x5514, [])
        call(0x49F9B0, HOUSE + 0x5564, [])
        u.mem_write(MAP + 0xF4, dwords(*MAP_INPUT['size']))
        u.mem_write(MAP + 0xFC, dwords(*MAP_INPUT['local_size']))
        u.mem_write(MAP + 0xEC, dwords(0, 0, *MAP_INPUT['size']))
        u.mem_write(MAP + 0x68, dwords(HEAP + 0x190000, 33 * 33, HEAP + 0x192000))
        for x, y in self.case['blocked_cells']:
            u.mem_write(cell(x, y) + 0x124, dwords(0x20))
        # Parent already has no map membership. Closed same-owner transport.
        u.mem_write(TRANSPORT, dwords(0x7F5C70))
        u.mem_write(TRANSPORT + 0x6C4, dwords(TRANSPORT_TYPE))
        u.mem_write(TRANSPORT + 0x21C, dwords(HOUSE))
        x, y = self.case['source_cell']
        u.mem_write(TRANSPORT + 0x9C, dwords(x * 256 + 128, y * 256 + 128, 0))
        u.mem_write(ACTOR + 0x9C, bytes(u.mem_read(TRANSPORT + 0x9C, 12)))
        call(0x4C91C0, TRANSPORT + 0x388, [])
        call(0x4C9680, TRANSPORT + 0x388, [5])
        u.mem_write(TRANSPORT + 0x388, dwords(0x8000, 0x8000, -1, 0, 0))
        template = bytes(u.mem_read(ACTOR, 0x700))
        self.passengers = []
        for index, spec in enumerate(self.case['passengers']):
            pointer = ACTOR if index == 0 else HEAP + 0x1D0000 + (index - 1) * 0x2000
            loco = LOCO if index == 0 else pointer + 0x1400
            if index:
                u.mem_write(pointer, template)
                call(0x4AF540, loco, [])
                u.mem_write(loco + 0xC, dwords(pointer))
                u.mem_write(loco + 0x14, dwords(1))
                u.mem_write(loco + 0x10, b'\1')
                u.mem_write(pointer + 0x674, dwords(loco + 4))
                # Independent empty radio vector storage for each passenger.
                u.mem_write(pointer + 0xE4, dwords(pointer + 0x1000))
            u.mem_write(pointer + 0xAC, dwords(spec['mission']))
            u.mem_write(pointer + 0x5A4, dwords(cell(*spec['nav_cell']) if spec['nav_cell'] else 0))
            # Original Foot constructor clears the saved MegaMission. Zero
            # instead means an active saved order and bypasses ordinary Idle.
            assert spec['mega_mission'] == -1
            u.reg_write(UC_X86_REG_ESI, pointer)
            u.reg_write(UC_X86_REG_EBX, 0)
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x4D32EC, 0x4D3308, count=1000,
                        required_addresses=(0x4D32EC,))
            assert signed(u, pointer + 0x5C4) == spec['mega_mission']
            self.passengers.append((pointer, loco))
        assert read(0x7F5C70 + 0xD8) == 0x737BA0
        assert read(0x7F5C70 + 0x174) == 0x743A50
        assert read(0x7F5C70 + 0x480) == 0x741970

    def state(self, index):
        pointer, loco = self.passengers[index]
        u, read = self.u, self.read
        nav = read(pointer + 0x5A4)
        return dict(alive=bool(u.mem_read(pointer + 0x90, 1)[0]),
                    limbo=bool(u.mem_read(pointer + 0x81, 1)[0]),
                    marked=bool(u.mem_read(pointer + 0x74, 1)[0]),
                    logic_registered=bool(u.mem_read(pointer + 0x98, 1)[0]),
                    position=list(struct.unpack('<3i', u.mem_read(pointer + 0x9C, 12))),
                    mission=signed(u, pointer + 0xAC), queued=signed(u, pointer + 0xB4),
                    mega_mission=signed(u, pointer + 0x5C4),
                    transporter=bool(read(pointer + 0x11C)),
                    nav_cell=list(struct.unpack('<hh', u.mem_read(nav + 0x24, 4))) if nav else None,
                    drive_destination=list(struct.unpack('<3i', u.mem_read(loco + 0x34, 12))))

    def execute(self):
        u, read = self.u, self.read
        rng_before = bytes(u.mem_read(SCENARIO + 0x218, 1012))
        outputs = []
        for index, (pointer, loco) in enumerate(self.passengers):
            self.current = index
            self.events, self.reached, self.snapshots = [], set(), {}
            self.admission, self.unlimbo_al = None, None
            before = self.state(index)
            assert read(0xA8E7AC) == 0
            u.mem_write(SP, bytes(0x60))
            for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, TRANSPORT),
                               (UC_X86_REG_EDI, pointer)):
                u.reg_write(reg, value)
            self.phase = 'escape'
            terminal = run_checked(u, BEGIN, (SUCCESS, REFUSED), count=2000000,
                                   required_addresses=COMMON_REQUIRED)
            assert u.reg_read(UC_X86_REG_ESP) == SP
            admitted = terminal == SUCCESS
            if admitted:
                assert set(SUCCESS_REQUIRED) <= self.reached
                assert self.unlimbo_al == 1 and read(0xA8E7AC) == 0
                assert not self.state(index)['transporter']
                scatter_prior = self.snapshots['before_scatter']
                if scatter_prior['mission'] == 5 and scatter_prior['nav_cell'] is None:
                    assert set(DESTINATION_ENTRIES) <= self.reached
                else:
                    assert not set(DESTINATION_ENTRIES) & self.reached
            else:
                # Stop before native RecordKill/UnInit; no destruction reply is
                # fabricated. There cannot be another measured passenger yet.
                assert index == len(self.passengers) - 1
                assert self.admission not in (0, 2) or self.unlimbo_al == 0
            outputs.append(dict(admission=self.admission, unlimbo_al=self.unlimbo_al,
                placement_admitted=admitted, before=before, **self.snapshots,
                after=self.state(index), terminal=f'0x{terminal:08X}',
                reached_addresses=[f'0x{p:08X}' for p in sorted(self.reached & WATCHED)]))
        assert bytes(u.mem_read(0x401000, 0x3E0000)) == self.text_before
        here = cell(*self.case['source_cell'])
        members, pointer = [], read(here + 0xE4)
        identities = {p: i for i, (p, _) in enumerate(self.passengers)}
        while pointer:
            assert pointer in identities and identities[pointer] not in members
            members.append(identities[pointer])
            pointer = read(pointer + 0x30)
        return dict(input=self.case, passengers=outputs,
                    source_occupation=read(here + 0x124), source_members=members,
                    scenario_init_counter=read(0xA8E7AC),
                    scenario_rng_unchanged=rng_before == bytes(u.mem_read(SCENARIO + 0x218, 1012)),
                    native_text_unchanged=True)


def cases():
    rows = []
    def add(name, *, passengers=None, frame=100, blocked_cells=None):
        rows.append(dict(name=name, source_cell=[10, 10], seed=31, frame=frame,
                         blocked_cells=blocked_cells or [],
                         passengers=[dict(spec, mega_mission=-1) for spec in
                                     (passengers or [dict(mission=5, nav_cell=None)])]))
    add('one_guard')
    add('two_guard', passengers=[dict(mission=5, nav_cell=None)] * 2)
    add('two_first_sleep', passengers=[dict(mission=0, nav_cell=None),
                                      dict(mission=5, nav_cell=None)])
    add('one_guard_frame_103', frame=103)
    add('one_retained_destination', passengers=[dict(mission=5, nav_cell=[12, 10])])
    add('one_blocked_preferred_neighbor', blocked_cells=[[11, 9]])
    return rows


def generate():
    return dict(type_input=TYPE_INPUT, map_input=MAP_INPUT,
                rows=[Fixture(case).execute() for case in cases()])


def metadata():
    return provenance(
        scope='UnitClass::ReceiveDamage selected passenger737FE3..7381AE: original numeric '
              'admission, scope increment, floor/facing, full Unit/Foot/Techno/Object Unlimbo, '
              'Map placement/occupation, transporter clear and actual virtual Unit Scatter. '
              'Human-owner tail and scope decrement execute. Refusals stop before RecordKill738188.',
        entry_points=dict(caller=BEGIN, successful_stop=SUCCESS, refusal_stop=REFUSED,
            unit_entry=0x73F0A0, unit_unlimbo=0x737BA0, foot_unlimbo=0x4D7170,
            techno_unlimbo=0x6F6CA0, object_unlimbo=0x5F4EC0, foot_mark=0x4D3780,
            place_down=0x5683C0, unit_occupation=0x7441B0, scatter_dispatch=0x73813D,
            unit_scatter=0x743A50, find_nearby_cell=0x56DC20, unit_set_destination=0x741970,
            foot_set_destination=0x4D94B0, drive_move_to=0x4AFD40),
        assumptions=[
            'Fresh VM per row. Reuses track_destination original Unit/UnitType/Cell vtables '
            'and constructed Drive; its source-scatter vtable observers are replaced with the original Unit vtable.',
            'Resolved Unit type inputs are in type_input; unarmed controlled type with selected stock '
            'MTNK placement fields, not a full MTNK INI/constructor replay. Foot occupation1 at4D344A, '
            'waypoint-1 at4D31F1, flags7, display layer-1, alive/limbo1, health300. Native type234/235 '
            'Logic/Cell-occupier defaults are class lifecycle fields, not INI Occupier. Original714664..71469F '
            'converts supplied Speed7 into type678=17; ROT5 is supplied. Original Foot constructor '
            'slice4D32EC..4D3308 initializes saved MegaMission=-1 and its cleared payload for every '
            'passenger; this input and observed field are exported. Zero would mean a saved order '
            'and incorrectly bypass ordinary Unit EnterIdle during Unlimbo.',
            'Supplied empty flat32x32 map and mutable class/height planes, size16x16; all clear-land '
            'speed rows1.0. Optional independent raw ground masks0x20 have no object-list occupants. '
            'Original placement produces measured passenger occupation and cell membership.',
            'One human owner/Country and display-surface prior, LevelHeight104, active-game1 and '
            'actual-game-mode5. Original Foot/Unit geometry, Display/Screen and House-counter constructors execute.',
            'Closed same-owner ground transport, already absent from map lists, settled facing0x8000; '
            'no IsABomb/IgnoreDefenses/selection/bridge. ESI parent, EDI selected passenger and zeroed '
            'parent frame are supplied. Passenger selection/cargo removal, boarding and damage prefix are excluded.',
            'Later passengers retain earlier native map/object/locomotor writes; no movement Process '
            'runs between them. Empty radio storage is independent. Refused rows stop before destruction, '
            'so their alive/limbo fields are observations at the kill boundary, not survival claims.',
            'Checks original text unchanged, balanced parent frame, terminal, required original callees, '
            'and exact Scatter arguments EmptyCoord/1/0. Positive Guard/no-Nav cases require FNPC and '
            'original Unit/Foot/Drive setters; retained-Nav cases require their absence. The Sleep input '
            'becomes Guard in original Unlimbo before Scatter; retained Nav selects Move. Drive Process '
            'and Find_Path are rejected. No physical scenario, full transport lifetime, '
            'AI/team/selection branch, later movement/pathfinding or arrival claim.',
        ],
        substitutions=[
            'Inherited imported Interlocked +/-1 transport, bounded successful heap allocation, '
            'atexit registration, setup-only wall clock. No selected gameplay method or return is substituted.',
        ])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
