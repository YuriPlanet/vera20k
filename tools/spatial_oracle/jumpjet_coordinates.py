"""Original Jumpjet coordinate storage and its Infantry destination producer.

Original constructors, getters, MoveTo, Stop and 54D6D0/4ACA10/481180 run.
FNPC returns a declared cell; owner vtable callbacks are explicit fixture seams.
This is not full flight, landing admission, or an unrestricted map search.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.walk_head_occupation import Original, CELL, CURRENT, INPUT, OUTPUT, OWNER, VTABLE, LOCO, TYPE, SCENARIO, SCRATCH, DUMMY

KIND, TYPE_GET, HEIGHT, MARK, SET_SPEED, CELL_GET, CELL_COORD, DAMAGE = [SCRATCH+0xe000+i*0x100 for i in range(8)]
RULES = SCRATCH+0xf000


def initialize_jumpjet_levels(call):
    """Original CRT startup table81373C..813754, in its recorded order."""
    for entry in (0x54AA30, 0x54AA60, 0x54AA80, 0x54AAA0,
                  0x54AAC0, 0x54AAE0, 0x54AB00):
        call(entry, 0, [])


class Jumpjet(Original):
    def __init__(self, row):
        self.row=row
        super().__init__()
        u=self.uc
        u.reg_write(UC_X86_REG_FPCW,0x0e7f)
        u.mem_write(0x8871e0,dwords(RULES))
        u.mem_write(RULES+0x40c,dwords(4))
        u.mem_write(0xabc5a8,dwords(0,0,0))
        u.mem_write(0xabc5dc,dwords(416))
        u.mem_write(0x8a0740,dwords(104))
        u.mem_write(0xa8e7ac,dwords(0))
        self.call(0x54ac40,LOCO,[])
        u.mem_write(LOCO+0xc,dwords(OWNER))
        for slot,fn in [(0x2c,KIND),(0x48,0x5f65a0),(0x4c,0x4dbdf0),(0x84,TYPE_GET),(0x1c8,HEIGHT),(0x124,MARK),(0x544,SET_SPEED),(0x2f4,CELL_COORD),(0x1bc,CELL_GET),(0x16c,DAMAGE)]:
            u.mem_write(VTABLE+slot,dwords(fn))
        u.mem_write(OWNER+0x674,dwords(LOCO+4))
        u.mem_write(OWNER+0x684,b'\xff')
        u.mem_write(OWNER+0x6c,dwords(100))
        u.mem_write(OWNER+0x90,b'\x01')
        u.mem_write(OWNER+0x9c,dwords(2496,2624,0))
        u.mem_write(OWNER+0xb4,dwords(0xffffffff))
        u.mem_write(TYPE+0x67c,dwords(3))
        u.mem_write(TYPE+0x5b4,dwords(9))
        self.setup(dict(input=row.get('request',[2688,2688,900]),ground=row.get('ground',0),deck=0,level=row.get('level',2),slope=row.get('slope',0),structural=row.get('structural',False)))
        u.mem_write(LOCO+0x50,dwords(row.get('phase',0)))
        self.call(0x65c6d0,SCENARIO+0x218,[31])
        self.events=[]
        self.fail=False
    def observe(self,u,address,size,data):
        sp=u.reg_read(UC_X86_REG_ESP)
        if address==KIND:self.ret(0,15)
        elif address==TYPE_GET:self.ret(0,TYPE)
        elif address==HEIGHT:self.ret(0,self.row.get('height',0))
        elif address==SET_SPEED:self.ret(8,0)
        elif address==MARK:self.ret(4,0)
        elif address==CELL_GET:self.ret(0,CURRENT)
        elif address==CELL_COORD:
            p=self.read32(sp+4);u.mem_write(p,packed(9,10));self.ret(4,p)
        elif address==DAMAGE:
            self.events.append('damage');self.ret(28,0)
        elif address==0x56dc20:
            args=[self.read32(sp+4+i*4) for i in range(15)]
            self.events.append(['fnpc',list(struct.unpack('<hh',u.mem_read(args[1],4))),args[2:12],list(struct.unpack('<hh',u.mem_read(args[12],4)))])
            u.mem_write(args[0],packed(0,0) if self.fail else packed(10,10));self.ret(60,args[0])
        else:super().observe(u,address,size,data)
    def snapshot(self):
        self.call(0x54d9b0,0,[LOCO+4,OUTPUT])
        own=list(struct.unpack('<iii',self.uc.mem_read(OUTPUT,12)))
        self.call(0x4dbdf0,OWNER,[OUTPUT,0])
        foot=list(struct.unpack('<iii',self.uc.mem_read(OUTPUT,12)))
        self.call(0x54ae50,0,[LOCO+4])
        return dict(destination=list(struct.unpack('<iii',self.uc.mem_read(LOCO+0x40,12))),moving=bool(self.uc.reg_read(UC_X86_REG_EAX)&255),phase=self.read32(LOCO+0x50),getter=own,foot=foot)
    def execute(self):
        trace=[self.snapshot()]
        for action in self.row['actions']:
            if action=='move':self.call(0x54b1c0,0,[LOCO+4,*self.row.get('request',[2688,2688,900])])
            elif action=='null':self.call(0x54b1c0,0,[LOCO+4,0,0,0])
            elif action=='activate':self.call(0x54b980,LOCO,[])
            elif action=='stop':self.call(0x54b4d0,0,[LOCO+4])
            elif action=='stop_fail':
                self.fail=True;self.call(0x54b4d0,0,[LOCO+4]);self.fail=False
            else:raise AssertionError(action)
            trace.append(self.snapshot())
        return dict(trace=trace,events=self.events,random_indices=[self.read32(SCENARIO+0x21c),self.read32(SCENARIO+0x220)],dummy=list(struct.unpack('<hh',self.uc.mem_read(DUMMY+0x24,4))))

def generate():
    cases=[dict(actions=[]),dict(actions=['activate']),dict(actions=['move','activate','null']),dict(actions=['move','activate','stop']),dict(actions=['move','activate','stop_fail']),dict(actions=['move'],ground=0x1c),dict(actions=['move'],ground=0x20),dict(actions=['move'],ground=4),dict(actions=['move'],level=2,slope=1),dict(actions=['move'],structural=True),dict(actions=['move'],phase=4),dict(actions=['null'],phase=3)]
    return [dict(input=row,output=Jumpjet(row).execute()) for row in cases]

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Jumpjet constructor/query, supplied-result FNPC MoveTo/Stop and original Infantry coordinate adjustment/slot selection; phase0 activation. Not full flight/landing/callbacks or unrestricted FNPC.',
        entry_points={'constructor':0x54ac40,'move_to':0x54b1c0,'stop':0x54b4d0,'adjust':0x54d6d0,'placement':0x4aca10,'cell_placement':0x481180,'coordinate':0x54d9b0,'foot_coordinate':0x4dbdf0,'moving':0x54ae50,'phase0':0x54b980},
        assumptions=['Infantry RTTI15, mission0 supplied, current9,10 and selected10,10; ground level/slope/raw and 104/416 runtime height inputs; subcell offsets from existing placement corpus; Scenario RNG seed31, FPCW0E7F; supplied nonzero phases in two rows.'],
        substitutions=['FNPC returns10,10 or NullCell0,0 as declared; owner type/kind/height/cell getters and Mark/SetSpeed/damage callbacks supplied; original House53A130 executes constantfalse.'],
    ))
