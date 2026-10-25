"""Bounded original Anytown concrete hut damage callers and their retry sweeps.

Physical stock cells and selected native Recalc are reused from anytown_damage.
Already-entered Map574000 is supplied; no hut/Bomb/Building lifecycle is claimed.
Animation construction, graphs and presentation stop at declared request seams.
"""
from pathlib import Path
import hashlib,struct
from unicorn.x86_const import *

from tools.native_oracle import RET_MAGIC,provenance
from .hut_publication import finish_vectors
from tools.spatial_oracle.anytown_damage.anytown_resident import Resident,rules,inputs,identity,sr
from tools.spatial_oracle.anytown_damage.validate_packet import inputs as validate_inputs
from tools.spatial_oracle.shrapnel_repair import retail_inputs as ri
from tools.rules_oracle.bridge_anim_lists import Lists,TYPE,HEAP
from tools.projectile_oracle.bridge_render_inputs import lexical

HERE=Path(__file__).resolve().parent
DISPLAY=0x46008000
FRAME=1000
ENTRIES={0x574000:'hut',0x5749C0:'selector',0x575870:'x_walker',0x575BA0:'y_walker',
         0x57CCF0:'damage',0x57D530:'damage_y',0x57ED00:'propagation',0x587180:'fallback_damage'}


def sha(raw):return hashlib.sha256(raw).hexdigest()


def animation_inputs():
    reader=Lists();rows=[]
    for name,path in [('RULESMD.INI',ri.ASSETS/'RULESMD.INI'),
                      ('LANGRULE.INI',ri.ASSETS/'LANGRULE.INI'),
                      ('MPBattleMD.ini',ri.ASSETS/'MPBattleMD.ini'),
                      ('XMP03T4.MAP',identity.ASSETS/'XMP03T4.MAP')]:
        if not path.exists():
            assert name=='LANGRULE.INI';rows.append(dict(file=name,absent=True));continue
        raw=path.read_bytes();sections,lines=lexical(raw,{'General'})
        value=sections.get('General',{}).get('BridgeExplosions')
        result=reader.read('BridgeExplosions',value)
        rows.append(dict(file=name,sha256=sha(raw),read=result,
                         source_lines=[line for line in lines if line['key']=='BridgeExplosions']))
    return reader,dict(constructor=reader.initial['BridgeExplosions'],layers=rows,
                       result=reader.state('BridgeExplosions'))


