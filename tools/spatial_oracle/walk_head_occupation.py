"""Original481180 placement and5217C0/521850 raw occupation leaves.

The cell, subcell-offset runtime table, height globals and owner identity are
explicit supplied runtime inputs. Original map lookup/ground calculation and
Scenario RNG execute. Gate lookup/answer are declared read-only virtual seams.
Walk75C240 producer executes for no-target, no-slave, no-crate cases; whole
Walk movement/PerCell and targeted/slave admission are outside this corpus.
"""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE, RET_MAGIC, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed

CELL, INPUT, OUTPUT, OWNER, VTABLE, OWNER_GET, BUILDING, TYPE, SCENARIO = [SCRATCH+i*0x1000 for i in range(9)]
CURRENT,LOCO,MISSION_GET=SCRATCH+9*0x1000,SCRATCH+10*0x1000,SCRATCH+11*0x1000
FALSE_GET,STOP_EVENT=SCRATCH+12*0x1000,SCRATCH+13*0x1000
MAP,TABLE,DUMMY=0x87F7E8,0xC00000,0xABDC50
OFFSETS=[(128,128,0),(64,64,0),(192,64,0),(64,192,0),(192,192,0)]

class Original:
    def __init__(self):
        self.uc=u=Uc(UC_ARCH_X86,UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE,STACK_SIZE)
        u.mem_map(SCRATCH,SCRATCH_SIZE)
        u.mem_map(RET_MAGIC,0x1000)
        u.reg_write(UC_X86_REG_FPCW,0x027f)
        table=bytearray(0x100000)
        struct.pack_into('<I',table,(10*512+10)*4,CELL)
        struct.pack_into('<I',table,(10*512+9)*4,CURRENT)
        u.mem_write(TABLE,bytes(table))
        u.mem_write(MAP+0x13c,dwords(TABLE,0x40000))
        u.mem_write(0x87F924,dwords(TABLE))
        u.mem_write(CELL+0x24,packed(10,10))
        u.mem_write(CURRENT+0x24,packed(9,10))
        for cell in [CELL,CURRENT,DUMMY]: u.mem_write(cell+0x44,dwords(0xffffffff))
        u.mem_write(CURRENT+0x11b,bytes((2,1)))
        u.mem_write(CELL+0x11b,bytes((2,1)))
        for address,value in [(0x89E7C0,104),(0x89E7B4,416),(0xA8F234,416)]:
            u.mem_write(address,dwords(value))
        u.mem_write(0x89E778,dwords(0,0,0))
        u.mem_write(0x89E9F0,dwords(*(v for row in OFFSETS for v in row)))
        u.mem_write(OWNER,dwords(VTABLE))
        u.mem_write(VTABLE+0x38,dwords(OWNER_GET))
        u.mem_write(VTABLE+0x184,dwords(MISSION_GET))
        u.mem_write(VTABLE+0xf0,dwords(0x5217C0))
        u.mem_write(VTABLE+0xf4,dwords(0x521850))
        u.mem_write(VTABLE+0x1d4,dwords(FALSE_GET))
        u.mem_write(VTABLE+0x1d8,dwords(FALSE_GET))
        u.mem_write(VTABLE+0x37c,dwords(FALSE_GET))
        u.mem_write(VTABLE+0x54c,dwords(STOP_EVENT))
        u.mem_write(LOCO+0xc,dwords(OWNER))
        u.mem_write(0xB45BE8,dwords(0,0,0))
        u.mem_write(0xB45C28,dwords(104))
        u.mem_write(BUILDING+0x520,dwords(TYPE))
        u.mem_write(TYPE+0x16b7,b'\1')
        u.mem_write(0xA8B230,dwords(SCENARIO))
        self.events=[]
        self.gate=0
        self.owner=41
        u.hook_add(UC_HOOK_CODE,self.observe)
    def read32(self,p):
        return struct.unpack('<I',self.uc.mem_read(p,4))[0]
    def ret(self,cleanup,result):
        u=self.uc;sp=u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX,result&0xffffffff)
        u.reg_write(UC_X86_REG_EIP,self.read32(sp))
        u.reg_write(UC_X86_REG_ESP,sp+4+cleanup)
    def observe(self,u,address,size,data):
        sp=u.reg_read(UC_X86_REG_ESP)
        if address==MISSION_GET:
            self.ret(0,0)
        elif address==FALSE_GET:
            self.ret(0,0)
        elif address==STOP_EVENT:
            self.events.append('stopped')
            self.ret(0,0)
        elif address==OWNER_GET:
            self.events.append('owner')
            self.ret(0,self.owner)
        elif address==0x47C4D0:
            assert (self.read32(sp+4),self.read32(sp+8))==(6,0)
            self.events.append('building')
            self.ret(8,BUILDING if self.gate else 0)
        elif address==0x4525F0:
            self.events.append('gate')
            self.ret(0,int(self.gate==2))
        elif address==0x578080:
            self.events.append(['ground_input',list(struct.unpack('<iii',u.mem_read(self.read32(sp+4),12)))])
    def call(self,entry,this,args):
        sp=STACK_BASE+STACK_SIZE-0x1000
        self.uc.mem_write(sp,dwords(RET_MAGIC,*args))
        self.uc.reg_write(UC_X86_REG_ESP,sp)
        self.uc.reg_write(UC_X86_REG_ECX,this)
        run_checked(self.uc,entry,RET_MAGIC,count=30000,required_addresses=[entry])
        assert self.uc.reg_read(UC_X86_REG_ESP)==sp+4*(len(args)+1)
    def setup(self,row):
        u=self.uc
        self.events=[];self.gate=row.get('gate',0)
        u.mem_write(CELL+0x124,dwords(row['ground']))
        u.mem_write(CELL+0x128,dwords(row['deck']))
        u.mem_write(CELL+0x54,dwords(row.get('ground_owner',41),row.get('deck_owner',42)))
        u.mem_write(CELL+0x140,dwords(0x100 if row.get('structural',False) else 0))
        u.mem_write(CELL+0x11b,bytes((row.get('level',2)&255,row.get('slope',1))))
        u.mem_write(INPUT,dwords(*row['input']))
    def selection(self,row):
        self.setup(row)
        self.call(0x65C6D0,SCENARIO+0x218,[row['seed']])
        self.call(0x481180,CELL,[OUTPUT,INPUT,row['priority'],row['bridge'],0])
        xyz=list(struct.unpack('<iii',self.uc.mem_read(OUTPUT,12)))
        return dict(head=xyz,events=self.events,random_indices=[self.read32(SCENARIO+0x21c),self.read32(SCENARIO+0x220)])
    def raw(self,row):
        self.setup(row)
        self.owner=row['owner']
        self.call(0x5217C0 if row['put'] else 0x521850,OWNER,[INPUT])
        return dict(ground=self.read32(CELL+0x124)&255,deck=self.read32(CELL+0x128)&255,
            owners=[self.read32(CELL+0x54),self.read32(CELL+0x58)],events=self.events)

    def producer(self,row):
        self.setup(row)
        u=self.uc; self.owner=row['owner']
        current=row['current']
        u.mem_write(OWNER+0x9c,dwords(*current))
        u.mem_write(OWNER+0x81,b'\0');u.mem_write(OWNER+0x90,b'\1')
        u.mem_write(OWNER+0x2dc,dwords(0));u.mem_write(OWNER+0x5a4,dwords(0))
        u.mem_write(LOCO+0x28,dwords(0,0,0))
        u.mem_write(CURRENT+0x124,dwords(4));u.mem_write(CURRENT+0x128,dwords(0))
        u.mem_write(CURRENT+0x54,dwords(self.owner,0xffffffff))
        self.call(0x65C6D0,SCENARIO+0x218,[row['seed']])
        self.call(0x75C240,LOCO,[INPUT])
        return dict(accepted=bool(u.reg_read(UC_X86_REG_EAX)&255),
            head=list(struct.unpack('<iii',u.mem_read(LOCO+0x28,12))),
            current_ground=self.read32(CURRENT+0x124)&255,
            current_owner=self.read32(CURRENT+0x54),
            ground=self.read32(CELL+0x124)&255,deck=self.read32(CELL+0x128)&255,
            owners=[self.read32(CELL+0x54),self.read32(CELL+0x58)],events=self.events)

    def moving(self,actions):
        u=self.uc
        self.setup(dict(input=[2752,2624,0],ground=0,deck=0,level=0,slope=0))
        self.call(0x75AA90,LOCO,[])
        u.mem_write(LOCO+0xc,dwords(OWNER))
        u.mem_write(OWNER+0x9c,dwords(2496,2624,0))
        u.mem_write(OWNER+0x81,b'\0');u.mem_write(OWNER+0x90,b'\1')
        u.mem_write(OWNER+0x2dc,dwords(0));u.mem_write(OWNER+0x5a4,dwords(0))
        u.mem_write(CURRENT+0x11b,b'\0\0')
        trace=[]
        def snapshot():
            self.call(0x75AB30,0,[LOCO+4])
            result=bool(u.reg_read(UC_X86_REG_EAX)&255)
            assert result==bool(u.mem_read(LOCO+0x34,1)[0])
            return dict(moving=result,
                destination=list(struct.unpack('<iii',u.mem_read(LOCO+0x1c,12))),
                head=list(struct.unpack('<iii',u.mem_read(LOCO+0x28,12))))
        trace.append(snapshot())
        for action in actions:
            if action=='move': self.call(0x75ACB0,0,[LOCO+4,2752,2624,0])
            elif action=='stop': self.call(0x75ADA0,0,[LOCO+4])
            elif action=='head': self.call(0x75C240,LOCO,[INPUT])
            elif action=='retire': self.call(0x75C240,LOCO,[0xB45BE8])
            else: raise AssertionError(action)
            trace.append(snapshot())
        return dict(trace=trace,events=self.events)

    def destination(self,row):
        u=self.uc
        u.reg_write(UC_X86_REG_FPCW,row['control'])
        # Ordered original CRT entries814E58/814E68/814EA4. No supplied414.
        for entry in [0x6D1830,0x6D18C0,0x6D1BF0]: self.call(entry,0,[])
        self.call(0x6D2120,60,[])
        adjustment=u.reg_read(UC_X86_REG_EAX)
        self.setup(dict(input=row['coord'],ground=0,deck=0,level=2,slope=1,structural=row['structural']))
        incoming=row['coord']
        if row['cell_target']:
            # Actual Cell+4C4104F0 forwards +48 to486840, which samples the
            # same receiver's ground through47B3A0 before Walk adjusts it.
            u.mem_write(CELL,dwords(0x7E4EEC))
            self.call(0x4104F0,CELL,[OUTPUT,OWNER])
            incoming=list(struct.unpack('<iii',u.mem_read(OUTPUT,12)))
        self.call(0x75AA90,LOCO,[])
        u.mem_write(LOCO+0xc,dwords(OWNER))
        self.call(0x75ACB0,0,[LOCO+4,*incoming])
        return dict(adjustment=adjustment,incoming=incoming,
            destination=list(struct.unpack('<iii',u.mem_read(LOCO+0x1c,12))),
            moving=bool(u.mem_read(LOCO+0x34,1)[0]))

