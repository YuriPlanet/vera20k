"""Original saved-game resources plus full 60B7A0 ordinary child placement.

558DD0 selects B7/2B4/2B5 for its saved-game receiver modes 1/2/3. Active
60C0C0 enrolls their ordinary list/prompt/edit controls in 60B7A0. The sibling
oracle supplies bounded Win32 hooks and executes that complete original body.
"""

from pathlib import Path
import hashlib
import struct
import tempfile

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools import native_oracle as native
from tools.native_oracle import finish_vectors, load_image, provenance
from tools.storage_oracle.shell_relayout import Image, Machine, PARENT, muldiv
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.spatial_oracle.building_body_rules import INI, SP, dwords
from tools.spatial_oracle import native_id_snapshot as scenario_owner
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.storage_oracle.in_game_shell_geometry import native_child_case


STUB = 0x50000000
BATTLE_SHA256 = "7e40691a3480b1e5ffe6d1cdfccb83f67da4447157607ec3ecb5d2278acf0791"

RESOURCES = ((0xB7, 0xBEFCF8, 340, 6),
             (0x2B4, 0xC012F4, 364, 7),
             (0x2B5, 0xC01460, 348, 6))


def resource(dialog, address, size, expected_count, bounds=(533, 369)):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    data = bytes(uc.mem_read(address, size))
    style, exstyle, count, *actual_bounds = struct.unpack_from("<IIH4h", data)
    if (style, exstyle, count, actual_bounds) != (
            0x40000040, 0, expected_count, [0, 0, *bounds]):
        raise RuntimeError("Unexpected original saved-game resource header")
    offset = 18

    def variable():
        nonlocal offset
        start = offset
        word = struct.unpack_from("<H", data, offset)[0]
        if word == 0xFFFF:
            offset += 4
            return struct.unpack_from("<H", data, start + 2)[0]
        while struct.unpack_from("<H", data, offset)[0]:
            offset += 2
        offset += 2
        return data[start:offset - 2].decode("utf-16-le")

    for _ in range(3):
        variable()
    point_size = struct.unpack_from("<H", data, offset)[0]
    offset += 2
    font = variable()
    if (point_size, font) != (8, "MS Sans Serif"):
        raise RuntimeError("Unexpected original saved-game resource font")
    controls = []
    for _ in range(count):
        offset = (offset + 3) & ~3
        child_style, child_exstyle, x, y, w, h, control = struct.unpack_from(
            "<II4hH", data, offset)
        offset += 18
        class_id, caption = variable(), variable()
        extra = struct.unpack_from("<H", data, offset)[0]
        offset += 2 + extra
        controls.append({"control_id": control, "style": child_style,
                         "exstyle": child_exstyle, "class": class_id,
                         "caption": caption, "dlu_rect": [x, y, w, h]})
    if offset > size or any(data[offset:]):
        raise RuntimeError("Unparsed saved-game resource bytes")
    return {"dialog_id": dialog, "address": address, "size": size,
            "sha256": hashlib.sha256(data).hexdigest(), "hex": data.hex(),
            "controls": controls}


def put32(u, at, value):
    u.mem_write(at, struct.pack('<I', value & 0xFFFFFFFF))


def get32(u, at):
    return struct.unpack('<I', u.mem_read(at, 4))[0]


def signed32(u, at):
    return struct.unpack('<i', u.mem_read(at, 4))[0]


def ret(u, cleanup=0, eax=None):
    sp = u.reg_read(UC_X86_REG_ESP)
    if eax is not None:
        u.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
    u.reg_write(UC_X86_REG_EIP, get32(u, sp))
    u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)


def invoke(u, function, *, ecx=0, args=(), required=(), sp=0x100DF000):
    u.mem_write(sp, struct.pack('<' + 'I' * (1 + len(args)), native.RET_MAGIC, *args))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, ecx)
    native.run_checked(u, function, native.RET_MAGIC, count=2_000_000,
                       required_addresses=required, context={'entry': hex(function)})
    assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + len(args) * 4


def code_guard(u):
    return [(native.IMAGE_BASE + rva,
             bytes(u.mem_read(native.IMAGE_BASE + rva, size)))
            for rva, _raw, size, _virtual, flags in native._sections(native.image_bytes())
            if flags & 0x20000000 and size]


