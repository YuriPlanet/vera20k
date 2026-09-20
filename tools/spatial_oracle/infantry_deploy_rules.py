"""Original AIAutoDeployFrameDelay vector copy/read/assignment/destruction.

Supplied cached INI entry and allocator backing; all original vector and CRT
operations execute. Missing/blank reads exercise retained prior vectors.
"""
import hashlib
import struct
from pathlib import Path
from unicorn.x86_const import *
from tools.native_oracle import run_checked, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.building_body_rules import Fixture, INI, RULES, SP, TYPE
from tools.native_oracle import SCRATCH

HEAP=0x20100000
ALLOC,FREE,LOCK,CURSOR=[SCRATCH+x for x in (0xD000,0xD100,0xD200,0xD300)]
GET_ERROR,TLS_GET,SET_ERROR,PTD=[SCRATCH+x for x in (0xD500,0xD600,0xD700,0xD800)]
SPANS=[(0x670232,0x67026C),(0x475D70,0x475F29),
       (0x4777B0,0x477840),(0x67A090,0x67A110),(0x679F70,0x679FB0)]


class VectorFixture(Fixture):
    def __init__(self):
        super().__init__()
        u=self.u
        u.mem_map(HEAP,0x100000)
        for a,v in ((0x87C584,0),(0xB78B94,0),(0xB78B98,0),
                    (0xB78B9C,0x12340000),(0x87C2A8+9*4,SCRATCH+0xD400)):
            u.mem_write(a,dwords(v))
        for slot,fn in ((0x7E1278,ALLOC),(0x7E127C,FREE),(0x7E11E8,LOCK),(0x7E11EC,LOCK)):
            u.mem_write(slot,dwords(fn))
        u.mem_write(ALLOC,b'\xA1'+dwords(CURSOR)+b'\x81\x05'+dwords(CURSOR,0x1000)+b'\xC2\x0C\x00')
        u.mem_write(FREE,b'\xB8\x01\x00\x00\x00\xC2\x0C\x00')
        u.mem_write(LOCK,b'\xC2\x04\x00')
        for slot,fn in ((0x7E1200,GET_ERROR),(0x7E12B8,TLS_GET),(0x7E12B4,SET_ERROR)):
            u.mem_write(slot,dwords(fn))
        u.mem_write(GET_ERROR,b'\x31\xC0\xC3')
        u.mem_write(TLS_GET,b'\xB8'+dwords(PTD)+b'\xC2\x04\x00')
        u.mem_write(SET_ERROR,b'\xC2\x04\x00')
        self.original=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]

    def execute(self,raw,prior):
        self.ini(0x83C300,raw)
        self.write(INI+4,0x826278)
        u=self.u
        u.mem_write(PTD,bytes(0x74))
        self.write(CURSOR,HEAP+0x1000)
        u.mem_write(HEAP,dwords(*prior))
        u.mem_write(RULES+0xE2C,dwords(0x7E4DD8,HEAP,len(prior),0x101,len(prior),10,0))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ESI,RULES)
        u.reg_write(UC_X86_REG_EDI,INI)
        run_checked(u,0x670232,0x67026C,count=2000000)
        assert u.reg_read(UC_X86_REG_ESP)==SP
        p,count=struct.unpack('<I',u.mem_read(RULES+0xE30,4))[0],struct.unpack('<i',u.mem_read(RULES+0xE3C,4))[0]
        assert 0<=count<512
        output=list(struct.unpack('<'+'i'*count,u.mem_read(p,count*4))) if count else []
        assert self.original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
        return dict(raw=raw,prior=prior,output=output)


def generate():
    f=VectorFixture()
    return [f.execute(raw,prior) for prior in ([],[15,25,100],[7,-9,65536,4])
            for raw in (None,'',' ','15,25,100','-1,0,65536','2147483648,4294967295,4294967296',
                        '1,,3,',',,','1, ,3','1,\v-2,3','x,1junk,+,0x10',
                        '9'*40,'1,'*300,' '*510+'7,8')]


def metadata():
    f=VectorFixture()
    out=provenance(scope='RulesGeneral AIAutoDeployFrameDelay exact vector reader/copy/assignment caller670232..67026C',
        assumptions=['Supplied retained prior DynamicVector and cached General INI entry/index',
                     'Original512-byte ReadString, strtok, atoi and vector copy/resize/assignment/destruction execute',
                     'Constructor vector count0 established separately; this witness executes reader with explicit prior state'],
        substitutions=['HeapAlloc bump arena, HeapFree success, critical-section no-op, GetLastError/SetLastError and TlsGetValue supplied per-thread CRT data imports only; original code/vtables unchanged'],
        entry_points={'read_general_vector':0x670232})
    out['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=code.hex(),
        sha256=hashlib.sha256(code).hexdigest()) for (a,b),code in zip(SPANS,f.original)]
    return out


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
