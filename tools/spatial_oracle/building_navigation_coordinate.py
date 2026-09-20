"""Original Building447E90 navigation dispatch and447B20 requester approach."""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE, RET_MAGIC
from tools.spatial_oracle.map_queries import dwords

BUILDING, TYPE, REQUESTER = SCRATCH+0x1000, SCRATCH+0x3000, SCRATCH+0x6000
CONTACTS, OFFSETS, OUTPUT = SCRATCH+0x8000, SCRATCH+0x9000, SCRATCH+0xA000
FLAGS = dict(helipad=0x16CB, repair=0x16A9, bunker=0x16AB, refinery=0x16BB, shipyard=0x16BC)

def query(row):
    u=Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE); u.mem_map(SCRATCH, SCRATCH_SIZE); u.mem_map(RET_MAGIC,4096)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    u.mem_write(BUILDING,dwords(0x7E3EBC)); u.mem_write(BUILDING+0x520,dwords(TYPE))
    u.mem_write(TYPE,dwords(0x7E4570)); u.mem_write(TYPE+0xEF0,dwords(row.get('foundation',0)))
    u.mem_write(BUILDING+0x9C,dwords(*row['current']))
    for flag in row.get('flags',[]): u.mem_write(TYPE+FLAGS[flag],b'\x01')
    u.mem_write(TYPE+0x1780,dwords(row.get('count',1)))
    u.mem_write(TYPE+0x1788,dwords(OFFSETS))
    for i, offset in enumerate(row.get('offsets',[[0,0,0]])): u.mem_write(OFFSETS+i*12,dwords(*offset))
    requester = REQUESTER if row.get('requester') is not None else 0
    u.mem_write(REQUESTER,dwords(0x7F5C70)); u.mem_write(REQUESTER+0x9C,dwords(*(row.get('requester') or [0,0,0])))
    requester_kind=row.get('requester_kind','unit')
    if requester_kind == 'building':
        requester_type=SCRATCH+0xC000
        u.mem_write(REQUESTER,dwords(0x7E3EBC)); u.mem_write(REQUESTER+0x520,dwords(requester_type))
        u.mem_write(requester_type,dwords(0x7E4570)); u.mem_write(requester_type+0xEF0,dwords(5))
    elif requester_kind in ['anim','attached_anim']:
        u.mem_write(REQUESTER,dwords(0x7E3354))
        if requester_kind == 'attached_anim':
            owner, owner_type=SCRATCH+0xB000,SCRATCH+0xC000
            u.mem_write(owner,dwords(0x7E3EBC));u.mem_write(owner+0x520,dwords(owner_type))
            u.mem_write(owner_type,dwords(0x7E4570));u.mem_write(owner_type+0xEF0,dwords(5))
            u.mem_write(owner+0x9C,dwords(*row.get('requester_owner',[1408,1664,100])))
            u.mem_write(REQUESTER+0xCC,dwords(owner))
    slots=row.get('contacts',[])
    u.mem_write(BUILDING+0xE4,dwords(CONTACTS)); u.mem_write(BUILDING+0xE8,dwords(len(slots)))
    for i,slot in enumerate(slots): u.mem_write(CONTACTS+i*4,dwords(REQUESTER if slot==1 else (REQUESTER+0x1000 if slot==2 else 0)))
    trace=[]
    def observe(_u,a,_s,_d):
        if a in [0x447E90,0x447B20,0x447AC0,0x65AD90,0x5F6C80,0x4CAE30,0x7C5F00]: trace.append(hex(a))
    u.hook_add(UC_HOOK_CODE,observe)
    sp=STACK_BASE+STACK_SIZE-0x1000
    u.mem_write(sp,dwords(RET_MAGIC,OUTPUT,requester)); u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,BUILDING)
    run_checked(u,0x447E90,RET_MAGIC,count=100000,required_addresses=[0x447E90])
    assert u.reg_read(UC_X86_REG_ESP)==sp+12
    assert u.reg_read(UC_X86_REG_EAX)==OUTPUT
    return dict(input=row,coordinate=list(struct.unpack('<iii',u.mem_read(OUTPUT,12))),trace=trace)