def check_code(u, guard):
    assert all(bytes(u.mem_read(at, len(data))) == data for at, data in guard)


def wrapper_case(result,handle,mouse_initial,postload_mouse):
    u=Uc(UC_ARCH_X86,UC_MODE_32)
    native.load_image(u)
    u.mem_map(native.STACK_BASE,native.STACK_SIZE)
    u.mem_map(native.SCRATCH,native.SCRATCH_SIZE)
    u.mem_map(native.RET_MAGIC,0x1000)
    u.mem_map(STUB,0x1000)
    guard=code_guard(u)
    mouse=native.SCRATCH
    label=native.SCRATCH+0x1000
    filename=native.SCRATCH+0x1100
    u.mem_write(label,'Loading'.encode('utf-16-le')+b'\0\0')
    u.mem_write(filename,b'fixture.SAV\0')
    put32(u,mouse,0x7F7B2C)
    put32(u,mouse+0xC,mouse_initial)
    u.mem_write(mouse+0x10,b'\0') # Original mode getter selects Win32 cursor path.
    put32(u,0x887640,mouse)
    put32(u,0xB78168,0xBEEF)
    put32(u,0xB72F50,0) # Supplied empty native window-stack transport context.
    u.mem_write(0xA8B238+0x30D8,b'\0') # Actual69BB40 inactive session cleanup arm.
    u.mem_write(0xA8ED5C,b'\x01');u.mem_write(0xA8E378,b'\x01')
    imports=Image(u).imports()
    events=[]
    os_cursor=[0 if mouse_initial==0 else -1]
    stub_by_address={}
    expected_imports={0x7E11DC:'WaitForSingleObject',0x7E1198:'ReleaseMutex',
                      0x7E13DC:'ShowCursor',0x7E1470:'SetWindowLongA',
                      0x7E14A8:'GetDlgItem',0x7E14A4:'SendMessageA',
                      0x7E1498:'ShowWindow',0x7E14D0:'SetForegroundWindow',0x7E13F4:'DestroyWindow'}
    for index,(iat,name) in enumerate(expected_imports.items()):
        assert imports[iat]==name,(hex(iat),imports[iat],name)
        pointer=STUB+index*0x10
        put32(u,iat,pointer);stub_by_address[pointer]=name
    observed_entries={0x559D60:'wrapper',0x623230:'progress_create',0x7B89F0:'mouse_get',
                      0x7B9930:'mouse_hide',0x7B9750:'mouse_show',0x622800:'progress_show',
                      0x69BB40:'session_prepare',0x622720:'progress_destroy'}

    def args(n):
        sp=u.reg_read(UC_X86_REG_ESP)
        return list(struct.unpack('<'+'I'*n,u.mem_read(sp+4,4*n)))

    def hook(_u,pc,_size,_data):
        if pc in observed_entries:
            events.append({'event':observed_entries[pc], 'entry':hex(pc),
                           'mouse':signed32(u,mouse+0xC)})
        if pc==0x734E60:
            assert Image(u).cstr(u.reg_read(UC_X86_REG_ECX))=='TXT_LOADING'
            events.append({'event':'CSF_lookup_transport','label':'TXT_LOADING'})
            ret(u,8,label)
        elif pc==0x622650:
            assert u.reg_read(UC_X86_REG_ECX)==0xF0
            assert u.reg_read(UC_X86_REG_EDX)==0x6232B0
            events.append({'event':'window_create_transport','resource':0xF0,'result':handle})
            ret(u,4,handle)
        elif pc==0x54F720:
            events.append({'event':'input_service54F720_transport'})
            ret(u,0,0)
        elif pc==0x608070:
            events.append({'event':'dialog_record_teardown608070_transport',
                           'handle':u.reg_read(UC_X86_REG_ECX)})
            ret(u,0,0)
        elif pc==0x67E440:
            assert Image(u).cstr(u.reg_read(UC_X86_REG_ECX))=='fixture.SAV'
            flags=[u.mem_read(0xA8ED5C,1)[0],u.mem_read(0xA8E378,1)[0]]
            assert flags==[0,0]
            events.append({'event':'Load_Game_transport','result':result,'flags_at_entry':flags,
                           'mouse_at_entry':signed32(u,mouse+0xC),
                           'supplied_postload_mouse':postload_mouse})
            if postload_mouse is not None:
                put32(u,mouse+0xC,postload_mouse)
                os_cursor[0]=0 if postload_mouse==0 else -1
            ret(u,0,result)
        elif pc in stub_by_address:
            name=stub_by_address[pc]
            count={'WaitForSingleObject':2,'ReleaseMutex':1,'ShowCursor':1,
                   'SetWindowLongA':3,'GetDlgItem':2,'SendMessageA':4,
                   'ShowWindow':2,'SetForegroundWindow':1,'DestroyWindow':1}[name]
            values=args(count)
            event={'event':name,'args':values}
            answer=1
            if name=='WaitForSingleObject': answer=0
            elif name=='ShowCursor':
                event['os_counter_before']=os_cursor[0]
                os_cursor[0]+=1 if values[0] else -1
                answer=os_cursor[0]
                event['os_counter_after']=answer
            elif name=='GetDlgItem': answer=0x2468
            elif name in ('SetWindowLongA','SendMessageA'): answer=0
            events.append(event)
            ret(u,count*4,answer)
    u.hook_add(UC_HOOK_CODE,hook)
    required=[0x559D60,0x623230,0x7B9930,0x7B89F0,0x69BB40,0x69BBDC,0x559E2E]
    if handle: required.extend([0x623269,0x622800,0x622720])
    invoke(u,0x559D60,args=(filename,),required=required)
    actual=u.reg_read(UC_X86_REG_EAX)&0xFF
    final=signed32(u,mouse+0xC)
    assert actual==result and final==mouse_initial
    destroyed=[e for e in events if e['event']=='DestroyWindow']
    assert len(destroyed)==int(bool(handle))
    if handle: assert destroyed[0]['args']==[handle]
    check_code(u,guard)
    return {'id':f'result{result}_handle{int(bool(handle))}_mouse{mouse_initial}_post{postload_mouse}',
            'input':{'loader_result':result,'progress_handle':handle,'mouse_initial':mouse_initial,
                     'postload_mouse':postload_mouse,'mouse_mode_byte10':0,'session_byte30D8':0,
                     'native_window_stack_count':0},
            'return_al':actual,'mouse_final':final,'os_cursor_final':os_cursor[0],
            'events':events,'original_executable_bytes_unchanged':True}


