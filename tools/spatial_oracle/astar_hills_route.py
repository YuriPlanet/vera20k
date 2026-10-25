"""Original complete Pathfinder/A* over authentic already-stamped Hills Cells.

Original bridge-record, connectivity, hierarchy, concrete MTNK admission,
reconstruction and both finishing wrapper bodies execute. Direct Move=2 caller
state and admitted poses exclude command dispatch, whole Unlimbo, original map
loader/world objects, scheduler and paid Drive movement. The occupied control
uses original Foot Mark and an identical-owner stationary MTNK; executed branch
receipts establish that weapons and House indices/alliances are unread. No route,
graph ID, native admission answer or gameplay return is supplied. This leaf has
no CLI; astar_structural_height owns publication.
"""
from pathlib import Path
import hashlib
import json
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import provenance, run_checked
from tools.spatial_oracle import astar_hills_bridge_inputs as inputs
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.anytown_damage.navigation import MAP, Navigation, sr


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def states(m):
    return {k: sr.rng_state(m.uc, p) for k, p in m.rngs.items()}


class Route:
    def __init__(self):
        self.m, self.inputs = inputs.establish()
        m = self.m
        self.before_graphs = states(m)
        m.finish_graphs(compute_bridge_records=True)
        self.after_graphs = states(m)
        self.navigation = m.nav_snapshot()
        self.graphs = m.graph_snapshot()
        m.activity = 'pathfinder_setup'
        Navigation.setup_pathfinder(m.call)
        self.after_setup = states(m)
        m, u = self.m, self.m.uc
        before = states(m)
        fields = (0xB1CFD0, 0xB1CFD8, 0xB1CFA0, 0xB1D0B8, 0xB1D0AC)
        self.occupancy_startup = dict(before={hex(p): bytes(u.mem_read(p, 8)).hex() for p in fields},
                                      original_calls=[], rng_before=before)
        # Original process-static Unit geometry initializers, in dependency
        # order. No supplied bridge height or duplicated trigonometry.
        for pc in (0x735180, 0x735210, 0x735230, 0x735250, 0x7352F0):
            m.call(pc, this=0, count=1000000)
            self.occupancy_startup['original_calls'].append(hex(pc))
        assert sr.i32(u, 0xB1D0B8) == sr.i32(u, 0x89E7C0)
        assert sr.i32(u, 0xB1D0AC) > 0
        assert before == states(m)
        self.occupancy_startup.update(after={hex(p): bytes(u.mem_read(p, 8)).hex() for p in fields},
                                      rng_after=states(m))

        self.events, self.pending, self.snapshots, self.parents = [], [], [], []
        self.reached = set()
        self.weapon_getter = sr.u32(m.uc, sr.u32(m.uc, m.actor) + 0x3F8)
        m.uc.hook_add(UC_HOOK_CODE, self.observe)
        self.path_buffer = m.allocate(0x2000)
        self.source_coord = m.allocate(4)
        self.goal_coord = m.allocate(4)

    def snapshot(self, name):
        u = self.m.uc
        count = sr.u32(u, 0x89A2E0)
        assert 1 <= count <= 2000
        return dict(name=name, start=list(struct.unpack('<hh', u.mem_read(0x89A2D8, 4))),
                    cost=sr.i32(u, 0x89A2DC), count=count,
                    directions=list(struct.unpack('<'+'i'*count, u.mem_read(self.path_buffer, count*4))),
                    retained_heights=list(struct.unpack('<'+'i'*(count-1), u.mem_read(0x89A324, (count-1)*4))))

    def observe(self, u, pc, _n, _data):
        m = self.m
        sp = u.reg_read(UC_X86_REG_ESP)
        traced = (0x73F487, 0x73F49B, 0x73F827, 0x73F84E, 0x73F865,
                  0x73F86B, 0x73F87F, 0x73F8A5, 0x73F8C7, 0x73FADF,
                  0x73FB77, 0x4F9AA4, 0x4F9AA8, 0x4F9AAE, self.weapon_getter)
        if pc in traced:
            self.reached.add(pc)
        if self.pending and pc == self.pending[-1]['return_pc']:
            row = self.pending.pop()
            row.pop('return_pc')
            row['result'] = u.reg_read(UC_X86_REG_EAX)
            self.events.append(row)
        if pc == 0x73F0A0:
            p = sr.u32(u, sp + 4)
            coord = tuple(struct.unpack('<hh', u.mem_read(p + 0x24, 4)))
            assert coord in m.ptrs, ('CanEnter outside original physical Hills', coord)
            self.pending.append(dict(kind='unit_entry', coord=list(coord),
                direction=sr.i32(u, sp+8), height=sr.i32(u, sp+12),
                previous_pointer=hex(sr.u32(u, sp+16)), arg5=sr.i32(u, sp+20),
                ground_level=u.mem_read(p+0x11B, 1)[0], flags=sr.u32(u, p+0x140),
                return_pc=sr.u32(u, sp)))
        elif pc in (0x42C900, 0x429A90, 0x42AA90, 0x42B210, 0x42B420,
                    0x42B7F0, 0x42BE20, 0x42ACF0):
            self.events.append(dict(kind='original_body', pc=hex(pc),
                                    this=hex(u.reg_read(UC_X86_REG_ECX))))
            if pc == 0x42AA90:
                outer = sr.u32(u, sp+4)
                descriptor = sr.u32(u, outer)
                chain = []
                while descriptor:
                    assert len(chain) < 2001
                    cell = sr.u32(u, sr.u32(u, descriptor))
                    chain.append(dict(coord=list(struct.unpack('<hh', u.mem_read(cell+0x24, 4))),
                                      height=sr.i32(u, descriptor+4)))
                    descriptor = sr.u32(u, descriptor+8)
                self.parents = list(reversed(chain))
        if pc in (0x42A40B, 0x42A41A, 0x42A423):
            self.snapshots.append(self.snapshot({0x42A40B:'reconstructed',
                0x42A41A:'after_corners', 0x42A423:'after_straight'}[pc]))

    def run(self, name, source, goal, *, height, on_bridge=False, urgency=0):
        m, u = self.m, self.m.uc
        m.activity = 'path_search'
        # Explicit admitted direct-call pose and actual Move mission; original
        # constructors establish target/navigation/facing/Drive idle fields.
        u.mem_write(m.actor+0xAC, dwords(2))
        u.mem_write(m.actor+0x9C, dwords(source[0]*256+128, source[1]*256+128,
                                       height*sr.i32(u, 0x89E7C0)))
        u.mem_write(m.actor+0x55C, packed(*source))
        u.mem_write(m.actor+0x8C, bytes([int(on_bridge)]))
        u.mem_write(self.source_coord, packed(*source))
        u.mem_write(self.goal_coord, packed(*goal))
        u.mem_write(self.path_buffer, b'\xff'*0x2000)
        self.events.clear(); self.snapshots.clear(); self.parents.clear(); self.reached.clear()
        prestate = dict(mission=sr.i32(u, m.actor+0xAC),
                        navigation_target=hex(sr.u32(u, m.actor+0x5A4)),
                        actor_bytes=bytes(u.mem_read(m.actor, 0x800)).hex(),
                        drive_bytes=bytes(u.mem_read(m.drive, 0x100)).hex())
        before = states(m)
        # Original4CBC31 caller of42C900: seven thiscall stack arguments.
        args = (self.source_coord, self.goal_coord, m.actor, self.path_buffer, 2000, -1, urgency)
        result = m.call(0x42C900, this=0x87E8B8, args=args, count=30000000)
        assert not self.pending
        after = states(m)
        assert sha(bytes(u.mem_read(0x401000, 0x3E0000))) == m.code_hash
        path = None
        if result:
            path = self.snapshot('returned')
            path.pop('name')
            assert len(self.snapshots) == 3
        weapon_pcs = {0x73F487, 0x73F49B, 0x73FB77, self.weapon_getter}
        assert not (self.reached & weapon_pcs), ('unestablished weapon dependency', self.reached)
        return dict(name=name, source=list(source), goal=list(goal), height=height,
                    on_bridge=on_bridge, urgency=urgency, prestate=prestate,
                    returned_null=result == 0, path=path, parent_chain=list(self.parents),
                    pass_snapshots=list(self.snapshots), events=list(self.events),
                    admission_branch_pcs=[hex(p) for p in sorted(self.reached)],
                    unread_weapon_pcs=[hex(p) for p in sorted(weapon_pcs)],
                    rng_before=before, rng_after=after, code_unchanged=True)

    def blocked(self):
        m, u = self.m, self.m.uc
        mover, drive = m.actor, m.drive
        house = sr.u32(u, mover+0x21C)
        m.actor_coord, m.actor_height = (78,75), 6
        m.build_actor(sr.u32(u, mover+0x6C4))
        blocker = m.actor
        u.mem_write(blocker+0x21C, dwords(house))
        u.mem_write(blocker+0x8C, b'\1')
        blocker_state = dict(mission=sr.i32(u, blocker+0xAC), same_owner=True,
            navigation_target=hex(sr.u32(u, blocker+0x5A4)),
            actor_bytes=bytes(u.mem_read(blocker, 0x800)).hex(),
            drive_bytes=bytes(u.mem_read(m.drive, 0x100)).hex())
        mark_rng = states(m)
        cell = m.ptrs[(78,75)]
        cell_before = bytes(u.mem_read(cell, 0x200)).hex()
        assert m.call(0x4D3780, this=blocker, args=(1,), count=1000000) == 1
        cell = m.ptrs[(78,75)]
        occupied = dict(deck_bits=sr.u32(u, cell+0x128),
                        deck_head_matches=sr.u32(u, cell+0xE8) == blocker)
        # Exact occupancy offsets are recorded below through the original
        # owner output, without supplying admission answers or route nodes.
        occupied['cell_bytes'] = bytes(u.mem_read(cell, 0x200)).hex()
        assert occupied['deck_bits'] == 0x20 and occupied['deck_head_matches']
        m.actor, m.drive = mover, drive
        row = self.run('occupied_deck_repath', (77,75), (81,75), height=6, on_bridge=True)
        assert 0x4F9AA8 in self.reached and 0x4F9AAE not in self.reached
        row.update(blocker_prestate=blocker_state, occupation=occupied,
                   same_house_pointer_branch=True, house_index_and_alliance_unread=True)
        assert m.call(0x4D3780, this=blocker, args=(0,), count=1000000) == 1
        assert mark_rng == states(m)
        row['mark_rng_before'] = mark_rng
        row['mark_rng_after'] = states(m)
        row['cell_before_mark1'] = cell_before
        row['cell_after_mark0'] = bytes(u.mem_read(cell, 0x200)).hex()
        assert row['cell_after_mark0'] == cell_before
        return row