class Hut(Resident):
    def __init__(self,case,r,t,animations):
        self.capture=False;self.visit=0;self.hut_returns=[];self.pc=0
        super().__init__(case,r,t)
        self.phase='setup';u=self.uc
        u.mem_map(HEAP,0x400000);u.mem_write(HEAP,bytes(animations.u.mem_read(HEAP,0x400000)))
        rule=sr.u32(u,0x8871E0)
        u.mem_write(rule+0x158,bytes(animations.u.mem_read(TYPE+0x158,24)))
        data=sr.u32(u,rule+0x15C);count=sr.u32(u,rule+0x168)
        self.animation_types={sr.u32(u,data+i*4):animations.string(sr.u32(u,data+i*4)+0x24)
                              for i in range(count)}
        for address in (0x561710,0x5617A0,0x5617C0,0x5617E0):self.call(address,count=200000)
        self.map_startup=dict(entries=[hex(v) for v in (0x561710,0x5617A0,0x5617C0,0x5617E0)],
                              level_height=sr.i32(u,0xABDE88),fpcw=u.reg_read(UC_X86_REG_FPCW))
        assert self.map_startup['level_height']==104
        u.mem_write(0x887324,sr.dwords(DISPLAY));u.mem_write(DISPLAY,bytes(0x1000))
        u.mem_write(0xA8ED84,sr.dwords(FRAME))
        self.phase='measure'

    def event(self,kind,**values):
        if self.capture:values.update(visit=self.visit,pc=hex(self.pc))
        super().event(kind,**values)

    def push_return(self,kind,return_pc,**values):
        row=dict(kind=kind,visit=self.visit,pc=hex(self.pc),caller=hex(return_pc),**values)
        self.trace.append(row);self.hut_returns.append((return_pc,row))

    def observe(self,u,address,size,data):
        if self.capture:
            self.visit+=1;self.pc=address;sp=u.reg_read(UC_X86_REG_ESP);this=u.reg_read(UC_X86_REG_ECX)
            if self.hut_returns and self.hut_returns[-1][0]==address:
                _,row=self.hut_returns.pop();row.update(return_visit=self.visit,
                    result_eax=u.reg_read(UC_X86_REG_EAX),result_al=u.reg_read(UC_X86_REG_EAX)&255)
                if row['kind']=='damage':row['after']=self.snapshot(self.ptrs[tuple(row['coord'])])
            if address in ENTRIES:
                coord=list(struct.unpack('<hh',u.mem_read(sr.u32(u,sp+4),4)))
                values=dict(coord=coord)
                if address==0x57CCF0:values['before']=self.snapshot(self.ptrs[tuple(coord)])
                self.push_return(ENTRIES[address],sr.u32(u,sp),**values)
            if address==0x5657A0:
                p=sr.u32(u,sp+4);self.event('lookup',coord=list(struct.unpack('<hh',u.mem_read(p,4))),caller=hex(sr.u32(u,sp)))
            if address==0x575C19:
                self.event('y_extent_counts',negative_including_failed_probe=u.reg_read(UC_X86_REG_EDI),
                           positive_including_failed_probe=u.reg_read(UC_X86_REG_ESI))
            if address==0x575C50:
                self.event('y_biased_start',coord=list(struct.unpack('<hh',u.mem_read(sp+0x40,4))),
                           direction=sr.i32(u,sp+0x28),iterations=sr.i32(u,sp+0x10))
            if address==0x575C86:
                self.event('y_sweep_step',cell=self.snapshot(u.reg_read(UC_X86_REG_EAX)),
                           remaining=sr.i32(u,sp+0x10))
            if address==0x57409D:self.event('fallback_entry')
            if address in (0x65C7E0,0x65C780):
                stream=next((k for k,p in self.rngs.items() if p==this),None);assert stream is not None
                args=[sr.i32(u,sp+4),sr.i32(u,sp+8)] if address==0x65C7E0 else []
                self.push_return('rng',sr.u32(u,sp),stream=stream,args=args)
            if address in (0x65C84B,0x65C79D):
                self.event('raw_rng',value=u.reg_read(UC_X86_REG_ESI))
            if address==0x421EA0:
                args=[sr.u32(u,sp+4+i*4) for i in range(7)]
                self.event('animation_request',caller=hex(sr.u32(u,sp)),type=self.animation_types[args[0]],
                           position=list(struct.unpack('<3i',u.mem_read(args[1],12))),delay=args[2],
                           loops=args[3],flags=args[4],z_adjust=args[5],reverse=args[6])
                self.ret(28,this);return
        mark=len(self.trace)
        super().observe(u,address,size,data)
        if self.capture:
            for row in self.trace[mark:]:
                row.setdefault('visit',self.visit);row.setdefault('pc',hex(address))

    def write(self,u,access,address,size,value,data):
        if self.capture and address==DISPLAY+0xD7C:
            self.event('display_dirty_write',value=value,size=size)
        super().write(u,access,address,size,value,data)

    def measured_call(self,fn,coord):
        self.capture=True;self.visit=0;self.trace.clear();self.writes.clear();self.hut_returns.clear()
        before={c:self.snapshot(p) for c,p in self.ptrs.items()}
        initial_rng={k:sr.rng_state(self.uc,p) for k,p in self.rngs.items()}
        frame0=sr.i32(self.uc,0xA8ED84);dirty0=self.uc.mem_read(DISPLAY+0xD7C,1)[0]
        self.uc.mem_write(sr.COORD,sr.packed(*coord));result=self.call(fn,args=(sr.COORD,),count=10000000)
        # The sentinel is the emulation endpoint, not an executed instruction.
        assert len(self.hut_returns)==1 and self.hut_returns[0][0]==RET_MAGIC,self.hut_returns
        _,root=self.hut_returns.pop();root.update(result_eax=result,result_al=result&255,
                                                return_boundary=hex(RET_MAGIC))
        assert not self.pending,self.pending
        self.capture=False
        after={c:self.snapshot(p) for c,p in self.ptrs.items()}
        final_rng={k:sr.rng_state(self.uc,p) for k,p in self.rngs.items()}
        assert sha(bytes(self.uc.mem_read(0x401000,0x3E0000)))==self.code_hash
        return dict(entry=hex(fn),coord=coord,result_eax=result,result_al=result&255,
                    frame_before=frame0,frame_after=sr.i32(self.uc,0xA8ED84),
                    display_dirty_before=dirty0,display_dirty_after=self.uc.mem_read(DISPLAY+0xD7C,1)[0],
                    instruction_visits=self.visit,trace=list(self.trace),writes=list(self.writes),
                    before=[before[c] for c in sorted(before)],after=[after[c] for c in sorted(after)],
                    changed=[dict(before=before[c],after=after[c]) for c in sorted(before) if before[c]!=after[c]],
                    rng_before=initial_rng,rng_after=final_rng,rng_unchanged={k:initial_rng[k]==final_rng[k] for k in initial_rng})


