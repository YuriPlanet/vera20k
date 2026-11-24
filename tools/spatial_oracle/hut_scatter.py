"""Original hut4576F0 -> Infantry51D0D0 forced NULL Scatter corridor.

The original Foot+4C and Walk+18/+10 receivers, fixed map lookup, mission
control and Scenario RNG execute. A successful FNPC result, destination setter
and locomotor Process are supplied observable seams. No DoAction31, failed
FNPC/eight-neighbour fallback, full path execution or mutable registry is claimed.
"""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed

MAP, TABLE, DUMMY = 0x87F7E8, 0xC00000, 0xABDC50
ACTOR, VT, TYPE, LOCO, LVT, HUT, BVT, CELL, DEST, ROWS, SCENARIO, RULES = [SCRATCH+i*0x2000 for i in range(12)]
SET, PROCESS, KIND = [SCRATCH+0x19000+i*0x100 for i in range(3)]


def query(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x20000)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    table = bytearray(0x100000)
    struct.pack_into('<I', table, (10*512+10)*4, CELL)
    struct.pack_into('<I', table, (10*512+11)*4, DEST)
    u.mem_write(TABLE, bytes(table))
    u.mem_write(MAP+0x13C, dwords(TABLE, 0x40000))
    u.mem_write(0x87F924, dwords(TABLE))
    u.mem_write(CELL+0x24, packed(10, 10))
    u.mem_write(DEST+0x24, packed(11, 10))
    u.mem_write(0xA8E9A0, b"\x01") # active object-list queries
    u.mem_write(CELL+0xE4, dwords(HUT))
    u.mem_write(HUT, dwords(BVT))
    u.mem_write(BVT+0x2C, dwords(KIND))
    u.mem_write(0xA83DEC, dwords(ROWS))
    u.mem_write(0xA83DF8, dwords(1))
    u.mem_write(ROWS, dwords(ACTOR))
    u.mem_write(ACTOR, dwords(VT))
    for offset, pointer in [(0x48,0x5F65A0),(0x4C,0x4DBDF0),(0x174,0x51D0D0),(0x480,SET)]:
        u.mem_write(VT+offset, dwords(pointer))
    # Original Walk constructor installs its real interface, then only Process
    # is replaced by a declared observer; its other table slots remain native.
    def read32(address): return struct.unpack('<I', u.mem_read(address, 4))[0]
    def call(entry, this, args, required):
        sp=STACK_BASE+STACK_SIZE-0x1000
        u.mem_write(sp,dwords(RET_MAGIC,*args))
        u.reg_write(UC_X86_REG_ESP,sp); u.reg_write(UC_X86_REG_ECX,this)
        run_checked(u,entry,RET_MAGIC,count=50000,required_addresses=required)
        assert u.reg_read(UC_X86_REG_ESP)==sp+4*(len(args)+1)
    call(0x75AA90,LOCO,[],[0x75AA90])
    u.mem_write(LVT,bytes(u.mem_read(read32(LOCO+4),0xAC)))
    u.mem_write(LVT+0x40,dwords(PROCESS))
    u.mem_write(LOCO+4,dwords(LVT));u.mem_write(LOCO+0xC,dwords(ACTOR))
    u.mem_write(LOCO+0x34,bytes([int(row['moving'])]))
    u.mem_write(ACTOR+0x674,dwords(LOCO+4))
    u.mem_write(ACTOR+0x684,b'\xff')
    u.mem_write(ACTOR+0x6C0,dwords(TYPE))
    u.mem_write(ACTOR+0x6C4,dwords(row['doing']))
    u.mem_write(ACTOR+0xAC,dwords(5)) # current Guard, with explicit table flag
    u.mem_write(0xA8E3A8+5*32+9,bytes([int(row['mission_scatter'])]))
    u.mem_write(TYPE+0xEBF,bytes([int(row['fraidycat'])]))
    u.mem_write(TYPE+0x67C,dwords(0))
    u.mem_write(ACTOR+0x2B4,dwords(HUT if row['attack'] else 0))
    u.mem_write(ACTOR+0x90,bytes([int(row.get('alive',True))]))
    u.mem_write(ACTOR+0x9C,dwords(10*256+128,10*256+128,0))
    nav={'null':0,'hut':HUT,'other':DEST}[row.get('nav','null')]
    u.mem_write(ACTOR+0x5A4,dwords(nav))
    if row.get('head'):
        u.mem_write(LOCO+0x28,dwords(*row['head']))
    if row.get('tube_exit'):
        u.mem_write(ACTOR+0x684,b'\x00')
        u.mem_write(0x8B413C,dwords(ROWS+0x100))
        u.mem_write(ROWS+0x100,dwords(ROWS+0x200))
        u.mem_write(ROWS+0x200+0x28,packed(*row['tube_exit']))
    u.mem_write(0xA8B230,dwords(SCENARIO))
    u.mem_write(0x8871E0,dwords(RULES))
    u.mem_write(RULES+0x17ED,b'\x01') # PlayerScatter true bypasses pure ability leaf
    for address in [0x89C848,0xA8F200,0x8B3DA8]: u.mem_write(address,dwords(0,0,0))
    events=[]; coordinates=[]
    def ret(cleanup,result):
        sp=u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX,result&0xffffffff)
        u.reg_write(UC_X86_REG_EIP,read32(sp));u.reg_write(UC_X86_REG_ESP,sp+4+cleanup)
    def observe(_u,address,_size,_data):
        sp=u.reg_read(UC_X86_REG_ESP)
        if address==KIND: ret(0,6)
        elif address==0x50B730: ret(0,0) # pure house result; no deploy rows
        elif address==0x4DBDF0: events.append('coordinate')
        elif address==0x457719:
            coordinates.append(list(struct.unpack('<iii',u.mem_read(u.reg_read(UC_X86_REG_EAX),12))))
        elif address==0x65C7E0: events.append('random')
        elif address==0x56DC20:
            args=[read32(sp+4+i*4) for i in range(15)]
            seed=list(struct.unpack('<hh',u.mem_read(args[1],4)))
            assert args[2:12]==[0,0xffffffff,0,0,1,1,0,1,0,1],args
            assert list(struct.unpack('<hh',u.mem_read(args[12],4)))==[0,0]
            assert args[13:]==[0,0]
            events.append(['fnpc',seed,[0,0]])
            u.mem_write(args[0],packed(11,10));ret(60,args[0])
        elif address==SET:
            assert read32(sp+4)==DEST and read32(sp+8)==1
            u.mem_write(ACTOR+0x5A4,dwords(DEST));events.append('destination');ret(8,0)
        elif address==PROCESS:
            assert read32(sp+4)==LOCO+4
            events.append('process');ret(4,1)
    u.hook_add(UC_HOOK_CODE,observe)
    call(0x65C6D0,SCENARIO+0x218,[31],[0x65C6D0])
    required=[0x4576F0,0x4DBDF0,0x565730,0x47C520]
    if row.get('alive',True) and row.get('nav')!='other' and not row.get('head') and not row.get('tube_exit'):
        required.append(0x51D0D0)
    call(0x4576F0,HUT,[],required)
    return dict(events=events,coordinates=coordinates,random_indices=[read32(SCENARIO+0x21C),read32(SCENARIO+0x220)],
        destination_changed=read32(ACTOR+0x5A4)!=nav,
        dummy=list(struct.unpack('<hh',u.mem_read(DUMMY+0x24,4))))