def generate():
    owner = Route()
    cases = [owner.run('ramp_to_deck', (75,75), (80,75), height=6),
             owner.run('ground_underpass', (78,77), (78,72), height=2), owner.blocked()]
    return dict(schema_version=1, scope=__doc__,
                physical=owner.inputs['physical'], native_type=owner.inputs['native_type'],
                input_receipt_sha256=sha(json.dumps(owner.inputs, sort_keys=True).encode()),
                navigation_sha256=sha(json.dumps(owner.navigation, sort_keys=True).encode()),
                graph_sha256=sha(json.dumps(owner.graphs, sort_keys=True).encode()),
                native_bridge_records=owner.m.bridge_record_production,
                graph_record_counts=[len(g['records']) for g in owner.graphs],
                rng_before_graphs=owner.before_graphs, rng_after_graphs=owner.after_graphs,
                rng_after_pathfinder_setup=owner.after_setup,
                occupancy_startup=owner.occupancy_startup, cases=cases,
                text_sha256=owner.m.code_hash, code_unchanged=True)


def metadata():
    return provenance(scope=__doc__, entry_points={
        'bridge_records':0x56D6E0, 'connectivity':0x56C510, 'hierarchy':0x581F90,
        'pathfinder_ctor':0x42A6D0, 'map_resize':0x42AC00, 'scratch':0x42C1C0,
        'pathfinder_find':0x42C900, 'astar':0x429A90, 'unit_entry':0x73F0A0,
        'reconstruct':0x42AA90, 'corners':0x42B210, 'smooth':0x42B420,
        'straight':0x42B7F0, 'straighten':0x42BE20, 'foot_mark':0x4D3780},
        assumptions=[
            'Authentic Hills Cells/TMPs/high bridge stamps and MTNK native scalar readers reuse astar_hills_bridge_inputs.establish; original scenario/file loader, world objects and final map initialization are excluded.',
            'Route is the common startup owner for its FootMark consumers: original Unit geometry initializers735180/735210/735230/735250/7352F0 execute from executable constants. Earlier receipts omitted these globals and therefore had a zero Unit occupancy bridge threshold; their stationary deck-only Mark case produced identical prior outputs, as checked before adding this receipt. Complete initializer bytes/calls/RNG states are recorded; CRT registration traversal is excluded.',
            'Existing Navigation.finish_graphs executes original bridge-record scan, connectivity and hierarchy with its established vector constructor boundary; no Rust graph IDs, edges or routes supplied.',
            'Original Pathfinder constructor/resize/scratch execute as in naval_head_producer. Original4CBC31 direct caller arguments and actor pose/Move2 are explicit inputs; full Foot Find_Path, command dispatch, scheduler and whole Unlimbo are excluded.',
            'Occupied control creates another original MTNK constructor prefix and Drive, assigns identical House pointer and admitted deck pose, and executes original FootMark1/0. Same-pointer alliance short circuit is executed before House index/bits; full House initialization is excluded.',
            'Target/navigation/facing and stationary Drive derive from constructor prefix with zero backing storage. Mission Move2 and Guard5 exercise the friendly stationary admission arm. Executed PCs prove Primary120mm, weapon getter and House index/alliance bytes unread in these cases. Enemy, attack/scatter, moving blocker and wall controls are excluded.'],
        substitutions=['Inherited bounded allocator/free, CRT TLS/atexit and presentation/visibility sinks. No gameplay admission, bridge record, graph, path, height or RNG result replaced.'])


