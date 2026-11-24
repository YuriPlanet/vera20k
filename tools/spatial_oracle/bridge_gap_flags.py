"""Original586BF0 ordered flag writes, independent of bridge record production.
Run python -m tools.spatial_oracle.bridge_gap_flags --check (or --write).
"""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, STACK_BASE, STACK_SIZE, RET_MAGIC
from tools.spatial_oracle.map_queries import packed, dwords
MAP,TABLE,CELLS,RECORDS,DUMMY=0x87F7E8,0xC00000,0xB00000,0xB90000,0xABDC50

def fixture(name, records, *, cells=(), holes=(), dummy=0, default=0x80000080):
    return dict(name=name, records=records, cells=cells, holes=holes, dummy=dummy, default=default)

def cases():
    h=[3,5,8,5,0,0];v=[5,3,5,8,0,0]
    return [fixture('empty',[]),fixture('horizontal',[h]),fixture('vertical',[v]),
        fixture('decreasing_horizontal',[[8,5,3,5,0,0]]),
        fixture('decreasing_vertical',[[5,8,5,3,0,0]]),
        fixture('active_and_tube_skip',[[*h[:4],1,0],[*v[:4],0,1]]),
        fixture('high_active_byte_nonzero',[[*h[:4],255,0]]),
        fixture('structural_center_skip',[h],cells=[[5,5,0xFFFF]]),
        fixture('vertical_preserve_and_clear',[v],default=0xF001F880),
        fixture('horizontal_preserve_all_other_bits',[h],default=0xF001D080),
        fixture('crossing_horizontal_then_vertical',[h,v]),
        fixture('crossing_vertical_then_horizontal',[v,h]),
        fixture('dummy_center_and_repeated_writes',[v],holes=[[5,4],[3,4],[4,4]],dummy=0x1880),
        fixture('dummy_structural_center_skips',[v],holes=[[5,4]],dummy=0x1180),
        fixture('boundary_horizontal',[[0,0,3,0,0,0]],dummy=0x80),
        fixture('boundary_vertical',[[0,0,0,3,0,0]],dummy=0x880),
        fixture('fixed_alias',[[512,0,516,0,0,0]]),
        fixture('negative_alias',[[-1,2,2,2,0,0]]),
        fixture('transverse_word_wrap_horizontal',[[4,32767,6,32767,0,0]]),
        fixture('transverse_word_wrap_vertical',[[32767,4,32767,6,0,0]],dummy=0x800)]

def execute(case):
    uc=Uc(UC_ARCH_X86,UC_MODE_32);load_image(uc)
    uc.mem_map(STACK_BASE,STACK_SIZE);uc.mem_map(RET_MAGIC,0x1000)
    sp=STACK_BASE+STACK_SIZE-0x1000
    def call(address):
        uc.mem_write(sp,dwords(RET_MAGIC));uc.reg_write(UC_X86_REG_ESP,sp);uc.reg_write(UC_X86_REG_ECX,MAP)
        run_checked(uc,address,RET_MAGIC,count=1000000)
    call(0x49F2F0)
    uc.mem_write(MAP+0x13C,dwords(TABLE,0x40000));uc.mem_write(TABLE,bytes(0x100000))
    holes={tuple(c) for c in case['holes']};overrides={(x,y):f for x,y,f in case['cells']}
    pointers={};flag_addresses={}
    for y in range(12):
        for x in range(12):
            if (x,y) in holes:continue
            ptr=CELLS+(y*12+x)*0x200;pointers[x,y]=ptr;flag_addresses[ptr+0x140]=[x,y]
            uc.mem_write(ptr+0x24,packed(x,y));uc.mem_write(ptr+0x140,dwords(overrides.get((x,y),case['default'])))
            uc.mem_write(TABLE+(y*512+x)*4,dwords(ptr))
    uc.mem_write(DUMMY,bytes(0x200));uc.mem_write(DUMMY+0x24,packed(1234,-2345));uc.mem_write(DUMMY+0x140,dwords(case['dummy']))
    flag_addresses[DUMMY+0x140]=None
    uc.mem_write(MAP+0x54,dwords(RECORDS));uc.mem_write(MAP+0x60,dwords(len(case['records'])))
    for index,(ax,ay,bx,by,active,kind) in enumerate(case['records']):
        uc.mem_write(RECORDS+index*16,packed(ax,ay)+packed(bx,by)+bytes((active,0,0,0))+dwords(kind))
    writes=[]
    def observe(machine,access,address,size,value,user):
        base=address if address in flag_addresses else address-1
        if base not in flag_addresses:return
        before=struct.unpack('<I',machine.mem_read(base,4))[0]
        shift=(address-base)*8;mask=((1<<(size*8))-1)<<shift
        after=(before&~mask)|(value<<shift)
        writes.append(dict(target=flag_addresses[base],dummy_coord=list(struct.unpack('<hh',machine.mem_read(DUMMY+0x24,4))) if base==DUMMY+0x140 else None, flags=after))
    uc.hook_add(UC_HOOK_MEM_WRITE,observe);call(0x586BF0)
    return {**case,'writes':writes,'final_flags':[[x,y,struct.unpack('<I',uc.mem_read(ptr+0x140,4))[0]] for (x,y),ptr in pointers.items()],
        'dummy_flags':struct.unpack('<I',uc.mem_read(DUMMY+0x140,4))[0],
        'dummy_coord':list(struct.unpack('<hh',uc.mem_read(DUMMY+0x24,4)))}

def setter_cases():
    results=[]
    for family,entry,store,stop in [('nesw',0x47E040,0x47E0F0,0x47E0F6),('nwse',0x47E470,0x47E520,0x47E526)]:
        for direction in (0,2,6):
            for set_flag in (0,1):
                for initial in (0,0xC00,0x1180,0xF001FFFF):
                    uc=Uc(UC_ARCH_X86,UC_MODE_32);load_image(uc);uc.mem_map(STACK_BASE,STACK_SIZE)
                    uc.mem_write(CELLS,bytes(0x200));uc.mem_write(CELLS+0x140,dwords(initial))
                    sp=STACK_BASE+STACK_SIZE-0x1000
                    uc.mem_write(sp,dwords(RET_MAGIC,direction,set_flag))
                    uc.reg_write(UC_X86_REG_ESP,sp);uc.reg_write(UC_X86_REG_ECX,CELLS)
                    run_checked(uc,entry,stop,count=100,required_addresses=[store])
                    results.append(dict(family=family,direction=direction,set=set_flag,initial=initial,
                                        flags=struct.unpack('<I',uc.mem_read(CELLS+0x140,4))[0]))
    return results

if __name__=='__main__':
    finish_vectors(lambda:{'cases':[execute(case) for case in cases()],'setter_prefixes':setter_cases()},Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Original586BF0 ordered writes on supplied high/Tube record fixtures',
        assumptions=['Original49F2F0 initializes direction table','Supplied12x12 allocation with explicit holes; native packed fixed aliases execute',
                     'Full real flag words retained; dummy input restricted to currently retained1D80 domain',
                     'Record production/zone construction excluded here; separate retail composition proves producer linkage',
                     'Only axis-aligned producer records; malformed nonterminating record geometry excluded',
                     'Setter prefix cases stop after original anchor flag store, before conditional bridge blowup; neighbor stores/lifecycle excluded'],
        substitutions=[],entry_points={'restamp':0x586BF0,'direction_initializer':0x49F2F0,'nesw_anchor_store_prefix':0x47E040,'nwse_anchor_store_prefix':0x47E470}))
