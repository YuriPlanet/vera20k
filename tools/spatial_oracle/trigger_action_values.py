"""Original TAction constructor/read -> global/local action dispatch.

Pins +90 materialization independently of ActionID and the four +34..40
parameters. Resolved-name parameter types6/7/8 are outside this numeric corpus.
"""
from pathlib import Path
import struct

from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.trigger_type_flags import FlagsFixture
from tools.spatial_oracle.infantry_deploy_rules import HEAP, PTD
from tools.spatial_oracle.map_queries import dwords

ACTION, BUFFER, SCENARIO = HEAP+0x1000, HEAP+0x2000, HEAP+0x20000


def execute(raw):
    f = FlagsFixture()
    u = f.u
    # Original full constructor publishes into these three initialized vectors.
    for n, base in enumerate((0xB0E658,0xB0F670,0xB0F658)):
        u.mem_write(base+4,dwords(HEAP+0x8000+n*0x100,16,0,0,16))
    u.mem_write(0xA8B230,dwords(SCENARIO))
    u.mem_write(PTD,bytes(0x74))
    u.mem_write(ACTION,b'\xA5'*0x94)
    ranges=[(0x6DD000,0x6DD17B),(0x6DD5B0,0x6DD87D),(0x6DD8B0,0x6E45D0)]
    code=[bytes(u.mem_read(a,b-a)) for a,b in ranges]
    f.call(0x6DD000,[],ecx=ACTION)
    # The caller consumes the count, leaving the original CRT tokenizer cursor
    # positioned before ActionID. The following full Read consumes all tokens.
    u.mem_write(BUFFER,('1,'+raw).encode('ascii')+b'\0')
    f.call(0x7C9CC2,[BUFFER,0x817F70],cdecl=True)
    f.call(0x6DD5B0,[],ecx=ACTION)
    kind=struct.unpack('<i',u.mem_read(ACTION+0x2C,4))[0]
    initial_globals=[0,7,49] if kind in (29,57) else []
    initial_locals=[0,7,49,99] if kind in (29,57) else []
    for index in initial_globals:
        u.mem_write(SCENARIO+0x1CB0+index*41,b'\1')
    for index in initial_locals:
        u.mem_write(SCENARIO+0x24B2+index*41,b'\1')
    returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
    assert code==[bytes(u.mem_read(a,b-a)) for a,b in ranges]
    def integer(offset): return struct.unpack('<i',u.mem_read(ACTION+offset,4))[0]
    return dict(raw=raw,kind=kind,value=integer(0x90),initial_globals=initial_globals,initial_locals=initial_locals,
                args=[integer(n) for n in (0x34,0x38,0x3C,0x40)],waypoint=integer(0x44),
                returned=returned,globals=[n for n in range(50) if u.mem_read(SCENARIO+0x1CB0+n*41,1)[0]],
                locals=[n for n in range(100) if u.mem_read(SCENARIO+0x24B2+n*41,1)[0]],
                changed=u.mem_read(SCENARIO+0x34AA,1)[0])


def generate():
    rows=[f'{kind},0,{value},41,42,43,44,Z' for kind in (28,29,56,57)
          for value in ('0','7','49','50','99','100','-1','4294967303','  +7tail','junk')]
    rows += [f'28,{typ},{value},41,42,43,44,{last}' for typ,value,last in
             [(11,'7','8'),(9,'text','7'),(9,'text','-1'),(5,'-1','7'),
              (5,'-1',''),(9,'text',''),(12,'7','8'),(-1,'7','8'),
              (1,'-1','Z'),(2,'-1','Z'),(3,'-1','Z'),(4,'text','Z'),(10,'text','7')]]
    rows += ['28,0,7,41,42,43,44', '28,0,7,41,42,43,44,',
             '28,,0,,7,,41,42,43,44,Z', '28,0, ,41,42,43,44,Z',
             '28,0,\t-2,41,42,43,44,Z', '28,0,\v7,41,42,43,44,Z',
             '28,0,2147483648,41,42,43,44,Z','28,0,-9223372036854775808,41,42,43,44,Z']
    return [execute(raw) for raw in rows]


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'constructor':0x6DD000,'read':0x6DD5B0,'execute':0x6DD8B0,
                      'set_global':0x689670,'set_local':0x689910},
        assumptions=['Supplied counted single-action string, initialized Scenario and empty live Tag registry.',
                     'Original full constructor, strtok, atoi, Read, action dispatch and Scenario variable writes execute.',
                     'Parameter types6/7/8 with named sound/theme/speech lookup, nonnull definition references and map ReadString truncation are excluded.'],
        substitutions=['Inherited fixture supplies GetLastError/SetLastError/TlsGetValue and unused heap/lock imports; original gameplay instructions and vtables are unchanged.'],
        scope='Numeric parameter materialization and global/local variable index bounds. Live timer-reset fanout is covered separately by tag_lifecycle, not by this empty-registry fixture.',
    ))
