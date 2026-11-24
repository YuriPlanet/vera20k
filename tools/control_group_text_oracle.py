"""Native oracle for the selected-unit control-group text tail."""
from pathlib import Path
import argparse
import hashlib
import json
import struct
u32=lambda x: struct.pack('<I', x & 0xffffffff)
def wide(s): return s.encode('utf-16le')+b'\0\0'
def readwide(e,p):
    u=e.uc if hasattr(e,'uc') else e.u
    chars=[]
    for i in range(128):
        c=struct.unpack('<H',u.mem_read(p+2*i,2))[0]
        if not c: break
        chars.append(chr(c))
    return ''.join(chars)
def dwords(e,p,n=4):
    u=e.uc if hasattr(e,'uc') else e.u
    return list(struct.unpack('<'+'i'*n,u.mem_read(p,n*4)))


def format_digit(m, arg):
    """The existing bounded CRT %d transport, shared by both font callers."""
    u=m.uc if hasattr(m,'uc') else m.u
    dest,fmt,value=(arg(i) for i in range(3))
    if readwide(m,fmt)!='%d':raise RuntimeError('unexpected group format')
    value=struct.unpack('<i',u32(value))[0]
    rendered=str(value)
    u.mem_write(dest,wide(rendered))
    return value,rendered

def initialize_font(raw, *, owner=None):
    """Load the physical FNT through the one existing native font owner.

    An optional already initialized PaletteReader supplies the shared VM,
    allocation and CRT TLS owner for status composition. Legacy callers retain
    their historical Emu/addresses and submission payload exactly. No font
    decoder or glyph raster is supplied by this transport.
    """
    from tools.ai_base_building_oracle import Emu,FAKE,STUBS
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import (UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP)
    legacy=owner is None
    m=Emu() if legacy else owner
    u=m.uc if legacy else m.u
    def put(p,value):u.mem_write(p,u32(value))
    def arg(index):return m.read32(u.reg_read(UC_X86_REG_ESP)+4+index*4)
    renderer=FAKE+0x20000 if legacy else m.alloc(0x100)
    FILE_VT=FAKE+0x24000 if legacy else m.alloc(0x100)
    transport=STUBS+0x200 if legacy else m.alloc(0x100)
    FILE_STUBS={'close':transport,'open':transport+4,'size':transport+8,
                'read':transport+12,'seek':transport+16,'lock':transport+20}
    for off,name in [(0x00,'close'),(0x14,'open'),(0x1c,'size'),(0x24,'read'),(0x28,'seek')]:
        put(FILE_VT+off,FILE_STUBS[name])
    loader={'position':0,'reads':[],'seeks':[],'path':None,'open_modes':[],'close_flags':[]}
    def readnarrow(e,p):
        out=bytearray()
        for i in range(128):
            b=u.mem_read(p+i,1)[0]
            if b==0: break
            out.append(b)
        return out.decode('ascii')
    def file_ctor():
        this=u.reg_read(UC_X86_REG_ECX); pathptr=arg(0)
        try: path=readnarrow(m,pathptr)
        except Exception as err:
            raise RuntimeError(f'font file constructor this=0x{this:08x} pathptr=0x{pathptr:08x} sp=0x{u.reg_read(UC_X86_REG_ESP):08x}: {err}')
        if path.upper()!='GAME.FNT': raise RuntimeError(f'unexpected native font resource {path!r}')
        loader['path']=path;loader['position']=0;put(this,FILE_VT);return this
    def file_close():
        loader['close_flags'].append(arg(0));return 1
    def file_open():
        mode=arg(0);loader['open_modes'].append(mode);loader['position']=0;return 1
    def file_size(): return len(raw)
    def file_read():
        dest,count=arg(0),arg(1);start=loader['position'];end=start+count
        if end>len(raw): raise RuntimeError(f'native read past GAME.FNT: offset=0x{start:x} size=0x{count:x}')
        u.mem_write(dest,raw[start:end]);loader['position']=end
        loader['reads'].append({'file_offset':start,'size':count,'destination':hex(dest)})
        return count
    def file_seek():
        offset=struct.unpack('<i',u32(arg(0)))[0];origin=arg(1)
        newpos=offset if origin==0 else loader['position']+offset if origin==1 else len(raw)+offset
        if not 0<=newpos<=len(raw): raise RuntimeError(f'invalid native font seek offset={offset} origin={origin}')
        loader['seeks'].append({'offset':offset,'origin':origin,'position':newpos});loader['position']=newpos;return newpos
    callbacks=[(0x4739f0,file_ctor,4),(FILE_STUBS['close'],file_close,4),
        (FILE_STUBS['open'],file_open,4),(FILE_STUBS['size'],file_size,4),
        (FILE_STUBS['read'],file_read,8),(FILE_STUBS['seek'],file_seek,8),
        (0x7c93e8,lambda:None,0)]
    if legacy:
        callbacks += [(FILE_STUBS['lock'],lambda:None,0x14),
                      (0x7c9430,lambda:m.operator_new(m),0)]
    else:
        # Win32 SetRect is platform transport, not a glyph/raster substitute.
        def set_rect():
            u.mem_write(arg(0),b''.join(u32(arg(i)) for i in range(1,5)))
            return 1
        callbacks.append((FILE_STUBS['lock'],set_rect,20))
    hooks=[]
    for address,callback,pops in callbacks:
        if legacy:
            m.hook(address,lambda _e,fn=callback:fn(),pops)
        else:
            def transport_call(_u,_pc,_size,_data,fn=callback,clean=pops):
                value=fn()
                m.ret(0 if value is None else value,clean)
            hooks.append(u.hook_add(UC_HOOK_CODE,transport_call,begin=address,end=address))
    put(0x7e14b4,FILE_STUBS['lock'])
    required=(0x433880,0x433990,0x433a3f,0x433a46,0x433b84,0x433bba,0x433bbf,0x433bee,0x434700)
    seen=set()
    trace=u.hook_add(UC_HOOK_CODE,lambda _u,pc,_n,_d:seen.add(pc))
    renderer=(m.invoke(0x434ad0,ecx=renderer,required=required) if legacy
              else m.invoke(0x434ad0,renderer))
    u.hook_del(trace)
    assert set(required)<=seen
    # The actual caller at 0x52BB95 stores this constructor result in the renderer global.
    put(0x89c4b8,renderer)
    font=m.read32(0x89c4d0);data=m.read32(font+4)
    font_meta={'font_object':hex(font),'renderer_object':hex(renderer),'font_data':hex(data),
        'height':m.read32(font+0x1c),'bytes_per_row':m.read32(data+4),'bitmap_rows':m.read32(data+8),
        'glyph_slots':m.read32(data+0x10),'glyph_stride':m.read32(data+0x14),
        'lookup':hex(m.read32(data+0x18)),'glyphs':hex(m.read32(data+0x1c))}
    lookup=m.read32(data+0x18);glyphs=m.read32(data+0x1c);stride=m.read32(data+0x14)
    if u.mem_read(lookup,0x20000)!=raw[0x1c:0x2001c]: raise RuntimeError('original loader lookup bytes differ from retail GAME.FNT file range')
    glyph_bytes=m.read32(data+0x10)*stride
    if u.mem_read(glyphs,glyph_bytes)!=raw[0x2001c:0x2001c+glyph_bytes]: raise RuntimeError('original loader glyph bytes differ from retail GAME.FNT file range')
    expected_reads=[(0,0x1c),(0x1c,0x20000),(0x2001c,0x162c27)]
    actual_reads=[(r['file_offset'],r['size']) for r in loader['reads']]
    if actual_reads!=expected_reads or loader['position']!=len(raw):
        raise RuntimeError(f'original loader read ranges differ: {actual_reads}, final={loader["position"]}, length={len(raw)}')
    native_digit_metrics=[]
    for digit in range(10):
        code=0x30+digit;index=struct.unpack('<H',u.mem_read(lookup+code*2,2))[0]
        glyph=glyphs+(index-1)*stride if index else 0;width=u.mem_read(glyph,1)[0] if glyph else 0
        native_digit_metrics.append({'digit':digit,'lookup_index':index,'glyph_width':width,'advance':width+m.read32(font+0x2c)})
    if loader['path']!='GAME.FNT' or loader['reads'][-1]['file_offset']!=0x2001c:
        raise RuntimeError(f'original loader did not read the retail glyph block from +0x2001c: {loader}')
    return dict(owner=m,legacy=legacy,hooks=hooks,renderer=renderer,font=font,
        font_meta=font_meta,loader=loader,expected_reads=expected_reads,
        native_digit_metrics=native_digit_metrics)


