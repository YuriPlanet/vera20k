"""Additive factory output research. Imports immutable accepted native owners."""
from pathlib import Path
from collections import deque
import argparse, hashlib, json, struct, sys, traceback

HERE = Path(__file__).resolve().parent
from .runtime import REPO_ROOT, assets_root
FROZEN = REPO_ROOT  # receipt's legacy metadata field now records the imported repository
ASSETS = assets_root()

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools import native_oracle as native
from tools.spatial_oracle import building_death_anims as death, building_construction as bc
from tools.spatial_oracle import engineer_repair_admission as er
from tools.spatial_oracle import factory_cadence as cadence
from tools.spatial_oracle.building_body_rules import SP
from tools.spatial_oracle.refinery_dock import cell
from tools.spatial_oracle.unit_source_scatter import SCENARIO
from tools.native_oracle import require

WATCH = {0x4C98B0:'factory_ctor',0x4C9C70:'start_production',0x4C9EA0:'start_build',
    0x4C9B20:'factory_ai',0x4CA130:'is_complete',0x4CA160:'get_product',
    0x4FB0E0:'house_place',0x443C60:'building_exit',0x65ADC0:'free_radio',
    0x65A970:'radio_transmit',0x43C2D0:'building_receive',0x6F4AB0:'techno_receive',
    0x70C610:'archive_assignment',0x51DFF0:'infantry_unlimbo',0x517A50:'infantry_ctor',
    0x51AA40:'infantry_destination',0x5B35E0:'queue_mission',0x55BAA0:'logic_add',
    0x4CA1A0:'factory_completed',0x4FAA10:'house_abandon',0x4CA5A0:'start_next',
    0x4CA770:'factory_scalar',0x4C9FF0:'factory_abandon',0x4FB6B0:'record_last_built',
    0x65FA70:'radar_request',0x752700:'eva_request',0x4C6AE0:'place_event_constructor',
    0x44EFB0:'dock_cell_query',0x51BF90:'infantry_can_enter',0x49F9B0:'counter_ctor',
    0x49FA00:'counter_increment',0x51BAB0:'infantry_ai',0x4DA530:'foot_ai',
    0x520F40:'infantry_movement',0x75AC80:'walk_ai',0x75AEC0:'walk_movement',
    0x65AE30:'radio_link_query',0x65AD30:'radio_contact_query',0x4D9960:'foot_pointer_expired',
    0x7077C0:'techno_pointer_expired',0x5F65F0:'object_uninit',0x7258D0:'expiry',0x725C70:'deferred_drain',
    0x4FA350:'human_begin_production',0x5F7900:'type_find_factory',0x4F7870:'house_can_build',
    0x6A6140:'sidebar_production_link',0x6A8710:'strip_insert',0x6A8B30:'strip_ai',
    0x4C6CB0:'event_execute',0x64C380:'frame_event_dispatch',0x517FA0:'infantry_receive_damage',
    0x517D90:'infantry_destructor',0x707CB0:'detach_all',0x51D6F0:'infantry_action'}

