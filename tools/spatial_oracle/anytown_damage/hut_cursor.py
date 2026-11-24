"""Original587410 repair cursor over both physical Anytown huts and four states.

Reuses the physical Resident owner. Damage/repair prerequisites execute original
bodies; cursor selection and walks execute without replaced dependencies.
Structural-record and other-axis branches remain outside this stock witness.
"""
from pathlib import Path
import hashlib,struct
from unicorn.x86_const import *

from tools.native_oracle import provenance
from tools.spatial_oracle.shrapnel_repair.packet_io import finish_vectors
from .publication import publication_projection
from tools.spatial_oracle.anytown_damage.anytown_resident import Resident,rules,inputs,identity,sr
from tools.spatial_oracle.anytown_damage.validate_packet import inputs as validate_inputs

HERE=Path(__file__).resolve().parent
FRAME=1000


def sha(raw):return hashlib.sha256(raw).hexdigest()


class Cursor(Resident):
    def __init__(self,case,r,t):
        self.capture=False;self.visit=0;self.pc=0;self.lookups=[]
        super().__init__(case,r,t)
        self.uc.mem_write(0xA8ED84,sr.dwords(FRAME))

    def event(self,kind,**values):
        if self.capture:values.update(visit=self.visit,pc=hex(self.pc))
        super().event(kind,**values)

    def observe(self,u,address,size,data):
        if self.capture:
            self.visit+=1;self.pc=address;sp=u.reg_read(UC_X86_REG_ESP)
            if self.lookups and self.lookups[-1][0]==address:
                _,row=self.lookups.pop();row.update(return_visit=self.visit,
                    result_cell=self.snapshot(u.reg_read(UC_X86_REG_EAX)))
            if address==0x5657A0:
                row=dict(kind='lookup',coord=list(struct.unpack('<hh',u.mem_read(sr.u32(u,sp+4),4))),
                         caller=hex(sr.u32(u,sp)),visit=self.visit,pc=hex(address))
                self.trace.append(row);self.lookups.append((sr.u32(u,sp),row))
            if address==0x587483:
                self.candidate=self.snapshot(u.reg_read(UC_X86_REG_EAX))
                self.event('scan_candidate',cell=self.candidate)
            if address==0x5876A6:
                selected=u.reg_read(UC_X86_REG_EBX)
                self.event('scan_result',candidate=self.candidate['coord'],
                    selected=self.snapshot(selected) if selected else None,
                    tile_family=u.mem_read(sp+0x12,1)[0],concrete=u.mem_read(sp+0x13,1)[0])
            if address==0x5876BA:
                selected=u.reg_read(UC_X86_REG_EBX)
                self.event('last_selection',cell=self.snapshot(selected) if selected else None,
                    tile_family=u.mem_read(sp+0x12,1)[0],concrete=u.mem_read(sp+0x13,1)[0])
            if address==0x5876C6:
                raise AssertionError('Uncovered structural-record cursor branch reached')
            if address in (0x587839,0x58793D,0x587A61,0x587B55):
                self.event('overlay_walk',axis='x' if address in (0x587839,0x587A61) else 'y',
                    first_direction=-1 if address in (0x587839,0x587A61) else 1,
                    cell=self.snapshot(u.reg_read(UC_X86_REG_EBX)))
            if address in (0x5877EF,0x587C41):
                self.event('cursor_return_path',can_repair=address==0x5877EF)
        mark=len(self.trace)
        super().observe(u,address,size,data)
        if self.capture:
            for row in self.trace[mark:]:
                row.setdefault('visit',self.visit);row.setdefault('pc',hex(address))

    def measured_call(self,entry,coord):
        self.capture=True;self.visit=0;self.trace.clear();self.writes.clear();self.lookups.clear()
        before=[self.snapshot(self.ptrs[c]) for c in sorted(self.ptrs)]
        cells_before=sha(bytes(self.uc.mem_read(sr.CELLS,len(self.ptrs)*0x200)))
        rng0={name:sr.rng_state(self.uc,p) for name,p in self.rngs.items()}
        frame0=sr.i32(self.uc,0xA8ED84)
        self.uc.mem_write(sr.COORD,sr.packed(*coord))
        result=self.call(entry,args=(sr.COORD,),count=4000000)
        assert not self.pending and not self.lookups,(self.pending,self.lookups)
        self.capture=False
        after=[self.snapshot(self.ptrs[c]) for c in sorted(self.ptrs)]
        rng1={name:sr.rng_state(self.uc,p) for name,p in self.rngs.items()}
        assert sha(bytes(self.uc.mem_read(0x401000,0x3E0000)))==self.code_hash
        return dict(entry=hex(entry),coord=coord,result_eax=result,result_al=result&255,
            instruction_visits=self.visit,trace=list(self.trace),writes=list(self.writes),
            before=before,after=after,changed=[dict(before=a,after=b) for a,b in zip(before,after) if a!=b],
            cells_memory_sha256_before=cells_before,
            cells_memory_sha256_after=sha(bytes(self.uc.mem_read(sr.CELLS,len(self.ptrs)*0x200))),
            rng_before=rng0,rng_after=rng1,rng_unchanged={k:rng0[k]==rng1[k] for k in rng0},
            frame_before=frame0,frame_after=sr.i32(self.uc,0xA8ED84))


