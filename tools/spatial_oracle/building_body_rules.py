"""Original BuildingType body range readers and asset-bound construction range.

Supplied INI lookup indexes stand in for file loading, not the native readers.
Original sprintf/ReadString/sscanf and integer/bool readers execute unchanged.
The construction slice starts after the asset lookup and receives a supplied SHP
pointer/header; it does not claim filename resolution, theater selection or IO.
"""
import hashlib
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import *
from tools.native_oracle import (load_image, run_checked, call, SCRATCH,
    STACK_BASE, STACK_SIZE, RET_MAGIC, finish_vectors, provenance)

TYPE = SCRATCH
SECTION = SCRATCH + 0x3000
INDEX = SCRATCH + 0x3100
ENTRY = SCRATCH + 0x3200
RAW = SCRATCH + 0x4000
SHP = SCRATCH + 0x5000
RULES = SCRATCH + 0x6000
SP = STACK_BASE + 0x80000
INI = 0x887180
TRIPLES = [(0x4615CA, 0x46164A, 0x81A600, 0xF10),
           (0x46164A, 0x4616C4, 0x81A5F4, 0xF1C),
           (0x4616C4, 0x46173E, 0x81A5E8, 0xF34),
           (0x46173E, 0x4617B8, 0x81A5DC, 0xF40)]
SPANS = [(0x45DD90,0x45DDA6),(0x45E27F,0x45E284),
         (0x45E13A,0x45E158),(0x45E1F0,0x45E1FA),
         (0x45E362,0x45E3BC),(0x460AC0,0x460AE0),
         (0x4612D1,0x4612EE),(0x4615CA,0x4617B8),
         (0x45F2AA,0x45F310),(0x45EAA5,0x45EADA)]


def dwords(*values):
    return b''.join(struct.pack('<I', v & 0xFFFFFFFF) for v in values)


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, 0x10000)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        self.u.mem_write(TYPE + 0x1F8, b'TEST\0')

    def write(self, address, value):
        self.u.mem_write(address, dwords(value))

    def ini(self, key, raw):
        u = self.u
        name = bytes(u.mem_read(key, 64)).split(b'\0')[0]
        crc = call(0x4A1DE0, ecx=SCRATCH, stack_args=[SCRATCH+0x100,len(name)],
            writes={SCRATCH:bytes(16), SCRATCH+0x100:name})['eax']
        u.mem_write(INI, bytes(0x40))
        self.write(INI+4, TYPE+0x1F8)
        self.write(INI+8, SECTION)
        self.write(SECTION+0x2C, INDEX)
        self.write(SECTION+0x30, int(raw is not None))
        self.write(SECTION+0x38, 1)
        self.write(SECTION+0x3C, INDEX)
        self.write(INDEX, crc)
        self.write(INDEX+4, ENTRY)
        self.write(ENTRY+0x10, RAW)
        u.mem_write(RAW, (raw or '').encode('ascii')+b'\0')

    def triple(self, raw, initial, which):
        u=self.u
        begin,end,key,offset=TRIPLES[which]
        self.ini(key,raw)
        u.mem_write(TYPE+offset, dwords(*initial))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_EBP,TYPE)
        u.reg_write(UC_X86_REG_EDI,TYPE+0x1F8)
        run_checked(u,begin,end)
        return list(struct.unpack('<iii',u.mem_read(TYPE+offset,12)))

    def scalar(self, raw, key, default, fn):
        self.ini(key,raw)
        u=self.u
        u.mem_write(SP,dwords(RET_MAGIC,TYPE+0x1F8,key,default))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ECX,INI)
        run_checked(u,fn,RET_MAGIC)
        value=u.reg_read(UC_X86_REG_EAX)
        return struct.unpack('<i',dwords(value))[0] if fn==0x5276D0 else bool(value & 255)

    def buildup(self, count, gate, stages, speed_bits):
        u=self.u
        u.mem_write(TYPE+0xF04,dwords(0,1,0))
        u.mem_write(TYPE+0x16B7,bytes([gate]))
        self.write(TYPE+0x16F8,stages)
        u.mem_write(SHP+6,struct.pack('<H',(count or 0)&65535))
        self.write(0x8871E0,RULES)
        u.mem_write(RULES+0x1518,struct.pack('<Q',int(speed_bits,16)))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_EBP,TYPE)
        u.reg_write(UC_X86_REG_EBX,0)
        u.reg_write(UC_X86_REG_EAX,0 if count is None else SHP)
        run_checked(u,0x45F2AA,0x45F310)
        return list(struct.unpack('<iii',u.mem_read(TYPE+0xF04,12)))


def generate():
    f=Fixture()
    triples=[]
    values=[None,'','1,2,3','-4,-5,-6','2147483648,4294967295,4294967296',
        '99999999999999999999999999999999999999999999999999999999999999999999,2,3',
        '1','1,','1,,3','1,2x,3','1 ,2,3','1, 2, 3','+,-1,2','x,2,3',
        '1,0x10,3','1,\t-2,3','1,\x1f2,3','1,2,3junk','1,2,3,4',
        '"1,2,3"',' 1,2,3 ',' '*62+'1,2,3','\v1,2,3','1,\v2,3','1,\f2,3']
    for initial in ([0,1,0],[7,-9,11]):
        for raw in values:
            outputs=[f.triple(raw,initial,n) for n in range(4)]
            assert all(x==outputs[0] for x in outputs)
            triples.append(dict(raw=raw,initial=initial,output=outputs[0]))
    scalars=[]
    for raw in (None,'','9','-1','2147483648','4294967295','$FFFFFFFF','65535','junk'):
        scalars.append(dict(raw=raw,output=f.scalar(raw,0x81A6F4,9,0x5276D0)))
    flags=[]
    for raw in (None,'','yes','no','true','false','1','0','junk'):
        flags.append(dict(raw=raw,output=f.scalar(raw,0x81AA20,0,0x5295F0)))
    buildup=[]
    for count in (None,0,1,2,23,24,32767,32768,65535):
        for gate,stages in ((False,9),(True,9),(True,-1),(True,2147483647)):
            bits='3fd3333333333333'
            buildup.append(dict(count=count,gate=gate,stages=stages,speed_bits=bits,
                output=f.buildup(count,gate,stages,bits)))
    return dict(schema_version=1,triples=triples,gate_stages=scalars,firestorm_wall=flags,buildup=buildup)


def metadata():
    f=Fixture()
    result=provenance(scope='BuildingType art body triples, GateStages, FirestormWall and ordinary buildup asset-count slice',
        assumptions=['Supplied INI section/entry indexes with native CRC; cached section pointer matches supplied TEST type name',
          'Original ReadString64, sprintf and sscanf execute for all four animation triplets',
          'Buildup count is supplied raw SHP header16, pointer presence supplied after asset lookup',
          'Constructor triple defaults0,1,0 from EBX0 at45DD9A and EBP1 at45E27F; GateStages9 at45E1F0; FirestormWall0 at45E14B',
          'Buildup ordinary45F230 path only; alternate theater45EAA5 full-frame-count path is recorded but not executed or unified'],
        substitutions=[],entry_points={'triples':0x4615CA,'integer_reader':0x5276D0,'bool_reader':0x5295F0,'buildup':0x45F2AA})
    result['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),
        sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return result


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
