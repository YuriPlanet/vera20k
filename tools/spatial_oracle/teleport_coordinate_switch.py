"""Original Teleport BEGIN/END and raw-copy HeadTo coordinate preservation.

Supplied complete interface objects; only OS Interlocked imports are emulated.
No Process, relocation or flight controller is claimed by this corpus.
"""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import (load_image, run_checked, STACK_BASE, STACK_SIZE,
    SCRATCH, SCRATCH_SIZE, RET_MAGIC, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

TELE, OLD, OWNER, OUT = SCRATCH+0x100, SCRATCH+0x300, SCRATCH+0x1000, SCRATCH+0x3000
TABLES = {'fly':0x7E89F4, 'hover':0x7EACFC, 'rocket':0x7F0B1C}

def case(family, coordinate, occupied):
    u=Uc(UC_ARCH_X86, UC_MODE_32); load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE);u.mem_map(SCRATCH,SCRATCH_SIZE);u.mem_map(RET_MAGIC,0x1000)
    def read(p):return struct.unpack('<I',u.mem_read(p,4))[0]
    def xyz(p):return list(struct.unpack('<iii',u.mem_read(p,12)))
    def hook(_u,at,_n,_d):
        if at in (read(0x7E11C8),read(0x7E11CC)):
            sp=u.reg_read(UC_X86_REG_ESP);p=read(sp+4)
            value=(read(p)+(1 if at==read(0x7E11C8) else -1))&0xffffffff
            u.mem_write(p,dwords(value));u.reg_write(UC_X86_REG_EAX,value)
            u.reg_write(UC_X86_REG_EIP,read(sp));u.reg_write(UC_X86_REG_ESP,sp+8)
    u.hook_add(UC_HOOK_CODE,hook)
    # Teleport ILocomotion at object+4; IPiggyback at object+18.
    u.mem_write(TELE+4,dwords(0x7F5000));u.mem_write(TELE+0xC,dwords(OWNER))
    u.mem_write(TELE+0x18,dwords(0x7F4FDC))
    u.mem_write(OLD+4,dwords(TABLES[family]));u.mem_write(OLD+0xC,dwords(OWNER))
    u.mem_write(OWNER+0x9C,dwords(*coordinate));u.mem_write(OWNER+0x428,dwords(71,73))
    u.mem_write(TELE+0x48,dwords(OLD+4 if occupied else 0))
    def invoke(entry,args):
        sp=STACK_BASE+STACK_SIZE-0x1000;u.mem_write(sp,dwords(RET_MAGIC,*args));u.reg_write(UC_X86_REG_ESP,sp)
        run_checked(u,entry,RET_MAGIC,count=10000,required_addresses=[entry])
        assert u.reg_read(UC_X86_REG_ESP)==sp+4*(len(args)+1)
        return u.reg_read(UC_X86_REG_EAX)
    begin=invoke(0x719E90,[TELE+0x18,OLD+4]);after_begin=xyz(OWNER+0x9C)
    assert read(0x7F5018)==0x55ACA0
    invoke(0x55ACA0,[TELE+4,OUT]);query=xyz(OUT)
    end=invoke(0x719EE0,[TELE+0x18,OUT+0x20]);after_end=xyz(OWNER+0x9C)
    return dict(input=dict(family=family,coordinate=coordinate,occupied=occupied),begin=begin,
                after_begin=after_begin,query=query,end=end,after_end=after_end,
                stash_cleared=read(TELE+0x48)==0,returned=read(OUT+0x20)==OLD+4,
                end_clears_owner_pair=[read(OWNER+0x428),read(OWNER+0x42C)])

def generate():
    return [case(family,c,occupied) for family in TABLES
            for c in ([1408,1664,541],[-255,0,-37],[0,0,0],[2147483647,-2147483648,2147483647])
            for occupied in (False,True)]

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Original Teleport BEGIN719E90 / END719EE0 interface transfer and HeadTo55ACA0 owner-XYZ preservation; supplied objects. No Process relocation or controller parity.',
        entry_points={'begin':0x719E90,'end':0x719EE0,'head_to':0x55ACA0},
        assumptions=['Original ILocomotion tables for suspended Fly/Hover/Rocket; linked owner and supplied signed XYZ.',
                     'Teleport IPiggyback is object+18 and its stash is +30. Occupied-slot cases demonstrate failed BEGIN retains coordinate and existing stash.',
                     'END additionally clears owner+428/+42C, which is recorded separately from coordinate ownership.'],
        substitutions=['Only OS InterlockedIncrement/Decrement imports emulate their integer operation and stdcall cleanup; original COM AddRef runs.']))