def generate():
    native=Original();selection=[];raw=[];producer=[]
    for xy in [(128,128),(192,64),(64,192),(192,192)]:
        for ground,deck in [(0,0),(4,8),(0x1c,0x1c),(0x20,0),(0,0x20),(0x40,0),(0x44,8)]:
            for bridge in [False,True]:
                for priority in [False,True]:
                    for gate in ([0,1,2] if ground&0x40 else [0]):
                        row=dict(input=[2560+xy[0],2560+xy[1],900],ground=ground,deck=deck,
                            bridge=bridge,priority=priority,gate=gate,seed=31)
                        selection.append(dict(input=row,output=native.selection(row)))
    for structural in [False,True]:
        for xy in [(128,128),(192,64),(64,192),(192,192)]:
            for z in [0,623,624,700,832]:
                for put in [False,True]:
                    for ground,deck in [(4,8),(0x1c,0x1c)]:
                        row=dict(input=[2560+xy[0],2560+xy[1],z],ground=ground,deck=deck,
                            structural=structural,put=put,owner=99,level=2,slope=1)
                        raw.append(dict(input=row,output=native.raw(row)))
    for ground in [0,4,0x1c,0x20]:
        row=dict(input=[2752,2624,260],current=[2496,2624,260],
            ground=ground,deck=0,structural=False,owner=99,seed=31,level=2,slope=1)
        producer.append(dict(input=row,output=native.producer(row)))
    moving=[]
    for actions in [[],['move'],['move','stop'],['move','head','stop'],
        ['move','head','stop','retire'],['move','head','stop','retire','stop'],
        ['head'],['head','move']]:
        moving.append(dict(actions=actions,output=Original().moving(actions)))
    destination=[]
    for control in [0x027f,0x0e7f]:
        for structural in [False,True]:
            for cell_target,z in [(False,0),(False,416),(True,0)]:
                row=dict(control=control,structural=structural,cell_target=cell_target,coord=[2752,2624,z])
                destination.append(dict(input=row,output=Original().destination(row)))
    return dict(selection=selection,raw=raw,producer=producer,moving=moving,destination=destination)

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Bounded original infantry placement/raw leaves and no-target Walk75C240 producer, not complete Walk movement or repair admission',
        assumptions=['Allocated destination Cell(10,10) and producer current Cell(9,10) with supplied signed level/slope and raw bytes/house indices',
        'Destination rows execute6D1830/6D18C0/6D1BF0 startup and6D2120(60) under027F and hardware-captured startup0E7F (tube_startup_capture.json); original CRT pointer order814E58/814E68/814EA4 lies in active812000..815DA4 loop7CBED3',
        'Cell destination rows execute original+4C4104F0 ->+48486840 ->sameCell47B3A0 before75ACB0; non-Cell rows supply literalXYZ, not a claim for arbitrary target+4C producers',
        'Supplied initialized subcell offsets and height steps104/416; map/owner constructors are excluded',
        '75C240 producer uses no-target/no-slave owner, no crate overlays and original raw/placement callees',
        '481180 receives final argument0 (use incoming coordinate); both priority and plane choices are supplied',
        'Original map lookup,578080/47B3A0 ground calculation and seeded Scenario Random execute',
        'Sloped input and selected XY intentionally differ; returned Z comes from actual original body',
        'Moving traces execute original Walk constructor75AA90, MoveTo75ACB0, Stop75ADA0 and FindSubCellDest75C240 on a flat nonstructural cell; private head calls are supplied states, not whole movement execution'],
        substitutions=['47C4D0 ground building lookup supplies missing or Gate object;4525F0 supplies closed/open result',
        'Producer Infantry+184 supplies mission0 (Sleep); no target/slave pointer is installed',
        'Moving traces supply false owner+37C/+1D4/+1D8 predicates and observe owner+54C as a no-op callback; no claim for those receiver side effects',
        'Events observe gate/owner/ground seams only; inline RandomRanged65C7E0 is verified by random_indices, not an event marker',
        'Infantry+38 supplies mark-time owner index; raw bitmap and owner writes execute unmodified'],
        entry_points={'producer':0x75C240,'placement':0x481180,'raw_mark':0x5217C0,'raw_clear':0x521850,'random_seed':0x65C6D0,'ground':0x578080,'walk_constructor':0x75AA90,'walk_move_to':0x75ACB0,'walk_stop':0x75ADA0,'walk_is_moving':0x75AB30,'height_numerator_init':0x6D1830,'height_angle_init':0x6D18C0,'height_factor_init':0x6D1BF0,'walk_destination_height':0x6D2120,'cell_destination':0x4104F0,'cell_coordinates':0x486840}))