def signed(value):
    return struct.unpack('<i', dwords(value))[0]


def campaign_cd_case(asset_root, section, keys, *, prior=None):
    machine = Reader(asset_root, {section: keys})
    obj = machine.alloc(0x3A0)
    machine.u.mem_write(obj + 0x24, section.encode('ascii') + b'\0')
    if prior is None:
        # Native CreateFromINIList established EBX=-1 at46CE31. Execute its
        # actual two constructor stores, not a synthesized initial field.
        machine.u.reg_write(UC_X86_REG_ESI, obj)
        machine.u.reg_write(UC_X86_REG_EBX, 0xFFFFFFFF)
        native.run_checked(machine.u, 0x46CEBA, 0x46CEC6)
    else:
        machine.u.mem_write(obj + 0x98, dwords(prior))
    before = signed(machine.read32(obj + 0x98))
    machine.u.reg_write(UC_X86_REG_EBX, obj)
    machine.u.reg_write(UC_X86_REG_EDI, INI)
    machine.u.reg_write(UC_X86_REG_ESP, SP)
    native.run_checked(machine.u, 0x46CCF8, 0x46CD1D,
                       required_addresses=(0x5276D0, 0x46CD17))
    assert machine.u.reg_read(UC_X86_REG_ESP) == SP - 8
    return dict(section=section, raw_cd=keys.get('CD'), before=before,
                after=signed(machine.read32(obj + 0x98)))