def run_case(hut,stage,t,r):
    case=inputs(t);m=Cursor(case,r,t);preparations=[]
    for _ in range({'healthy':0,'first_damage':1,'collapsed':2,'repaired':2}[stage]):
        preparations.append(m.measured_call(0x57CCF0,case['impact']))
    if stage=='repaired':preparations.append(m.measured_call(0x573540,hut))
    result=m.measured_call(0x587410,hut)
    return dict(hut=hut,starting_stage=stage,input=case,bootstrap=m.bootstrap,
        cell_startup=m.cell_startup,assets=m.assets,initial_recalc=m.initial_recalc,
        zone_storage=m.zone_storage,preparatory_native_calls=preparations,result=result,
        text_sha256=m.code_hash)


def generate():
    retail=validate_inputs();t=identity.theater();r,fields=rules()
    return dict(schema=1,retail_inputs=retail,theater=t,rules=fields,
        cases=[run_case(hut,stage,t,r) for hut in ([85,58],[89,51])
               for stage in ('healthy','first_damage','collapsed','repaired')],
        boundary='Original full587410 on physical concrete overlay cells; direct supplied hut query. Original57CCF0 and573540 create prerequisites using inherited Resident seams. No native scenario load, complete cursor UI/click/Engineer route, structural-record branch, other axis or Rust/production comparison.')


def metadata():
    return provenance(scope=__doc__,assumptions=[
        'Physical XMP03T4.MAP cells, exact theater tile bases and original overlay/land readers are reused from anytown_damage.Resident. Both actual hut coordinates are supplied directly to original587410.',
        'Healthy, first_damage, collapsed and repaired states are made by zero/one/two original57CCF0 calls at87,54, plus original573540 from the selected hut for repaired. No overlay edits manufacture cursor results.',
        'Original587410 executes its entire selected path, including native Y-major25-cell selection, tile-family precedence, live last matching cell, concrete overlay orientation and both directed walks as reached. No cursor predicate or lookup answer is substituted.',
        'All180 cell snapshots, complete cell-memory hashes, scan/lookup/walk trace, return EAX/AL, frame1000 and complete Main/Scenario/MapGen states are recorded around the query. AL is the cursor Boolean; upper EAX bits are retained mechanically.',
        'Complete mapped.text is unchanged after each measured native call.'
    ],substitutions=[
        'The inherited sparse physical map, pristine TMP heads, supplied initial zone planes, empty objects, zero presentation/graph callbacks and successful bounded allocator remain as documented by the Resident owner. Actual selected Recalc executes.',
        'Map query starts at587410; no Input/Mouse/Engineer or full Building lifecycle executes. Structural record branch fails closed if reached and is not represented by synthetic fixtures. The stock overlay family takes the Y-axis path; the other-axis code is instruction-only.',
        'Native seed0 streams and frame1000 are supplied, not a full scenario-loaded RNG position. Preparatory repair consumes native MapGen; every cursor query records its own unchanged or changed streams independently.'
    ],entry_points={'cursor':0x587410,'lookup':0x5657A0,'damage':0x57CCF0,'repair':0x573540,
                    'recalc':0x47D2B0,'structural_branch_excluded':0x5876C6})


if __name__=='__main__':finish_vectors(generate,HERE/'hut_cursor.json.gz',provenance=metadata,
    promotion_path=HERE/'hut_cursor_promotion.json',projection=publication_projection)
