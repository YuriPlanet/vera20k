"""Original GetROF6FCFA0 with explicit virtual query boundaries.

Original arithmetic, veteran predicates, Scenario RNG and ftol execute. Supplied
queries expose class, weapon slot, type, garrison flag/count; their producers are
outside this fixture. This is not FireAt scheduling or weapon-selection parity.
"""
import hashlib
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import (load_image,run_checked,SCRATCH,STACK_BASE,
    STACK_SIZE,RET_MAGIC,finish_vectors,provenance)
from tools.spatial_oracle.map_queries import dwords

OWNER,TYPE,WEAPON,SLOT,HOUSE,RULES,SCENARIO,VTABLE,CALLBACKS=[SCRATCH+n*0x3000 for n in range(9)]
SP=STACK_BASE+STACK_SIZE-0x1000
SPAN=(0x6FCFA0,0x6FD205)


def f64(bits):
    return struct.pack('<Q',int(bits,16))


class Fixture:
    def __init__(self):
        self.u=Uc(UC_ARCH_X86,UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH,0x20000)
        self.u.mem_map(STACK_BASE,STACK_SIZE)
        self.u.mem_map(RET_MAGIC,0x1000)
        self.original=bytes(self.u.mem_read(SPAN[0],SPAN[1]-SPAN[0]))
        self.events=[]
        self.u.hook_add(UC_HOOK_CODE,self.observe)

    def word(self,address):
        return struct.unpack('<I',self.u.mem_read(address,4))[0]

    def observe(self,u,pc,_size,_data):
        if CALLBACKS<=pc<CALLBACKS+0x500 and (pc-CALLBACKS)%0x100==0:
            self.events.append(dict(query=['class','weapon','type','garrison','occupants'][(pc-CALLBACKS)//0x100]))
        elif pc==0x65C7E0:
            sp=u.reg_read(UC_X86_REG_ESP)
            self.events.append(dict(random_bounds=[self.word(sp+4),self.word(sp+8)]))
        elif pc==0x7C5F00:
            self.events.append(dict(ftol=True))

    def call(self,address,owner,args=()):
        u=self.u
        u.mem_write(SP,dwords(RET_MAGIC,*args))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ECX,owner)
        run_checked(u,address,RET_MAGIC,count=100000)
        assert u.reg_read(UC_X86_REG_ESP)==SP+4+len(args)*4
        return struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]

    def execute(self,row):
        u=self.u
        u.mem_write(SCRATCH,bytes(0x20000))
        u.reg_write(UC_X86_REG_FPCW,0x0E7F)
        u.mem_write(OWNER,dwords(VTABLE))
        queries=[(0x2C,row.get('class',15),0),(0x3F8,SLOT,4),
                 (0x84,TYPE,0),(0x400,row.get('garrison',0),0),
                 (0x408,row.get('occupants',0),0)]
        for n,(offset,result,pop) in enumerate(queries):
            pointer=CALLBACKS+n*0x100
            u.mem_write(VTABLE+offset,dwords(pointer))
            u.mem_write(pointer,b'\xB8'+dwords(result)+(b'\xC2'+struct.pack('<H',pop) if pop else b'\xC3'))
        u.mem_write(OWNER+0x21C,dwords(HOUSE))
        u.mem_write(OWNER+0x6C4,dwords(TYPE))
        u.mem_write(OWNER+0x150,struct.pack('<f',row.get('veterancy',0)))
        u.mem_write(OWNER+0x2FC,dwords(row.get('building_ammo',0)))
        u.mem_write(OWNER+0x3B8,dwords(row.get('burst_index',1)))
        u.mem_write(OWNER+0x2E4,dwords(row.get('bunker',0)))
        u.mem_write(TYPE+0x2A0,bytes([row.get('veteran_ability',0)]))
        u.mem_write(TYPE+0x2B2,bytes([row.get('elite_ability',0)]))
        for n,v in enumerate(row.get('unit_delays',[-1]*4)):
            u.mem_write(TYPE+0xE48+n*4,dwords(v))
        u.mem_write(SLOT,dwords(0 if row.get('no_weapon',False) else WEAPON))
        u.mem_write(WEAPON+0x9C,dwords(row.get('burst',1)))
        u.mem_write(WEAPON+0xB0,dwords(row.get('rof',50)))
        u.mem_write(WEAPON+0x130,bytes([row.get('direct',0)]))
        for flag,ownerfield,name in ((0x12A,0x308,'temporal'),(0x129,0x304,'parasite'),(0x12D,0x314,'magnet')):
            enabled,active=row.get(name,[0,0])
            u.mem_write(WEAPON+flag,bytes([enabled]))
            u.mem_write(OWNER+ownerfield,dwords(active))
        u.mem_write(HOUSE+0x1A8,f64(row.get('house_bits','3ff0000000000000')))
        u.mem_write(0x8871E0,dwords(RULES))
        u.mem_write(RULES+0x690,f64(row.get('veteran_bits','3fe3333340000000')))
        u.mem_write(RULES+0xF44,dwords(int(row.get('occupy_bits','3f800000'),16)))
        u.mem_write(RULES+0xF50,dwords(int(row.get('bunker_bits','3f800000'),16)))
        u.mem_write(0xA8B230,dwords(SCENARIO))
        self.call(0x65C6D0,SCENARIO+0x218,[row.get('seed',31)])
        self.events=[]
        before=bytes(u.mem_read(SCENARIO+0x218,0x3F4)).hex()
        value=self.call(0x6FCFA0,OWNER,[0])
        assert self.original==bytes(u.mem_read(SPAN[0],SPAN[1]-SPAN[0]))
        return dict(input=row,output=value,events=self.events,rng_before=before,
                    rng_after=bytes(u.mem_read(SCENARIO+0x218,0x3F4)).hex())


def inputs():
    rows=[dict(rof=r,house_bits=h,seed=s) for r in (-2147483648,-65537,-1,0,1,50,65536,2147483647)
          for h in ('3ff0000000000000','3fe3333340000000','bff0000000000000','0000000000000000',
                    '7ff0000000000000','7ff8000000000001') for s in (1,31)]
    rows += [dict(rof=r,veterancy=v,veteran_ability=a,elite_ability=b,veteran_bits=m)
             for r in (1,50,65536,-1) for v,a,b in ((0,1,1),(1,0,1),(1,1,0),(2,1,0),(2,0,1))
             for m in ('3fe3333340000000','c000000000000000','7ff8000000000001')]
    rows += [dict(garrison=1,occupants=n,occupy_bits=m,rof=r)
             for n in (0,1,3) for m in ('00000000','3fc00000','bf800000','7fc00001','7f800000') for r in(-1,50,65536)]
    rows += [dict(bunker=1,bunker_bits=m,rof=r)
             for m in ('00000000','3fc00000','bf800000','7fc00001','7f800000') for r in (-1,50,65536)]
    rows += [dict(burst=5,burst_index=i,**{'class':c},unit_delays=delays)
             for i in (-1,0,1,2,4,5) for c in (1,15,28)
             for delays in ([-1]*4,[0,-2,65536,2147483647])]
    rows += [dict(rof=-77,direct=1),dict(no_weapon=True),dict(**{'class':6},building_ammo=2)]
    rows += [{name:[a,b], 'rof':-77} for name in ('temporal','parasite','magnet') for a in (0,1) for b in (0,1)]
    return rows


def generate():
    f=Fixture()
    return [f.execute(row) for row in inputs()]


def metadata():
    f=Fixture()
    out=provenance(scope='Original6FCFA0 GetROF signed numeric stores, branch order and RNG with supplied query callbacks',
        assumptions=['Queried type, weapon, class, garrison/count and retained HouseROF are supplied inputs',
                     'Original Scenario RNG seed constructor, range draws, veterancy predicates and ftol execute',
                     'Original RTTI leaves establish Unit1, Aircraft2, Building6 and Infantry15; class1 Type+6C4 delays belong to Unit',
                     'No caller FireAt scheduling, weapon selection, INI parsing or House factor construction claim'],
        substitutions=['Scratch-owned virtual query table returns supplied class/weapon/type/garrison/occupants; originalPE code/vtables unchanged'],
        entry_points={'get_rof':0x6FCFA0})
    out['original_classes'] = []
    for name, vtable in (('unit',0x7F5C70),('aircraft',0x7E22A4),
                         ('building',0x7E3EBC),('infantry',0x7EB058)):
        entry=f.word(vtable+0x2C)
        value=f.call(entry,OWNER)
        out['original_classes'].append(dict(name=name,vtable=f'{vtable:08X}',
            entry=f'{entry:08X}',value=value,hex=bytes(f.u.mem_read(entry,6)).hex()))
    out['original_slices']=[dict(start=f'{SPAN[0]:08X}',end_exclusive=f'{SPAN[1]:08X}',hex=f.original.hex(),sha256=hashlib.sha256(f.original).hexdigest())]
    return out


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