def install_digit_format(m, *, native=False):
    """Share the format boundary, optionally executing the complete CRT body.

    Legacy digit callers retain their bounded transport. Power labels need two
    integer substitutions: their native option supplies only the existing
    single-thread Win32 atomic imports and leaves7CA564/7D21BE unchanged.
    """
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP
    if native:
        from types import SimpleNamespace
        from tools.spatial_oracle.infantry_deploy_action import Fixture as AtomicTransportOwner
        sinks=[m.alloc(16),m.alloc(16)]
        atomic=SimpleNamespace(u=m.u,imports=sinks,read=m.read32)
        m.u.mem_write(0x7E11C8,b''.join(u32(p) for p in sinks))
        def atomic_import(u,pc,size,data):
            if pc in sinks:AtomicTransportOwner.observe(atomic,u,pc,size,data)
        return m.u.hook_add(UC_HOOK_CODE,atomic_import)
    def formatted(_u,_pc,_size,_data):
        sp=m.u.reg_read(UC_X86_REG_ESP)
        _value,rendered=format_digit(m,lambda i:m.read32(sp+4+i*4))
        m.ret(len(rendered))
    return m.u.hook_add(UC_HOOK_CODE,formatted,begin=0x7CA564,end=0x7CA564)


class TextRecorder:
    """Observe original label calls, reusing the status recorder's color stores.

    This adds no pixel writer or raster model. The original BSurface fill,
    outline and glyph functions execute to their actual returns.
    """
    def __init__(self,m,status,size,*,direct_simple_text=False):
        from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_READ,UC_HOOK_MEM_WRITE
        from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_EDX,UC_X86_REG_ECX
        from tools.projectile_oracle.bridge_render_shape import A,Z
        self.m,self.status,self.rows,self.entries=m,status,[],[]
        self.active=None
        u=m.u
        def finish():
            if self.active is not None:
                row,stage,start,ret=self.active
                row['stages'][-1]['color_stores']=len(status.stores)-start
                if stage=='glyph':
                    row['glyph_stores']=status.stores[start:].copy()
                    if '_simple_result' in row:
                        row['result_position']=dwords(m,row.pop('_simple_result'),2)
                self.active=None
        def observe(_u,pc,_n,_d):
            if self.active is not None and pc==self.active[3]:finish()
            if pc in (0x70A6FD,0x4A66D0,0x4A5EB0,0x434B90,0x434500,0x434120):
                self.entries.append(dict(actor=status.actor_index,entry=f'{pc:08X}'))
            # PRIMARY calls SimpleText4A5EB0 directly. A group4A66D0
            # wrapper already owns its nested SimpleText interval; do not
            # observe it twice or change any historical/default payload.
            simple=direct_simple_text and pc==0x4A5EB0 and self.active is None
            if pc not in (0x7BB020,0x7BAD90,0x4A66D0) and not simple:return
            sp=u.reg_read(UC_X86_REG_ESP)
            args=[m.read32(sp+4+i*4) for i in range(7 if simple else 8 if pc==0x4A66D0 else 2)]
            if pc==0x7BB020:
                row=dict(actor=status.actor_index,rect=dwords(m,args[0]),
                         fill_word=args[1],stages=[])
                self.rows.append(row)
                stage='fill'
            else:
                row=self.rows[-1]
                if pc==0x7BAD90:
                    row.update(outline_rect=dwords(m,args[0]),outline_word=args[1])
                    stage='outline'
                else:
                    row.update(text=readwide(m,u.reg_read(UC_X86_REG_EDX)),
                        position=dwords(m,args[2],2),clip=dwords(m,args[1]),
                        packed_owner_rgb=args[3],flags=args[5])
                    if simple:
                        row.update(draw_mode=args[6],_simple_result=u.reg_read(UC_X86_REG_ECX))
                    else:
                        row.update(color_mode=args[6],draw_mode=args[7])
                    stage='glyph'
            row['stages'].append(dict(stage=stage,entry=f'{pc:08X}',a_reads=0,z_reads=0,
                                     a_writes=0,z_writes=0))
            self.active=(row,stage,len(status.stores),m.read32(sp))
        def access(write,address):
            if self.active is None:return
            plane='a' if A<=address<A+size[0]*size[1]*2 else 'z' if Z<=address<Z+size[0]*size[1]*2 else None
            if plane:self.active[0]['stages'][-1][plane+('_writes' if write else '_reads')]+=1
        self.finish=finish
        self.hooks=[u.hook_add(UC_HOOK_CODE,observe),
            u.hook_add(UC_HOOK_MEM_READ,lambda _u,_a,p,_n,_v,_d:access(False,p)),
            u.hook_add(UC_HOOK_MEM_WRITE,lambda _u,_a,p,_n,_v,_d:access(True,p))]

    def close(self):
        self.finish()
        for hook in self.hooks:self.m.u.hook_del(hook)