def run_case(hut,stage,t,r,anim):
    case=inputs(t);m=Hut(case,r,t,anim);setup=[]
    for _ in range({'healthy':0,'first_damaged':1,'collapsed':2}[stage]):
        setup.append(m.measured_call(0x57CCF0,case['impact']))
    result=m.measured_call(0x574000,hut)
    return dict(hut=hut,starting_stage=stage,input=case,bootstrap=m.bootstrap,
                cell_startup=m.cell_startup,map_startup=m.map_startup,assets=m.assets,
                initial_recalc=m.initial_recalc,preparatory_original_damage=setup,result=result,
                text_sha256=m.code_hash)


def generate():
    retail=validate_inputs();t=identity.theater();r,fields=rules();anim,anim_fields=animation_inputs()
    rows=[run_case(hut,stage,t,r,anim) for hut in ([85,58],[89,51])
          for stage in ('healthy','first_damaged','collapsed')]
    return dict(schema=1,retail_inputs=retail,theater=t,
                rules=fields,animation_inputs=anim_fields,cases=rows,
                caller_coverage='Actual574000/5749C0/575BA0 on both physical huts. 575870 is the original X-axis alternative; instruction-established only for this Y-axis stock span. Native fallback587180 route is instruction-established only; stock cases retain a primary overlay seed.',
                boundary='No native physical map/Rules/type load. Current sparse map cells, empty occupants, initial native-seeded0 RNG streams and frame1000 are supplied. Selected original Recalc, lookup, selector, extent, sweep, retries, original primitives, RNG and FTOL execute. Anim421EA0 returns supplied constructor storage at the exact request boundary; graph/presentation callbacks remain inherited seams. No active hut lifecycle, animations, audio, rendering or Rust parity claim.')


def metadata():
    return provenance(scope=__doc__,assumptions=[
        'Unmodified stock Anytown map and native-read concrete overlay/TMP bindings from the frozen anytown_damage owner. Both authored huts are supplied directly to original Map574000. First-damaged/collapsed prerequisites are produced by one/two actual57CCF0 calls at87,54, not manufactured overlay writes.',
        'The complete original lookup, primary selection, physical Y-axis bounded sweep, retries and57CCF0 execute. Native future cell reads observe each prior mutation immediately. Both function return EAX and low byte are recorded mechanically; hut root/walker have no declared Boolean return contract.',
        'Actual Lists reader executes BridgeExplosions constructor/read blocks over physical layer strings. Original AnimType constructors and vector storage are copied unchanged into the resident VM. No ART or Anim instance construction is inferred from retained names.',
        'Original CRT-then-WinMain precision/rounding setup comes from shared Repair bootstrap. Actual Map startup561710/5617A0/5617C0/5617E0 derivesABDE88=104; signed Cell level forms animation Z. No host jitter/rounding substitutes native arithmetic.',
        'Native seed0 states bracket preparations and the hut call. Main/Scenario/MapGen, ordered request/result/raw draw events, frame1000 and Display dirty writes are retained. No whole-frame scheduling or native scenario-loaded stream is claimed.',
        'Original binary/text is verified; no executable instruction is patched. Other-axis575870 and fallback574000→587180→57CCF0 are instruction-only leads, not stock-route dynamic coverage.'
    ],substitutions=[
        'All inherited sparse map, INI cache, supplied TMP head/zone plane, bounded allocator, zero display/radar/graph callback seams remain explicit in the packaged Resident owner.',
        'Anim421EA0 records all seven native arguments and returns its already-allocated storage. It executes no constructor, ART, lifetime, random-rate/audio or spawned-effect callbacks; those could interleave further RNG in a broader chain.',
        'The hut/Building/Bomb lifecycle prefix is excluded. Active caller instruction slices identify Bomb438982 and Building44031B, but neither full owner update nor triggering a live bomb is executed. No occupant, Engineer, whole-graph or production parity claim.'
    ],entry_points={'hut':0x574000,'selector':0x5749C0,'x_walker_instruction_only':0x575870,
                    'y_walker':0x575BA0,'damage':0x57CCF0,'map_lookup':0x5657A0,
                    'scenario_range':0x65C7E0,'animation_request_boundary':0x421EA0,
                    'fallback_dispatch_instruction_only':0x587180,'bridge_explosion_reader':0x66DB93,
                    'map_height_initializer':0x5617E0})


if __name__=='__main__':
    finish_vectors(generate,HERE/'hut.json.gz',provenance=metadata)
