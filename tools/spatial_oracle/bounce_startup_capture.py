"""Bounded original-process CRT capture; no executable bytes/data are written.

Creates and owns one hidden debug child, stops before WinMain, and terminates only
that child. Hardware execution breakpoints observe PE entry, Bounce initializer stages and WinMain.
The captured range includes Bounce level height89C778 and deck offset89C76C.
The debugger scaffolding is shared in design with tube_startup_capture.py; each
fixture retains a standalone original-process replay and its own golden.
API layouts: learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-wow64_context
and /api/minwinbase/ns-minwinbase-debug_event. Not a gameplay-state capture.
"""
import ctypes as C
from ctypes import wintypes as W
import argparse, hashlib, json, platform, time
from pathlib import Path
from tools.native_oracle import configured_gamemd, image_bytes, image_sha256, _sections

parser=argparse.ArgumentParser(description=__doc__)
mode=parser.add_mutually_exclusive_group()
mode.add_argument('--write',action='store_true')
mode.add_argument('--check',action='store_true')
args=parser.parse_args()
golden=Path(__file__).with_suffix('.json')
if platform.system()!='Windows' or C.sizeof(C.c_void_p)!=8:
    raise SystemExit('Requires 64-bit Windows Python and the original retail executable')
k = C.WinDLL('kernel32', use_last_error=True)
P = C.c_void_p
D = W.DWORD
class SI(C.Structure):
    _fields_ = [('cb', D), ('reserved', W.LPWSTR), ('desktop', W.LPWSTR), ('title', W.LPWSTR),
                ('x', D), ('y', D), ('xs', D), ('ys', D), ('xc', D), ('yc', D), ('fill', D),
                ('flags', D), ('show', W.WORD), ('reserved2len', W.WORD), ('reserved2', P),
                ('stdin', P), ('stdout', P), ('stderr', P)]
class PI(C.Structure):
    _fields_ = [('process', P), ('thread', P), ('pid', D), ('tid', D)]
class ER(C.Structure):
    _fields_ = [('code', D), ('flags', D), ('record', P), ('address', P), ('count', D), ('info', C.c_size_t*15)]
class EI(C.Structure):
    _fields_ = [('record', ER), ('first', D)]
class CPI(C.Structure):
    _fields_ = [('file', P), ('process', P), ('thread', P), ('base', P), ('offset', D), ('size', D), ('tls', P), ('start', P), ('name', P), ('unicode', W.WORD)]
class DI(C.Union):
    _fields_ = [('exception', EI), ('create', CPI), ('data', C.c_byte*160), ('file', P), ('exitcode', D)]
class DE(C.Structure):
    _fields_ = [('code', D), ('pid', D), ('tid', D), ('u', DI)]
class FS(C.Structure):
    _fields_ = [(n,D) for n in ['control','status','tag','erroroffset','errorselector','dataoffset','dataselector']] + [('registers', C.c_byte*80), ('cr0',D)]
class WC(C.Structure):
    _fields_ = [(n,D) for n in ['flags','dr0','dr1','dr2','dr3','dr6','dr7']] + [('fp',FS)] + [(n,D) for n in ['gs','fs','es','ds','edi','esi','ebx','edx','ecx','eax','ebp','eip','cs','eflags','esp','ss']] + [('extended',C.c_byte*512)]
assert C.sizeof(WC)==716 and C.sizeof(DE)==176 and C.sizeof(FS)==112
def api(name,args,result=W.BOOL):
    f=getattr(k,name); f.argtypes=args; f.restype=result; return f
create=api('CreateProcessW',[W.LPCWSTR,W.LPWSTR,P,P,W.BOOL,D,P,W.LPCWSTR,C.POINTER(SI),C.POINTER(PI)])
wait=api('WaitForDebugEvent',[C.POINTER(DE),D])
cont=api('ContinueDebugEvent',[D,D,D])
getctx=api('Wow64GetThreadContext',[P,C.POINTER(WC)])
setctx=api('Wow64SetThreadContext',[P,C.POINTER(WC)])
read=api('ReadProcessMemory',[P,P,P,C.c_size_t,C.POINTER(C.c_size_t)])
terminate=api('TerminateProcess',[P,W.UINT])
close=api('CloseHandle',[P])
waitobj=api('WaitForSingleObject',[P,D],D)
def checked(ok):
    if not ok: raise C.WinError(C.get_last_error())
path=configured_gamemd(); original=image_bytes()
si=SI(); si.cb=C.sizeof(si); si.flags=1; si.show=0
pi=PI(); event=DE(); pending=False; exited=False
payload={'native_sha256':image_sha256(),'os':platform.platform(),'mechanism':'hardware execution breakpoints, no WriteProcessMemory or code patches','coverage':'original process PE entry, Bounce initializer439610 and following439640 and WinMain entry, before application startup; no runtime immutability or arbitrary process-memory proof','events':[],'captures':[]}
def mem(addr,n):
    out=C.create_string_buffer(n); got=C.c_size_t()
    checked(read(pi.process,addr,out,n,C.byref(got))); assert got.value==n
    return out.raw