def apply_group_commands(m,actor,house,commands,*,mode=1):
    """Original assignment/clear, with one admitted current actor array prior."""
    u=m.u
    table=m.alloc(4);u.mem_write(table,u32(actor))
    u.mem_write(0xA8EC7C,u32(table));u.mem_write(0xA8EC88,u32(1))
    u.mem_write(0xA83D4C,u32(house));u.mem_write(0xA8B238,u32(mode))
    u.mem_write(house+0x1ED,b'\1')
    events=[]
    draw_selected=bytes(u.mem_read(actor+0x83,1))
    for command in commands:
        if 'selected' in command:u.mem_write(actor+0x83,bytes([command['selected']]))
        entry={'assign':0x731060,'clear':0x7310D0}[command['operation']]
        before=struct.unpack('<i',u.mem_read(actor+0x214,4))[0]
        m.invoke(entry,command['group'])
        events.append(dict(command=command,entry=f'{entry:08X}',before=before,
                           after=struct.unpack('<i',u.mem_read(actor+0x214,4))[0]))
    u.mem_write(actor+0x83,draw_selected)
    return events


def assignment_controls(m,template):
    """Existing actor constructor prior, real731060/7310D0 across house gates."""
    from unicorn import UC_HOOK_CODE
    u=m.u
    houses=[m.alloc(0x18000) for _ in range(2)]
    actors=[m.alloc(len(template)) for _ in range(3)]
    table=m.alloc(12)
    u.mem_write(table,b''.join(u32(p) for p in actors))
    def actor(slot,selected,owner,player_control=True):
        return dict(group_slot=slot,selected=selected,owner=owner,player_control=player_control)
    cases=[dict(name='assign_reassign_clear',mode=1,actors=[actor(-1,True,0),actor(0,False,0),actor(9,True,0)],
                operations=[dict(operation='assign',group=1),dict(operation='assign',group=10),dict(operation='clear',group=10)]),
           dict(name='clear_latent_other_owner',mode=1,actors=[actor(0,False,0),actor(0,False,1),actor(9,True,0)],
                operations=[dict(operation='clear',group=1)]),
           dict(name='mode_zero_player_control',mode=0,actors=[actor(0,False,0),actor(0,True,1,False),actor(-1,True,0)],
                operations=[dict(operation='assign',group=1),dict(operation='clear',group=1)])]
    rows=[]
    for case in cases:
        u.mem_write(0xA8EC7C,u32(table));u.mem_write(0xA8EC88,u32(len(actors)))
        u.mem_write(0xA83D4C,u32(houses[0]));u.mem_write(0xA8B238,u32(case['mode']))
        for p,supplied in zip(actors,case['actors']):
            u.mem_write(p,template);u.mem_write(p+0x214,u32(supplied['group_slot']))
            u.mem_write(p+0x83,bytes([supplied['selected']]))
            owner=houses[supplied['owner']]
            u.mem_write(p+0x21C,u32(owner));u.mem_write(owner+0x1ED,bytes([supplied['player_control']]))
        steps=[];rng=[]
        hook=u.hook_add(UC_HOOK_CODE,lambda _u,pc,_n,_d:rng.append(pc) if pc in (0x65C780,0x65C7E0) else None)
        for command in case['operations']:
            before=[struct.unpack('<i',u.mem_read(p+0x214,4))[0] for p in actors]
            timers=[bytes(u.mem_read(p+0x174,0x90)) for p in actors]
            entry={'assign':0x731060,'clear':0x7310D0}[command['operation']]
            m.invoke(entry,command['group'])
            steps.append(dict(command=command,entry=f'{entry:08X}',before=before,
                after=[struct.unpack('<i',u.mem_read(p+0x214,4))[0] for p in actors],
                timers_unchanged=timers==[bytes(u.mem_read(p+0x174,0x90)) for p in actors]))
        u.hook_del(hook)
        assert not rng and all(row['timers_unchanged'] for row in steps)
        rows.append(dict(name=case['name'],input=case,steps=steps,rng_entries=rng))
    return rows


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--font', required=True, help='Extracted retail GAME.FNT (ra2.mix -> local.mix)')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--write', action='store_true', help='Replace the checked-in native corpus')
    mode.add_argument('--check', action='store_true', help='Compare the native run with the checked-in corpus')
    return parser.parse_args(argv)