class MarkerControls(Route):
    """Observe original marker/cost bodies in the existing physical-map VM."""

    def __init__(self):
        self.marker_events, self.cost_events, self.marker_pending = [], [], []
        self.cost_pending = None
        super().__init__()

    def marker_cells(self):
        u = self.m.uc
        return [list(coord) for coord, pointer in self.m.ptrs.items()
                if sr.u32(u, pointer + 0x140) & 0x40000]

    def observe(self, u, pc, size, data):
        super().observe(u, pc, size, data)
        sp = u.reg_read(UC_X86_REG_ESP)
        if self.marker_pending and pc == self.marker_pending[-1]['return_pc']:
            event = self.marker_pending.pop()
            event.pop('return_pc')
            event.update(effective_urgency=sr.u32(u, 0x87E8B8 + 0x3C),
                         marked_cells_after=self.marker_cells())
            self.marker_events.append(event)
        if pc == 0x42ACF0:
            self.marker_pending.append(dict(requested_urgency=sr.u32(u, 0x87E8B8 + 0x3C),
                marked_cells_before=self.marker_cells(), branch_pcs=[], return_pc=sr.u32(u, sp)))
        elif self.marker_pending and pc in (0x42AE54, 0x42AE56, 0x42AE9E, 0x42AECA,
                                           0x42AEF1, 0x42AFCB, 0x42B063):
            self.marker_pending[-1]['branch_pcs'].append(hex(pc))
        if pc == 0x429830:
            source, candidate = sr.u32(u, sp + 4), sr.u32(u, sp + 8)
            self.cost_pending = dict(
                source=list(struct.unpack('<hh', u.mem_read(sr.u32(u, source) + 0x24, 4))),
                candidate=list(struct.unpack('<hh', u.mem_read(sr.u32(u, candidate) + 0x24, 4))),
                cost_class=sr.u32(u, sp + 16), urgency=sr.u32(u, 0x87E8B8 + 0x3C),
                candidate_marked=bool(sr.u32(u, sr.u32(u, candidate) + 0x140) & 0x40000),
                branch_pcs=[])
        elif self.cost_pending and pc in (0x429886, 0x4298B3, 0x4298C4, 0x429978,
                                         0x429986, 0x42998D, 0x42999E, 0x4299B8):
            self.cost_pending['branch_pcs'].append(hex(pc))
        elif self.cost_pending and pc == 0x429FEA:
            self.cost_pending.update(caller_step_cost_f32_bits=bytes(u.mem_read(sp + 0x34, 4)).hex())
            self.cost_events.append(self.cost_pending)
            self.cost_pending = None

    def moving_peer(self, queue):
        m, u = self.m, self.m.uc
        self.mover, self.mover_drive = m.actor, m.drive
        house, typ = sr.u32(u, m.actor + 0x21C), sr.u32(u, m.actor + 0x6C4)
        m.actor_coord, m.actor_height = (78, 75), 2
        m.build_actor(typ)
        self.blocker, self.blocker_drive = m.actor, m.drive
        u.mem_write(self.blocker + 0x21C, dwords(house))
        # Explicit Foot queue/reference and retained Drive prestates. Queue
        # directions are copied from the original ground_underpass output.
        ahead = (78 * 256 + 128, 74 * 256 + 128, 2 * sr.i32(u, 0x89E7C0))
        self.cell = m.ptrs[(78, 75)]
        self.cell_before_mark = bytes(u.mem_read(self.cell, 0x200)).hex()
        before_mark = states(m)
        assert m.call(0x4D3780, this=self.blocker, args=(1,), count=1000000) == 1
        assert before_mark == states(m)
        assert sr.u32(u, self.cell + 0xE4) == self.blocker
        assert sr.u32(u, self.cell + 0x124) & 0x20
        # Preserve the original admitted cell claim, then supply the retained
        # moving-query state independently, as unit_entry_motion does.
        u.mem_write(self.blocker + 0xAC, dwords(2))
        u.mem_write(self.blocker + 0x558, packed(78, 75))
        u.mem_write(self.blocker + 0x5E0, dwords(*queue[:2], -1))
        u.mem_write(self.blocker_drive + 0x34, dwords(*ahead))
        u.mem_write(self.blocker_drive + 0x40, dwords(*ahead))
        m.actor, m.drive = self.mover, self.mover_drive
        return dict(coord=[78, 75], on_bridge=False, mission=2,
                    same_house_pointer=True, same_type_pointer=True,
                    navigation_target=hex(sr.u32(u, self.blocker + 0x5A4)),
                    occupation=bool(u.mem_read(self.blocker + 0x6B6, 1)[0]),
                    applied_speed_f64_bits=bytes(u.mem_read(self.blocker + 0x578, 8)).hex(),
                    ground_occupation=sr.u32(u, self.cell + 0x124),
                    deck_occupation=sr.u32(u, self.cell + 0x128),
                    queue_source='original ground_underpass returned directions',
                    supplied_queue=list(queue[:2]) + [-1], supplied_reference=[78, 75],
                    actor_bytes=bytes(u.mem_read(self.blocker, 0x800)).hex(),
                    drive_bytes=bytes(u.mem_read(self.blocker_drive, 0x100)).hex(),
                    rng_before_mark=before_mark, rng_after_mark=states(m))

    def marker_route(self, urgency):
        self.marker_events.clear(); self.cost_events.clear()
        before = self.marker_cells()
        row = self.run('same_type_request_' + str(urgency), (78, 76), (78, 72),
                       height=2, urgency=urgency)
        assert not self.marker_pending and self.cost_pending is None
        assert self.marker_cells() == before == []
        row.update(marker_transactions=list(self.marker_events), edge_costs=list(self.cost_events),
                   marker_cleanup=True)
        assert row['rng_before'] == row['rng_after']
        if urgency == 1:
            assert len(self.marker_events) == 1
            assert self.marker_events[0]['effective_urgency'] == 0
            assert '0x42aeca' in self.marker_events[0]['branch_pcs']
            assert self.marker_events[0]['marked_cells_after'] == []
        return row

    def edge_cost(self, urgency, speed_bits):
        m, u = self.m, self.m.uc
        u.mem_write(self.blocker + 0x578, struct.pack('<Q', speed_bits))
        before = states(m)
        # Obtain the concrete class through the original Unit slot, never
        # supply a CanEnter answer. It is then passed to the original cost body.
        classification = m.call(0x73F0A0, this=self.mover,
            args=(self.cell, 0, 2, m.ptrs[(78, 76)], 1), count=1000000)
        assert classification == 2
        assert 0x4F9AA8 in self.reached and 0x4F9AAE not in self.reached
        source_slot, candidate_slot = m.allocate(4), m.allocate(4)
        u.mem_write(source_slot, dwords(m.ptrs[(78, 76)]))
        u.mem_write(candidate_slot, dwords(self.cell))
        u.mem_write(0x87E8B8 + 0x3C, dwords(urgency))
        self.cost_pending = None
        m.call(0x429830, this=0x87E8B8,
               args=(source_slot, candidate_slot, 0, classification, 1), count=1000000)
        event = self.cost_pending
        assert event is not None
        # Original caller's own FSTP captures the returned ST0 without
        # modifying any executable instruction or introducing a return stub.
        sp = u.reg_read(UC_X86_REG_ESP)
        run_checked(u, 0x429F9D, 0x429FA1, count=10, required_addresses=(0x429F9D,))
        raw = bytes(u.mem_read(sp + 0x34, 4))
        self.cost_pending = None
        assert before == states(m)
        assert sha(bytes(u.mem_read(0x401000, 0x3E0000))) == m.code_hash
        return dict(urgency=urgency, supplied_speed_f64_bits=f'{speed_bits:016x}',
                    concrete_class=classification, cost_f32_bits=raw.hex(),
                    cost=struct.unpack('<f', raw)[0], original_cost_trace=event,
                    rng_before=before, rng_after=states(m), code_unchanged=True)