def campaign_cd_inputs():
    # Archive selection is a physical stock fixture, not native archive IO.
    # Reuse the MIX decoder; exact bytes must match the researched retail input.
    root = native.configured_gamemd().parent
    archive = mix((root / 'ra2md.mix').read_bytes())
    local = mix(archive[mix_hash('localmd.mix')])
    raw = local[mix_hash('BATTLEMD.INI')]
    assert hashlib.sha256(raw).hexdigest() == BATTLE_SHA256
    roster, _ = lexical(raw, {'Battles'})
    sections, _ = lexical(raw, set(roster['Battles'].values()))
    # The shared native INI fixture accepts an asset root. This reader slice
    # never requests any SHP or sound; an empty temporary directory declares
    # that boundary without copying its INI lookup-cache/CRT implementation.
    with tempfile.TemporaryDirectory(prefix='vera20k-campaign-cd-') as folder:
        asset_root = Path(folder)
        campaign = [campaign_cd_case(asset_root, name, sections[name])
                    for name in roster['Battles'].values()]
        campaign.extend((campaign_cd_case(asset_root, 'MISSING_CD', {}),
                         campaign_cd_case(asset_root, 'RETAINED_CD', {}, prior=7)))
    retail = dict(path='ra2md.mix/localmd.mix/BATTLEMD.INI', bytes=len(raw),
                  sha256=hashlib.sha256(raw).hexdigest(),
                  source='ra2md.mix -> localmd.mix', entry_id='0x07E280DD',
                  battle_roster=roster['Battles'])
    return retail, campaign


def cd_request_cases():
    cases = []
    for flag in (0, 1, 2):
        for request in (-3, -2, -1, 0, 1, 2, 17):
            result = native.call(0x4790B0, ecx=request & 0xFFFFFFFF,
                                 writes={0x89E3A0: bytes([flag]),
                                         0x81C1D0: dwords(0x12345678)},
                                 dumps={'request': (0x81C1D0, 4)})
            cases.append(dict(flag=flag, requested=request,
                              stored=struct.unpack('<i', bytes.fromhex(result['dumps']['request']))[0]))
    return cases


def postload_clock_reset_case(clock):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    native.load_image(u)
    u.mem_map(native.STACK_BASE, native.STACK_SIZE)
    u.mem_map(native.RET_MAGIC, 0x1000)
    u.mem_map(STUB, 0x1000)
    assert Image(u).imports()[0x7E1530] == 'timeGetTime'
    put32(u, 0x7E1530, STUB)
    for at in (0x845550, 0x845554, 0x845558, 0x84555C):
        put32(u, at, 0x12345678)
    calls, guard = [], code_guard(u)

    def hook(_u, pc, _size, _data):
        if pc == STUB:
            assert len(calls) < len(clock)
            calls.append(clock[len(calls)])
            ret(u, 0, calls[-1])

    u.hook_add(UC_HOOK_CODE, hook)
    invoke(u, 0x734210, required=(0x734210, 0x734244))
    check_code(u, guard)
    assert len(calls) == 2
    return dict(input_clock_returns=clock, calls=calls,
                group_last_time=get32(u, 0x845550), group_last_slot=signed32(u, 0x845554),
                other_last_time=get32(u, 0x845558), other_last_id=signed32(u, 0x84555C),
                code_unchanged=True)


class ElapsedScenario(scenario_owner.ScenarioFixture):
    """Reuse full Scenario storage, executing its original OS-clock helper."""

    TIME = 0x30000300

    def __init__(self, spec):
        self.spec, self.clocks, self.boundaries = spec, [], []
        super().__init__()
        assert Image(self.u).imports()[0x7E1530] == 'timeGetTime'
        self.u.mem_write(0x7E1530, dwords(self.TIME))
        self.u.mem_write(scenario_owner.SCENARIO + 0x614, dwords(*spec['elapsed_before']))
        self.u.mem_write(scenario_owner.SCENARIO + 0x620, dwords(111, 222, 333))
        self.guard = code_guard(self.u)

    def timer(self, offset):
        return list(struct.unpack('<3I', self.u.mem_read(scenario_owner.SCENARIO + offset, 12)))

    def hook(self, u, pc, size, data):
        if pc == 0x6C8C40:
            return  # Execute the actual timeGetTime >> 4 body.
        if pc == self.TIME:
            index = len(self.clocks)
            assert index < len(self.spec['milliseconds'])
            value = self.spec['milliseconds'][index]
            self.clocks.append(dict(milliseconds=value,
                                    return_address=hex(get32(u, u.reg_read(UC_X86_REG_ESP))),
                                    clock_this=hex(u.reg_read(UC_X86_REG_ECX))))
            self.ret()
            u.reg_write(UC_X86_REG_EAX, value)
            return
        return super().hook(u, pc, size, data)

    def call(self, entry, args=(), required=()):
        before = self.timer(0x614)
        super().call(entry, args, required)
        self.boundaries.append(dict(entry=hex(entry), before=before, after=self.timer(0x614),
                                    auxiliary_timer=self.timer(0x620)))