def generate():
    rows=[]
    for doing,moving,mission,fraidy,attack in [
        (-1,False,False,False,False),(0,False,False,False,True),
        (7,False,True,False,False),(-1,True,False,True,False),
        (-1,True,True,False,False),(-1,True,True,True,True),
        (31,True,True,True,False),(-1,False,True,True,False),
    ]:
        row=dict(doing=doing,moving=moving,mission_scatter=mission,fraidycat=fraidy,attack=attack)
        rows.append(dict(input=row,output=query(row)))
    for extra in [dict(alive=False,head=[40*256+128,41*256+128,0]),dict(nav='other'),dict(nav='hut'),dict(tube_exit=[-1,2])]:
        row=dict(doing=-1,moving=False,mission_scatter=True,fraidycat=False,attack=False,**extra)
        rows.append(dict(input=row,output=query(row)))
    return rows


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Forced NULL Scatter front gates, coordinate-before-alive/nav, actual RNG ordering, FNPC literal arguments, synchronous destination-before-Process; not DoAction31, failed-FNPC fallback, mutable registry, full FNPC or movement.',
        entry_points={'hut':0x4576F0,'scatter':0x51D0D0,'coordinate':0x4DBDF0,'walk_constructor':0x75AA90,'random_seed':0x65C6D0,'random_ranged':0x65C7E0},
        assumptions=['Single Infantry registry entry; active object-list lookup globalA8E9A0=true; original Walk getter and current mission Guard table flag; seed31; real center cells10,10 and11,10; supplied retained head for pending-Uninit case; supplied active Foot684 and TubeClass exit in one query row.'],
        substitutions=['House50B730 returnsfalse (no deploy Doing cases); PlayerScattertrue avoids pure ability leaf; Building RTTI returns6; FNPC successful11,10 supplied; SetDestination records pointer; Process observes call only.'],
    ))