def generate_markers():
    owner = MarkerControls()
    queue = owner.run('native_queue_producer', (78, 77), (78, 72), height=2)
    blocker = owner.moving_peer(queue['path']['directions'])
    routes = [owner.marker_route(urgency) for urgency in (0, 1, 2)]
    costs = [owner.edge_cost(urgency, speed) for speed in (0, 0x3FF0000000000000)
             for urgency in (0, 1, 2)]
    m, u = owner.m, owner.m.uc
    before_unmark = states(m)
    assert m.call(0x4D3780, this=owner.blocker, args=(0,), count=1000000) == 1
    assert before_unmark == states(m)
    assert bytes(u.mem_read(owner.cell, 0x200)).hex() == owner.cell_before_mark
    assert owner.marker_cells() == []
    return dict(schema_version=1,
                scope='Original same-type MTNK marker downgrade, full physical Hills routes and code2 clearing-chain costs; explicit peer queue/Drive prestates, no paid movement or scheduler.',
                physical=owner.inputs['physical'], native_type=owner.inputs['native_type'],
                occupancy_startup=owner.occupancy_startup,
                queue_producer=queue, blocker_prestate=blocker, routes=routes, costs=costs,
                original_mark_cleanup=True, marker_cleanup=True,
                rng_before_unmark=before_unmark, rng_after_unmark=states(m),
                text_sha256=m.code_hash, code_unchanged=True)


