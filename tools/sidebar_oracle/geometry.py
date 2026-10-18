"""Original sidebar screen constants, shape dimensions and tab initialization."""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE, RET_MAGIC

def put32(u,address,value):u.mem_write(address,struct.pack('<I',value&0xffffffff))
def read(u,address,count=1):return list(struct.unpack('<'+'i'*count,u.mem_read(address,count*4)))
def machine():
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
 for address,size in [(STACK_BASE,STACK_SIZE),(SCRATCH,SCRATCH_SIZE),(RET_MAGIC,4096)]:u.mem_map(address,size)
 return u
def call(u,address,args=(),ecx=0):
 sp=STACK_BASE+STACK_SIZE-4096;u.mem_write(sp,struct.pack('<'+'I'*(1+len(args)),RET_MAGIC,*args))
 u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,ecx);run_checked(u,address,RET_MAGIC)
def initialize_layout(u,w,h,side,view_buffer):
 """Bind declared screen inputs through the original shared layout sequence."""
 scenario=read(u,0xA8B230)[0]&0xffffffff;put32(u,scenario+0x34B8,side)
 u.mem_write(0xA8EB7C,b'\x01');u.mem_write(0xA8ED6B,b'\0')
 for address,value in [(0xA8EB84,w),(0xA8EB88,h),(0x886FB8,w),(0x886FBC,h)]:put32(u,address,value)
 call(u,0x72AD20,ecx=view_buffer);screen_view=read(u,view_buffer,4)
 call(u,0x72AD90,ecx=view_buffer);view=read(u,view_buffer,4);assert view==screen_view
 u.mem_write(0x886FA0,bytes(u.mem_read(view_buffer,16)))
 call(u,0x6A5130,[0]);call(u,0x6A5090);call(u,0x6A5130,[0])
 return view

def generate():
 cases=[]
 for w,h in [(640,480),(800,600),(1024,768),(1280,720),(1600,900),(1920,1080),(2560,1440)]:
  for side in [0,1,2]:
   u=machine();put32(u,0xA8B230,SCRATCH)
   view=initialize_layout(u,w,h,side,SCRATCH+0x4000)
   cases.append(dict(screen=[w,h],side=side,tactical=view,body=read(u,0x886F90,4),globals=read(u,0xB0B4DC,15)))
 gadgets=[]
 for w,h,over_w,over_h in [(28,27,0,0),(32,28,0,0),(64,31,0,0),(52,32,0,0),(46,25,0,0),(46,27,0,0),(72,18,0,0),(168,110,7,0),(168,110,0,9),(-1,-2,0,0)]:
  u=machine();u.mem_write(SCRATCH+256,struct.pack('<HhhH',0,w,h,1))
  call(u,0x69DE00,[SCRATCH+256,over_w,over_h],ecx=SCRATCH)
  gadgets.append(dict(canvas=[w,h],override=[over_w,over_h],size=read(u,SCRATCH+20,2)))
 tab_initialization=[]
 for disabled_before in [[False]*4,[False,True,False,True],[True]*4]:
  u=machine()
  for tab in range(4):
   gadget=0xB07C48+tab*0x60
   call(u,0x69DCF0,ecx=gadget)
   u.mem_write(gadget+0x1E,bytes([disabled_before[tab]]))
  # Sidebar Init's complete four-button loop. Original Disable4E1460 and
  # shape-size update69DFF0 execute; no callback supplies disabled results.
  # The rest of Sidebar Init (radar, strip zones, observer UI) is excluded.
  # No RNG, simulation timer or detach calls occur in this bounded loop.
  run_checked(u,0x6A5411,0x6A5486,
              required_addresses=(0x4E1460,0x69DFF0),
              context={'case':'tab-initialization','disabled_before':disabled_before})
  tab_initialization.append(dict(disabled_before=disabled_before,
   disabled_after=[bool(u.mem_read(0xB07C48+tab*0x60+0x1E,1)[0]) for tab in range(4)]))
 return dict(cases=cases,shape_gadgets=gadgets,tab_initialization=tab_initialization)

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
  scope='21 ordinary right-sidebar layouts, 10 shape-gadget header/override cases and three four-tab initialization controls; original native instructions only.',
  assumptions=['Normal sidebar-right; editor flag clear; scenario side0/1/2.', 'Set_View_Dimensions copies native viewport before InitSurface; rectangle initialization precedes constants.', 'Gadgets have no previously-owned SHP to free. Synthetic signed SHP canvas headers are explicit inputs.', 'Cameo60x48 positions are separate from shape gadgets; original6A82FD/8304 and6ABF6E..BFA7 establish their fixed hit dimensions.'],
  substitutions=[],entry_points={'screen':0x72AD20,'options_viewport':0x72AD90,'constants':0x6A5090,'layout':0x6A5130,'gadget_shape':0x69DE00,'tab_init_loop':0x6A5411,'tab_gadget_ctor':0x69DCF0,'gadget_disable':0x4E1460}))
