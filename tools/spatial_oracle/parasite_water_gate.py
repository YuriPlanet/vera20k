"""Original ParasiteClass CanInfect water gate for a Naval owner.

CanInfect @ 0x0062A8E0 runs whole, with the victim and owner as UnitClass
objects on the original vtable (0x007F5C70): GetTechnoType (vt+0x84 ->
0x006F3270 -> 0x00741490) reads +0x6C4, and GetCell (vt+0x1BC -> 0x005F6960)
looks the victim's Location up through MapClass @ 0x00565730, which answers
the shared dummy (0x00ABDC50) for an index past the cell count or a NULL slot.
The WaterSet test 0x00485060 reads the cell's tile (+0x38) against the theater
base at 0x00AA0738. Every other CanInfect gate is supplied passing.
"""
from pathlib import Path
import struct

from tools.native_oracle import SCRATCH, call, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import dwords

CAN_INFECT = 0x0062A8E0
UNIT_VTABLE = 0x007F5C70
MAP = 0x0087F7E8
WATER_SET = 0x00AA0738
DUMMY = 0x00ABDC50

PARASITE, OWNER, OWNER_TYPE, VICTIM, VICTIM_TYPE, CELL, CELLS = (
    SCRATCH + offset for offset in (0x0000, 0x0800, 0x1000, 0x2000, 0x3000, 0x4000, 0x5000))
# The real cell sits at (5, 2), slot 2 * 512 + 5; slot (4, 2) is NULL and the
# map holds slots 0..1029, so (6, 2) is past the count.
REAL_CELL, HOLE_CELL, PAST_CELL = (5, 2), (4, 2), (6, 2)
CELL_COUNT = 2 * 512 + 6


def dword(value):
    return struct.pack('<I', value & 0xFFFFFFFF)


def run(name, *, water_set, tile, owner, naval, victim_cell):
    """One CanInfect call; returns the row with its AL result."""
    x, y = victim_cell
    writes = {
        WATER_SET: dword(water_set),
        # Cell constructor 0x0047BC11 gives the shared dummy tile 0xFFFF.
        DUMMY + 0x38: dword(0xFFFF),
        MAP + 0x13C: dword(CELLS),
        MAP + 0x140: dword(CELL_COUNT),
        CELLS + 4 * (REAL_CELL[1] * 512 + REAL_CELL[0]): dword(CELL),
        CELL + 0x38: dword(tile),
        PARASITE + 0x24: dword(OWNER if owner else 0),
        OWNER: dword(UNIT_VTABLE),
        OWNER + 0x6C4: dword(OWNER_TYPE),
        OWNER_TYPE + 0xCCE: bytes([1 if naval else 0]),
        VICTIM: dword(UNIT_VTABLE),
        VICTIM + 0x6C: dword(100),  # Health
        VICTIM + 0x90: b'\x01',  # IsAlive
        VICTIM + 0x6C4: dword(VICTIM_TYPE),
        VICTIM + 0x9C: struct.pack('<3i', x * 256 + 128, y * 256 + 128, 0),
        VICTIM_TYPE + 0xD38: b'\x01',  # Parasiteable
    }
    result = call(CAN_INFECT, ecx=PARASITE, stack_args=[VICTIM], writes=writes)
    return dict(name=name, water_set=water_set, tile=tile, owner=owner, naval=naval,
                victim_cell=list(victim_cell), admits=bool(result['eax'] & 0xFF))


def generate():
    rows = []
    for water_set in (314, -1):
        for offset in (-1, 0, 13, 14):
            tile = water_set + offset
            rows.append(run(f'naval_tile_{offset:+d}_base_{water_set}', water_set=water_set,
                            tile=tile, owner=True, naval=True, victim_cell=REAL_CELL))
        rows.append(run(f'land_owner_tile_-1_base_{water_set}', water_set=water_set,
                        tile=water_set - 1, owner=True, naval=False, victim_cell=REAL_CELL))
    rows.append(run('no_owner_tile_-1', water_set=314, tile=313, owner=False, naval=False,
                    victim_cell=REAL_CELL))
    for name, cell in (('naval_null_slot_dummy', HOLE_CELL), ('naval_past_count_dummy', PAST_CELL)):
        rows.append(run(name, water_set=314, tile=314, owner=True, naval=True, victim_cell=cell))
    rows.append(run('naval_null_slot_dummy_lunar', water_set=-1, tile=0, owner=True, naval=True,
                    victim_cell=HOLE_CELL))
    return dict(rows=rows)


