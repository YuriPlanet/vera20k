"""Original4912B0 skipped stores over actual stock radar frame histories.

Decoded frame bytes are explicit inputs. Original N1 tables and the complete
16-bit leaf execute; this fixture does not stand in for the outer radar draw,
wall-clock, or generated minimap content producers.
"""
from pathlib import Path
import hashlib,struct
from tools.sidebar_oracle.geometry import machine,call,put32
from tools.sidebar_oracle.stock import stock_bytes,shp
from tools.sidebar_oracle.palette import table
from tools.native_oracle import finish_vectors,provenance

ASSETS=[('sidec01.mix','radar.shp','sidebar.pal'),('sidec02.mix','radar.shp','sidebar.pal'),('sidec02md.mix','radary.shp','radaryuri.pal')]
SEQUENCES={'open_close':list(range(33))+list(range(31,-1,-1)),
 'reversals':list(range(11))+list(range(9,2,-1))+list(range(4,17))+list(range(15,7,-1))+list(range(9,33))}

def generate():
 cases=[]
 for archive,name,pal in ASSETS:
  raw=stock_bytes(archive,name);w,h,frames=shp(raw);_,packed=table(stock_bytes(archive,pal))
  assert (w,h,len(frames))==(168,110,33)
  u=machine();u.mem_map(0x21000000,0x50000);source,dest,palette,drawer=0x21000000,0x21020000,0x21040000,0x21041000
  put32(u,drawer+4,palette);u.mem_write(palette,packed)
  sequences=[]
  for label,indices in SEQUENCES.items():
   u.mem_write(dest,struct.pack('<H',0xA55A)*(w*h));outputs=[]
   for index in indices:
    f=frames[index];assert [f['x'],f['y'],f['w'],f['h']]==[0,0,w,h]
    u.mem_write(source,f['pixels']);call(u,0x4912B0,[dest,source,w*h,0,0,0,0,0],ecx=drawer)
    outputs.append(hashlib.sha256(bytes(u.mem_read(dest,w*h*2))).hexdigest())
   sequences.append(dict(name=label,frames=indices,output_rgb565_sha256=outputs))
  cases.append(dict(archive=archive,name=name,palette=pal,source_sha256=hashlib.sha256(raw).hexdigest(),
    decoded_frame_sha256=[hashlib.sha256(f['pixels']).hexdigest() for f in frames],
    zero_counts=[f['pixels'].count(0) for f in frames],sequences=sequences))
 return dict(cases=cases)
if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
 scope='Three original stock radar assets, two sequential/reversal histories each, complete4912B0 stores after every frame; explicit initial A55A backing.',
 assumptions=['Row-decoded stock pixels are inputs, independently parsed by Rust retail test; all frames have full168x110 canvas.',
 'Actual nativeN1 RGB565 tables from palette.py; ECX drawer+4 selects table.',
 'Outer63FB20/653100/656EC0 caller evidence establishes persistent surface; minimap and wall timing are separate.'],
 substitutions=[],entry_points={'N1_convert':0x4BBB00,'skip_zero_draw':0x4912B0}))
