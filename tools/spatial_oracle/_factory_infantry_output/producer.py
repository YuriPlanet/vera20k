"""Additive producer admission control using existing frozen native owners.

The selected House vector/counter constructor corridors execute unchanged;
the new GAPILE is never supplied as an admitted object or roster member.
"""
from pathlib import Path
from collections import deque
import argparse, hashlib, json, struct, sys, traceback

HERE = Path(__file__).resolve().parent
from .runtime import REPO_ROOT, assets_root
FROZEN = REPO_ROOT
ASSETS = assets_root()
from tools import native_oracle as native
from tools.spatial_oracle import building_death_anims as death, building_construction as bc
from tools.spatial_oracle import engineer_repair_admission as er
from tools.spatial_oracle.refinery_dock import cell
from tools.spatial_oracle.building_body_rules import SP
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *

WATCH = {
    0x43B740: 'building_ctor', 0x440580: 'building_unlimbo', 0x6F6CA0: 'techno_unlimbo',
    0x5F4EC0: 'object_unlimbo', 0x449440: 'building_can_enter',
    0x4FF700: 'add_tracking', 0x502A80: 'add_live', 0x4FFA50: 'factory_count_add',
    0x508C30: 'house_power_consumer', 0x448070: 'set_primary', 0x509140: 'update_factory_tech', 0x445880: 'building_limbo',
    0x4FF980: 'factory_count_remove', 0x4FF550: 'remove_tracking', 0x5025F0: 'remove_live',
    0x5F65F0: 'object_uninit', 0x7258D0: 'pointer_expiry', 0x725C70: 'deferred_drain',
    0x43BCF0: 'building_destructor', 0x65ADC0: 'free_radio', 0x5F7900: 'find_factory',
    0x4FB9B0: 'house_invalidate_pointer', 0x49F9B0: 'counter_ctor', 0x49FA00: 'counter_increment', 0x49FAE0: 'counter_decrement',
    0x65A970: 'radio_transmit', 0x43C2D0: 'building_receive', 0x6F4AB0: 'techno_receive',
    0x447780: 'begin_mode', 0x44D6A0: 'enter_idle_mode', 0x445F80: 'grand_opening',
}