def scenario_elapsed_case(spec):
    fixture = ElapsedScenario(spec)
    result = fixture.round_trip()
    check_code(fixture.u, fixture.guard)
    assert len(fixture.clocks) == len(spec['milliseconds'])
    return dict(input=spec, clock_calls=fixture.clocks, boundaries=fixture.boundaries,
                saved_elapsed=list(struct.unpack_from('<3I', fixture.data, 0x614)),
                loaded_elapsed=fixture.timer(0x614), loaded_auxiliary=fixture.timer(0x620),
                owner_result=result, code_unchanged=True)


def scenario_elapsed_cases():
    rows = [
        dict(name='running', elapsed_before=[100, 0xA5A5A5A5, 50],
             milliseconds=[16000, 16160, 32000, 32160, 64000]),
        dict(name='stopped', elapsed_before=[0xFFFFFFFF, 0xA5A5A5A5, 77],
             milliseconds=[16000, 16160]),
        dict(name='stopped_zero', elapsed_before=[0xFFFFFFFF, 0, 0], milliseconds=[0, 0]),
        dict(name='same_bucket', elapsed_before=[100, 0, 50],
             milliseconds=[1601, 1615, 1615, 1615, 1615]),
        dict(name='millisecond_wrap', elapsed_before=[0x0FFFFFFC, 0xA5A5A5A5, 0xFFFFFFF0],
             milliseconds=[32, 48, 64, 80, 96]),
        dict(name='accumulator_wrap', elapsed_before=[1, 0, 0xFFFFFFFE],
             milliseconds=[80, 96, 112, 128, 144]),
    ]
    return [scenario_elapsed_case(row) for row in rows]



def progress_geometry_case(width,height):
 m=Machine(0xF0,width,height);u=m.uc
 raw=Image(u).dialog_template(0xF0)
 assert list(struct.unpack_from('<4h',raw,10))==[0,0,300,200]
 m.windows[PARENT]['rect']=[0,0,muldiv(300,6,4),muldiv(200,13,8)]
 for h,c in zip(m.children,m.controls):
  x,y,w,hgt=c['template'];x,y,w,hgt=muldiv(x,6,4),muldiv(y,13,8),muldiv(w,6,4),muldiv(hgt,13,8)
  m.windows[h]['rect']=[x,y,x+w,y+hgt]
 m.windows[0x7000]={'id':0,'style':0,'class':'Main','rect':[0,0,width,height]}
 m.w32(0xB73550,0x7000);m.w32(0xB72F54,300);m.w32(0xB72F58,163)
 events=[]
 def get_long():
  hwnd=m.arg(0);index=m.signed_args(2)[1];w=m.windows[hwnd]
  value={-8:0x7000 if hwnd==PARENT else PARENT,-16:w['style'],-12:w['id']}.get(index,0)
  m.ret(8,value)
 m.stubs[m.r32(0x7E1474)]=get_long
 for iat,name in ((0x7E1398,'MoveWindow'),(0x7E13F0,'SetWindowPos')):
  old=m.stubs[m.r32(iat)]
  def report(old=old,name=name):
   events.append({'event':name,'args':[m.arg(i) for i in range(6 if name=='MoveWindow' else 7)]});old()
  m.stubs[m.r32(iat)]=report
 guard=code_guard(u)
 initial={str(m.windows[h]['id']):list(m.windows[h]['rect']) for h in [PARENT]+m.children}
 # Same callback order as775BA0: original callback for each child, then parent.
 # Enumeration transport itself is supplied, as in the existing shell owner.
 for h in m.children:m.call(0x775BC0,stack_args=(h,1))
 m.call(0x775BC0,stack_args=(PARENT,0))
 m.call(0x60C540,ecx=PARENT);fullscreen=u.reg_read(UC_X86_REG_EAX)&255
 m.call(0x60C7D0,ecx=PARENT);mode2=u.reg_read(UC_X86_REG_EAX)&255
 m.call(0x777060,ecx=PARENT)
 check_code(u,guard)
 x,y,r,b=m.windows[PARENT]['rect']
 return dict(input={'width':width,'height':height,'dialog_base_units':[6,13],'benchmark_client':[300,163],'initial_window_rects':initial},
             fullscreen=fullscreen,mode2=mode2,background_mode=m.r32(m.record(PARENT)+0xB0),
             dialog_rect=[x,y,r-x,b-y],children=[dict(id=c['id'],style=c['style'],visible=bool(c['style']&0x10000000),
              rect=[m.windows[h]['rect'][0]+x,m.windows[h]['rect'][1]+y,m.windows[h]['rect'][2]-m.windows[h]['rect'][0],m.windows[h]['rect'][3]-m.windows[h]['rect'][1]]) for h,c in zip(m.children,m.controls)],events=events,code_unchanged=True)