def metadata():
    return provenance(
        scope=__doc__,
        assumptions=[
            'Victim and owner are UnitClass objects on the original vtable; the parasite '
            'object supplies only its Owner (+0x24).',
            'Victim gates other than the water test are supplied passing: not in limbo '
            '(+0x81 = 0), alive (+0x90 = 1), Health (+0x6C) = 100, no parasite (+0x694 = 0), '
            'Parasiteable type (+0xD38 = 1), no bunker link (+0x2E4 = 0).',
            'The shared dummy cell holds the tile 0xFFFF its constructor (0x0047BC11) stores.',
        ],
        substitutions=['None: CanInfect, the vtable getters, GetCell, the map lookup and the '
                       'WaterSet test all run original code.'],
        entry_points={'can_infect': CAN_INFECT, 'get_cell': 0x005F6960, 'map_cell': 0x00565730,
                      'water_set_test': 0x00485060})



def grapple_generate(*,drone=False):
    import os,hashlib
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_ECX
    from tools.spatial_oracle import building_death_anims as host,building_construction as bc
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    from tools.procedural_drawing_oracle.electric_bolt import RngTrace
    root=Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    inputs=host.grapple_inputs(root,drone=drone);f=host.grapple_fixture(inputs,root,drone=drone)
    u,read=f.u,f.read32;source,victim,manager=f.grapple_source,f.grapple_victim,f.grapple_manager
    scenario=read(0xA8B230)
    rng=RngTrace.__new__(RngTrace);rng.u=u
    rng.streams={'main':0x886B88,'scenario':scenario+0x218,'mapgen':0xABE890}
    rng.requests=[];rng.pending=[];u.hook_add(UC_HOOK_CODE,rng.observe)
    def xyz(p):return list(struct.unpack('<3i',u.mem_read(p,12)))
    def member(p,base):return p in [read(read(base+4)+4*i) for i in range(read(base+16))]
    def actor(p):
        return dict(native_id=read(p+0x10),alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],
            health=read(p+0x6C),location=xyz(p+0x9C))
    def timer(p):
        return dict(start=read(p),opaque=read(p+4),duration=read(p+8))
    def snapshot():
        a=read(manager+0x44)
        result=dict(source=actor(source),victim=dict(actor(victim),sinking=u.mem_read(victim+0x3CD,1)[0],
                angle_bits=read(victim+0x328),paralysis_timer=[read(victim+0x6A0+o) for o in (0,4,8)]),
            manager=dict(native_id=struct.unpack('<i',dwords(read(manager+0x10)))[0],phase=read(manager+0x48),stage=read(manager+0x4C),delay=read(manager+0x50),
                invalidated=u.mem_read(manager+0x54,1)[0],anim_native_id=read(a+0x10) if a else None,
                timer_2c=timer(manager+0x2C),timer_38=timer(manager+0x38)),
            held_anim=constructor_state(u,a) if a else None)
        if drone:result['victim']['rocking_bits']=list(struct.unpack('<4I',u.mem_read(victim+0x328,16)))
        return result
    pending=[];constructed=[];events=[];particle_constructed=[];systems=[];impulse_pending=[]
    def event(kind,**values):events.append(dict(event=kind,rng_request_count=len(rng.requests),native_cursor=read(scenario+0x214),**values))
    def observe(_u,pc,_size,_data):
        sp=u.reg_read(UC_X86_REG_ESP)
        if impulse_pending and (pc,sp)==impulse_pending[-1][:2]:
            _,_,row=impulse_pending.pop();row['after_bits']=list(struct.unpack('<4I',u.mem_read(victim+0x328,16)))
        if pending and (pc,sp)==pending[-1][:2]:
            _,_,p,row=pending.pop()
            if row.pop('_kind',None)=='system':
                from types import SimpleNamespace
                from tools.procedural_drawing_oracle.electric_bolt import SparkBirth
                proxy=SimpleNamespace(u=u,read32=read,scenario=scenario,systems=systems,particles=[],lights=[])
                state=SparkBirth.runtime_state(proxy)
                row.update(state=state['systems'][-1],rng_request_after=len(rng.requests),native_cursor_after=read(scenario+0x214))
                particle_constructed.append(row);event('ParticleSystemCtor_return',native_id=row['state']['native_id'])
                return
            row.update(constructor_state(u,p),logic_registered=member(p,0x87F778),
                display_layers=[i for i in range(5) if member(p,0x8A0360+24*i)],
                rng_request_after=len(rng.requests),native_cursor_after=read(scenario+0x214))
            constructed.append(row);event('AnimCtor_return',native_id=row['native_id'])
        if pc==0x421EA0:
            p=u.reg_read(UC_X86_REG_ECX);t=read(sp+4)
            row=dict(type=f.string(t+0x24),args=dict(position=xyz(read(sp+8)),delay=read(sp+12),
                loops=read(sp+16),flags=read(sp+20),z_adjust=struct.unpack('<i',dwords(read(sp+24)))[0],extra=read(sp+28)),
                native_cursor_before=read(scenario+0x214),rng_request_before=len(rng.requests))
            pending.append((read(sp),sp+32,p,row));event('AnimCtor',type=row['type'])
        elif drone and pc==0x62DC50:
            p=u.reg_read(UC_X86_REG_ECX);systems.append(p)
            row=dict(_kind='system',type=f.string(read(sp+4)+0x24),position=xyz(read(sp+8)),
                args=dict(target_object_is_null=read(sp+12)==0,owner_is_null=read(sp+16)==0,
                    target=xyz(read(sp+20)),house_is_null=read(sp+24)==0),
                native_cursor_before=read(scenario+0x214),rng_request_before=len(rng.requests))
            pending.append((read(sp),sp+28,p,row));event('ParticleSystemCtor',type=row['type'])
        elif drone and pc==0x70B280:
            row=dict(origin=xyz(read(sp+4)),force_bits=read(sp+8),no_dampen=read(sp+12)&255,
                before_bits=list(struct.unpack('<4I',u.mem_read(victim+0x328,16))))
            event('ApplyRocker',**row);impulse_pending.append((read(sp),sp+16,events[-1]))
        elif pc==0x410230:event('CreateId',caller=f'{read(sp):08X}')
        elif pc==0x629D3B:event('ReceiveDamage_call')
    u.hook_add(UC_HOOK_CODE,observe)
    selected={};angle_rows={};first_roll=None;opposite=None;damage=[];previous=None
    for frame in range(201):
        u.mem_write(bc.FRAME,dwords(frame));before=rng.states();prior=snapshot()
        cursor=read(scenario+0x214);start=len(rng.requests);mark=len(constructed);ev=len(events);particle_mark=len(particle_constructed)
        if drone:bc.invoke(u,0x70B570,victim)
        bc.invoke(u,0x629FD0 if drone else 0x6297F0,manager)
        after=snapshot();assert not pending and not rng.pending and not impulse_pending
        assert after['source']['alive']==1 and after['source']['limbo']==1
        assert after['victim']['alive']==1 and after['victim']['limbo']==0
        row=dict(input=dict(frame=frame,entry='006297F0'),before=prior,after=after,
            native_cursor_before=cursor,native_cursor_after=read(scenario+0x214),
            rng_before=before,rng_after=rng.states(),rng_requests=rng.requests[start:],
            raw_draw_count=sum(len(r['raw_draws']) for r in rng.requests[start:]),
            anims=constructed[mark:],events=events[ev:])
        if drone:
            row['input']['entry']=['0070B570','00629FD0']
            row['particle_systems']=particle_constructed[particle_mark:]
            from types import SimpleNamespace
            from tools.projectile_oracle.ifv_fire_coord import native_get_flh
            prior_rng=rng.states();prior_cursor=read(scenario+0x214)
            out=f.allocate(12)
            view=SimpleNamespace(invoke=lambda address,obj,args:bc.invoke(u,address,obj,*args))
            native_get_flh(view,victim,out)
            row['victim_flh']=xyz(out)
            assert rng.states()==prior_rng and read(scenario+0x214)==prior_cursor
            if frame in (0,1,2,10):selected[frame]=row
            if after['victim']['health']<prior['victim']['health']:
                damage.append(row);selected[frame]=row
                if previous is not None:selected[previous['input']['frame']]=previous
            if damage and frame==damage[-1]['input']['frame']+1:selected[frame]=row
            if len(damage)>=2 and frame>damage[-1]['input']['frame']:break
            previous=row
            continue
        if frame==0:selected[frame]=row
        branch=next((r for r in row['rng_requests'] if r['caller'] in ('0x6299ae','0x629d5d')),None)
        if branch is not None:
            if first_roll is None:first_roll=(after['manager']['phase'],frame);selected[frame]=row
            elif after['manager']['phase']!=first_roll[0] and opposite is None:opposite=frame;selected[frame]=row
        if after['victim']['health']<prior['victim']['health']:
            damage.append(row)
            if len(damage)==1:selected[frame]=row
        phase,stage=after['manager']['phase'],after['manager']['stage']
        if phase in (2,3) and stage in (1,5) and stage!=prior['manager']['stage']:
            angle_rows.setdefault((phase,stage),row)
        if len(angle_rows)==4 and opposite is not None and len(damage)>=2:
            for value in angle_rows.values():selected[value['input']['frame']]=value
            if len(selected)<8:selected[damage[1]['input']['frame']]=damage[1]
            break
    assert (drone or len(angle_rows)==4 and opposite is not None) and len(damage)>=2
    assert (drone or 7<=len(selected)<=8) and f.code_unchanged()
    return dict(schema=1,initial=dict(f.grapple_context,layers=inputs.grapple_layers,
        anim_types=inputs.grapple_anims,assets={name:dict(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
            for name in (() if drone else ('SQDG.SHP','WAKE1.SHP','H2O_EXP1.SHP','H2O_EXP2.SHP','H2O_EXP3.SHP'))
            for raw in [(root/name).read_bytes()]}),visits_end=frame,rows=[selected[k] for k in sorted(selected)])


def grapple_metadata():
    return provenance(scope='Fresh stock SQD/DEST direct Parasite6297F0 RNG/angle component with actual constructors, Unlimbo, ObjectLimbo and AttachTo',
        assumptions=[
            'Existing building_death_anims physical reader/VM/allocator/House projection owns runtime. Actual SQD/DEST747620, ASW41CC20, SquidGrab/SQDJUMP/ParasitePlus readers and SQDG/WAKE1/H2O_EXP1..3 ART/SHP/sound binding execute; no full Rules process or native VXL binding is claimed.',
            'Existing physical TMP binding owner loads genuine Water01.tem through original IsoTileType5447C0. Native TEMPERATMD scalar reader supplies WaterSet314/count838; sparse primary array/cache header and relocated TMP offsets are explicit existing input boundaries. Original47D2B0 Recalc precedes real UnitUnlimbo and derives water LandType2. Native tile before/after and true admission results are retained.',
            'Original AircraftType4E73E0/Aircraft4E6BE0/Parasite629190/Spawn6B6C10 registries, Ship empty Coord69EBF0 and House counters49F9B0/748FD0 initialize selected prerequisites. Real stock DEST SpawnManager/ASW constructors execute. Source/victim are opposite existing fixture Houses, health200/600 from physical types, normal rank and fixed admitted positions. No full House/scenario startup claim.',
            'Frame0 precedes all selected constructors. Actual ObjectLimbo5F4D30 returns1 immediately before CanInfect/AttachTo62A980. Both entry snapshots require source alive1/limbo1 and victim alive1/limbo0; real reciprocal links are asserted. Full FireAt/Bullet/LimboLaunch producer is excluded, not replaced by an attached prior.',
            'Scenario map lighting/reset and cell relight are excluded. Initial lighting records the actual supplied zero Scenario profile and selected Cell104..10E scalars. Snapshot z_adjust reads Anim100, not AnimFC. Original629DDE/629DE5 copies signed Cell10A into AnimFC; this zero-light corpus neither records that field nor measures pixels. The separate squid_grapple_light control supplies the nonzero field witness.',
            'Initial game_speed is the signed observed GameOptions+A8EB60 input inherited from the joined construction fixture. Normalized Anim rates execute original SpeedNormalize5FB2E0 with that input; consumers must import it rather than use their default speed.',
            'Only whole6297F0 is visited at monotonically increasing frames through the bounded collection endpoint. Other actor/Anim AI, audio update, whole Logic, culling/release/expiry and scene pixels are excluded. Original helper, direct sine/f32 angle store, all wake/splash ftol, full Anim constructors/Start/ID/Logic/Display and nonlethal receiver execute.',
            'Full Main/Scenario/MapGen states are observed before/after every retained row without reseeding the admitted world. Ordered raw words include rejected ranged draws; constructor entry/return request indexes and native cursors preserve interleaving. Stored roll is native f32; simulation fixed-point quantization is a separate consumer boundary.',
        ],substitutions=[
            'Inherited successful bounded allocation/free, physical lexical INI and requested-SHP cache, IDX/BAG transport, Windows GUID/COM activation/OleRun through original factories, and declared presentation sinks. No selector/trig/ftol/RNG/GetCoords/constructor/Unlimbo/AttachTo/damage gameplay return or original instruction is replaced.'
        ],entry_points=dict(grapple=0x6297F0,stage=0x629720,wakes=0x629E90,attach=0x62A980,can_infect=0x62A8E0,unit_ctor=0x7353C0,unit_unlimbo=0x737BA0,object_limbo=0x5F4D30,anim_ctor=0x421EA0,anim_start=0x424CE0,next=0x65C780,ranged=0x65C7E0))


def drone_metadata():
    return provenance(scope='Two whole original629FD0 DRON bites interleaved with preceding70B570 visits and real SparkSys constructors, plus retained fullGetFLH readbacks',
        assumptions=[
            'Existing grapple_inputs/grapple_fixture configuration owns the physical reader and joined runtime, parameterized to DRON/MTNK/DroneJump/JUMP/Parasite on native-recalculated physical Clear tile0. Full selected Unit/weapon/projectile/warhead readers and actual selected SoundList execute. Type Weight/Strength, weapon Damage/ROF/Paralyzes, actual actor facing/coordinates/House slots, timers, game speed and component lighting are observed rather than imported from Rust defaults.',
            'Existing electric_bolt.read_spark_types owns full original SparkSys/Spark constructors and layered readers on this same physical Reader VM; native type registries and heap references transfer through the existing joined runtime owner. Prepared empty runtime capacity is the existing transport boundary. Full62DC50/AbstractCreateID/Unlimbo/Logic execute before each upstream random-side request; no constructor or RNG result is supplied.',
            'GTNK VXL/HVA/VPL original loader755DB0/5BD5C0 and postpass7160B4 execute through the existing physical raster VM. Actual produced header bytes/error flag and half-sizes are explicit component inputs to the joined MTNK Type+B0 and scalar fields; original6F9E10 admission returns1 and70B280 gate executes. Internal model pointers are not claimed as a full joined TypeReadVXL/I/O binding. File identities and produced fields are retained; sidecar records supported binary and replay command.',
            'Frame0 precedes selected construction. Original source ObjectLimbo5F4D30 then CanInfect/AttachTo execute with living DRON/MTNK; full FireAt/Bullet/LimboLaunch is excluded. At each monotonic visit the victim whole70B570 precedes manager whole629FD0. Seven sparse rows cover two nonlethal bites, both actual random sides, lurch and settled incoming state. Full three-stream states and every original raw request/rejection, constructor/ID checkpoints and live coordinates are retained without per-bite reseeding.',
            'Native f32 angles/rates remain authoritative between visits. Current component excludes other actor/ParticleSystem/Particle/Anim AI, whole Logic, culling/release/death/expiry and native scene pixels. Selected SparkSys constructors create no particles before AI and consume no RNG; this is observed, not assumed. RNG after this whole encounter is not an attribution proof for an ordinary full match.',
            'Existing native_slope initializes original matrix/quaternion/camera prerequisites on the joined VM. Full GetFLH6F3AD0 reuses the ifv_fire_coord owner on actual MTNK inputs after each visit and asserts unchanged RNG/native cursor. The readonly query is a coordinate-control comparison, not a native actual tank firing claim.',
        ],substitutions=grapple_metadata()['substitutions'],entry_points=dict(parasite=0x629FD0,rocking=0x70B570,impulse=0x70B280,
            side_choice=0x62A17F,impulse_call=0x62A21C,particle_ctor=0x62DC50,get_flh=0x6F3AD0,drive_matrix=0x4AFF60))

if __name__ == '__main__':
    import sys
    if '--drone' in sys.argv:
        finish_vectors(lambda:grapple_generate(drone=True),Path(__file__).with_name('parasite_drone_rng.json'),
            provenance=drone_metadata,argv=[a for a in sys.argv[1:] if a!='--drone'])
        raise SystemExit(0)
    if '--grapple' in sys.argv:
        finish_vectors(grapple_generate,Path(__file__).with_name('parasite_grapple_rng.json'),
            provenance=grapple_metadata,argv=[a for a in sys.argv[1:] if a!='--grapple'])
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
