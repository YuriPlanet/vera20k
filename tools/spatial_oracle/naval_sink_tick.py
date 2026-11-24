"""Original UnitAI sinking suffix on state produced by the actual C4 receiver.

Entry7364A1 is an explicit interior frame after FootAI/TurretAI. No preceding
or subsequent AI is executed. Ordinary rows stop at7365BB; wake rows stop at
the actual Anim constructor entry after the original Scenario jitter; terminal
rows stop at FootUnInit entry after the actual second RecordKill callback.
"""
from pathlib import Path
import struct
from unicorn.x86_const import *
from tools.native_oracle import run_checked,finish_vectors,provenance
from naval_occupants import Native,inputs,SP,RULES,INI,RULE_BLOCKS,dwords,signed,sha

HERE=Path(__file__).resolve().parent
SPANS=((0x7364A1,0x7365BB),(0x4DB810,0x4DB895),(0x5F5F40,0x5F5FA0),
       (0x744720,0x744795),(0x4DE5D0,0x4DE612),(0x5F65F0,0x5F6683),
       (0x65C7E0,0x65C88D))

class Sink(Native):
    def __init__(self,case):
        self.tick=False;self.tick_events=[];self.tick_pending=[]
        super().__init__(next(c for c in inputs() if c['name']=='west_water_head_road_dz0'))
        self.damage_output=self.run()
        assert self.damage_output['health']==1 and self.damage_output['alive']==1 and self.damage_output['sinking']==1
        self.phase='setup';self.tick_input=case
        self.wake_layers=[]
        for layer in self.layers:
            if layer.get('absent') or 'General' not in layer['sections']:continue
            self.make_ini({'General':layer['sections']['General']})
            _,a,b,_=RULE_BLOCKS['Wake']
            self.block(a,b,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI})
            pointer=self.read32(RULES+0x94)
            self.wake_layers.append(dict(file=layer['file'],raw=layer['sections']['General'].get('Wake'),name=self.string(pointer+0x24) if pointer else None))
        self.wake=self.read32(RULES+0x94)
        assert self.string(self.wake+0x24)=='WAKE1'
        u=self.u
        u.mem_write(self.actor+0xA4,dwords(208+case.get('relative_z',0)))
        u.mem_write(self.actor+0x3CD,bytes([case.get('sinking',1)]))
        u.mem_write(0xA8ED84,dwords(case.get('frame',1001)))
        for p in self.rngs.values():self.invoke(0x65C6D0,p,(case.get('seed',1),))
        self.calls=[];self.pending=[];self.accesses={};self.writes=[];self.seams=[]
        self.tick_events=[];self.tick_pending=[];self.original=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
        self.phase='measure';self.tick=True

    def hook(self,u,pc,n,d):
        if self.tick:
            sp=u.reg_read(UC_X86_REG_ESP)
            if self.tick_pending and pc==self.tick_pending[-1]['return']:
                row=self.tick_pending.pop();row.pop('return');row.update(result=signed(u.reg_read(UC_X86_REG_EAX)))
                self.tick_events.append(row)
            if pc==0x7C8E17:
                size=self.read32(sp+4);pointer=self.alloc(size)
                self.seams.append(dict(pc=hex(pc),kind='operator_new bump allocation',size=size,result=hex(pointer),caller=hex(self.read32(sp))))
                self.ret(pointer);return
            # RandomRanged inlines the xor-table step and may reject candidates;
            # counting calls to RandomNext65C780 alone would incorrectly say0.
            if pc in (0x65C837,0x65C87C):
                p=u.reg_read(UC_X86_REG_EDX)
                row=dict(kind='raw_draw_inline' if pc==0x65C837 else 'raw_draw_inline_result',pc=hex(pc),rng=next((name for name,q in self.rngs.items() if p==q),hex(p)),indices=[self.read32(p+4),self.read32(p+8)])
                if pc==0x65C87C:
                    raw=u.reg_read(UC_X86_REG_ESI);mask=u.reg_read(UC_X86_REG_EBP);maximum=u.reg_read(UC_X86_REG_EDI)
                    row.update(raw_u32=raw,mask=mask,candidate=raw&mask,rejected=signed(raw&mask)>signed(maximum))
                self.tick_events.append(row)
            names={0x4DB810:'set_coords',0x5F6940:'set_raw_coords',0x5F5F40:'height',0x65C7E0:'range',0x65C780:'raw_draw',0x421EA0:'wake_constructor_boundary',0x4DE5D0:'uninit_boundary'}
            if pc in names:
                row=dict(kind=names[pc],pc=hex(pc),this=hex(u.reg_read(UC_X86_REG_ECX)),caller=hex(self.read32(sp)))
                if pc in (0x4DB810,0x5F6940):row['coord']=list(struct.unpack('<3i',u.mem_read(self.read32(sp+4),12)))
                if pc in (0x65C7E0,0x65C780):
                    row['rng']=next((name for name,p in self.rngs.items() if p==u.reg_read(UC_X86_REG_ECX)),hex(u.reg_read(UC_X86_REG_ECX)))
                    if pc==0x65C7E0:row['bounds']=[signed(self.read32(sp+4)),signed(self.read32(sp+8))]
                if pc==0x421EA0:
                    args=[self.read32(sp+4+i*4) for i in range(7)]
                    row.update(type_name=self.string(args[0]+0x24),coord=list(struct.unpack('<3i',u.mem_read(args[1],12))),delay=args[2],loops=args[3],draw_flags=args[4],z_adjust=signed(args[5]),reverse=args[6])
                if pc in (0x5F5F40,0x65C7E0,0x65C780):self.tick_pending.append(dict(row,kind=row['kind']+'_return',return_=self.read32(sp)));self.tick_pending[-1]['return']=self.tick_pending[-1].pop('return_')
                self.tick_events.append(row)
        super().hook(u,pc,n,d)

    def run_tick(self):
        u=self.u
        before={name:bytes(u.mem_read(p,0x3F4)) for name,p in self.rngs.items()}
        u.mem_write(SP,bytes(0x40));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,self.actor)
        endpoint=run_checked(u,0x7364A1,(0x7365BB,0x421EA0,0x4DE5D0),count=300000,required_addresses=(0x7364A1,))
        assert self.original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
        assert self.code_before==[bytes(u.mem_read(a,b-a)) for a,b in self.code_spans]
        return dict(endpoint=hex(endpoint),coord=list(struct.unpack('<3i',u.mem_read(self.actor+0x9C,12))),
            health=signed(self.read32(self.actor+0x6C)),alive=u.mem_read(self.actor+0x90,1)[0],sinking=u.mem_read(self.actor+0x3CD,1)[0],is_on_map=u.mem_read(self.actor+0x74,1)[0],
            owner_units_lost=self.read32(self.house+0x5434),events=self.tick_events,receiver_calls=self.calls,writes=self.writes,
            rng={name:dict(before_hex=raw.hex(),after_hex=bytes(u.mem_read(self.rngs[name],0x3F4)).hex(),unchanged=raw==bytes(u.mem_read(self.rngs[name],0x3F4)),range_requests=sum(e['kind']=='range' and e['rng']==name for e in self.tick_events),raw_draws=sum(e['kind'] in ('raw_draw','raw_draw_inline') and e['rng']==name for e in self.tick_events)) for name,raw in before.items()},
            fpcw=hex(u.reg_read(UC_X86_REG_FPCW)),substitutions=self.seams,code_unchanged=True)

