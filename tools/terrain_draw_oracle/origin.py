"""Original ObjectClass dirty-rectangle rebasing before Terrain DrawIt.

The projected point and viewport/dirty rectangles are explicit prepared inputs.
Execution stops immediately before the original virtual DrawIt call; no
projection, surface allocation or complete scene is claimed by this region.
"""
import argparse
import json
import struct
from pathlib import Path
from tools.native_oracle import (
    image_sha256, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE,
    Uc, UC_ARCH_X86, UC_MODE_32, load_image, run_checked,
)
from unicorn.x86_const import UC_X86_REG_ESP


def generate():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    sp, obj, clip = STACK_BASE+0x4000, SCRATCH, SCRATCH+0x1000

    def put(address, *values):
        u.mem_write(address, struct.pack('<'+'I'*len(values),
                                       *(v & 0xffffffff for v in values)))

    def read(address, count=1):
        return list(struct.unpack('<'+'i'*count,u.mem_read(address,count*4)))

    assert read(0x7f522c+0x114) == [0x71c1b0]
    put(obj,0x7f522c)
    cases=[]
    for viewport_y in (0,37):
        for dirty_y in (viewport_y,100,350):
            u.mem_write(sp-32,bytes(160))
            put(0x886fa0,0,viewport_y,632,568)
            put(sp+8,obj)
            put(sp+0xc,376,404-viewport_y)
            put(sp+0x28,clip)
            put(clip,0,dirty_y,632,600-dirty_y)
            u.reg_write(UC_X86_REG_ESP,sp)
            run_checked(u,0x5f4b88,0x5f4cfd,required_addresses=(0x5f4cb6,0x5f4cf9))
            point_ptr,clip_ptr=read(u.reg_read(UC_X86_REG_ESP),2)
            point,actual_clip=read(point_ptr,2),read(clip_ptr,4)
            assert point[1]+actual_clip[1] == 404
            cases.append({'viewport_y':viewport_y,'dirty_y':dirty_y,
                          'projected_viewport_point':[376,404-viewport_y],
                          'draw_point':point,'clip':actual_clip,
                          'framebuffer_y':point[1]+actual_clip[1]})
    return {'executable_sha256':image_sha256(),'region':'005F4B88..005F4CFD',
            'virtual_target':'0071C1B0','cases':cases,
            'limit':'Projected point supplied; original dirty rebasing runs before the Terrain virtual call. Effective native row is framebuffer top minus viewport Y, not minus dirty-clip Y.'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    result=generate()
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print('Original dirty-clip point rebasing: 6 cases retain framebuffer Y404')