def progress_background_case(active, side, missing=None):
 m=Machine(0xF0,800,600);u=m.uc;events=[]
 m.windows[PARENT]['rect']=[175,137,626,463]
 cache,alternate=m.alloc(32),m.alloc(32)
 names={cache:'dialog_cache',alternate:'alternate887310'}
 def rect(at):return list(struct.unpack('<4i',u.mem_read(at,16)))
 def copy():
  events.append(dict(event='copy',destination=names[u.reg_read(UC_X86_REG_ECX)],
   destination_rect=rect(m.arg(0)),source=names[m.arg(1)],source_rect=rect(m.arg(2)),
   flags=[m.arg(3),m.arg(4)]));m.ret(20,1)
 vtable=m.alloc(16);m.w32(vtable+8,m.stub(copy))
 for surface in names:m.w32(surface,vtable)
 m.w32(m.record(PARENT)+0x10,cache);m.w32(m.record(PARENT)+0xB0,2)
 m.w32(0x887310,alternate)
 scenario=m.alloc(0x3500);m.w32(0xA8B230,scenario);m.w32(scenario+0x34B8,side)
 u.mem_write(0xA8B238+0x30D8,bytes([active]))
 image=Image(u);shapes={};palettes={}
 for slot,literal in ((0xB0FC80,0x844C44),(0xB0FC84,0x844C48),(0xB0FC88,0x844C4C),(0xB0FC8C,0x844C50)):
  p=m.alloc(16);name=image.cstr(m.r32(literal));shapes[p]=name;m.w32(slot,0 if missing==name else p)
 for slot,literal in ((0xB0FB68,0x844BA4),(0xB0FB70,0x844BA0),(0xB0FB60,0x844B9C)):
  p=m.alloc(16);name=image.cstr(m.r32(literal));palettes[p]=name;m.w32(slot,0 if missing==name else p)
 def observe(_u,pc,_size,_data):
  if pc==0x4AED70:
   events.append(dict(event='draw',surface=names[u.reg_read(UC_X86_REG_ECX)],
    shape=shapes[m.arg(0)],palette=palettes[u.reg_read(UC_X86_REG_EDX)],
    frame=m.arg(1),point=list(struct.unpack('<2i',u.mem_read(m.arg(2),8))),
    clip=rect(m.arg(3)),flags=m.arg(4)))
   m.ret(0x38,1)
  elif pc in (0x69BBE0,0x72AFF0,0x72B010,0x72B030):
   events.append(dict(event='entry',address=hex(pc)))
 guard=code_guard(u);u.hook_add(UC_HOOK_CODE,observe)
 m.call(0x621E90,ecx=PARENT)
 check_code(u,guard)
 return dict(input={'session_byte30D8':active,'scenario_side34B8':side,'missing':missing,
                    'dialog_rect':[175,137,451,326]},events=events,code_unchanged=True)