def generate(inputs=None):
    inputs = inputs or death.joined_inputs(ASSETS, damage_fires=True)
    f = death.joined_fixture(inputs, 750, seed=2, building_ai=True)
    u, read = f.u, f.read32
    producer, coordinate = f.allocate(0x1000), f.allocate(16)
    u.mem_write(coordinate, bc.dwords(20*256+128, 14*256+128, 0))
    events, writes, steps, pending = [], [], [], []
    recent = deque(maxlen=50)

    def signed(a): return struct.unpack('<i', u.mem_read(a, 4))[0]
    def vector(a):
        p, count = read(a+4), read(a+16)
        if count > 1024: raise ValueError(('unexpected_vector_count', hex(a), count))
        return dict(vtable=read(a), pointer=p, capacity=read(a+8), count=count,
                    items=list(struct.unpack('<'+'I'*count, u.mem_read(p, count*4))) if count else [])
    def radio_links(a):
        p, count = read(a+4), read(a+8)
        if count > 1024: raise ValueError(('unexpected_radio_size', hex(a), count))
        return dict(vtable=read(a),pointer=p,size=count,
                    items=list(struct.unpack('<'+'I'*count,u.mem_read(p,count*4))) if count else [])
    def state():
        p = producer
        return dict(frame=read(bc.FRAME), active_game=u.mem_read(0xA8E9A0, 1)[0], game_mode=read(0xA8B238),
            rng=f.rng(), priority=read(0xA8E7AC),
            type_registration=dict(registry=vector(0xA83C68), gapile_index=signed(f.types['GAPILE']+0xDF8)),
            house=dict(pointer=er.HOUSE, buildings=vector(er.HOUSE+0x68), quantity=signed(er.HOUSE+0x2F0),
                       raw_factory_counters={hex(o):signed(er.HOUSE+o) for o in (0x5378,0x537C,0x5380,0x5384,0x5388)},
                       counters={hex(o):bytes(u.mem_read(er.HOUSE+o,20)).hex() for o in (0x5500,0x5550,0x553C,0x558C)},
                       power=signed(er.HOUSE+0x53A4),drain=signed(er.HOUSE+0x53A8),dirty=list(u.mem_read(er.HOUSE+0x5778,2)),
                       tech_recheck=u.mem_read(er.HOUSE+0x1FC,1)[0]),
            producer=dict(pointer=p,id=read(p+0x10),abstract_flags=read(p+0x14),type=read(p+0x520),
                       health=signed(p+0x6C),alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],
                       logic=u.mem_read(p+0x98,1)[0],marked=u.mem_read(p+0x74,1)[0],primary=u.mem_read(p+0x3D3,1)[0],
                       mission=signed(p+0xAC),queued=signed(p+0xB4),body=signed(p+0x534),placed=u.mem_read(p+0x6E4,1)[0],
                       location=list(struct.unpack('<3i',u.mem_read(p+0x9C,12))),contacts=radio_links(p+0xE0)),
            expiry_consumers=vector(0xB0F720),house_registry=vector(0xA80228),logic=vector(0x87F778),pending=vector(0xB0F698),
            footprint=[dict(cell=[x,y],head=read(cell(x,y)+0xE4),occupation=read(cell(x,y)+0x124))
                       for y in (14,15) for x in (20,21,22)])
    def observe(_u, pc, size, _data):
        recent.append(f'0x{pc:08X}')
        for ret, row in pending[:]:
            if pc==ret and u.reg_read(UC_X86_REG_ESP)>row['sp']:
                row['result']=u.reg_read(UC_X86_REG_EAX); row['after']=state(); pending.remove((ret,row))
        if pc in WATCH:
            sp=u.reg_read(UC_X86_REG_ESP)
            row=dict(kind=WATCH[pc],pc=f'0x{pc:08X}',phase=f.phase,frame=read(bc.FRAME),
                     sp=sp,this=u.reg_read(UC_X86_REG_ECX),caller=f'0x{read(sp):08X}',
                     args=list(struct.unpack('<4I',u.mem_read(sp+4,16))),before=state())
            events.append(row);pending.append((read(sp),row))
    watched_ranges=((er.HOUSE+0x68,er.HOUSE+0x80),(er.HOUSE+0x2F0,er.HOUSE+0x2F4),
                    (er.HOUSE+0x5378,er.HOUSE+0x538C),(producer+0xE0,producer+0xF8))
    def written(_u,_access,address,size,value,_data):
        if any(address<b and a<address+size for a,b in watched_ranges):
            writes.append(dict(phase=f.phase,frame=read(bc.FRAME),pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',
                               address=address,size=size,before=bytes(u.mem_read(address,size)).hex(),value=value))
    u.hook_add(UC_HOOK_CODE,observe);u.hook_add(UC_HOOK_MEM_WRITE,written)
    f.events.clear();f.trace.clear();f.draws.clear();f.advances.clear();f.executed.clear()

    def invoke(label,pc,this,*args):
        f.phase=label;s=dict(label=label,entry=f'0x{pc:08X}',this=this,args=list(args),before=state())
        steps.append(s);s['result']=bc.invoke(u,pc,this,*args);s['after']=state();return s['result']
    def corridor(label,entry,stop,**registers):
        f.phase=label;s=dict(label=label,entry=f'0x{entry:08X}',stop=f'0x{stop:08X}',registers=registers,before=state())
        steps.append(s)
        for name,value in registers.items():u.reg_write(globals()['UC_X86_REG_'+name.upper()],value)
        u.reg_write(UC_X86_REG_ESP,SP);bc.run_checked(u,entry,stop,count=20_000_000,timeout_us=50_000_000)
        s['after']=state()
    fault=None
    try:
        corridor('original_prepare_session_active_game_store',0x52D9D7,0x52D9DE)
        corridor('original_selected_skirmish_mode_store',0x52E10F,0x52E119)
        corridor('original_house_membership_vector_constructor_sequence',0x4F54B4,0x4F55FE,ebp=er.HOUSE,ebx=0,eax=inputs.country)
        corridor('original_house_interface_constructor_block',0x4F5DCC,0x4F5E02,ebp=er.HOUSE,ebx=0)
        corridor('original_house_counter_constructor_sequence',0x4F5AAB,0x4F5B39,ebp=er.HOUSE,ebx=0)
        invoke('original_house_registry_startup',0x4E6CE0,0)
        corridor('original_house_constructor_expiry_consumer_registration',0x4F5FB1,0x4F6004,ebp=er.HOUSE,ebx=0)
        corridor('original_house_constructor_identity_from_registry',0x4F6008,0x4F6013,ebp=er.HOUSE,ebx=0)
        corridor('original_house_constructor_registry_insert',0x4F619C,0x4F61EF,ebp=er.HOUSE,ebx=0)
        corridor('original_gapile_type_registration_tail',0x45E2ED,0x45E362,esi=f.types['GAPILE'],ebx=0,edi=0xFFFFFFFF)
        invoke('original_empty_house_power_consumer',0x508C30,er.HOUSE)
        invoke('original_stock_gapile_constructor',0x43B740,producer,f.types['GAPILE'],er.HOUSE)
        invoke('original_stock_gapile_unlimbo',0x440580,producer,coordinate,64)
        invoke('original_admitted_house_power_consumer',0x508C30,er.HOUSE)
    except Exception as exc:
        fault=dict(type=type(exc).__name__,message=str(exc),pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',
                   recent=list(recent),traceback=traceback.format_exc())
    return dict(schema_version=1,kind='bounded-native-producer-admission',
        native_path=str(native.configured_gamemd()),
        source_frozen=str(FROZEN),assets_reference=str(ASSETS),
        selected_inputs=dict(construction_layers=inputs.layers,type_rows=inputs.type_rows,scenario_projection=inputs.death_scenario_projection),
        steps=steps,events=events,writes=writes,requests=f.draws,advances=f.advances,transport_trace=f.trace,
        final=state(),fault=fault,executed=[f'0x{p:08X}' for p in sorted(f.executed)],original_code_unchanged=f.code_unchanged(),
        bounds=['Original selected PrepareSession stores set ActiveGame1 and GameMode5. Full menu/scenario startup excluded.',
                'Existing frozen joined fixture supplies unrelated admitted actors, country/House/map priors and flat-cell terrain. None receives a selected Logic visit.',
                'Original House membership-vector corridor,12Counter constructors, interface vtable block, expiry-consumer insertion, House-global vector startup/identity/insertion run; wider House constructor/admission is excluded.',
                'Original45E2ED..45E362 registers GAPILE after the existing selected registry, including native-produced GAPOWR and dependency types. Sparse registry storage is supplied; this is not full retail type registration order.',
                'New producer is whole original GAPILE ctor/Unlimbo. No admitted flags, radio vector or House roster entry is supplied for it.',
                'CanEnter rejection uses the existing physical occupied GAPOWR coordinate; later legal retry uses authored20,14. No placement decision is supplied.',
                'Original508C30 power consumer executes for empty/admitted/removed House; wider House AI scheduling is excluded.',
                'No building-factory build completion, click/HousePlace ingress, later building AI or whole match is established here.'])


