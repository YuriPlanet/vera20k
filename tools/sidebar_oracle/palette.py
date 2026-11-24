"""Original72AE3E PAL expansion and complete4BBB00 N1 RGB565 conversion."""
from pathlib import Path
import struct,hashlib
from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_EBP,UC_X86_REG_EBX,UC_X86_REG_EDX
from tools.native_oracle import finish_vectors,provenance,run_checked,STACK_BASE,STACK_SIZE,SCRATCH
from tools.sidebar_oracle.geometry import machine,call,put32
from tools.sidebar_oracle.stock import stock_bytes

PALETTES=[('sidec01.mix','sidebar.pal'),('sidec02.mix','sidebar.pal'),('sidec02md.mix','radaryuri.pal')]
def table(raw):
 u=machine();source,expanded,ptr,dest=[SCRATCH+off for off in (0,0x1000,0x2000,0x3000)]
 u.mem_write(source,raw);put32(u,ptr,expanded);sp=STACK_BASE+STACK_SIZE-4096
 put32(u,sp+0x14,ptr);u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_EBP,source);u.reg_write(UC_X86_REG_EBX,ptr)
 run_checked(u,0x72AE3E,0x72AE99)
 for address,value in [(0x8A0DD0,11),(0x8A0DD4,3),(0x8A0DD8,0),(0x8A0DDC,3),(0x8A0DE0,5),(0x8A0DE4,2)]:put32(u,address,value)
 u.reg_write(UC_X86_REG_EDX,1);call(u,0x4BBB00,[expanded],ecx=dest)
 return bytes(u.mem_read(expanded,768)),bytes(u.mem_read(dest,512))
def generate():
 cases=[]
 for archive,name in PALETTES+[('synthetic','full-byte')]:
  raw=stock_bytes(archive,name) if archive!='synthetic' else bytes(v for i in range(256) for v in (i,(i*73)&255,255-i))
  rgb,packed=table(raw)
  cases.append(dict(archive=archive,name=name,source_sha256=hashlib.sha256(raw).hexdigest(),raw=list(raw),rgb=list(rgb),words=list(struct.unpack('<256H',packed)),table_sha256=hashlib.sha256(packed).hexdigest()))
 return dict(cases=cases)
if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
 scope='Original ordinary N1 converter for three stock sidebar palettes and full-byte-domain synthetic PAL.',
 assumptions=['Allocated source/output buffers; established RGB565 shifts11/5/0 and losses3/2/3.',
 'N1 sidebar converter87F6CC; radar usesB0FBF8 from72F510 with side2 RADARYURI.PAL.',
 'Source index0 is not suppressed by table construction;4912B0 owns skipped stores.'],
 substitutions=[],entry_points={'pal_expand':0x72AE3E,'N1_convert':0x4BBB00}))
