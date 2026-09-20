"""Retained Building+6E6, original transition callers, and selfheal smoke tail.

Original code is unchanged. Anim allocation is an explicit recorded boundary:
the caller selects name/slot/flag/order; this harness supplies a replacement
pointer, not a synthetic AnimClass constructor. Selfheal and smoke Destroy use
original bodies, including the native virtual eligibility/type/ratio getters.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools.native_oracle import finish_vectors, provenance, run_checked, RET_MAGIC, SCRATCH
from tools.spatial_oracle.object_health import Fixture, SP, OBJ, TYPE, RULES, dwords

SPANS = ((0x451EE0,0x451F58), (0x451750,0x4517CA), (0x451890,0x4518CF),
         (0x442A95,0x442B37), (0x4508D7,0x450998), (0x451400,0x4514A3),
         (0x6FA743,0x6FA793), (0x6301E0,0x6301E8), (0x70BE80,0x70BF47))

class ArtFixture(Fixture):
    def __init__(self):
        super().__init__()
        self.u.hook_add(UC_HOOK_CODE, self.alloc)
        self.capture = False

    def alloc(self, u, address, _size, _data):
        if not self.capture or address != 0x451890:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        name_ptr, slot, damaged, garrisoned, extra = [self.read(sp+n) for n in (4,8,12,16,20)]
        name = bytes(u.mem_read(name_ptr,16)).split(b'\0')[0].decode('ascii')
        self.replacements.append(dict(slot=slot, name=name, damaged=damaged,
            garrisoned=garrisoned, extra=extra,
            retained_flag=int(u.mem_read(OBJ+0x6E6,1)[0])))
        u.mem_write(OBJ+0x55C+slot*4, dwords(SCRATCH+0xD000+slot*0x10))
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_ESP, sp+24)
        u.reg_write(UC_X86_REG_EIP, self.read(sp))

    def prepare(self, hp, strength, old_flag, pattern):
        self.reset('building', hp, strength)
        self.capture = True
        self.replacements = []
        self.u.mem_write(OBJ+0x6E6, bytes((old_flag,)))
        self.u.mem_write(RULES+0x1700, struct.pack('<d', .5))
        self.u.mem_write(SCRATCH+0xF000, dwords(0x7EFB9C))
        self.u.mem_write(OBJ+0x310, dwords(SCRATCH+0xF000))
        slots = []
        for slot in range(21):
            occupied = pattern != 'sparse' or slot % 3 != 1
            normal = pattern != 'missing' or slot % 2 == 0
            damaged = pattern != 'missing' or slot % 2 == 1
            pointer = SCRATCH+0xB000+slot*0x10 if occupied else 0
            self.u.mem_write(OBJ+0x55C+slot*4, dwords(pointer))
            for enabled, offset, prefix in ((normal,0xF4C,'N'),(damaged,0xF5C,'D')):
                self.u.mem_write(TYPE+slot*0x44+offset, (f'{prefix}{slot:02}' if enabled else '').encode()+b'\0')
            slots.append(dict(slot=slot, occupied=occupied, normal=normal, damaged=damaged, pointer=pointer))
        return slots

    def output(self):
        return dict(actual=self.read(OBJ+0x6C), retained_flag=int(self.u.mem_read(OBJ+0x6E6,1)[0]),
            smoke_done=int(self.u.mem_read(SCRATCH+0xF0F8,1)[0]),
            smoke_pointer=self.read(OBJ+0x310),
            slot_pointers=[self.read(OBJ+0x55C+n*4) for n in range(21)],
            replacements=self.replacements)

def generate():
    f = ArtFixture()
    original = [bytes(f.u.mem_read(a,b-a)) for a,b in SPANS]
    rows=[]
    for source, old, requested, pattern in product(('setter','receiver','repair'), (0,1), (0,1), ('all','sparse','missing')):
        hp = 25 if requested else 75
        slots=f.prepare(hp,100,old,pattern)
        if source == 'setter':
            f.u.mem_write(SP,dwords(RET_MAGIC,requested))
            run_checked(f.u,0x451EE0,RET_MAGIC,count=2000)
        elif source == 'receiver':
            f.u.mem_write(SP+0x30,dwords(1))
            run_checked(f.u,0x442A95,0x442B37,count=2000)
        else:
            run_checked(f.u,0x4508D7,0x450998,count=2000)
        rows.append(dict(input=dict(source=source,current=hp,strength=100,old_flag=old,
            requested=requested,pattern=pattern,slots=slots),output=f.output()))
    for hp,strength,old in product((49,50,51,0,-1,2147483647),(100,0,-100),(0,1)):
        slots=f.prepare(hp,strength,old,'all')
        f.u.mem_write(TYPE+0xD14,b'\1')
        f.u.mem_write(RULES+0x16E0,struct.pack('<d',1.0))
        f.u.mem_write(0xA8ED84,dwords(900))
        run_checked(f.u,0x6FA743,0x6FA793,count=1000)
        rows.append(dict(input=dict(source='selfheal',current=hp,strength=strength,old_flag=old,
            requested=None,pattern='all',slots=slots),output=f.output()))
    assert [bytes(f.u.mem_read(a,b-a)) for a,b in SPANS] == original
    return rows

def metadata():
    f=ArtFixture()
    p=provenance(scope='Building retained art transition, ordered slot replacement call boundary, selfheal mark-only smoke tail',
        assumptions=['PC53/chop0E7F; original building vtable/type getter and health ratio',
        '21 slots, all/sparse occupancy and alternating missing normal/damaged names',
        'Receiver slice supplies result1 and stops before body-frame recomparison; paid repair starts after HP update',
        'Selfheal executes original eligibility and INC through smoke tail with original ParticleSystem vtable7EFB9C/Destroy6301E0',
        'Not full AnimClass construction/destruction, renderer, full receiver, repair economy/cadence, or special negative state coverage'],
        substitutions=['451890 allocation call records native arguments and supplies replacement pointer; no original code bytes patched'],
        entry_points={'setter':0x451EE0,'receiver':0x442A95,'repair':0x4508D7,'selfheal':0x6FA743})
    p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
        hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return p

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
