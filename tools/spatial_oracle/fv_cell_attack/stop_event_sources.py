"""Pinned original Stop-command producer, Event6 dispatch and receiver instructions."""
from pathlib import Path
import hashlib,struct
import capstone
from capstone import Cs,CS_ARCH_X86,CS_MODE_32
from tools.native_oracle import image_bytes,file_span,finish_vectors,provenance
HERE=Path(__file__).resolve().parent
SPANS=(
    ('stop_command',0x730EA0,0x730F21),
    ('unit_event_enqueue',0x6FFE00,0x6FFEB7),
    ('target_constructor',0x6E6AB0,0x6E6B16),
    ('event_constructor',0x4C65E0,0x4C6647),
    ('event_execute',0x4C6CB0,0x4C8114),
)

def generate():
    image=image_bytes();dis=Cs(CS_ARCH_X86,CS_MODE_32);bodies=[]
    for name,start,end in SPANS:
        offset,raw=file_span(image,start,end-start)
        instructions=[]
        for ins in dis.disasm(raw,start):
            if name=='event_execute' and not (ins.address<0x4C6CDE or 0x4C74CB<=ins.address<0x4C76BC):
                continue
            instructions.append(dict(address=f'0x{ins.address:08x}',bytes=ins.bytes.hex(),mnemonic=ins.mnemonic,operands=ins.op_str))
        bodies.append(dict(name=name,start=f'0x{start:08x}',end=f'0x{end:08x}',file_offset=offset,
            sha256=hashlib.sha256(raw).hexdigest(),raw=raw.hex(),instructions=instructions))
    vt=0x7F5C70;slots={f'0x{off:x}':f'0x{struct.unpack("<I",file_span(image,vt+off,4)[1])[0]:08x}' for off in (0x374,0x280,0x480,0x3C8,0x1E8,0x1EC)}
    branch=struct.unpack('<I',file_span(image,0x4C8114+(6-1)*4,4)[1])[0]
    assert branch==0x4C74CB
    return dict(schema=1,bodies=bodies,unit_vtable=f'0x{vt:08x}',unit_slots=slots,
                event6_switch=dict(table='0x004c8114',index=5,destination=f'0x{branch:08x}'))

def metadata():
    result=provenance(scope=__doc__,entry_points={name:start for name,start,_ in SPANS},
        assumptions=['All bytes come from verified active-retail file-backed PE spans. The Event body is disassembled from its actual entry, then only entry and reached Event6 instructions are selected. Unit vtable slots and Event switch entries are original bytes.'],
        substitutions=['Static original byte/disassembly evidence only; does not claim caller runtime admission or outcomes. Conditional paid Stop execution separately supplies those results.'])
    result.update(capstone_version=capstone.__version__)
    return result

if __name__=='__main__':
    finish_vectors(generate,HERE/'stop_event_sources.json',provenance=metadata)
