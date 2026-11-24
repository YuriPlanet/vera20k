"""Execute the original RadarClass::Draw housing timer/frame branch.

The sole substituted function is the imported Windows timeGetTime API. Its
caller6C8C40 and every internal timer/endpoint instruction execute unchanged.
No drawing, movie/jammed mode, or availability producer equivalence is claimed.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_EBX,UC_X86_REG_EBP,UC_X86_REG_ESI,UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, run_checked, SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC
from tools.sidebar_oracle.geometry import machine,put32,read

def generate():
 cases=[]
 for phase,frame in [(0,0),(1,32),(2,32),(2,1),(2,0),(3,0),(3,1),(3,31)]:
  for last,duration in [(0,0),(0,4),(100,4),(-1,0),(-1,4)]:
   for wall_ms in [0,15,16,47,48,63,64,65,1599,1600,1647,1648,1663,1664,100000]:
    u=machine()
    for offset,value in [(0x14AC,phase),(0x14FC,frame),(0x1500,last),(0x1508,duration)]:put32(u,SCRATCH+offset,value)
    # Explicit OS time input; no original executable instruction is patched.
    clock=RET_MAGIC+256;u.mem_write(clock,b'\xB8'+struct.pack('<I',wall_ms)+b'\xC3');put32(u,0x7E1530,clock)
    u.reg_write(UC_X86_REG_ESI,SCRATCH);u.reg_write(UC_X86_REG_EBP,1);u.reg_write(UC_X86_REG_EBX,0)
    u.reg_write(UC_X86_REG_ESP,STACK_BASE+STACK_SIZE-4096)
    run_checked(u,0x6531DB,(0x6532AA,0x65336D))
    cases.append(dict(input=[phase,frame,last,duration,wall_ms],output=[read(u,SCRATCH+0x14AC)[0],read(u,SCRATCH+0x14FC)[0],read(u,SCRATCH+0x1500)[0],read(u,SCRATCH+0x1508)[0],u.mem_read(SCRATCH+0x14DA,1)[0]]))
 return dict(cases=cases)

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
  scope='600 staged ordinary housing timer cases: phases0/1/2/3, due/notdue/stopped timers, exact0/32 endpoints, bucket edges and a long draw stall.',
  assumptions=['Original RadarDraw register EBP=1, ESI points at explicit phase/frame/timer input.', 'SoundEvent release has no attached live audio handle.', 'Availability/mode transitions and visual draws are outside this branch fixture.'],
  substitutions=['Imported timeGetTime returns the explicit wall_ms input; native6C8C40 SHR4 executes unchanged.'],
  entry_points={'housing_timer':0x6531DB,'wall_clock_bucket':0x6C8C40,'endpoint_sound_release':0x406060}))
