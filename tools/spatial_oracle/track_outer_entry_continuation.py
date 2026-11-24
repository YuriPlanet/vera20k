"""Original ordinary Process -> supplied fresh return -> real Process_Track entry.

Only Process_Movement is a supplied boundary. Original outer CALLs/branches and
TrackProcess entry/getters/residual-clear epilogue execute unchanged. Eligible
entries stop before the speed prefix, not after simulated movement.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle import track_outer_continuation as outer
from tools.spatial_oracle import track_process_entry as entry

LOCO, OWNER, TYPE, SP = outer.LOCO, outer.OWNER, SCRATCH+0x7000, outer.SP
FRESH_BODY = outer.EXTERNAL_FRESH
SPANS = ((0x4B0A6B,0x4B0ACE),(0x6A0134,0x6A0193)) + entry.CODE_RANGES


def state(u):
    return dict(alive=outer.byte(u,OWNER+0x90), valid=outer.byte(u,LOCO+0x63),
        selector=entry.read_i32(u,LOCO+0x58), queue_head=entry.read_i32(u,OWNER+0x5E0),
        latch=outer.byte(u,LOCO+0x62), turret=outer.byte(u,TYPE+0xCA1),
        residual=entry.read_i32(u,LOCO+0x4C))


class Fixture:
    def __init__(self):
        self.u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(self.u)
        for base,size in ((STACK_BASE,STACK_SIZE),(SCRATCH,0x10000),(RET_MAGIC,0x1000)):
            self.u.mem_map(base,size)
        self.original=[bytes(self.u.mem_read(a,b-a)) for a,b in SPANS]
        self.vtable=bytes(self.u.mem_read(entry.UNIT_VTABLE,0x600))
        self.u.hook_add(UC_HOOK_CODE,self.observe)
        self.u.hook_add(UC_HOOK_MEM_WRITE,self.observe_write)
        self.record=False

    def observe(self,u,address,_size,_data):
        if self.record and address in self.boundaries:
            self.visits.append(dict(address=f'{address:08X}',state=state(u)))
        if self.record and address in (entry.GET_TYPE,entry.UNIT_TYPE):
            assert u.reg_read(UC_X86_REG_ECX)==OWNER

    def observe_write(self,u,_access,address,size,value,_data):
        if self.record and address in (LOCO+0x4C,SP+0x24):
            self.writes.append(dict(instruction=f'{u.reg_read(UC_X86_REG_EIP):08X}',
                field='residual' if address==LOCO+0x4C else 'output',size=size,value=value))

    def execute(self,row):
        u=self.u;self.record=False
        u.mem_write(SCRATCH,bytes(0x10000))
        u.mem_write(STACK_BASE+STACK_SIZE-0x2000,bytes(0x2000))
        u.mem_write(OWNER,dwords(entry.UNIT_VTABLE))
        u.mem_write(OWNER+0x6C4,dwords(TYPE));u.mem_write(OWNER+0x90,b'\1')
        u.mem_write(OWNER+0x5E0,dwords(-1))
        u.mem_write(LOCO+0xC,dwords(OWNER))
        u.mem_write(LOCO+0x58,dwords(-1,0x12345678))
        u.mem_write(LOCO+0x4C,dwords(row['residual']))
        u.mem_write(SP,dwords(*outer.SAVED))
        u.mem_write(SP+0x20,dwords(RET_MAGIC,LOCO+4))
        for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBX,0),
                (UC_X86_REG_EBP,0xFFFFFFFF),(UC_X86_REG_ESI,LOCO+4),
                (UC_X86_REG_EDI,LOCO),(UC_X86_REG_ECX,OWNER)):
            u.reg_write(reg,value)
        family=outer.FAMILIES[row['family']];gate=entry.FAMILIES[row['family']]
        self.visits=[];self.writes=[]
        self.boundaries={family['ordinary'],family['fresh'],gate['entry'],gate['prefix'],
            gate['early_clear'],gate['early_ret'],entry.GET_TYPE,entry.UNIT_TYPE}
        self.record=True
        run_checked(u,family['ordinary'],family['fresh'],count=30)
        sp=u.reg_read(UC_X86_REG_ESP)
        assert u.reg_read(UC_X86_REG_ECX)==LOCO
        return_address=outer.read_u32(u,sp)
        output=outer.read_u32(u,sp+4)
        args=[outer.read_u32(u,sp+8),outer.read_u32(u,sp+12)]
        assert output==SP+0x24 and args==[1,0] and outer.byte(u,output)==0
        before=state(u)
        # Supplied fresh effects execute as a clearly external scratch body.
        # The original CALL supplied its arguments/return address, and RET12
        # returns there normally. No hook forces an original decision branch.
        body=outer.byte_write(output,row['output'])
        for address,value in ((OWNER+0x90,row['alive']), (LOCO+0x63,row['valid']),
                (LOCO+0x62,row['latch']),(TYPE+0xCA1,row['turret'])):
            body+=outer.byte_write(address,value)
        body+=outer.dword_write(LOCO+0x58,row['selector'])
        body+=outer.dword_write(OWNER+0x5E0,row['queue_head'])
        body+=outer.return_body(row['fresh_al'],12)
        u.mem_write(FRESH_BODY,body)
        run_checked(u,FRESH_BODY,return_address,count=40)
        assert u.reg_read(UC_X86_REG_ESP)==SP
        assert u.reg_read(UC_X86_REG_EAX)&255==row['fresh_al']
        supplied_after=state(u)
        end=run_checked(u,return_address,(RET_MAGIC,gate['entry']),count=40)
        track_called=end==gate['entry'];track_argument=None;track_return=None
        if track_called:
            sp=u.reg_read(UC_X86_REG_ESP)
            assert u.reg_read(UC_X86_REG_ECX)==LOCO
            track_return=outer.read_u32(u,sp)
            track_argument=outer.read_u32(u,sp+4)
            assert track_argument==0
            registers_before=tuple(u.reg_read(r) for r in
                (UC_X86_REG_EDI,UC_X86_REG_ESI,UC_X86_REG_EBP,UC_X86_REG_EBX))
            end=run_checked(u,gate['entry'],(gate['prefix'],track_return),count=100)
            if end==track_return:
                assert u.reg_read(UC_X86_REG_EAX)&255==0
                assert u.reg_read(UC_X86_REG_ESP)==SP
                assert registers_before==tuple(u.reg_read(r) for r in
                    (UC_X86_REG_EDI,UC_X86_REG_ESI,UC_X86_REG_EBP,UC_X86_REG_EBX))
                end=run_checked(u,track_return,(family['common_tail'],RET_MAGIC),count=40)
        prefix=end==gate['prefix']
        boundary=('before_speed_prefix' if prefix else 'common_process_tail'
            if end==family['common_tail'] else 'original_outer_return')
        assert prefix or end in (family['common_tail'],RET_MAGIC)
        if end==RET_MAGIC:
            assert u.reg_read(UC_X86_REG_EAX)&255==0
            assert u.reg_read(UC_X86_REG_ESP)==SP+0x28
            assert outer.SAVED==tuple(u.reg_read(r) for r in
                (UC_X86_REG_EDI,UC_X86_REG_ESI,UC_X86_REG_EBP,UC_X86_REG_EBX))
        elif not prefix:
            assert u.reg_read(UC_X86_REG_ESP)==SP
        assert self.original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
        assert self.vtable==bytes(u.mem_read(entry.UNIT_VTABLE,0x600))
        assert outer.read_u32(u,LOCO+0x5C)==0x12345678
        self.record=False
        return dict(input=row,before_fresh=before,supplied_after_fresh=supplied_after,
            after=state(u),fresh_caller_return=f'{return_address:08X}',fresh_args=args,
            track_called=track_called,track_argument=track_argument,
            track_caller_return=None if track_return is None else f'{track_return:08X}',
            prefix_eligible=prefix,boundary=boundary,boundary_address=f'{end:08X}',
            visits=self.visits,writes=self.writes,
            original_code_and_vtable_unchanged=True)


def inputs():
    for family,al,residual,valid,selector,queue,latch,turret in product(
            outer.FAMILIES,(0,1),(14,-2),(0,1),(-1,0,-2),(-1,2,8),(0,1),(0,1)):
        yield dict(family=family,fresh_al=al,residual=residual,output=0,alive=1,
            valid=valid,selector=selector,queue_head=queue,latch=latch,turret=turret)
    # Output-byte retirement and owner death bypass the TrackProcess entry,
    # preserving residual even when its own gate would have cleared it.
    for family,al,residual,life,active in product(outer.FAMILIES,(0,1),(14,-2),
            ((1,1),(0,0),(1,0)),(False,True)):
        output,alive=life
        yield dict(family=family,fresh_al=al,residual=residual,output=output,alive=alive,
            valid=int(active),selector=0 if active else -1,queue_head=-1,latch=0,turret=0)


def generate():
    fixture=Fixture()
    return dict(schema_version=1,rows=[fixture.execute(row) for row in inputs()])


def metadata():
    fixture=Fixture();u=fixture.u
    result=provenance(scope='Original ordinary Drive/Ship outer continuation joined to actual TrackProcess entry and early-clear epilogue',
        assumptions=['Outer execution starts at ordinary interior4B0A6B/6A0134 with a native-shaped saved-register/local frame and owner link',
            'Original outer CALLfresh supplies output pointer and arguments1,0; supplied fresh returnAL/output/alive/valid/selector/queue/latch/turret effects are explicit boundary inputs',
            'Original outer output/alive tests choose whether real TrackProcess executes with argument0; AL does not replace output/lifecycle state',
            'Actual TrackProcess entry includes original Unit type getters, residual-clear store and complete epilogue/RET4 when rejected',
            'Admitted entries stop before speed prefix4B0F69/6A0639; no speed or paid movement body is claimed',
            'Initial residual14/-2 and invalid selectors demonstrate retained residual handling, not downstream selector-2 geometry validity',
            'Output retirement is supplied, not inferred from a Rust finished list; no fresh turn/refusal receiver or full Process_Movement is executed',
            'No null owner-link input is supplied; native ordinary alive branch dereferences that link',
            'Read-only observers record native visits/writes; original code and Unit vtable checked unchanged'],
        substitutions=['Scratch Process_Movement boundary body supplies listed data writes and AL then RET12 to the original caller return address; no original code bytes patched'],
        entry_points={f'{family}_{key}': value for family in outer.FAMILIES
            for key,value in dict(ordinary=outer.FAMILIES[family]['ordinary'],
                fresh=outer.FAMILIES[family]['fresh'],**entry.FAMILIES[family]).items()})
    result['original_code']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
        hex=bytes(u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return result


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
