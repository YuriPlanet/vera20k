"""Original Unit/Building conversion health arithmetic; no native code patches.

Full ratio5F5C60 and retail type getters execute. Unit runs73992B..739956;
Building captures ratio449E64..449E74 then executes44A010..44A03C with the
same frame/local slot. Construction, Unlimbo, callbacks between those spans,
manager transfer and source disposal are excluded. PC53/chop ambient state.
"""
from pathlib import Path
from itertools import product
import hashlib
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, SCRATCH, STACK_BASE, STACK_SIZE
from tools.spatial_oracle.map_queries import dwords

SOURCE, DEST, SOURCE_TYPE, DEST_TYPE = [SCRATCH+n for n in (0x1000,0x2000,0x3000,0x4000)]
SP = STACK_BASE+STACK_SIZE-0x1000
RANGES=((0x5F5C60,0x5F5C80),(0x73992B,0x739956),(0x449E64,0x449E74),
        (0x44A010,0x44A03C),(0x741490,0x741497),(0x459EE0,0x459EE7),(0x7C5F00,0x7C5F2C))

def execute(kind, current, source_strength, destination_strength):
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
 u.mem_map(SCRATCH,0x10000);u.mem_map(STACK_BASE,STACK_SIZE)
 original=[bytes(u.mem_read(a,b-a)) for a,b in RANGES]
 for obj,typ,is_unit in ((SOURCE,SOURCE_TYPE,kind=='unit'),(DEST,DEST_TYPE,kind=='building')):
  u.mem_write(obj,dwords(0x7F5C70 if is_unit else 0x7E3EBC))
  u.mem_write(obj+(0x6C4 if is_unit else 0x520),dwords(typ))
 u.mem_write(SOURCE+0x6C,dwords(current));u.mem_write(SOURCE+0x70,dwords(-987))
 u.mem_write(DEST+0x6C,dwords(777));u.mem_write(DEST+0x70,dwords(-123))
 u.mem_write(SOURCE_TYPE+0xA0,dwords(source_strength));u.mem_write(DEST_TYPE+0xA0,dwords(destination_strength))
 u.reg_write(UC_X86_REG_EBP,SOURCE);u.reg_write(UC_X86_REG_EBX,DEST)
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_FPCW,0x0E7F)
 if kind=='unit':run_checked(u,0x73992B,0x739956)
 else:
  run_checked(u,0x449E64,0x449E74)
  run_checked(u,0x44A010,0x44A03C)
 assert u.reg_read(UC_X86_REG_ESP)==SP
 assert original==[bytes(u.mem_read(a,b-a)) for a,b in RANGES]
 actual,estimated=struct.unpack('<ii',u.mem_read(DEST+0x6C,8))
 assert actual==estimated
 return dict(kind=kind,current=current,source_strength=source_strength,destination_strength=destination_strength,
             actual=actual,estimated=estimated,source_estimated=struct.unpack('<i',u.mem_read(SOURCE+0x70,4))[0])

def generate():
 inputs=list(product(('unit','building'),(0,1,2,49,99,100,449,65535), (3,100,450,65535),(1,100,1000,65535)))
 inputs +=[(kind,*values) for kind in ('unit','building') for values in
           ((-1,100,1000),(100,-100,1000),(100,100,-1000),(2147483647,1,2147483647),
            (65535,1,65536),(1,2147483647,2147483647),(0,0,100),(1,0,100))]
 return dict(schema_version=1,rows=[execute(*args) for args in inputs])

def metadata():
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
 p=provenance(scope='Original conversion ratio, multiply, ftol and both destination health stores',
  assumptions=['PC53/chop FPCW0E7F; signed dword actual/Strength inputs; native741490/459EE0 type getters and5F5C60 execute unchanged.',
               'Unit ratio remains in x87. Building ratio explicitly stores binary64 across two original instruction regions. All construction/Unlimbo/intervening callbacks and lifecycle producers are excluded.',
               'Zero source Strength runs original masked x87 divide-by-zero/invalid and ftol; these edge rows document native behavior rather than asserting production admission.',
               'No Rust comparison or complete conversion-loop parity is claimed.'],
  substitutions=['Only data-shaped owner/type objects, stack and ambient FPCW are supplied; no callbacks or original instruction patches.'],
  entry_points=dict(ratio=0x5F5C60,unit=0x73992B,building_capture=0x449E64,building_apply=0x44A010))
 p['original_code']={f'{a:08X}..{b:08X}':dict(hex=bytes(u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(u.mem_read(a,b-a))).hexdigest()) for a,b in RANGES}
 return p

if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