def tick_inputs():
    for frame in (1000,1001,1002,1003):yield dict(name=f'surface_frame_{frame}',frame=frame)
    for z in (-394,-395,-396,-400):
        for frame in (1000,1001):yield dict(name=f'threshold_z{z}_frame{frame}',relative_z=z,frame=frame)
    yield dict(name='sink_guard_clear',sinking=0,frame=1000)
    for seed in (0,0xffffffff):yield dict(name=f'wake_seed_{seed}',seed=seed,frame=1000)

def generate():
    rows=[];m=None
    for case in tick_inputs():
        m=Sink(case);rows.append(dict(input=case,output=m.run_tick()))
    return dict(schema_version=1,scope=__doc__,source_receiver_case='west_water_head_road_dz0',
        wake_layers=m.wake_layers,cases=rows)

def metadata():
    m=Sink(next(tick_inputs()))
    p=provenance(scope=__doc__,assumptions=[
        'Each row first executes full original Cell487A10 and Unit/Foot/Techno/Object receiver on the documented west-Water fixture; resulting HP1/Alive1/sinking1 state is retained. IsOnMap0 is constructor state; marked-world coordinate remove/put is not executed.',
        'Only initial relative Z, frame, optional sink-byte control and seed are changed before supplying the UnitAI interior frame. Flat physical cell is level2, Z208. Each row is one native suffix visit, not a captured frame of whole UnitAI or a sequential complete game simulation.',
        'Original selected Rules Wake block66D847..66D894 executes each applicable physical General cache layer and creates WAKE1. ART loading and Anim421EA0 body/playback are excluded. Wake cases stop at its real call entry and preserve original arguments.',
        'Terminal cases execute the actual Unit+E0/Techno RecordKill callback and stop at FootUnInit entry4DE5D0. Unit/Foot/Techno/Object Limbo, marked-ground unlink, Logic unregister, deferred queue and destructor are instruction-traced separate dependencies, not executed by these rows.',
        'Full0x3F4-byte Main/Scenario/MapGen states, separate range request/raw draw counts, reached store PCs and FPCW are preserved. RandomRanged65C7E0 inlines its xor-table draw at65C837..65C87C and retries masked candidates above the range width; those inline draws are counted explicitly, independently of calls to65C780. Ambient0x0E7F is supplied by the base fixture; no active-retail process FPCW proof.'],substitutions=[
        'Same setup allocator/free/TLS and measured OS Interlocked boundaries as naval_occupants. During measured wake branch only, operator_new returns zero-initialized bump storage of requested456bytes, permitting actual call argument preparation; original allocator and animation constructor are excluded.'],
        entry_points={'unit_ai_suffix':0x7364A1,'set_coords':0x4DB810,'get_height':0x5F5F40,'range':0x65C7E0,'raw_rng':0x65C780,'wake_constructor_boundary':0x421EA0,'record_kill':0x744720,'uninit_boundary':0x4DE5D0})
    p['original_slices']=[dict(start=hex(a),end_exclusive=hex(b),hex=raw.hex(),sha256=sha(raw)) for (a,b),raw in zip(SPANS,m.original)]
    return p

if __name__=='__main__':finish_vectors(generate,HERE/'naval_sink_tick.json',provenance=metadata)
