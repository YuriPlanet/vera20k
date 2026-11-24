"""Jumpjet Stop's Health-pointer call and original Object fatal arithmetic.

The concrete Infantry/Foot/Techno receiver prelude and post-core callbacks are
supplied seams. Original Stop, Object5F5390 through5F55EA, and Stop's subsequent
cache clear execute. Copied-input controls expose the stock fatal-core quotient;
this is not generic aliased packet support or full death-callback execution.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EBX,UC_X86_REG_EBP,UC_X86_REG_ESI,UC_X86_REG_EDI,UC_X86_REG_ESP,UC_X86_REG_EIP
from tools.native_oracle import finish_vectors,provenance
from tools.spatial_oracle.jumpjet_coordinates import Jumpjet,DAMAGE,TYPE_GET,RULES
from tools.spatial_oracle.walk_head_occupation import OWNER,VTABLE,TYPE,LOCO,SCRATCH
from tools.spatial_oracle.map_queries import dwords

PACKET=SCRATCH+0x10000

class StopDamage(Jumpjet):
    def __init__(self,health,copy):
        self.damage_trace=[];self.copy=copy;self.damage_sp=None;self.required=set()
        super().__init__({'actions':[]})
        self.uc.mem_map(SCRATCH+0x10000,0x10000)
        self.uc.mem_write(VTABLE+0x88,dwords(TYPE_GET))
        self.uc.mem_write(TYPE+0xa0,dwords(100))
        self.uc.mem_write(RULES+0xfa8,dwords(SCRATCH+0x12000))
        self.uc.mem_write(OWNER+0x6c,dwords(health))
        self.uc.mem_write(RULES+0x1708,struct.pack('<d',.5))
        self.uc.mem_write(LOCO+0x40,dwords(2752,2752,208))
        self.uc.mem_write(LOCO+0x4c,b'\x01')
        self.uc.mem_write(LOCO+0x50,dwords(1))
        self.fail=True
    def observe(self,u,address,size,data):
        if address in (0x54b6a0,0x5f54c2,0x5f550d,0x5f55ea,0x54b6bc):self.required.add(address)
        if address==DAMAGE:
            self.damage_sp=u.reg_read(UC_X86_REG_ESP)
            self.saved={r:u.reg_read(r) for r in (UC_X86_REG_EBX,UC_X86_REG_EBP,UC_X86_REG_ESI,UC_X86_REG_EDI)}
            args=[self.read32(self.damage_sp+4+i*4) for i in range(7)]
            self.damage_trace.append(dict(stage='entry',health=self.read32(OWNER+0x6c),damage_is_health=args[0]==OWNER+0x6c,distance=args[1],warhead_is_rules_c4=args[2]==self.read32(RULES+0xfa8),attacker=args[3],ignore_defenses=args[4],prevent_escape=args[5],source_house=args[6],destination=list(struct.unpack('<iii',u.mem_read(LOCO+0x40,12)))))
            if self.copy:
                u.mem_write(PACKET,dwords(self.read32(args[0])));u.mem_write(self.damage_sp+4,dwords(PACKET))
            u.reg_write(UC_X86_REG_EIP,0x5f5390)
        elif address==0x5f55ea:
            self.damage_trace.append(dict(stage='before_fatal_callbacks',health=self.read32(OWNER+0x6c),destination=list(struct.unpack('<iii',u.mem_read(LOCO+0x40,12)))))
            # Declared remainder/concrete receiver callback seam returns fatal4.
            for r,v in self.saved.items():u.reg_write(r,v)
            u.reg_write(UC_X86_REG_ESP,self.damage_sp);self.ret(28,4)
        else:super().observe(u,address,size,data)
    def execute_stop(self):
        health=struct.unpack('<i',self.uc.mem_read(OWNER+0x6c,4))[0]
        self.call(0x54b4d0,0,[LOCO+4])
        if health>0:assert {0x5f54c2,0x5f550d,0x5f55ea,0x54b6bc}<=self.required,self.required
        else:assert not self.damage_trace
        return dict(health=struct.unpack('<i',self.uc.mem_read(OWNER+0x6c,4))[0],state=self.snapshot(),damage_trace=self.damage_trace)

def generate():
    return [dict(input=dict(health=health,copy_control=copy),output=StopDamage(health,copy).execute_stop()) for health,copy in [(100,False),(100,True),(0,False),(-1,False),(-2147483648,False)]]

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Original Jumpjet54B4D0 failed Stop caller and Object5F5390 fatal Health arithmetic before callbacks; positive-health alias/copy controls and signed nonpositive-health early return.',
        entry_points={'stop':0x54b4d0,'object_damage_core':0x5f5390,'before_fatal_callbacks':0x5f55ea},
        assumptions=['Runtime outside ScenarioInit A8E7AC0; Infantry RTTI15; FNPC returns NullCell; constructor owner plus moving1/phase1/retained destination supplied; Health100,0,-1,MIN; ignore_defenses and prevent_escape are captured from original Stop.'],
        substitutions=['FNPC/owner getters as jumpjet_coordinates; concrete Infantry/Foot/Techno prelude replaced by Object entry; post-core fatal callbacks return4, so callback/lifecycle parity is not claimed.']))
