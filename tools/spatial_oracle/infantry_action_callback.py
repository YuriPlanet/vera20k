"""Whole zero-health DoAction -> failed-path -> WalkStop callback ordering.

The current-cell CanEnter virtual result is supplied at its body boundary; the
original action/failed-path/Stop code and original vtables remain unchanged.
"""
from pathlib import Path
import hashlib
import struct
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, SCRATCH
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.infantry_deploy_action import Fixture as Base, ACTOR

CELL = SCRATCH+0x1A000
TABLE = 0xC00000


class Fixture(Base):
    def __init__(self):
        super().__init__()
        self.can_enter = self.read(0x7EB058+0x1AC)
        self.answer = 0
        self.map_ready = False

    def observe(self, u, address, size, data):
        if address == 0x51D6F0 and not self.map_ready:
            table = bytearray(0x100000)
            struct.pack_into('<I', table, (10*512+10)*4, CELL)
            u.mem_write(TABLE, bytes(table))
            u.mem_write(0x87F7E8+0x13C, dwords(TABLE,0x40000))
            u.mem_write(0x87F924, dwords(TABLE))
            u.mem_write(CELL, dwords(0x7E4EEC))
            u.mem_write(CELL+0x24, struct.pack('<hh',10,10))
            self.map_ready = True
        if address == self.can_enter:
            sp = u.reg_read(UC_X86_REG_ESP)
            self.events.append(dict(address=f'{address:08X}',
                                    supplied_can_enter=self.answer, state=self.state()))
            u.reg_write(UC_X86_REG_EAX, self.answer & 0xffffffff)
            u.reg_write(UC_X86_REG_EIP, self.read(sp)&0xffffffff)
            u.reg_write(UC_X86_REG_ESP, sp+24)
            return
        if address in (0x51DAF0,0x51DB46,0x51DBAC,0x51DBBE,0x4D55C0):
            self.events.append(dict(address=f'{address:08X}',state=self.state()))
        super().observe(u,address,size,data)

    def execute(self, row):
        self.answer = row.get('can_enter',0)
        self.map_ready = False
        original = bytes(self.u.mem_read(0x51DAF0,0xDF))
        result = super().execute(row)
        result['cell_entry_blocked'] = self.u.mem_read(ACTOR+0x6DC,1)[0]
        assert original == bytes(self.u.mem_read(0x51DAF0,0xDF))
        return result


def generate():
    f=Fixture()
    rows=[dict(kind='action',health=hp,doing=-1,request=request,prone=prone,
               pending=pending,can_enter=answer,moving=1,motion=1)
          for hp in (0,-1,1) for request in (0,2,27,31)
          for prone,pending,answer in ((0,0,0),(1,0,1),(1,1,0))]
    rows += [dict(kind='callback',health=0,doing=doing,counts=[[28,count]],
                  can_enter=answer) for doing in (0,31) for count in (0,6)
             for answer in (0,2)]
    return [f.execute(row) for row in rows]


def metadata():
    f=Fixture()
    out = provenance(
        scope='Whole DoAction zero-health synchronous failed-path action and Stop callback; bounded supplied current-cell admission result.',
        assumptions=[
            'Original action51D6F0, failed-path51DAF0, WalkStop75ADA0 and pending521B40 code/vtables remain unchanged. Original Map565730 resolves supplied Cell10,10 and original Facing/current-bridge query execute.',
            'Current-cell Infantry CanEnter virtual body result is supplied0/1/2; collision/occupation/type producers of that result are outside this fixture.',
            'Type/House/sequence/locomotor inputs and OS-interlocked support inherit infantry_deploy_action. Original Scenario RNG executes; full state captured.',
            'Health0 contrasts with signed health-1/+1. Ordinary movement zone, no carry/airborne/falling remaps and no sequence sound entries.'
        ], substitutions=[f'Infantry virtual CanEnter body{f.can_enter:08X}: return supplied EAX and original five-argument stack cleanup; original callable bytes/vtable unchanged'],
        entry_points={'do_action':0x51D6F0,'failed_path':0x51DAF0,'walk_stop':0x75ADA0})
    out['original_failed_path'] = dict(start='0051DAF0',end_exclusive='0051DBCF',
        hex=(code:=bytes(f.u.mem_read(0x51DAF0,0xDF))).hex(),sha256=hashlib.sha256(code).hexdigest())
    return out


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