def marker_metadata():
    result = metadata()
    result['scope'] = 'Original same-type MTNK42ACF0 marker downgrade and429830 code2 clearing-chain controls on physical Hills, with original full42C900 routes.'
    result['entry_points'].update({
        'markers': '0x0042acf0', 'same_type_gate': '0x0042ae54',
        'urgency_downgrade': '0x0042aeca', 'edge_cost': '0x00429830',
        'edge_return_capture': '0x00429f9d', 'drive_is_moving': '0x004afb80',
        'unit_geometry_scale': '0x00735180', 'unit_geometry_angle1': '0x00735210',
        'unit_geometry_angle2': '0x00735230', 'unit_level_height': '0x00735250',
        'unit_bridge_height': '0x007352f0'})
    result['assumptions'] = [
        'Physical Hills/type/map/graph setup reuses the existing Route owner. A separately executed original ground_underpass path supplies two retained peer queue directions; reference coordinate and Drive head/destination are explicit fixture inputs, not whole Find_Path/Drive lifecycle.',
        'Original process-static Unit geometry initializers735180/735210/735230/735250/7352F0 execute from original executable constants before occupancy; registered CRT traversal itself is excluded. Their complete output bytes, original calls and all RNG states are saved, without supplied104/416 or host math.',
        'Original Unit/Drive constructors and FootMark1/0 establish a same-type, identical-owner ground MTNK blocker at78,75. Constructor fields and supplied Move2/queue/+578/Drive state are saved. Source78,76 default north-facing probe selects its actual original ground list.',
        'Original42C900/429A90/42ACF0 and all concrete Unit entry, reconstruction and finishing execute for requested urgencies0/1/2. Full Cell marker sets and exact downgrade/replay branch PCs establish preparation and cleanup, without Rust marker/admission/route inputs.',
        'Direct429830 controls use independently executed original Unit+1AC class2, original map Cell pointers and explicit urgency0/1/2. Original caller FSTP429F9D..429FA1 captures binary32 ST0 before caller scaling/tiebreak; supplied Foot+578 zero/one exercises queue/facing clearing prediction. Ten-hop jams and complete moving-blocker lifecycle are excluded.',
        'All three complete RNG states and original text hash are retained. Original same-House short circuit excludes weapon/House-index dependencies. Original scenario loader, command dispatch, scheduler, whole Unlimbo and paid Drive Process are excluded.']
    return result
