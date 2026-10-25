"""Narrow executed RadarCombatFlash accesses during the frozen lethal packets."""
from pathlib import Path
from unicorn.x86_const import UC_X86_REG_EIP,UC_X86_REG_ECX
from tools.native_oracle import finish_vectors,provenance,_canonical
from tools.spatial_oracle.shrapnel_damage import occupants as shared
from tools.spatial_oracle.shrapnel_damage.occupants import WoodenOccupied,Rules,theater,input_case,ResidentRepair,SCENE
from tools.spatial_oracle.shrapnel_repair import packet_io

HERE=Path(__file__).resolve().parent
FROZEN_CASES_SHA256='f3af7eba4981a8f9430a48d4eaf6727f8c996cde77e5025ce0c34addc001298d'

class Observed(WoodenOccupied):
    def __init__(self,*args,**kwargs):
        self.timer_access=[]
        super().__init__(*args,**kwargs)
    def record(self,kind,u,address,size,value=None):
        if self.phase!='measure':return
        for actor in (self.actor,getattr(self,'second',0)):
            if actor and actor+0x174<=address<actor+0x180:
                self.timer_access.append(dict(stage=self.active_stage,visit=self.visit,
                    kind=kind,actor=self.who(actor),offset=hex(address-actor),size=size,
                    value=value,pc=hex(u.reg_read(UC_X86_REG_EIP)),
                    alive=u.mem_read(actor+0x90,1)[0],limbo=u.mem_read(actor+0x81,1)[0]))
    def read_hook(self,u,access,address,size,value,data):
        self.record('read',u,address,size)
        super().read_hook(u,access,address,size,value,data)
    def write_hook(self,u,access,address,size,value,data):
        self.record('write',u,address,size,value)
        super().write_hook(u,access,address,size,value,data)
    def hook(self,u,pc,n,data):
        if self.phase=='measure' and pc==0x70D990:
            self.timer_access.append(dict(stage=self.active_stage,visit=self.visit,
                kind='radar_update_entry',actor=self.who(u.reg_read(UC_X86_REG_ECX))))
        super().hook(u,pc,n,data)

def generate():
    r=Rules();t=theater();d=ResidentRepair(input_case([117,56],t),r,t);d.run()
    rows=[]
    for case in (dict(name='resident_center',current_cell=[115,59]),
        dict(name='north_head_center',current_cell=[115,58],head_cell=[115,59]),
        dict(name='resident_two_members',current_cell=[115,59],second_member=True),
        dict(name='north_two_heads',current_cell=[115,58],head_cell=[115,59],second_member=True)):
        print(case['name'],flush=True)
        m=Observed(d,t,case,scene=SCENE);result=m.run()
        rows.append(dict(input=case,timer_access=m.timer_access,
            unit_vtable=hex(m.read32(m.actor)),radar_slot_4a0=hex(m.read32(m.read32(m.actor)+0x4A0)),
            flash_slot_49c=hex(m.read32(m.read32(m.actor)+0x49C)),
            final=result[-1]['after'],events=[e for e in result[-1]['events'] if e['kind'] in
                ('mark','remove_content','display_remove','logic_remove','foot_uninit','object_uninit','unit_limbo','foot_limbo','techno_limbo','object_limbo')]))
        assert not any(e['kind'] in ('read','radar_update_entry') for e in m.timer_access)
    assert packet_io.digest(_canonical(rows))==FROZEN_CASES_SHA256, 'original timer case values changed'
    # Original-byte proof stays reproducible without relying on a Ghidra label.
    # Every interval starts/ends at an instruction boundary; these are excerpts,
    # not newly supplied native results or a second gameplay implementation.
    spans=(('ReceiveDamage timer writes',0x701FA6,0x701FCB),
        ('current-house timer reader and flash call',0x70DBCC,0x70DC45),
        ('radar flash request',0x70CCF0,0x70CD02),
        ('Limbo radar-present gate',0x6F6C35,0x6F6C4A),
        ('remove radar and clear presence',0x70CCC0,0x70CCE5),
        ('Techno AI Alive gate',0x6FA735,0x6FA743),
        ('Techno AI radar update',0x6FAF01,0x6FAF0D),
        ('Unit tube-only update',0x7363A4,0x7363C9),
        ('other TriggerAction global Unit traversal',0x6E225C,0x6E2284))
    source=[dict(role=role,start=hex(a),end_exclusive=hex(b),
        bytes_hex=bytes(m.u.mem_read(a,b-a)).hex()) for role,a,b in spans]
    return dict(schema=1,cases=rows,original_source=source,
        frozen_cases_sha256=FROZEN_CASES_SHA256,
        radix='all source addresses and bytes are hexadecimal',
        slot_498_remove_radar=hex(m.read32(m.read32(m.actor)+0x498)))

def metadata():
    return provenance(scope=__doc__,assumptions=[
        'Only the four frozen MTNK lethal cases are rerun; all existing occupants boundary substitutions remain. This observes exact actor+174..17F CPU reads/writes and original70D990 entry through controller return.',
        'No read or radar update is observed during the synchronous selected lethal receiver/cleanup. Original removal ends with objectalive0, limbo1, no cell/Logic/Display membership and deferred deletion. This is not a global claim that the timer has no surviving-object consumer.',
        'Original source excerpts expose the radar reader, ordinary AI/tube callers, conditional Limbo removal and a separate TriggerAction global-Unit caller. Conditional armed radar removal and trigger scheduling are instruction evidence only; fixture placement did not execute native radar registration.',
    ],substitutions=['Inherited occupants.md sinks only; no timer read, write or radar-update callback is replaced.'],entry_points={'area_low_admitted':0x48A25A,'radar_update':0x70D990})|dict(
        upstream=shared.metadata(),
        inherited_corpus_sha256=packet_io.digest(shared.OUTPUT.read_bytes()))

if __name__=='__main__':
    finish_vectors(generate,HERE/'occupants_timer.json',provenance=metadata)