def context():
    c=WC(); c.flags=0x1003f; checked(getctx(pi.thread,C.byref(c))); return c
breakpoints=[0x7cd80f,0x439610,0x439640,0x6bb9a0]
def arm(c,which):
    c.flags=0x10010; c.dr0,c.dr1,c.dr2,c.dr3=breakpoints; c.dr6=0; c.dr7=which
    checked(setctx(pi.thread,C.byref(c)))
try:
    command=C.create_unicode_buffer('"'+str(path)+'"')
    checked(create(str(path),command,None,None,False,2,None,str(path.parent),C.byref(si),C.byref(pi)))
    payload['owned_pid']=pi.pid
    deadline=time.monotonic()+20
    armed=False; active=0x55
    while time.monotonic()<deadline:
        if not wait(C.byref(event),1000):
            if C.get_last_error()==121: continue
            raise C.WinError(C.get_last_error())
        pending=True
        assert event.pid==pi.pid
        status=0x10002
        if event.code==3:
            assert event.u.create.base==0x400000
            if event.u.create.file: close(event.u.create.file)
        elif event.code==6:
            if event.u.file: close(event.u.file)
        elif event.code==1:
            er=event.u.exception.record
            item={'code':hex(er.code),'address':hex(er.address or 0),'first_chance':event.u.exception.first}
            payload['events'].append(item)
            if er.code in (0x80000003,0x4000001f,0x80000004,0x4000001e):
                c=context(); item['eip']=hex(c.eip)
                if not armed:
                    arm(c,active); armed=True
                if c.eip in breakpoints:
                    region=mem(0x89c760,0x20)
                    text_bytes=mem(c.eip,16)
                    for rva,raw,size,virtual,flags in _sections(original):
                        if 0x400000+rva<=c.eip<0x400000+rva+size:
                            off=raw+c.eip-0x400000-rva
                            assert text_bytes==original[off:off+16]
                            break
                    else: raise RuntimeError('Observed instruction not in original image')
                    payload['captures'].append({'eip':hex(c.eip),'fpcw':hex(c.fp.control),'region_address':'0x89c760','region_hex':region.hex(),'original_instruction_bytes':text_bytes.hex()})
                    if c.eip==0x6bb9a0:
                        payload['executable_sections']=[]
                        for rva,raw,size,virtual,flags in _sections(original):
                            if flags & 0x20000000 and size:
                                loaded=mem(0x400000+rva,size)
                                section=original[raw:raw+size]
                                assert loaded==section,'Loaded executable section differs from pinned original'
                                payload['executable_sections'].append({'address':hex(0x400000+rva),'size':size,'original_and_loaded_sha256':hashlib.sha256(section).hexdigest()})
                        break
                    active &= ~(1 << (breakpoints.index(c.eip)*2))
                    arm(c,active)
            else:
                status=0x80010001
        elif event.code==5:
            payload['exit_code']=event.u.exitcode; exited=True
            checked(cont(event.pid,event.tid,status)); pending=False; break
        checked(cont(event.pid,event.tid,status)); pending=False
    payload['captured_winmain']=any(x['eip']=='0x6bb9a0' for x in payload['captures'])
finally:
    if pi.process:
        if not exited:
            checked(terminate(pi.process,0))
            if pending:
                cont(event.pid,event.tid,0x10002); pending=False
            end=time.monotonic()+5
            while time.monotonic()<end:
                if wait(C.byref(event),100):
                    cont(event.pid,event.tid,0x10002)
                    if event.code==5: break
            payload['owned_process_stopped']=waitobj(pi.process,1000)==0
        close(pi.thread); close(pi.process)
if not payload.get('captured_winmain') or not payload.get('owned_process_stopped'):
    raise SystemExit('No complete WinMain capture/owned process termination: no parity claim')
if {int(c['eip'],16) for c in payload['captures']} != set(breakpoints):
    raise SystemExit('Incomplete initializer captures: no parity claim')
stable={key:payload[key] for key in ['native_sha256','mechanism','coverage','captures','executable_sections']}
if args.write:
    golden.write_text(json.dumps({'recorded_os':payload['os'],**stable},indent=2)+'\n',encoding='utf-8')
else:
    expected=json.loads(golden.read_text(encoding='utf-8'))
    expected.pop('recorded_os',None)
    if stable!=expected:
        raise SystemExit('Native startup capture differs from saved reference; no golden written')
print(json.dumps({'result':'wrote' if args.write else 'matched','captures':len(payload['captures']),'native_sha256':image_sha256(),'all_executable_sections_original':True,'owned_process_stopped':True}))