def generate():
    resources = [resource(*args) for args in RESOURCES]
    cases = []
    for item in resources:
        for child in item["controls"]:
            if child["control_id"] not in (0x40C, 0x525, 0x526, 0x527, 0x528):
                continue
            x, y, w, h = child["dlu_rect"]
            pixels = [(x * 6 + 2) // 4, (y * 13 + 4) // 8,
                      (w * 6 + 2) // 4, (h * 13 + 4) // 8]
            for width, height in ((640, 480), (800, 600), (1024, 768)):
                case = native_child_case(child["control_id"], pixels, width, height)
                case["dialog_id"] = item["dialog_id"]
                case["dlu_rect"] = child["dlu_rect"]
                cases.append(case)
    if len(cases) != 21:
        raise RuntimeError("Missing ordinary saved-game geometry cases")
    retail, campaign = campaign_cd_inputs()
    return {"source": "unicorn/gamemd.exe", "resources": resources, "cases": cases,
            "wrapper_cases": [wrapper_case(result, handle, initial, post)
                              for result in (0, 1) for handle in (0, 0x1234)
                              for initial, post in ((0, None), (-2, None), (-2, -5), (-2, 0))],
            "campaign_cd_retail": retail, "campaign_cd_reader": campaign,
            "cd_request_setter": cd_request_cases(),
            "postload_clock_reset": [postload_clock_reset_case(clock)
                                     for clock in ([2000, 2010], [0, 1], [0xFFFFFFFF, 0])],
            "scenario_elapsed_clock": scenario_elapsed_cases(),
            "progress_resource": resource(0xF0, 0xBF8A08, 148, 2, (300, 200)),
            "progress_geometry": [progress_geometry_case(w, h) for w, h in
                                  ((640, 480), (800, 600), (1024, 768), (801, 601), (320, 200))],
            "progress_background": [
                progress_background_case(active, side, missing)
                for active, side, shape, palette in (
                    (0, 2, 'PUDLGBGN.SHP', 'DIALOGN.PAL'),
                    (1, 0, 'PUDLGBGA.SHP', 'DIALOG.PAL'),
                    (1, 1, 'PUDLGBGS.SHP', 'DIALOG.PAL'),
                    (1, 2, 'PUDLGBGY.SHP', 'DIALOGY.PAL'))
                for missing in (None, shape, palette)]}



if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: dict(provenance(
        scope="Original B7/2B4/2B5 resources and 21 complete60B7A0 ordinary placements; original559D60 load controller, campaign CD scalars,734210 click clocks, six Scenario elapsed-clock controls; F0 resource, five geometry and twelve mode2 background controls",
        assumptions=[
            "Pinned original resource addresses independently checked through PE RT_DIALOG tree; raw bytes and SHA256 retained",
            "Supplied retail 6x13 dialog base units map DLU through MulDiv rounding; Windows font metrics not emulated",
            "60C0C0 active dispatcher and 608500/608CD0 exclusions established by instruction inspection; only complete 60B7A0 is executed",
            "Parent origin0 with extent640x480,800x600,1024x768; original base800/600 globals remain untouched",
            "Title, right-button and footer placement, painting, list input and full native SAV persistence are outside this fixture",
            "Controller rows execute559D60, progress623230/622800/622720 and original WWMouse methods with byte10=0 and nonpositive counts; window-stack count0 and inactive session+30D8=0 bound cleanup coverage",
            "Lower loader AL and optional post-load mouse count are explicit fixture inputs; neither AL nor these rows proves native rollback, world restoration, audio cleanup or full media retry behavior",
            "Physical BATTLEMD.INI from stock ra2md.mix/localmd.mix is SHA-pinned; all14 Battles CD values, missing default and retained7 pass original46CCF8..46CD1D/ReadInt5276D0",
            "Original campaign constructor stores46CEBA..46CEC6 run with caller-established EBX=-1; full campaign registry and AbstractType admission are outside the slice",
            "CD request setter4790B0 executes with byte89E3A0=0/1/2 and requests-3/-2/-1/0/1/2/17; drive search and physical media dialogs are not executed",
            "Click-clock reset executes734210 with two explicit timeGetTime returns, including zero/wrap controls; group/reset globals are observed, not a whole selection or input-state claim",
            "Six elapsed-clock cases reuse ScenarioFixture full689310Save/689470Load/683560 postread with supplied+614 start,+618 opaque word,+61C accumulated buckets and ordered OS milliseconds. Real6C8C40 shifts timeGetTime by4; saved stopped state, source resume, loaded resume and distinct+620/+628 auxiliary reset are observed",
            "Elapsed controls cover running/stopped/zero/same-bucket/millisecond rollover and accumulation wrap. They do not execute world load, UI/offline pause, score-seconds consumption or assert a complete save transaction clock prefix",
            "F0 geometry supplies initial Windows6x13 DLU conversions and cached C6 benchmark300x163, then executes775BC0 callbacks in775BA0 child-then-parent order,60C540/60C7D0 classification and777060/777080 centering. Windows font metrics, enumeration transport and non-client borders are not emulated",
            "F0 mode2 controls execute full621E90 with supplied record/cache/window extent and original69BBE0/palette getters. Surface copy and shape draw calls record exact original arguments; glyph/SHP rasterization and Win32 parent-message scheduling are excluded. Existing Alternate887310 is read and updated only within the dialog rectangle, not cleared or recomposed",
            "Mode2 literals are read from original844C44/48/4C/50 and844BA4/BA0/B9C pointers. Inactive chooses neutral, active chooses side0/1/other; missing selected shape or palette suppresses drawing without fallback. These are supplied session/side controls, not a whole559D60 session teardown prefix",
        ],
        substitutions=[
            "Reuse native_child_case hooks for supplied GetParent/GetWindowRect/MoveWindow and true69BBE0; original placement instructions unchanged",
            "Controller CSF734E60 returns TXT_LOADING text; lower window creation622650 supplies F0 handle;54F720 input service and608070 dialog-record teardown are platform seams",
            "Controller lower structured-storage loader67E440 supplies AL/post-load mouse count; Win32 mutex, ShowCursor counter and window services return declared OS results; original wrapper/WWMouse instructions remain unchanged",
            "Reuse lexical physical values and bridge_anim_inputs.Reader native INI cache/CRT environment; MIX/file loading and complete campaign registry creation are not emulated; no integer parser or setter result is substituted",
            "Click-clock reset timeGetTime returns each declared ordered pair; original subtraction, two calls and sentinel writes execute unchanged",
            "Elapsed controls retain the existing Scenario in-memory IStream, empty arrays/string admission and diagnostic transport; only the timeGetTime IAT is replaced for the real clock body, and original executable bytes remain unchanged",
            "F0 reuses shell_relayout.Machine window-record/Win32 transport; preexisting cached surfaces expose supplied vtable copy methods, and4AED70 drawing is an argument-recording boundary. Original621E90 and its branches execute unchanged",
        ],
        entry_points={"browser": 0x558DD0, "dispatcher": 0x60C0C0,
                      "ordinary_child": 0x60B7A0, "controller_load": 0x559D60,
                      "progress_create": 0x623230, "mouse_get": 0x7B89F0,
                      "mouse_hide": 0x7B9930, "mouse_show": 0x7B9750,
                      "campaign_cd_reader": 0x46CCF8, "read_int": 0x5276D0,
                      "campaign_default_stores": 0x46CEBA,
                      "cd_request_setter": 0x4790B0, "postload_clock_reset": 0x734210,
                      "scenario_save": 0x689310, "scenario_load": 0x689470,
                      "scenario_postread": 0x683560, "scenario_clock": 0x6C8C40,
                      "progress_resize": 0x775BC0, "progress_center": 0x777060,
                      "progress_center_body": 0x777080, "progress_fullscreen": 0x60C540,
                      "progress_mode2": 0x60C7D0, "progress_background": 0x621E90,
                      "session_active": 0x69BBE0, "palette_as": 0x72AFF0,
                      "palette_y": 0x72B010, "palette_neutral": 0x72B030},
    ), command='python -m tools.storage_oracle.saved_game_layout --check'), source_paths={
        "producer": Path(__file__), "shared_runner": Path(native.__file__),
        "geometry": Path(__file__).with_name("in_game_shell_geometry.py"),
        "pe_imports": Path(__file__).with_name("shell_relayout.py"),
        "ini_reader": Path(__file__).parents[1] / "rules_oracle/bridge_anim_inputs.py",
        "lexical": Path(__file__).parents[1] / "projectile_oracle/bridge_render_inputs.py",
        "stock_mix": Path(__file__).parents[1] / "sidebar_oracle/stock.py",
        "scenario_storage": Path(scenario_owner.__file__),
    })
