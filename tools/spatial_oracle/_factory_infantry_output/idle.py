"""Observation-only real GI idle/latch extension of immutable P2 callers.

One AST insertion installs hooks at the unchanged P2 caller boundary: before
rally configure, or immediately before no-rally open_producer returns.
No native decision, object field, virtual table or runtime order is changed.
The 98MB parent receipt is compared in memory and referenced, not copied.
"""
from pathlib import Path
import argparse, ast, hashlib, importlib.util, json, struct, traceback

HERE = Path(__file__).resolve().parent
P2 = HERE
from .runtime import load_module, read_accepted, compare_primary
P2_MANIFEST = '3b42b8512826dc477250a2b894585724db4cb35264770618729c8fd47a083bf5'
VTABLE = 0x7EB058
SLOTS = (0x2C, 0x48, 0x174, 0x184, 0x1D8, 0x1E8, 0x2AC, 0x430, 0x480, 0x484, 0x4AC, 0x4D0, 0x544)
SPANS = {
    'foot_ctor': (0x4D31E0, 0x4D353C),
    'foot_idle': (0x4D82B0, 0x4D8557),
    'infantry_idle': (0x51CBA0, 0x51CDAC),
    'infantry_movement_prefix': (0x520F40, 0x520FAF),
    'techno_idle': (0x709A40, 0x709A82),
    'foot_ai_reset_prefix': (0x4DA530, 0x4DA554),
    'foot_recycle_destination': (0x4DA030, 0x4DA0DF),
    'techno_ctor': (0x6F2B40, 0x6F3269),
    'foot_idle_predicate': (0x4DF4B0, 0x4DF507),
    'techno_idle_predicate': (0x705D50, 0x705D5E),
    'techno_271_getter': (0x70C5C0, 0x70C5C7),
    'techno_514_getter': (0x705D20, 0x705D27),
    'techno_idle_effect': (0x6385C0, 0x6386D4),
    'infantry_ctor': (0x517A50, 0x517CB9),
    'infantry_init': (0x517CC0, 0x517D8F),
    'techno_init': (0x6F3F40, 0x6F42F9),
    'planning_create': (0x638A80, 0x638B43),
    'planning_setter': (0x705D10, 0x705D1D),
    'infantry_building_scatter_prefix': (0x51BCA4, 0x51BDCF),
    'infantry_scatter': (0x51D0D0, 0x51D6F0),
    'building_exit_tail': (0x444C7C, 0x444DE4),
}


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def canonical_sha(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def load(name, p):
    return load_module(name, p)


def generate(repeat=True, control='rally', capture=None):
    parent_driver=P2 / ('exit.py' if control=='rally' else 'construction.py')
    read_accepted(control)  # verifies exact decompressed original P2 bytes
    original = load('immutable_p2_idle_observation', parent_driver)
    original_vtable_receipt = json.loads((HERE / 'native-infantry-vtable.json').read_text(encoding='utf-8'))
    original_vtable_bytes = bytes.fromhex(original_vtable_receipt['result']['bytes'])
    declared_slots = {hex(o): struct.unpack_from('<I', original_vtable_bytes, o)[0] for o in SLOTS}
    events, writes, setters, streams, stream_index, registry = [], [], [], {}, {}, {}
    returns, pending_writes, hooks, vm = [], [], [], {}
    tracked, active, pcs = set(), [], {k: set() for k in SPANS}
    sequence = 0

    def install_observer(f, producer):
        from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
        from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP
        from tools.spatial_oracle import building_construction as bc
        u, r = f.u, f.read32
        vm.update(f=f, p=producer, bc=bc)
        def next_sequence():
            nonlocal sequence
            sequence += 1
            return sequence
        def rng_refs():
            result = {}
            all_rng = f.rng()
            if set(all_rng) != {'main', 'scenario', 'mapgen'}:
                raise ValueError(('Missing complete RNG stream', set(all_rng)))
            for name, value in all_rng.items():
                if len(bytes.fromhex(value)) != 0x3F4:
                    raise ValueError(('Wrong original RNG state size', name))
                digest = hashlib.sha256(bytes.fromhex(value)).hexdigest()
                key = name + ':' + digest
                if key not in stream_index:
                    stream_index[key] = len(stream_index)
                    streams[str(stream_index[key])] = dict(stream=name, sha256=digest, bytes=value)
                result[name] = stream_index[key]
            return result
        def signed(a):
            return struct.unpack('<i', u.mem_read(a, 4))[0]
        def ints(a, n):
            return list(struct.unpack('<' + 'i' * n, u.mem_read(a, 4*n)))
        def vector(pointer, count):
            if count > 1024:
                raise ValueError(('Unbounded GI vector', hex(pointer), count))
            return [r(pointer + 4*i) for i in range(count)]
        def snapshot(p):
            vtable = r(p)
            infantry_type = r(p+0x6C0)
            loco = r(p+0x674)
            out = dict(pointer=p, id=r(p+0x10), vtable=vtable,
                flags=r(p+0x14), health=signed(p+0x6C), alive=u.mem_read(p+0x90,1)[0],
                limbo=u.mem_read(p+0x81,1)[0], location=ints(p+0x9C,3),
                mission=signed(p+0xAC), suspended=signed(p+0xB0), queued=signed(p+0xB4),
                status=signed(p+0xBC), mission_timer=ints(p+0xC8,3),
                idle_latch_6b3=u.mem_read(p+0x6B3,1)[0], legacy_index=signed(p+0x520),
                scatter_pending_687=u.mem_read(p+0x687,1)[0], archive=r(p+0x218),
                owner=r(p+0x21C), target=r(p+0x2B4), slave_owner_2dc=r(p+0x2DC), team_5d4=r(p+0x5D4),
                planning_object_514=r(p+0x514), warped_out_271=u.mem_read(p+0x271,1)[0],
                attack_move_saved_mission_5c4=signed(p+0x5C4), attack_move_engaged_5d1=u.mem_read(p+0x5D1,1)[0],
                temporal=r(p+0x274), nav=r(p+0x5A4),
                infantry_doing=signed(p+0x6C4),
                nav_queue=dict(pointer=r(p+0x58C), count=r(p+0x598), items=vector(r(p+0x58C),r(p+0x598))),
                destination_history=dict(pointer=r(p+0x5B0), count=r(p+0x5BC), items=vector(r(p+0x5B0),r(p+0x5BC))),
                path_head=ints(p+0x5E0,4), speed_fraction_bytes=bytes(u.mem_read(p+0x578,8)).hex(),
                movement_timer=ints(p+0x640,3), retries=signed(p+0x64C), blockage_timer=ints(p+0x668,3),
                tether=u.mem_read(p+0x418,1)[0],
                contacts=vector(r(p+0xE4),r(p+0xE8)),
                locomotion_interface=loco, walk=None, type=infantry_type,
                mission_raw=bytes(u.mem_read(p+0xAC,0x28)).hex(),
                foot_private_raw_520_6c0=bytes(u.mem_read(p+0x520,0x1A0)).hex(), rng=rng_refs())
            plan=r(p+0x514)
            out['planning_object'] = None if not plan else dict(pointer=plan,raw=bytes(u.mem_read(plan,0x9C)).hex(),
                owner=r(plan),vtable=r(plan+4),vector=r(plan+8),capacity=signed(plan+0xC),count=signed(plan+0x14),
                growth=signed(plan+0x18),active_latch=u.mem_read(plan+0x1C,1)[0],
                limits=ints(plan+0x8C,3),flags=bytes(u.mem_read(plan+0x98,2)).hex())
            out['planning_registry'] = dict(raw=bytes(u.mem_read(0xAC4C78,0x18)).hex(),
                vector=r(0xAC4C7C),capacity=r(0xAC4C80),count=r(0xAC4C88),
                items=vector(r(0xAC4C7C),r(0xAC4C88)))
            out['producer_archive'] = r(producer+0x218)
            producer_type=r(producer+0x520)
            out['producer_scatter_gate_state']=dict(pointer=producer,type=producer_type,
                invisible_in_game=u.mem_read(producer_type+0x1701,1)[0],
                laser_fence=u.mem_read(producer_type+0x16BF,1)[0],
                firestorm_wall=u.mem_read(producer_type+0x16C0,1)[0],
                gate=u.mem_read(producer_type+0x16B7,1)[0],
                laser_fence_frame=signed(producer+0x618),house_firestorm_active=u.mem_read(r(producer+0x21C)+0x1FA,1)[0])
            if loco:
                base = loco-4
                out['walk'] = dict(base=base, raw=bytes(u.mem_read(base,0x38)).hex(),
                    persist_vtable=r(base), locomotion_vtable=r(loco), owner=r(base+0xC),
                    references=r(base+0x14), destination=ints(base+0x1C,3),
                    paid_head=ints(base+0x28,3), moving=u.mem_read(base+0x34,1)[0],
                    motion=u.mem_read(base+0x36,1)[0])
            if vtable == VTABLE and infantry_type in f.types.values():
                out['type_flags'] = {k:u.mem_read(infantry_type+o,1)[0] for k,o in
                    dict(default_to_guard_area=0xD39, agent=0xEC3, vehicle_thief=0xEC6).items()}
            return out
        def register(p):
            if p in registry or r(p) != VTABLE:
                return
            slots = {hex(o):r(r(p)+o) for o in SLOTS}
            if slots != declared_slots:
                raise ValueError(('Original GI vtable differs',slots,declared_slots))
            registry[p] = dict(pointer=p, vtable=r(p), slots=slots,
                original_bytes_equal=bytes(u.mem_read(VTABLE,len(original_vtable_bytes))) == original_vtable_bytes,
                first_observed_frame=r(bc.FRAME), first_observed_phase=f.phase)
        watch = {0x517A50:'InfantryCtor',0x517CC0:'InfantryInit',0x6F3F40:'TechnoInit',
                 0x638A80:'PlanningCreate',0x705D10:'PlanningSetter',
                 0x51D0D0:'InfantryScatter',
                 0x4D31E0:'FootCtor', 0x4D82B0:'FootIdle', 0x51CBA0:'InfantryIdle',
                 0x520F40:'InfantryMovement', 0x709A40:'TechnoIdle', 0x4DA030:'RecycleDestination',
                 0x51AA40:'InfantryDestination', 0x4D94B0:'FootDestination',
                 0x70C610:'SetArchive', 0x5B35E0:'QueueMission', 0x4D3710:'SetSpeedFraction',
                 0x4DF1C0:'RetainedAttackMove', 0x4DF4B0:'TechnoIdlePredicate4D0',
                 0x705D50:'TechnoIdlePredicate430', 0x6385C0:'TechnoIdleEffect',
                 0x5B3040:'EffectiveMission', 0x523340:'InfantryRTTI',
                 0x70C5C0:'Techno271Getter',0x705D20:'Techno514Getter'}
        def complete_row(row):
            row.update(return_sequence=next_sequence(), result=u.reg_read(UC_X86_REG_EAX),
                result_al=u.reg_read(UC_X86_REG_EAX)&255,
                return_pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}', after=snapshot(row['this']))
            if row in active:
                active.remove(row)
        def observe(_u, pc, size, _data):
            for row in pending_writes[:]:
                row.update(after=bytes(u.mem_read(row['address'],row['width'])).hex(),
                    after_state=snapshot(row['this']), next_pc=f'0x{pc:08X}')
                pending_writes.remove(row)
            for ret, row in returns[:]:
                if pc == ret and u.reg_read(UC_X86_REG_ESP) > row['sp']:
                    complete_row(row)
                    returns.remove((ret,row))
            this = u.reg_read(UC_X86_REG_ECX)
            if pc in (0x517A50,0x4D31E0):
                tracked.add(this)
            for name,(lo,hi) in SPANS.items():
                if lo <= pc < hi:
                    pcs[name].add(pc)
            if pc in (0x4D8523,0x4D8530,0x51CBB6,0x51CD9C,0x520F92,0x709A63,0x709A71,
                      0x6385DC,0x638AB5,0x638B3E,0x51BD16,0x51BDC7,0x51D422,0x51D45B,
                      0x444CA3,0x444D11) or (pc in (0x4DA87A,0x51BCA4) and r(bc.FRAME) in (269,486)):
                # These boundaries use ESI or EBP, not an invented this value.
                from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EDI
                register_owner=UC_X86_REG_EBP if pc==0x520F92 else UC_X86_REG_EDI if pc in (0x638AB5,0x638B3E,0x444CA3,0x444D11) else UC_X86_REG_ESI
                owner=u.reg_read(register_owner)
                if owner in tracked:
                    row=dict(sequence=next_sequence(),kind='NativeBoundary',pc=f'0x{pc:08X}',
                        frame=r(bc.FRAME),phase=f.phase,this=owner,result=u.reg_read(UC_X86_REG_EAX),state=snapshot(owner))
                    if pc==0x51D422:row['fnpc_result_cell']=list(struct.unpack('<2h',u.mem_read(u.reg_read(UC_X86_REG_EAX),4)))
                    if pc==0x638AB5:row['created_planning_header']=bytes(u.mem_read(u.reg_read(UC_X86_REG_EAX),0x9C)).hex()
                    events.append(row)
            if pc not in watch or this not in tracked:
                return
            idle_active=any(x['kind'] in ('FootIdle','InfantryIdle') and x['this']==this for x in active)
            if watch[pc] in ('QueueMission','SetSpeedFraction','EffectiveMission','InfantryRTTI','RetainedAttackMove') and not idle_active:
                return
            if watch[pc]=='TechnoIdlePredicate4D0' and not any(x['kind']=='TechnoIdle' for x in active):
                return
            if watch[pc]=='TechnoIdlePredicate430' and not any(x['kind']=='TechnoIdle' for x in active):
                return
            if watch[pc] in ('Techno271Getter','Techno514Getter') and not any(x['kind']=='TechnoIdle' for x in active):
                return
            sp=u.reg_read(UC_X86_REG_ESP)
            register(this)
            row=dict(sequence=next_sequence(),kind=watch[pc],pc=f'0x{pc:08X}',frame=r(bc.FRAME),phase=f.phase,
                this=this,sp=sp,caller=f'0x{r(sp):08X}',args=[r(sp+4*i) for i in range(1,5)],before=snapshot(this))
            events.append(row)
            if row['kind']=='InfantryScatter':row['source_coordinate']=ints(row['args'][0],3)
            if row['kind'] in ('FootIdle','InfantryIdle','TechnoIdle'):
                active.append(row)
            returns.append((r(sp),row))
            if row['kind'] in ('InfantryDestination','FootDestination','QueueMission','SetArchive','SetSpeedFraction'):
                setters.append(row['sequence'])
        def written(_u,_access,address,size,value,_data):
            observed_fields=((0x6B3,1),(0x2DC,4),(0x5D4,4),(0x271,1),(0x514,4))
            owner=next((p for p in tracked if any(address<p+o+n and p+o<address+size for o,n in observed_fields)),None)
            if owner is None:
                return
            row=dict(sequence=next_sequence(),frame=r(bc.FRAME),phase=f.phase,this=owner,
                pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',address=address,width=size,
                offset=hex(address-owner),
                before=bytes(u.mem_read(address,size)).hex(),value=value,before_state=snapshot(owner))
            writes.append(row);pending_writes.append(row)
        hooks.extend([u.hook_add(UC_HOOK_CODE,observe),u.hook_add(UC_HOOK_MEM_WRITE,written)])
        vm.update(snapshot=snapshot,complete_row=complete_row,rng_refs=rng_refs)

    fn=next(n for n in ast.parse(Path(original.__file__).read_text(encoding='utf-8')).body
        if isinstance(n,ast.FunctionDef) and n.name=='generate')
    class Observer(ast.NodeTransformer):
        def visit_FunctionDef(self,node):
            if control=='rally' and node.name=='configure':
                node.body.insert(0,ast.parse('install_observer(f,p)').body[0])
            if control=='no_rally' and node.name=='open_producer':
                node.body.insert(len(node.body)-1,ast.parse('install_observer(f,p)').body[0])
            return self.generic_visit(node)
    fn=Observer().visit(fn)
    module=ast.fix_missing_locations(ast.Module(body=[fn],type_ignores=[]))
    ns=dict(vars(original));ns.update(install_observer=install_observer)
    exec(compile(module,str(HERE/'<observation-only-idle-caller>'),'exec'),ns)
    receipt=None;comparison=None;post=[];fault=None
    try:
        receipt=ns['generate']('rally' if control=='rally' else 'construction')
        joined=receipt['joined'] if control=='rally' else receipt
        if receipt['fault'] or joined['admitted_queue']['queue']['fault']:
            raise ValueError(('Unchanged P2 native loop fault',receipt['fault']))
        if capture is not None:
            capture(receipt)
        comparison=compare_primary(control,receipt)
        f,bc=vm['f'],vm['bc']
        final=joined['admitted_queue']['queue']['final']
        primary_boundary=[vm['snapshot'](p['pointer']) for p in final['delivered_infantry']]
        if repeat:
            p=final['delivered_infantry'][-1]['pointer']
            if vm['snapshot'](p)['idle_latch_6b3']!=1:
                raise ValueError('Repeated-call control requires actual native primary latch1')
            for i in range(2):
                f.phase='external_repeated_real_infantry_idle_'+str(i+1)
                start=len(events);di=len(f.draws);ai=len(f.advances)
                row=dict(label=f.phase,entry='0x0051CBA0',this=p,args=[0,1],
                    before=vm['snapshot'](p),request_start=di,advance_start=ai)
                row['result']=bc.invoke(f.u,0x51CBA0,p,0,1)
                # run_checked stops before RET_MAGIC; complete only the actual
                # top-level observed call at its returned emulator boundary.
                for ret,pending in returns[:]:
                    if pending['kind']=='InfantryIdle' and pending['sequence']>=events[start]['sequence']:
                        vm['complete_row'](pending);returns.remove((ret,pending))
                row.update(after=vm['snapshot'](p),requests=f.draws[di:],advances=f.advances[ai:],
                    first_sequence=events[start]['sequence'],last_sequence=sequence)
                post.append(row)
        code_unchanged=f.code_unchanged()
    except Exception as e:
        fault=dict(type=type(e).__name__,message=str(e),traceback=traceback.format_exc())
        primary_boundary=[];code_unchanged=vm['f'].code_unchanged() if 'f' in vm else None
    finally:
        if 'f' in vm:
            for h in hooks:
                vm['f'].u.hook_del(h)
    return dict(schema_version=1,kind='observation-only-real-produced-gi-idle-latch',
        control=control,
        parent_manifest_sha256=P2_MANIFEST,primary_comparison=comparison,
        original_infantry_vtable=dict(address=VTABLE,source=str(HERE/'native-infantry-vtable.json'),
            sha256=sha(HERE/'native-infantry-vtable.json'),slots=declared_slots),
        live_virtual_identities=list(registry.values()),primary_boundary=primary_boundary,
        events=events,foot_field_writes=writes,latch_writes=[x for x in writes if x['offset']=='0x6b3'],
        setter_sequences=setters,complete_rng_states=streams,
        original_executed_spans={name:dict(start=f'0x{SPANS[name][0]:08X}',end_exclusive=f'0x{SPANS[name][1]:08X}',
            executed=[f'0x{p:08X}' for p in sorted(values)]) for name,values in pcs.items()},
        repeated_real_calls=post,native_code_unchanged=code_unchanged,fault=fault,
        bounds=[
            'Observer hooks read state only. One caller AST insertion installs hooks before unchanged P2 rally configure, or before no-rally open_producer returns. All original native owners, priors, calls, order and stopping criteria remain P2.',
            'The complete unchanged P2 receipt is compared in memory against its immutable selected rally or construction primary. It is referenced by physical SHA rather than copied into this packet.',
            'Latches initialize and reset by original Foot ctor and live FootAI, then set by original Foot idle. No latch/Archive/mission/virtual identity/private Walk state is supplied by this observer.',
            'All three complete native RNG states are content-addressed to reduce duplicate bytes, with exact snapshots at every recorded boundary; parent native request and advancement order remains in the referenced primary.',
            'Repeated calls, when requested, run original Infantry51CBA0(0,1) twice on the second GI after the unchanged primary reaches frame515. Its actual latch1 and Archive0 are retained; this is an additional labeled caller control, not an extra native scheduler visit.',
            'SlaveOwner is Techno+2DC (original ctor6F2E75 and SlaveManager assignment6AF274); Team is Foot+5D4 (ctor4D3308 and Team member insertion6EA56E). Draft attempt1 preserved the actual bytes but mislabeled these two fields; its semantic names are rejected.',
            'Original GI ctor creates a nonnull514 planning object after the early clear; the actual original9C object/header has count+14=0, and real6385D9/6385DC reads0 at the idle predicate. The earlier null-pointer inference is rejected.',
            'No-rally control retains actual producerArchive0 and native UnlimboGuard/Nav0. The original post-Foot InfantryAI occupied-building prefix then calls actual Scatter(NULL,1,1), including native RNG/FNPC/destination and synchronous WalkProcess. No default rally or movement answer is supplied.',
            'Native Gate predicate51BD7B JZ51BD7F retains the Building when4525F0 returns false; true falls to51BD7D XOR EDI,EDI and suppresses Scatter. Earlier prose saying closed/false suppresses Scatter was inverted and is rejected. Stock GAPILE Gate0 skips that branch.',
            'Raw three-word mission/movement/blockage timer observations retain native middle words without assigning semantics to those unused or opaque bytes. Native CdTimer start and duration are first/third words.',
            'No negative-coordinate, END/legacy planning/queued waypoint/slave/team/AI Agent/VehicleThief/DefaultToGuardArea specialized branch execution is asserted. Full startup, producer whole AI/render/network and crowded final second-GI arrival retain P2 limits.',
        ])