def generate():
    rows=[]
    for foundation in [0,1,5,10]:
        base=dict(foundation=foundation,current=[1408,1664,416],requester=[2560,2560,0],count=3,offsets=[[11,-22,33],[44,55,-66],[-77,88,99]],contacts=[2,0,1])
        for flags in [[],['refinery'],['shipyard'],['helipad'],['repair'],['bunker'],['helipad','shipyard'],['repair','refinery']]:
            rows.append(base|dict(name='dispatch_'+str(foundation)+'_'+'_'.join(flags),flags=flags))
    base=dict(current=[1408,1664,-17],flags=['helipad'],offsets=[[0,0,0],[101,-202,303],[-1,2,-3]],requester=[0,0,0])
    for count in [-1,0,1,2,3]:
        for contacts in [[],[0],[1],[2,0,1],[1,1,0]]:
            for requester in [None,[0,0,0]]:
                rows.append(base|dict(name=f'docks_{count}_{contacts}_{requester}',count=count,contacts=contacts,requester=requester))
    for x,y in [(0,0),(128,0),(0,128),(-128,0),(0,-128),(128,128),(-128,128),(-128,-128),(128,-128),(1,1000),(-1,1000),(1000,1),(1000,-1)]:
        rows.append(dict(name=f'bunker_{x}_{y}',current=[0,0,17],flags=['bunker'],requester=[x,y,900]))
    for current in [[-257,-1,-2147483648],[2147483647,2147483647,2147483647]]:
        for flags in [[],['helipad'],['helipad','shipyard'],['repair','refinery']]:
            rows.append(dict(name='signed_'+str(current)+'_'+str(flags),current=current,flags=flags,count=1,offsets=[[2147483647,-2147483648,1]]))
    # Quantization-near-axis probes distinguish native rounded DirStruct from
    # high-byte truncation or a sign-only quadrant. Deltas are wide x87 values.
    for long in [81,82,10000]:
        for short in [-124,-123,-122,-2,-1,0,1,2,122,123,124]:
            for x,y in [(short,long),(-long,short),(-short,-long),(long,-short)]:
                rows.append(dict(name=f'bunker_round_{x}_{y}',current=[0,0,17],flags=['bunker'],requester=[x,y,900]))
    for current,requester in [([-2147483648,-2147483648,0],[2147483647,2147483647,0]),([2147483647,-2147483648,0],[-2147483648,2147483647,0])]:
        rows.append(dict(name='bunker_wide_'+str(current),current=current,flags=['bunker'],requester=requester))
    for flags in [['bunker'],['bunker','helipad'],['bunker','repair'],['bunker','refinery'],['bunker','shipyard'],['helipad','repair','bunker','refinery','shipyard']]:
        for requester in [None,[3000,4000,0]]:
            rows.append(dict(name='priority_'+str(flags)+'_'+str(requester),flags=flags,current=[1408,1664,11],requester=requester,count=1,offsets=[[1,2,3]]))
    for kind in ['building','anim','attached_anim']:
        for position in [[0,0,0],[1408,1664,100],[-1,2500,10]]:
            rows.append(dict(name='requester_'+kind+'_'+str(position),requester_kind=kind,flags=['bunker'],current=[1408,1664,11],requester=position,requester_owner=[1408,1664,100]))
    return [query(row) for row in rows]

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Original Building447E90 virtual navigation query, original447B20 approach dispatch,447AC0 foundation center and65AD90 sparse contacts. Supplied typed state, not parser/producer/docking mission parity.',
        entry_points={'navigation':0x447E90,'approach':0x447B20,'center':0x447AC0,'contact_index':0x65AD90},
        assumptions=['Original Building/BuildingType and Unit, Building, Anim requester vtables; attached Anim calls original owner Building+48. Immutable native foundation dimension arrays retained. Supplied physical XYZ, signed dock count, declared offset array and sparse radio slots.', 'Original x87 atan2/ftol path for bunker directional approach, FPCW0E7F. Current/requester do not move during read. Refineries and shipyards only reach approach special branches when one of Helipad/UnitRepair/Bunker dispatch flags is also present.'],substitutions=[]))