def main(argv=None):
    options = arguments(argv)
    # Delay native/oracle dependencies until after argparse handles --help. This
    # keeps importing the tool and asking for usage independent of retail assets
    # and optional native-emulation packages.
    from tools.ai_base_building_oracle import FAKE, STUBS
    from tools.native_oracle import (run_checked, RET_MAGIC,
                                     STACK_BASE, STACK_SIZE, NATIVE_FPCW)
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import (UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_EAX,
                                   UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP,
                                   UC_X86_REG_FPCW)
    font_path=Path(options.font)
    raw=font_path.read_bytes()
    if len(raw)!=1584195: raise RuntimeError(f'unexpected retail GAME.FNT length: {len(raw)}')
    font_sha256=hashlib.sha256(raw).hexdigest()
    house_color_path=Path(__file__).with_name('procedural_drawing_oracle')/'house_color.json'
    house_color_corpus=json.loads(house_color_path.read_text(encoding='utf-8'))
    gold_rows=[row for row in house_color_corpus['colors'] if row.get('name')=='Gold']
    if len(gold_rows)!=1: raise RuntimeError(f'expected one Gold entry in {house_color_path}')
    house_rgb_input=gold_rows[0]['modes']['scalar']['rgb']
    initialized=initialize_font(raw)
    m=initialized['owner'];u=m.uc
    renderer,font,font_meta,loader,expected_reads,native_digit_metrics=(initialized[k] for k in
        ('renderer','font','font_meta','loader','expected_reads','native_digit_metrics'))
    obj=FAKE+0x28000;vt=FAKE+0x29000;house=FAKE+0x30000
    surface=FAKE+0x31000;svt=FAKE+0x32000;anchor=FAKE+0x33000;clip=FAKE+0x33100
    for a,v in [(obj,vt),(obj+0x21c,house),
     (vt+0x2c,STUBS+0x100),(0x887314,surface),(surface,svt),(svt+0x14,STUBS+0x110),(svt+0x58,STUBS+0x120)]: m.write32(a,v)
    # Feed stock Gold's native House+0x56F9 result through the original text tail.
    u.mem_write(house+0x56f9,bytes(house_rgb_input))
    # The actual renderer format is an input to this bounded presentation fixture: RGB565.
    u.mem_write(0x8a0dd4,b'\x03');m.write32(0x8a0dd0,11)
    u.mem_write(0x8a0de4,b'\x02');m.write32(0x8a0de0,5)
    u.mem_write(0x8a0ddc,b'\x03');m.write32(0x8a0dd8,0)
    state={'kind':1,'events':[],'frame':0}
    def kind_stub(e): return state['kind']
    def swprintf(e):
        value,rendered=format_digit(e,e.arg)
        state['events'].append(['format',value,rendered]);return len(rendered)
    def fill(e):
        p=e.arg(0);state['events'].append(['fill',dwords(e,p),e.arg(1)]);return 1
    def surface_draw(e):
        p=e.arg(0);state['events'].append(['surface_draw',dwords(e,p),e.arg(1)]);return 1
    def text_consumer(e):
        args=[e.arg(i) for i in range(8)];ecx=e.uc.reg_read(UC_X86_REG_ECX);edx=e.uc.reg_read(UC_X86_REG_EDX)
        row={'text':readwide(e,edx),'text_ptr':hex(edx),'ecx_this':hex(ecx),'stack_args':[hex(v) for v in args],
             'clip':dwords(e,args[1]),'arg2_data':dwords(e,args[2]),'packed_owner_rgb':hex(args[3]),
             'arg4':args[4],'flags':hex(args[5]),'color_mode':args[6],'draw_mode':args[7]}
        state['events'].append(['text_draw_consumer',row]);return 0
    def after_measure(uc,address,size,userdata):
        if address==0x70a77b:
            p=uc.reg_read(UC_X86_REG_EAX);state['events'].append(['measure_rect',dwords(type('E',(),{'uc':uc})(),p)])
    def after_font_measure(uc,address,size,userdata):
        if address==0x4a59f6:
            state['events'].append(['font_measure',uc.reg_read(UC_X86_REG_EAX)])
    # Existing fixture owns only these non-game edges: format CRT and renderer submission / virtual surface.
    m.hook(STUBS+0x100,kind_stub,0);m.hook(0x7ca564,swprintf,0)
    m.hook(STUBS+0x110,fill,8);m.hook(STUBS+0x120,surface_draw,8);m.hook(0x4a66d0,text_consumer,0x20)
    u.hook_add(UC_HOOK_CODE,after_measure);u.hook_add(UC_HOOK_CODE,after_font_measure)
    cases=[{'slot':i,'kind':1,'clip':[0,0,640,480],'label':'full','caller_point':[320,200]} for i in (0,9,1,8)]
    cases += [{'slot':0,'kind':1,'clip':[305,177,312,185],'label':'partial','caller_point':[320,200]},
              {'slot':0,'kind':1,'clip':[0,0,100,100],'label':'outside','caller_point':[320,200]},
              {'slot':0,'kind':0xF,'clip':[0,0,640,480],'label':'infantry','caller_point':[320,200]}]
    # Aircraft WhatAmI 2 takes the same non-building caller and shared tail as
    # units; keep both registered endpoint digits explicit in the native corpus.
    cases += [
        {'slot':0,'kind':2,'clip':[0,0,640,480],'label':'aircraft_1','caller_point':[320,200]},
        {'slot':9,'kind':2,'clip':[0,0,640,480],'label':'aircraft_0','caller_point':[320,200]},
    ]
    # These two caller points come from the aligned native DrawHealthBar
    # Building arm at 0x6F64A0. Its BuildingTypeClass::Dimension2 slot at
    # 0x464AF0 returns the retail dimensions below; the original projection
    # helper at 0x6D1F10 produced the recorded half-foundation offsets before
    # the shared 0x709A90 tail. The tail itself still runs natively here.
    cases += [
        {'slot':0,'kind':6,'clip':[0,0,640,480],'label':'building_2x2',
         'caller_point':[320,200],'tail_input_point':[260,200],
         'type_name':'YAPOWR','foundation':'2x2',
         'native_dimension2':[512,512,416],
         'native_projection_input':[-256,256,0],
         'native_projected_offset':[-60,0]},
        {'slot':9,'kind':6,'clip':[0,0,640,480],'label':'building_4x4',
         'caller_point':[320,200],'tail_input_point':[200,200],
         'type_name':'GACNST','foundation':'4x4',
         'native_dimension2':[1024,1024,416],
         'native_projection_input':[-512,512,0],
         'native_projected_offset':[-120,0]},
    ]
    rows=[]
    for case in cases:
        state['kind']=case['kind'];state['events']=[];m.write32(obj+0x214,case['slot'])
        # The regular non-building caller, including Aircraft, supplies
        # caller_point + {-10,+10}.
        # The native Building arm supplies its projected foundation point
        # directly, captured separately as tail_input_point.
        caller_x,caller_y=case['caller_point']
        tail_x,tail_y=case.get('tail_input_point',(caller_x-10,caller_y+10))
        u.mem_write(anchor,struct.pack('<ii',tail_x,tail_y));u.mem_write(clip,struct.pack('<4i',*case['clip']))
        frame=STACK_BASE+STACK_SIZE-0x10000;state['frame']=frame
        u.mem_write(frame,struct.pack('<4I',0x11111111,0x22222222,0x33333333,0x44444444)+bytes(0x60))
        u.mem_write(frame+0x74,u32(RET_MAGIC));u.mem_write(frame+0x78,u32(anchor));u.mem_write(frame+0x7c,u32(0));u.mem_write(frame+0x80,u32(clip))
        u.reg_write(UC_X86_REG_ESP,frame);u.reg_write(UC_X86_REG_EBP,obj);u.reg_write(UC_X86_REG_EBX,anchor);u.reg_write(UC_X86_REG_FPCW,NATIVE_FPCW)
        run_checked(u,0x70a6fd,RET_MAGIC,count=500000,required_addresses=(0x70a776,0x70a97b),
          context={'slot':case['slot'],'kind':case['kind'],'clip':case['clip'],'fixture':'tail 70A6FD with original font measure/clipping; final renderer submission stub'})
        rows.append({'case':case,'events':state['events']})
    def event(row,name): return next(e for e in row['events'] if e[0]==name)
    corpus_rows=[]
    for row in rows:
        if row['case']['label']=='full' and row['case']['slot'] not in (0,9,1,8): continue
        fmt=event(row,'format');measure=event(row,'font_measure');measure_rect=event(row,'measure_rect')
        fill_event=event(row,'fill');surface_event=event(row,'surface_draw');submit=event(row,'text_draw_consumer')[1]
        corpus_rows.append({
            'case':row['case'],'text':fmt[2],'font_measure':measure[1],
            'measure_rect':measure_rect[1],
            'background_fill':{'rect':fill_event[1],'color_arg':fill_event[2]},
            'surface_draw':{'rect':surface_event[1],'color_arg':surface_event[2]},
            'text_submission':{
                'clip':submit['clip'],'position':submit['arg2_data'][:2],
                'packed_owner_rgb':submit['packed_owner_rgb'],'arg4':submit['arg4'],
                'flags':submit['flags'],'color_mode':submit['color_mode'],'draw_mode':submit['draw_mode']}})
    payload={
        'schema':'control-group-tail-native-v1',
        'provenance':{
            'font':{'name':'GAME.FNT','resolution':'ra2.mix -> local.mix',
                    'file_length':len(raw),'file_length_hex':hex(len(raw)),
                    'sha256':font_sha256},
            'house_rgb_input':house_rgb_input,
            'house_rgb_input_source':'tools/procedural_drawing_oracle/house_color.json: Gold scalar House+0x56F9 RGB',
            'command':"$env:VERA20K_GAMEMD_EXE='<supported retail gamemd.exe>'; python -m tools.control_group_text_oracle --font '<extracted retail GAME.FNT>' --write",
            'substitutions':['native virtual file I/O returns the pinned retail GAME.FNT bytes','native allocator and lock edges are stubbed','CRT %d formatting is stubbed','virtual fill/surface and final 4A66D0 text renderer edges are submission captures/stubs'],
            'coverage_limit':'Original loader, font measurement, tail rect and clip path execute. Final renderer submission arguments are captured, but glyph raster output is not established.'},
        'native_font_initialization':{
            'native_chain':'434AD0 -> 433880 -> 433990 -> 434700; caller 52BB95 stores returned renderer at global 89C4B8',
            'renderer_global':'0x89c4b8','current_font_global':'0x89c4d0','resource':loader['path'],
            'native_file_reads':[{'file_offset':o,'file_offset_hex':hex(o),'size':n,'size_hex':hex(n)} for o,n in expected_reads],
            'runtime_fields':{'font_height':font_meta['height'],'font_bytes_per_row':font_meta['bytes_per_row'],
                'bitmap_rows':font_meta['bitmap_rows'],'glyph_slots':font_meta['glyph_slots'],
                'glyph_stride':font_meta['glyph_stride'],'char_spacing':m.read32(font+0x2c)},
            'digit_metrics':native_digit_metrics},
        'cases':corpus_rows}
    out=Path(__file__).with_name('control_group_text_native.json')
    rendered=json.dumps(payload,separators=(',',':'))+'\n'
    if options.write:
        out.write_text(rendered,encoding='utf-8')
    else:
        if not out.is_file():
            raise RuntimeError(f'missing native corpus: {out}')
        expected=out.read_text(encoding='utf-8')
        if rendered != expected:
            raise RuntimeError(f'native output differs from {out}; inspect before replacing it with --write')
    print(json.dumps({'corpus':str(out),'cases':len(corpus_rows),
        'font_sha256':payload['provenance']['font']['sha256'],'native_file_reads':payload['native_font_initialization']['native_file_reads'],
        'digit_advances':[x['advance'] for x in native_digit_metrics]},separators=(',',':')))




if __name__ == '__main__':
    main()
