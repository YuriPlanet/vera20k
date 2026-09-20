"""Original InfantryType sequence constructor loop and complete ReadSequenceData.

The fixture supplies cached INI indexes at ReadString entry; original ReadString,
sscanf, case-sensitive facing comparisons and 42-action traversal execute intact.
No sequence Sounds values are supplied: sound registry population is out of scope.
"""
import hashlib
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import (load_image, run_checked, call, SCRATCH,
    STACK_BASE, STACK_SIZE, RET_MAGIC, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

TYPE, RECORDS, SECTION, INDEX, ENTRY, RAW = [SCRATCH+n*0x2000 for n in range(6)]
SP = STACK_BASE+STACK_SIZE-0x1000
INI = 0x887180
SPANS = [(0x52392C,0x523970),(0x523D00,0x524097)]


class Fixture:
    def __init__(self):
        self.u=Uc(UC_ARCH_X86,UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH,0x10000)
        self.u.mem_map(STACK_BASE,STACK_SIZE)
        self.u.mem_map(RET_MAGIC,0x1000)
        self.u.reg_write(UC_X86_REG_FPCW,0x0E7F)
        self.original=[bytes(self.u.mem_read(a,b-a)) for a,b in SPANS]
        self.crcs={}
        self.values={}
        self.reads=[]
        self.names=[self.string(self.word(0x8255C8+4*n)) for n in range(42)]
        self.u.hook_add(UC_HOOK_CODE,self.read_boundary,begin=0x528A10,end=0x528A10)

    def word(self,addr):
        return struct.unpack('<I',self.u.mem_read(addr,4))[0]

    def string(self,addr):
        return bytes(self.u.mem_read(addr,256)).split(b'\0')[0].decode('ascii')

    def read_boundary(self,_u,_a,_size,_data):
        u=self.u
        sp=u.reg_read(UC_X86_REG_ESP)
        section_ptr=self.word(sp+4)
        section=self.string(section_ptr)
        key=self.string(self.word(sp+8))
        raw=self.values.get((section,key))
        self.reads.append([section,key,raw])
        if key not in self.crcs:
            name=key.encode('ascii')
            self.crcs[key]=call(0x4A1DE0,ecx=SCRATCH,
                stack_args=[SCRATCH+0x100,len(name)],
                writes={SCRATCH:bytes(16),SCRATCH+0x100:name})['eax']
        # Only the section cache/index backing data are supplied. ReadString
        # performs its original CRC, entry search, copy bound and trim.
        u.mem_write(INI,bytes(0x40))
        u.mem_write(INI+4,dwords(section_ptr,SECTION))
        u.mem_write(SECTION+0x2C,dwords(INDEX,int(raw is not None)))
        u.mem_write(SECTION+0x38,dwords(1,INDEX))
        u.mem_write(INDEX,dwords(self.crcs[key],ENTRY))
        u.mem_write(ENTRY+0x10,dwords(RAW))
        u.mem_write(RAW,(raw or '').encode('ascii')+b'\0')

    def constructor(self):
        u=self.u
        u.mem_write(RECORDS,b'\xCD'*0x5E8)
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ESI,TYPE)
        u.reg_write(UC_X86_REG_EAX,RECORDS)
        u.reg_write(UC_X86_REG_EBX,0)
        u.reg_write(UC_X86_REG_EDI,0xFFFFFFFF)
        run_checked(u,0x52392C,0x523970)

    def records(self):
        return [list(struct.unpack('<5i',self.u.mem_read(RECORDS+n*36,20)))
                for n in range(42)]

    def execute(self,layers,initial=None,sequence='ArbitraryActions'):
        u=self.u
        self.constructor()
        if initial is not None:
            for n in range(42):
                u.mem_write(RECORDS+n*36,dwords(*initial,0))
        u.mem_write(TYPE+0x1F8,b'TEST\0')
        before=self.records()
        self.reads=[]
        for layer in layers:
            self.values={('TEST','Sequence'):sequence}
            self.values.update({(sequence,k):v for k,v in layer.items()})
            u.mem_write(SP,dwords(RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP,SP)
            u.reg_write(UC_X86_REG_ECX,TYPE)
            run_checked(u,0x523D00,RET_MAGIC,count=200000)
        assert self.original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
        return dict(layers=layers,initial=initial,sequence=sequence,
                    before=before,after=self.records(),reads=self.reads)


def generate():
    f=Fixture()
    rows=[]
    values=[None,'','1,2,3','-4,-5,-6','2147483648,4294967295,4294967296',
        '99999999999999999999999999999999,2,3','1','1,','1,,3','1,2x,3',
        '1 ,2,3','1, 2, 3','+,-1,2','x,2,3','1,0x10,3','1,\t-2,3',
        '1,\x1f2,3','1,2,3junk','1,2,3,4','"1,2,3"',' 1,2,3 ',
        ' '*30+'1,2,3','\v1,2,3','1,\f2,3','1,2,3,N','1,2,3,n',
        '1,2,3,NE','1,2,3, E','1,2,3,SE tail','1,2,3,S','1,2,3,SW',
        '1,2,3,W','1,2,3,NW','1,2,3,BAD','1,2,3,S; tail']
    for initial in (None,[7,-9,11,5]):
        for raw in values:
            rows.append(f.execute([{'Deploy':raw}],initial))
    rows += [f.execute([{'Ready':'1,2,3','Guard':'4,5,6'}]),
             f.execute([{'Deploy':'1,2,3,S'},{'Deploy':'9'}]),
             f.execute([{'Deploy':'1,2,3,S'},{'Deploy':'9,8,7,n'}]),
             f.execute([{'Deploy':'1,2,3,S'},{}]),
             f.execute([{'Deploy':'1,2,3'}],sequence=None),
             f.execute([{name:f'{n},{n+1},{n+2}' for n,name in enumerate(f.names)}])]
    return dict(schema_version=1,names=f.names,cases=rows)


def metadata():
    f=Fixture()
    out=provenance(scope='InfantryType sequence constructor and complete42-action ReadSequenceData, numeric/facing fields',
        assumptions=['Supplied cached section/index backing data; original ReadString and sscanf retained',
                     'No sequence Sounds entries supplied; sound registry and successful sound reads excluded',
                     'Initial retained records are explicitly supplied for reload cases; constructor runs first',
                     'Constructor allocation supplied; original initialization loop runs on all42 records'],
        substitutions=[],entry_points={'constructor_loop':0x52392C,'read_sequence':0x523D00})
    out['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),
        sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return out


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
