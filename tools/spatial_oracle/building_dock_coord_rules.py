"""Original INI ReadCoord for Building docking offsets, with explicit defaults."""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_ESP
from tools.native_oracle import run_checked, finish_vectors, provenance, RET_MAGIC, SCRATCH
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, SP, dwords
KEY,DEFAULT,OUTPUT=SCRATCH+0x7000,SCRATCH+0x7100,SCRATCH+0x7200

def generate():
    f=Fixture();u=f.u
    u.mem_write(KEY,b'DockingOffset0\0')
    rows=[]
    for initial in [[0,0,0],[7,-9,11]]:
        for raw in [None,'','1,2,3','-4,-5,-6','2147483648,4294967295,4294967296','1','1,2','1,,3','x,2,3','1 ,2,3','1, 2, 3','1,2,3junk','1,2,3,4',' 1,2,3 ',' '*62+'1,2,3']:
            f.ini(KEY,raw);u.mem_write(DEFAULT,dwords(*initial));u.mem_write(OUTPUT,dwords(99,99,99))
            u.mem_write(SP,dwords(RET_MAGIC,OUTPUT,TYPE+0x1F8,KEY,DEFAULT))
            u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,INI)
            run_checked(u,0x529CA0,RET_MAGIC,count=100000,required_addresses=[0x529CA0])
            assert u.reg_read(UC_X86_REG_EAX)==OUTPUT and u.reg_read(UC_X86_REG_ESP)==SP+20
            rows.append(dict(initial=initial,raw=raw,output=list(struct.unpack('<iii',u.mem_read(OUTPUT,12)))))
    return rows

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Original529CA0 ReadCoord and unchanged sscanf/INI lookup for DockingOffset0. Explicit supplied default and exact fixture pointers; not full BuildingType46492E caller or layer/array ownership.',
        assumptions=['INI cached TEST section and native CRC indexes supplied using existing Fixture; actual original string truncation/trim/scan run.', 'Malformed partial scans can retain overwritten argument words (section pointer, key CRC, default pointer) in outputs. Those outputs certify only this exact ABI fixture; they are not portable numeric defaults or permission to invent prior-component retention.'],substitutions=[],entry_points={'read_coord':0x529CA0,'scan':0x7CA530}))