def generate(mode, map_prior='legacy', inputs=None):
    if mode != 'queue' or map_prior != 'physical_clear':
        raise ValueError('Private caller requires admitted ordinary queue/physical_clear adapter')
    if inputs is None:
        inputs = death.joined_inputs(ASSETS, damage_fires=True)
    f = death.joined_fixture(inputs, 750, seed=2, building_ai=True)
    u, read = f.u, f.read32
    runtime_mode_before=read(0xA8B238)
    f.phase='native_selected_skirmish_startup_store'
    bc.run_checked(u,0x52E10F,0x52E119)
    runtime_mode=dict(before=runtime_mode_before,after=read(0xA8B238),entry='0x0052E10F',stop='0x0052E119',
        boundary='Selected original PrepareSession Skirmish arm store only; full menu/dialog startup is excluded. Native52E158 tests5 and52E168 calls Skirmish runner6AE2C0; modes0/5 reach local647260 command transfer.')
    assert read(0xA8B238)==5
    clear_map = None
    if map_prior == 'physical_clear':
        before = f.rng()
        bc.invoke(u,0x4E69E0,0)
        tile,name=f.allocate(0x400),f.allocate(32)
        u.mem_write(name,b'Clear01\0')
        require(bc.invoke(u,0x5447C0,tile,0,0xFFFFFFBF,0,name,0)==tile, 'native 0x5447C0 returned an unexpected result')
        raw=(ASSETS/'CLEAR01.TEM').read_bytes()
        w,h=struct.unpack_from('<2I',raw)
        if (w,h)!=(1,1):raise ValueError('This selected retail one-subtile cache boundary requires1x1 Clear01')
        data=f.allocate(len(raw)+16)
        offset=struct.unpack_from('<I',raw,16)[0]
        cache=bytearray(raw);struct.pack_into('<I',cache,16,data+offset)
        u.mem_write(data,bytes(cache));u.mem_write(tile+0xA4,bc.dwords(data))
        u.mem_write(tile+0x2E4,bc.dwords(w&255,h&255))
        prior,after=[],[]
        f.phase='physical_clear_original_cell_recalc'
        for y in range(1,32):
            for x in range(1,32):
                p=cell(x,y);prior.append(read(p+0xEC))
                u.mem_write(p+0x38,bc.dwords(0))
                bc.invoke(u,0x47D2B0,p,0xFFFFFFFF)
                after.append(read(p+0xEC))
        clear_map=dict(source=str(ASSETS/'CLEAR01.TEM'),sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw),
            dimensions=[w,h],tile_constructor='0x005447C0',tile=tile,
            native_recalc='0x0047D2B0',prior_land_counts={str(x):prior.count(x) for x in set(prior)},
            after_land_counts={str(x):after.count(x) for x in set(after)},rng_before=before,rng_after=f.rng(),
            boundary='Retained physical1x1 TMP cache and rebased source pointer supplied as Navigation owner loads it; original IsoTile ctor and Cell Recalc derive land/slope/class. Physical file-loader/theater loop excluded.')
        assert before==f.rng()
    barracks, radio = f.allocate(0x1000), f.allocate(0x100)
    f.building('GAPILE', barracks, radio, coord=(14,14))
    barracks_constructor=f.building_constructor
    door=f.allocate(4);u.mem_write(door,struct.pack('<2h',15,16))
    f.phase='legal_map_diamond_door_control'
    door_admission=bc.invoke(u,0x568300,0x87F7E8,door)&255
    assert door_admission==1
    u.mem_write(barracks+0x74,b'\0')
    require(bc.invoke(u,0x43F180,barracks,3)&255 == 1, 'native 0x43F180 returned an unexpected result')
    u.mem_write(barracks+0xAC,bc.dwords(5))
    u.mem_write(barracks+0xB4,bc.dwords(-1))
    u.mem_write(barracks+0x534,bc.dwords(1))
    f.house_prior(er.HOUSE,(barracks,))
    f.owner_houses = [er.HOUSE, *f.owner_houses[1:]]
    factory, product = 0,0
    delivered=[]
    strip=0x87F7E8+0x1544+2*0xF94
    events,steps,recent=[],[],deque(maxlen=60)
    clocks=[];dispatch_slots={};dispatch_stack=[]
    def signed(a):return struct.unpack('<i',u.mem_read(a,4))[0]
    def vector(a):
        p,n=read(a+4),read(a+16)
        return dict(pointer=p,count=n,items=list(struct.unpack('<'+'I'*n,u.mem_read(p,n*4))) if n else [])
    def infantry_state(p):
        return dict(pointer=p,id=read(p+0x10),type=read(p+0x6C0),
            health=signed(p+0x6C),alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],
            logic=u.mem_read(p+0x98,1)[0],marked=u.mem_read(p+0x74,1)[0],
            location=list(struct.unpack('<3i',u.mem_read(p+0x9C,12))),
            mission=signed(p+0xAC),queued=signed(p+0xB4),nav=read(p+0x5A4),archive=read(p+0x218),
            tether=u.mem_read(p+0x418,1)[0],doing=signed(p+0x6C4),
            contacts=[read(read(p+0xE4)+i*4) for i in range(read(p+0xE8))])
    def state():
        p=product or (read(factory+0x58) if factory else 0)
        return dict(frame=read(bc.FRAME),game_mode=read(0xA8B238),priority=read(0xA8E7AC),rng=f.rng(),
            cursor=read(SCENARIO+0x214),
            house=dict(pointer=er.HOUSE,credits=signed(er.HOUSE+0x30C),
                infantry_factory=read(er.HOUSE+0x53B0),raw_2f8=signed(er.HOUSE+0x2F8),
                spent=signed(er.HOUSE+0x2DC),tech_recheck=u.mem_read(er.HOUSE+0x1FC,1)[0],
                buildings=vector(er.HOUSE+0x68),dirty=list(u.mem_read(er.HOUSE+0x5778,2))),
            barracks=dict(pointer=barracks,type=read(barracks+0x520),health=signed(barracks+0x6C),
                type_flags=({k:u.mem_read(read(barracks+0x520)+v,1)[0] for k,v in dict(gdi=0x16E4,nod=0x16E5,yuri=0x16E6,hospital=0x16C1,armory=0x16C2,weapons_factory=0x16BD).items()} if read(barracks+0x520) else None),
                location=list(struct.unpack('<3i',u.mem_read(barracks+0x9C,12))),
                mission=signed(barracks+0xAC),factory=read(barracks+0x524),
                archive=read(barracks+0x218),contacts=[read(read(barracks+0xE4)+i*4) for i in range(read(barracks+0xE8))]),
            factory=None if not factory else dict(pointer=factory,product=read(factory+0x58),
                stage=signed(factory+0x24),balance=signed(factory+0x60),timer=list(struct.unpack('<4i',u.mem_read(factory+0x2C,16))),
                suspended=u.mem_read(factory+0x70,1)[0],latch=u.mem_read(factory+0x71,1)[0],queue=vector(factory+0x40),
                cadence=cadence.state(bytes(u.mem_read(factory,cadence.FACTORY_SIZE)),signed(er.HOUSE+0x30C),signed(er.HOUSE+0x2DC))),
            infantry=None if not p else infantry_state(p),
            delivered_infantry=[infantry_state(x) for x in delivered],
            strip=dict(pointer=strip,count=read(strip+0x54),factory=read(strip+0x64),changed=u.mem_read(strip+0x3D,1)[0]),
            outlist=dict(count=read(0xA802C8),head=read(0xA802CC),tail=read(0xA802D0),
                first=bytes(u.mem_read(0xA802D4+read(0xA802CC)*0x6F,0x6F)).hex() if read(0xA802C8) else None),
            dolist=dict(count=read(0x8B41F8),head=read(0x8B41FC),tail=read(0x8B4200)),
            house_registry=vector(0xA80228),logic=vector(0x87F778),pending=vector(0xB0F698))
    pending_returns=[]
    def observe(_u,pc,size,_data):
        recent.append(f'0x{pc:08X}')
        if pc==0x64C380:
            sp=u.reg_read(UC_X86_REG_ESP)
            dispatch_slots['return']=sp
            dispatch_stack.append(dict(kind='entry',frame=read(bc.FRAME),sp=sp,ret=read(sp)))
        if pc==0x64CC5E:
            sp=u.reg_read(UC_X86_REG_ESP)
            dispatch_stack.append(dict(kind='exit',frame=read(bc.FRAME),sp=sp,ret=read(sp)))
        if pc in (0x6A8F18,0x647545,0x64C704,0x64C780,0x64C8EB):
            # Physical WINMM wall clock only; simulation eligibility uses
            # the native Event stamp and A8ED84. No native decision is replaced.
            clocks.append(dict(pc=f'0x{pc:08X}',phase=f.phase,frame=read(bc.FRAME),wall_ms=0,
                boundary='Imported WINMM timeGetTime transport returns explicit wall-clock0; only ring timestamps consume it here.'))
            u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_EIP,pc+size)
        for ret,row in pending_returns[:]:
            if pc==ret and u.reg_read(UC_X86_REG_ESP)>row['sp']:
                row.update(result=u.reg_read(UC_X86_REG_EAX),returned_sp=u.reg_read(UC_X86_REG_ESP));pending_returns.remove((ret,row))
                if row['kind']=='place_event_constructor':
                    row['event_bytes']=bytes(u.mem_read(row['this'],0x6F)).hex()
        if pc in WATCH:
            sp=u.reg_read(UC_X86_REG_ESP)
            row=dict(kind=WATCH[pc],pc=f'0x{pc:08X}',phase=f.phase,
                frame=read(bc.FRAME),sp=sp,this=u.reg_read(UC_X86_REG_ECX),caller=f'0x{read(sp):08X}',
                args=list(struct.unpack('<4I',u.mem_read(sp+4,16))))
            if pc==0x65A970:row.update(message=read(sp+4),receiver=read(sp+12))
            if pc==0x752700:row.update(name=f.string(u.reg_read(UC_X86_REG_ECX)))
            if pc==0x51BF90:
                row.update(cell=read(sp+4),land=signed(read(sp+4)+0xEC),ground=read(read(sp+4)+0xE4),
                    occupation=[read(read(sp+4)+x) for x in (0x124,0x128)],priority=read(0xA8E7AC))
            if pc==0x51DFF0:row.update(priority=read(0xA8E7AC))
            pending_returns.append((read(sp),row))
            events.append(row)
    u.hook_add(UC_HOOK_CODE,observe)
    def stack_write(_u,_access,address,size,value,_data):
        slot=dispatch_slots.get('return')
        if slot is not None and address<=slot<address+size:
            dispatch_stack.append(dict(kind='write',frame=read(bc.FRAME),pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',address=address,size=size,value=value))
    u.hook_add(UC_HOOK_MEM_WRITE,stack_write)
    f.events.clear();f.trace.clear();f.draws.clear();f.advances.clear();f.executed.clear()
    def invoke(label,pc,this,*args):
        f.phase=label
        s=dict(label=label,entry=f'0x{pc:08X}',this=this,args=list(args),before=state(),draw_start=len(f.draws),event_start=len(events))
        steps.append(s)
        s['result']=bc.invoke(u,pc,this,*args)
        s.update(after=state(),requests=f.draws[s['draw_start']:],events=events[s['event_start']:])
        return s['result']
    fault=None
    try:
        f.phase='native_selected_house_interface_constructor_block'
        before=state()
        u.reg_write(UC_X86_REG_EBP,er.HOUSE)
        bc.run_checked(u,0x4F5DCC,0x4F5E02)
        steps.append(dict(label=f.phase,entry='0x004F5DCC',stop='0x004F5E02',before=before,after=state(),
            boundary='Original House normal constructor writes seven interface vtables. EBP=this supplied at the original corridor; no whole House normal constructor claim.'))
        assert read(er.HOUSE+0x24)==cadence.HOUSE_MONEY_VTABLE
        invoke('native_authored_cash_control_add_10000',0x4F9950,er.HOUSE,10000)
        invoke('native_selected_infantry_built_counter_ctor',0x49F9B0,er.HOUSE+0x55C8)
        invoke('native_house_registry_startup',0x4E6CE0,0)
        f.phase='native_selected_house_constructor_registry_insert'
        before=state()
        u.reg_write(UC_X86_REG_EBP,er.HOUSE)
        u.reg_write(UC_X86_REG_EBX,0)
        u.reg_write(UC_X86_REG_ESP,SP)
        bc.run_checked(u,0x4F619C,0x4F61EF)
        steps.append(dict(label=f.phase,entry='0x004F619C',stop='0x004F61EF',before=before,after=state(),
            boundary='Original House normal constructor inserts this House into the initialized global vector. EBP=this and EBX=0 supplied at the original corridor; no whole House normal constructor claim.'))
        assert vector(0xA80228)['items']==[er.HOUSE]
        invoke('factory_registry_startup',0x4E6E60,0)
        if mode=='queue':
            invoke('native_place_sentinel_startup',0x6A4BC0,0)
            for i in range(4):invoke('native_strip_constructor_'+str(i),0x6A80A0,0x87F7E8+0x1544+i*0xF94,0)
            invoke('native_selected_infantry_type_cameo_insert',0x6A8710,strip,16,1)
            invoke('native_human_first_e1_begin',0x4FA350,er.HOUSE,16,1,0,0)
            factory=read(er.HOUSE+0x53B0)
            assert factory
            product=read(factory+0x58)
            invoke('native_human_second_e1_queue',0x4FA350,er.HOUSE,16,1,0,0)
            rounds=[];delivered=[]
            steps.append(dict(label='ordered_native_sidebar_foot_factory_event_control',rounds=rounds,before=state(),
                boundary='Native bodies are externally visited in original MainTick/Sidebar/Logic order: Strip, delivered Infantry, Factory, local OutList→DoList/dispatch corridor. Frame input increments after that sequence. Unrelated objects and the whole rendering/checksum MainTick prefix are excluded.'))
            for frame in range(0,400):
                u.mem_write(bc.FRAME,bc.dwords(frame));di,ei=len(f.draws),len(events)
                f.phase='ordered_sidebar_'+str(frame)
                bc.invoke(u,0x6A8B30,strip,0,0)
                for p in delivered:
                    if u.mem_read(p+0x90,1)==b'\1':
                        f.phase='ordered_delivered_infantry_'+str(frame)
                        # Invoke the unchanged native body using the existing
                        # checked execution owner with a larger observer budget.
                        u.mem_write(SP,bc.dwords(native.RET_MAGIC))
                        u.reg_write(UC_X86_REG_ECX,p);u.reg_write(UC_X86_REG_ESP,SP)
                        bc.run_checked(u,0x51BAB0,native.RET_MAGIC,count=20_000_000,timeout_us=50_000_000)
                if read(er.HOUSE+0x53B0):
                    factory=read(er.HOUSE+0x53B0);f.phase='ordered_factory_'+str(frame);bc.invoke(u,0x4C9B20,factory)
                pending_product=read(factory+0x58)
                f.phase='ordered_native_local_events_'+str(frame)
                u.reg_write(UC_X86_REG_ESP,SP);bc.run_checked(u,0x6474C7,0x6474BF)
                if pending_product and u.mem_read(pending_product+0x81,1)==b'\0' and pending_product not in delivered:
                    delivered.append(pending_product)
                now=state()
                rounds.append(dict(frame=frame,rng=now['rng'],factory=now['factory'],infantry=now['infantry'],barracks=now['barracks'],
                    house=now['house'],strip=now['strip'],outlist=now['outlist'],dolist=now['dolist'],
                    delivered=list(delivered),delivered_infantry=now['delivered_infantry'],events=events[ei:],requests=f.draws[di:]))
                if len(delivered)==2 and not any(now['barracks']['contacts']):break
            steps[-1]['after']=state()
            steps[-1]['delivered']=list(delivered)
    except Exception as exc:
        fault=dict(type=type(exc).__name__,message=str(exc),pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',recent=list(recent),traceback=traceback.format_exc())
    return dict(schema_version=1,kind='bounded-factory-output-research',mode=mode,
        native_path=str(native.configured_gamemd()),
        runtime=dict(python=sys.version,unicorn=__import__('unicorn').__version__),
        source_frozen=str(FROZEN),assets_reference=str(ASSETS),
        selected_inputs=dict(construction_layers=inputs.layers,type_rows=inputs.type_rows,
            death_layers=inputs.death_layers,engineer_layers=inputs.engineer_layers,
            scenario_projection=inputs.death_scenario_projection,native_code_sections=inputs.code_identity),
        map_prior=map_prior,runtime_mode=runtime_mode,physical_clear_map=clear_map,barracks_constructor=barracks_constructor,
        map_diamond_prior=dict(width=read(0x87F7E8+0xF4),height=read(0x87F7E8+0xF8),door=[15,16],native_door_admitted=door_admission),
        steps=steps,events=events,requests=f.draws,advances=f.advances,clocks=clocks,dispatch_stack=dispatch_stack,transport_trace=f.trace,final=state(),fault=fault,
        executed=[f'0x{p:08X}' for p in sorted(f.executed)],original_code_unchanged=f.code_unchanged(),
        bounds=['Existing fatal fixture supplies admitted unrelated GAPOWR and MTNK/map/House priors; neither receives a Logic visit in this factory control.',
            'Whole original GAPILE and held E1 constructors execute. GAPILE current admission/House list is a supplied ordinary prior; original Mark3 populates its raw foundation.',
            ('Whole original House BeginProduction creates the first E1 and queues the second; original FactoryAI produces stage54/balance0; the original Strip/OutList/DoList/dispatch emits and consumes actual PLACE. Only selected actors are visited, with a20M instruction/50s checked InfantryAI execution bound.' if mode=='queue' else 'House existing Factory link is supplied, not whole BeginProduction ingress.'),
            ('No completion stage or balance is supplied in the human queue control.' if mode=='queue' else 'The initial downstream PLACE control supplies stage54 with its original200 balance explicitly; it is not real build completion or a charged/refunded first-E1 control.')])

