"""Original counted Event reader, constructor, list order and predicates.

The original TriggerType reader loop7274DC..727516 constructs and prepends
records, using real TEvent::Read71F4E0/strtok/atoi. Only allocation storage and
the inherited Windows CRT imports are supplied. No gameplay call is replaced.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, finish_vectors, provenance, run_checked
from tools.spatial_oracle.trigger_type_flags import FlagsFixture
from tools.spatial_oracle.infantry_deploy_rules import HEAP, PTD
from tools.spatial_oracle.building_body_rules import INI, SP, TYPE
from tools.spatial_oracle.map_queries import dwords

KEY, BUFFER, DEFINITION, SCENARIO = TYPE+0x24, HEAP+0x1000, HEAP+0x2000, HEAP+0x20000
TIMER, REPEAT = HEAP+0x3000, HEAP+0x3010
FRAMES=[0,14,15,29,30,149,150,0x7FFFFFFF,0x80000000,0xFFFFFFFF]


def execute(raw):
    f=FlagsFixture()
    u=f.u
    u.mem_write(KEY,b'EVENT_TEST\0')
    f.ini(KEY,raw)
    u.mem_write(PTD,bytes(0x74))
    for n,base in enumerate((0xB0F1A0,0xB0F658,0xB0F670)):
        u.mem_write(base+4,dwords(HEAP+0x8000+n*0x400,128,0,0,128))
    u.mem_write(0xA8B230,dwords(SCENARIO))
    u.mem_write(SCENARIO+0x1CB0+7*41,b'\1')
    u.mem_write(SCENARIO+0x24B2+9*41,b'\1')
    u.mem_write(TIMER,dwords(0,0,30))
    allocations=[]
    def allocate(_u,pc,_size,_data):
        if pc!=0x7C8E17: return
        sp=u.reg_read(UC_X86_REG_ESP)
        size=struct.unpack('<I',u.mem_read(sp+4,4))[0]
        assert size==0x58 and len(allocations)<128
        pointer=HEAP+0x10000+len(allocations)*0x60
        allocations.append(pointer)
        u.mem_write(pointer,b'\xA5'*size)
        u.reg_write(UC_X86_REG_EAX,pointer)
        u.reg_write(UC_X86_REG_ESP,sp+4)
        u.reg_write(UC_X86_REG_EIP,struct.unpack('<I',u.mem_read(sp,4))[0])
    u.hook_add(UC_HOOK_CODE,allocate)
    ranges=[(0x7274DC,0x727516),(0x71E6A0,0x71E7FA),(0x71F4E0,0x71F5B0),(0x71E940,0x71F216)]
    code=[bytes(u.mem_read(a,b-a)) for a,b in ranges]
    length=f.call(0x528A10,[TYPE+0x1F8,KEY,0x889F64,BUFFER,512],ecx=INI)
    read=bytes(u.mem_read(BUFFER,512)).split(b'\0')[0].decode('ascii')
    if length:
        pointer=f.call(0x7C9CC2,[BUFFER,0x817F70],cdecl=True)
        count=f.call(0x7C9BFD,[pointer],cdecl=True)
        assert 0<=count<128
        u.reg_write(UC_X86_REG_EAX,count)
        u.reg_write(UC_X86_REG_EBX,0)
        u.reg_write(UC_X86_REG_EBP,DEFINITION)
        u.reg_write(UC_X86_REG_ESP,SP)
        run_checked(u,0x7274DC,0x727516)
        assert u.reg_read(UC_X86_REG_ESP)==SP
    def uint(address):return struct.unpack('<I',u.mem_read(address,4))[0]
    def integer(address):return struct.unpack('<i',u.mem_read(address,4))[0]
    conditions=[]
    pointer=uint(DEFINITION+0xAC)
    while pointer:
        kind,value=integer(pointer+0x2C),integer(pointer+0x34)
        name=bytes(u.mem_read(pointer+0x38,25)).split(b'\0')[0].decode('ascii')
        # The native invalid-index Get leaves its output byte untouched.
        # Do not turn that stack residue into a semantic predicate golden.
        safe=kind in (13,47,51,60,61) or (kind in (27,28) and 0<=value<50) or (kind in (36,37) and 0<=value<100)
        results=[]
        for frame in FRAMES if safe else []:
            u.mem_write(0xA8ED84,dwords(frame))
            u.mem_write(REPEAT,b'\0')
            results.append(f.call(0x71E940,[13,0,0,TIMER,REPEAT,0],ecx=pointer)&255)
        conditions.append(dict(kind=kind,value=value,name=name,team_present=bool(uint(pointer+0x30)),results=results))
        pointer=uint(pointer+0x28)
        assert len(conditions)<=len(allocations)
    assert code==[bytes(u.mem_read(a,b-a)) for a,b in ranges]
    return dict(raw=raw,read=read,conditions=conditions,frames=FRAMES)


def generate():
    rows=['0','1,47,0,3','2,47,0,3,27,0,7','3,47,0,3,60,2,2,E1,27,0,7',
          '2,60,2,2,E1,61,2,0,MTNK','2,47,1,MissingTeam,47,0,2',
          '1,60,2,2,ABCDEFGHIJKLMNOPQRSTUVWXYZ','1,60,2,2, E1',
          '1,47,3,7','1,47,2,7,Text','1,47,0,  +7tail','1,47,0,junk',
          ',,2,,47,,0,,3,,27,0,7','2,47,0, ,27,0,7','1,47,0,4294967299',
          '1,47,0,-1','1,47,0,2147483648','1,47,0,-9223372036854775808',
          '1,47,0,\v7','1,13,0,2','1,51,0,2','word']
    rows += [f'1,{kind},0,{value}' for kind in (27,28,36,37) for value in (0,7,9,49,50,99,100,-1)]
    return [execute(raw) for raw in rows]


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'read_string':0x528A10,'counted_loop':0x7274DC,'constructor':0x71E6A0,
                      'read':0x71F4E0,'evaluate':0x71E940},
        assumptions=['Original ReadString512 on a supplied INI entry; empty TeamType/TechnoType registries; global7/local9 set.',
                     'Original reader loop prepends Events; input count is nonnegative and bounded by fixture capacity.',
                     'Predicate calls use incoming kind13, null House/Object, timer start0/duration30, explicit frame and no repeat.',
                     'Invalid variable indices are reader-only: native Get leaves the caller output byte untouched.'],
        substitutions=['Operator-new supplies poisoned Event storage; inherited fixture supplies Windows CRT TLS/error and unused heap/lock imports. No gameplay callees or instructions are replaced.'],
        scope='Counted variable-width Event records, native reversed list order, literal/type-name materialization and bounded predicates. Excludes resolved Team references, nonempty TechnoType scans and live Trigger completion/timers.',
    ))
