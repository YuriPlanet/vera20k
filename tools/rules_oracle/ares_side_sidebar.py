"""Bounded installed Ares Side sidebar initialization, reads and archive route.

Set VERA20K_ARES_DLL and VERA20K_GAMEMD_EXE to local enrolled binaries.
python -m tools.rules_oracle.ares_side_sidebar --check
Only synthetic names/state are used. No mod artwork or loader adapter is needed.
"""
import argparse
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools import native_oracle as native
from tools.native_oracle import OracleError, finish_vectors, provenance, run_checked
from tools.rules_oracle.ares_prerequisites import ARES_SHA256, Probe, identified_bytes
from tools.input_oracle.fast_scroll import return_from_sink
from tools.rules_oracle.ini_token_readers import Crt, THREAD_BLOCK

STOP = 0x30000000
ARCHIVE_SLOTS = (0x884E68, 0x884E74, 0x884E70, 0x884E78)


class SideFixture(Probe):
    """Probe owns images, arena and execution; this adds declared Side seams."""
    def __init__(self, images):
        self.input_bytes = images
        self.extension_by_side = {}
        self.thread_block_calls = 0
        super().__init__()
        self.u.mem_write(THREAD_BLOCK,bytes(0x400))
        # Original CRT initializer selects file-backed C locale; the supplied
        # thread uses that locale, as in10085FED->10085D5E. No scanner answer.
        super().call(0x10085839)
        self.write(THREAD_BLOCK+0x4C,self.read(0x100C28C0))
        self.u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        self.original_code = [(base+rva, raw[off:off+length])
            for raw, base, size in zip(images, (0x10000000,0x400000), (0xE0000,0xA00000))
            for rva,off,length,_,flags in native._sections(raw,image_base=base,image_size=size)
            if flags & 0x20000000]

    def check_code(self):
        assert all(bytes(self.u.mem_read(at,len(raw)))==raw for at,raw in self.original_code)
        return True

    def ret(self, value=0, cleanup=0):
        return_from_sink(self.u, cleanup, value)

    def hook(self, u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        arg = lambda n: self.read(sp+4+n*4)
        if pc == 0x10085F5C:
            return Crt.thread_block(self,u,pc,size,data)
        if pc == 0x10005800:
            section,key,default,target,capacity = [arg(i) for i in range(5)]
            pair = (self.string(section),self.string(key))
            value = self.reads.get(pair,self.string(default))
            payload = value.encode('ascii')[:capacity-1]
            u.mem_write(target,payload+b'\0')
            self.events.append(dict(service='supplied_ReadString',section=pair[0],key=pair[1],value=value,capacity=capacity))
            return self.ret(len(payload),20)
        if pc == 0x10057C70:
            if u.reg_read(UC_X86_REG_ECX)!=0x100C1BE8:
                raise OracleError('Unexpected extension container')
            return self.ret(self.extension_by_side[arg(0)],4)
        if pc in (0x65C6D0,0x65C780,0x65C7E0,0x7258D0):
            raise OracleError(f'Unexpected RNG/detach entry {pc:08X}')
        return super().hook(u,pc,size,data)

    def invoke(self, address, *args, ecx=0, end=STOP, required=()):
        sp = 0x2000F000
        self.u.mem_write(sp,struct.pack('<'+'I'*(len(args)+1),STOP,*[a&0xffffffff for a in args]))
        self.u.reg_write(UC_X86_REG_ESP,sp)
        self.u.reg_write(UC_X86_REG_ECX,ecx)
        run_checked(self.u,address,end,count=200000,timeout_us=5000000,
            required_addresses=required,context={'scope':'Ares Side sidebar controls'})
        self.check_code()
        return self.u.reg_read(UC_X86_REG_EAX)

    def setup(self, ids):
        array = self.alloc(max(4,4*len(ids)))
        self.sides,self.exts = [],[]
        for index,name in enumerate(ids):
            side,ext = self.alloc(0xB0),self.alloc(0x300)
            self.u.mem_write(side+0x24,name.encode('ascii')+b'\0')
            self.write(ext,side);self.write(array+4*index,side)
            self.sides.append(side);self.exts.append(ext)
            self.extension_by_side[side] = ext
        self.write(0x8B4124,array);self.write(0x8B4130,len(ids))
        self.write(0xA8B228,0)  # initializer's unrelated Aircraft FindIndex sees no entries
        self.scenario = self.alloc(0x4000);self.write(0xA8B230,self.scenario)
        return self

    def state(self,index):
        ext = self.exts[index]
        signed = lambda value: struct.unpack('<i',struct.pack('<I',value))[0]
        return dict(phase=signed(self.read(ext+4)),mix_file_index=signed(self.read(ext+0x88)),
            yuri_file_names=bool(self.u.mem_read(ext+0x8C,1)[0]))

    def selection(self,side):
        shim,stack = self.alloc(0x40),self.alloc(0x40)
        self.write(shim+0xC,side);self.write(shim+0x14,stack)
        target = self.invoke(0x1002FA00,shim)
        adjusted = self.read(shim+0x18)
        row = dict(mix_hook_return=f'{target:08X}',adjusted_index_bits=f'{adjusted:08X}',
            preserved_side_index=self.read(stack+0x10))
        if target==0x535003:
            self.u.reg_write(UC_X86_REG_EBX,adjusted)
            self.invoke(0x5350D3,end=0x5350E6)
            row['md_mix_name'] = self.string(0x2000F014)
            self.invoke(0x535179,end=0x535189)
            row['mix_name'] = self.string(0x2000F014)
        self.write(self.scenario+0x34B8,side)
        row['filename_branches'] = [f'{self.invoke(entry):08X}' for entry in (0x1002FA50,0x1002FA60,0x1002FA70)]
        row['scenario_side_after'] = self.read(self.scenario+0x34B8)
        return row


def constructor(images):
    p=SideFixture(images).setup(['SyntheticSide'])
    ext=p.exts[0];p.u.mem_write(ext,bytes([0xCC])*0x280)
    p.invoke(0x1002D9F0,p.sides[0],ecx=ext,end=0x1002DA89,
        required=(0x1002DA04,0x1002DA7D,0x1002DA83))
    return dict(initial_fill_byte=0xCC,state=p.state(0),entry='1002D9F0',end_exclusive='1002DA89',
        boundary='Literal constructor prefix only; later unrelated member construction excluded.')


def initial_controls(images):
    ids=['FourthSynthetic','ThirdSide','Nod','GDI']
    p=SideFixture(images).setup(ids);rows=[]
    for index,name in enumerate(ids):
        p.invoke(0x1002E190,0,ecx=p.exts[index])
        rows.append(dict(name=name,array_index=index,state=p.state(index)))
    return dict(fixture_side_order=ids,rows=rows,executable_sections_unchanged=p.check_code())


def retained_controls(images):
    ids=['GDI','Nod','ThirdSide','FourthSynthetic']
    p=SideFixture(images).setup(ids);index=3
    p.invoke(0x1002E190,0,ecx=p.exts[index]);p.write(p.exts[index]+4,2)
    initial=p.state(index);rows=[]
    steps=[('omitted_defaults',{}),('explicit_extra',{'Sidebar.MixFileIndex':'4','Sidebar.YuriFileNames':'yes'}),
        ('later_omitted_retains',{}),('negative_retained',{'Sidebar.MixFileIndex':'-1','Sidebar.YuriFileNames':'yes'}),
        ('zero_false',{'Sidebar.MixFileIndex':'0','Sidebar.YuriFileNames':'no'}),
        ('partial_override',{'Sidebar.MixFileIndex':'5'}),
        ('empty_retains',{'Sidebar.MixFileIndex':'','Sidebar.YuriFileNames':''}),
        ('invalid_retains',{'Sidebar.MixFileIndex':'garbage','Sidebar.YuriFileNames':'?'}),
        ('dollar_decimal',{'Sidebar.MixFileIndex':'$10'}),
        ('suffix_hex',{'Sidebar.MixFileIndex':'10h'})]
    for name,values in steps:
        p.reads={('FourthSynthetic',k):v for k,v in values.items()};before=p.state(index);begin=len(p.events)
        p.invoke(0x1002B450,0,ecx=p.exts[index],end=0x1002E57B,
            required=(0x1002E450,0x1002E560,0x1002E576))
        events=p.events[begin:]
        rows.append(dict(name=name,supplied_ascii_reads=values,before=before,after=p.state(index),
            selected_reads=[e for e in events if e.get('key') in ('Sidebar.MixFileIndex','Sidebar.YuriFileNames')],
            diagnostic_events=[e for e in events if e.get('service')!='supplied_ReadString'],
            stop_before='1002E57B'))
    return dict(fixture_side_order=ids,selected_side_index=index,initial=initial,rows=rows,
        boundary='Supplied phase2; continuous selected ReadINI prefix executes; unrelated reader tail and final phase store excluded.')


def phase_dispatch_controls(images):
    rows=[]
    for phase in (0,1,2,-1,3):
        p=SideFixture(images).setup(['SyntheticSide']);ext=p.exts[0];p.write(ext+4,phase);events=[]
        def sink(u,pc,size,data):
            if pc==0x1002E450:
                events.append(dict(service='recorded_complete_reader_boundary',phase_at_entry=p.state(0)['phase']))
                p.ret(0,4)
        p.u.hook_add(UC_HOOK_CODE,sink)
        p.invoke(0x1002B450,0,ecx=ext)
        rows.append(dict(initial_phase=phase,after=p.state(0),events=events))
    return rows


def read_all_control(images):
    p=SideFixture(images).setup(['SyntheticA','SyntheticSkipped','SyntheticB']);ids=['SyntheticA',None,'SyntheticB']
    p.write(p.read(0x8B4124)+4,0)
    for ext in p.exts:p.write(ext+4,2)
    shim,stack,ini=p.alloc(0x40),p.alloc(0x40),p.alloc(0x40)
    p.write(shim+0x14,stack);p.write(stack+4,ini);events=[]
    def sink(u,pc,size,data):
        if pc==0x1002E450:
            ext=u.reg_read(UC_X86_REG_ECX);sp=u.reg_read(UC_X86_REG_ESP)
            events.append(dict(side=p.string(p.read(ext)+0x24),received_original_ini=p.read(sp+4)==ini))
            p.ret(0,4)
    p.u.hook_add(UC_HOOK_CODE,sink);p.invoke(0x1002EFC0,shim)
    return dict(initial_registered_side_ids=ids,initial_extension_phase=2,events=events,
        boundary='Real existing-Side loop and phase dispatcher; whole field reader is a recorded sink. No section-presence guard or physical INI byte ingestion.')


def selector_controls(images):
    p=SideFixture(images).setup(['GDI','Nod','ThirdSide','FourthSynthetic']);rows=[]
    for side,mix,yuri in ((2,3,True),(3,4,True),(2,3,False),(3,1,True),(0,4,True),(3,0,False),(3,-1,False),(3,-2147483648,False)):
        p.write(p.exts[side]+0x88,mix);p.u.mem_write(p.exts[side]+0x8C,bytes([yuri]))
        rows.append(dict(input=dict(actual_side=side,mix_file_index=mix,yuri_file_names=yuri),selection=p.selection(side)))
    return dict(rows=rows,boundary='Explicit existing typed fields; actual MIX/SHP/palette branch hooks and original numeric filename formatting. No House rebind, mount or rendering.')


def fallback_control(images):
    p=SideFixture(images);events=[]
    for index,slot in enumerate(ARCHIVE_SLOTS):p.write(slot,0x21000000+index*0x100)
    def sink(u,pc,size,data):
        if pc==0x4068E0:events.append('suppressed_DebugLog');p.ret(0)
        elif pc==0x4A80D0:events.append('original_disk_selector')
    p.u.hook_add(UC_HOOK_CODE,sink)
    p.invoke(0x534FC6,end=0x534FFA,required=(0x4A80D0,))
    return dict(selector_bytes=bytes(p.u.mem_read(0x4A80D0,6)).hex(),returned_al=p.u.reg_read(UC_X86_REG_EAX)&255,
        archive_sentinels_unchanged=[p.read(s)==0x21000000+i*0x100 for i,s in enumerate(ARCHIVE_SLOTS)],events=events,
        boundary='This executable disk-selector leaf and fallback execute; log is a sink; stop after epilogue before RET. The result is binary-specific.')


def archive_request_controls(images):
    rows=[]
    for number,available in ((3,{'SIDEC03MD.MIX','SIDEC03.MIX','SIDENC03.MIX'}),(4,{'SIDEC04.MIX'}),(3,{'SIDEC03MD.MIX'})):
        p=SideFixture(images);events=[];files={};vt=p.alloc(16);p.write(vt,0x30000300)
        for slot in ARCHIVE_SLOTS:
            obj=p.alloc(0x28);p.write(obj,vt);p.write(obj+0xC,p.text('OldSynthetic.mix'));p.write(slot,obj)
        filevt=p.alloc(0x40);p.write(filevt+0x14,0x30000320)
        def sink(u,pc,size,data):
            sp=u.reg_read(UC_X86_REG_ESP);arg=lambda n:p.read(sp+4+n*4);this=u.reg_read(UC_X86_REG_ECX)
            if pc==0x4068E0:return p.ret(0)
            if pc==0x30000300:
                events.append(dict(service='old_archive_destructor',slot=f'{next(s for s in ARCHIVE_SLOTS if p.read(s)==this):08X}'));return p.ret(0,4)
            if pc==0x4739F0:
                files[this]=p.string(arg(0));p.write(this,filevt);return p.ret(this,4)
            if pc==0x30000320:
                name=files[this];events.append(dict(service='supplied_file_availability',name=name,present=name in available));return p.ret(int(name in available),4)
            if pc in (0x43AE50,0x431B80):return p.ret(0)
            if pc==0x7C8E17:return p.ret(p.alloc(arg(0)))
            if pc==0x5B3C20:
                name=p.string(arg(0));events.append(dict(service='supplied_archive_constructor',name=name));p.write(this,vt);p.write(this+0xC,arg(0));return p.ret(this,8)
        p.u.hook_add(UC_HOOK_CODE,sink);p.u.reg_write(UC_X86_REG_EBX,number-1)
        end=0x5352E3 if f'SIDEC{number:02d}.MIX' in available else 0x535247
        p.invoke(0x535003,end=end,required=(0x5350D3,0x535179))
        rows.append(dict(input=dict(mix_file_index=number,available_archives=sorted(available)),events=events,
            reaches_ui_reload=end==0x5352E3,returned_al_on_failure=(p.u.reg_read(UC_X86_REG_EAX)&255) if end!=0x5352E3 else None))
    return dict(rows=rows,boundary='Original controller/formatting/retirement/early-return; destructor, heap, file availability and archive constructor are returning sinks. No MIX byte parser/list registration or actual resource payload.')


def cache_retirement_control(images):
    p=SideFixture(images);slots=(0x844C08,0x844C04,0x844C00,0x844BF0,0x844BF4)
    call_names=[p.string(p.read(s)) for s in slots];names=list(dict.fromkeys(call_names))+['SyntheticKeep.SHP']
    def crc(name):
        p.invoke(0x5B4270,ecx=p.text(name),end=0x5B42D3)
        return p.u.reg_read(UC_X86_REG_EAX)
    hashes={name:crc(name) for name in names};nodes={}
    for index,name in enumerate(names):
        node=p.alloc(16);p.write(node,0);p.write(node+4,0);p.write(node+8,hashes[name]);p.write(node+12,0x22000000+index*0x100);nodes[name]=node
    signed=lambda x:struct.unpack('<i',struct.pack('<I',x))[0]
    def tree(items):
        if not items:return 0
        items=sorted(items,key=lambda name:signed(hashes[name]));index=len(items)//2;node=nodes[items[index]]
        p.write(node,tree(items[:index]));p.write(node+4,tree(items[index+1:]));return node
    p.write(0xABF00C,tree(names));before={name:p.read(nodes[name]+12)!=0 for name in names}
    p.invoke(0x730400,required=(0x730421,0x73045A,0x730493,0x7304CC,0x730505,0x5B4270))
    after={name:p.read(nodes[name]+12)!=0 for name in names}
    assert not any(after[name] for name in names[:-1]) and after[names[-1]]
    return dict(invalidation_call_names=call_names,before_cached=before,after_cached=after,
        boundary='Existing nonzero cache-node values with native-computed CRCs; UI payload/surface globals zero. Actual selective key retirement, no payload free/destructor or subsequent rendering.')


def installed_hooks(images):
    p=SideFixture(images)
    wanted={'SideClass_CTOR','SideClass_LoadAllFromINI','RulesData_LoadBeforeTypeData','Sides_MixFileIndex',
        'Sides_MixFileYuriFiles1','Sides_MixFileYuriFiles2','Sides_MixFileYuriFiles3','SideClass_SaveLoad_Prefix','SideClass_Load_Suffix','SideClass_Save_Suffix'}
    return [dict(site=f'{p.read(at):08X}',length=p.read(at+4),name=p.string(p.read(at+8)))
        for at in range(0x100C3000,0x100C8A80,16) if p.string(p.read(at+8)) in wanted]


def filename_sets(images):
    p=SideFixture(images)
    text=lambda slot:p.string(p.read(slot))
    return dict(
        plain=dict(radar=text(0x844D14),backgrounds=[text(s) for s in (0x844D08,0x844D04,0x844D00)],radar_palette=text(0x844C00),background_palette=text(0x844BF4)),
        yuri=dict(radar=text(0x844D48),backgrounds=[text(s) for s in (0x844D44,0x844D3C,0x844D40)],radar_palette=text(0x844BFC),background_palette=text(0x844BF8)),
        common_palettes=[text(s) for s in (0x844BF0,0x844C04,0x844C08)],
        boundary='Actual file-backed filename pointer slots used at72FA23/72FA6A and72F379/72F3A0; no resource payload or rendered pixels.')


def generate(ares):
    images=ares,native.image_bytes()
    return dict(schema_version=1,installed_hook_records=installed_hooks(images),filename_sets=filename_sets(images),constructor=constructor(images),
        initial=initial_controls(images),retained=retained_controls(images),phase_dispatch=phase_dispatch_controls(images),
        read_all=read_all_control(images),selector=selector_controls(images),fallback=fallback_control(images),
        archive_requests=archive_request_controls(images),cache_retirement=cache_retirement_control(images))


def metadata():
    return provenance(scope='Installed Ares Side selected sidebar defaults/retained reads, phase and existing-Side traversal, independent MIX/Yuri filename selection, bounded archive controller and palette cache retirement.',
        assumptions=[
            'Synthetic existing Side arrays/SideExt associations, lexical ASCII ReadString values and initial phase are declared inputs; no physical INI ingestion, whole Process or full DLL startup.',
            'Constructor control is only the literal-store prefix1002D9F0..1002DA89; four full selected initializers execute with an empty Aircraft registry.',
            'Retained controls enter dispatcher at supplied phase2 and stop before1002E57B. Actual selected reader/numeric/Boolean instructions execute; later unrelated fields and final phase store are excluded.',
            'Phase and read-all controls execute actual controller/initializer; complete reader is a recorded returning sink. This establishes call order/phase stores, not reader-tail effects.',
            'Positive archive controls execute original formatting, optional/required branches and old-slot retirement; no actual archive byte parser, global registration, file search or GPU execution.',
            'Fallback result is bounded to the actual enrolled executable selector bytes. INT_MIN dec/add wrapping and negative numeric formatting execute.',
            'Five palette-key retirement calls clear four unique CRC values; a non-UI cache entry survives. Raw payload free/surface destruction, Yuri palette special lifetimes and whole scenario reload remain unexecuted.',
            'Whole SideExt stream persistence is established separately by instruction spans1002D410/1002D1D0 and hooks; this corpus does not execute the whole extension stream.',
            'Selected controls reject observed RNG/detach entries; no whole-startup random-order, pixel or full Ares compatibility claim.'
        ],substitutions=[
            'Public ares_prerequisites.Probe/native_oracle own PE mapping, arena and execution; no copied mapper or numeric parser.',
            'ReadString lexical returns, existing SideExt lookup, inherited ASCII comparison/copy and diagnostic leaves are supplied; Ares integer/Boolean parser and linked scanner execute.',
            'Ares CRT thread accessor10085F5C reuses ini_token_readers.Crt.thread_block to return declared per-thread storage. Actual initializer10085839 selects the file-backed C locale; PTD+4C is supplied that same locale pointer, following10085D5E initialization. No numeric scanner result is supplied.',
            'Archive file availability, constructor/destructor, successful heap backing and log leaves are explicit transport sinks.',
            'Phase/read-all whole-reader callback returns without emulating field writes; cache control supplies only existing nodes and zero UI payload/surface globals.'
        ],entry_points=dict(constructor=0x1002D9F0,initializer=0x1002E190,dispatcher=0x1002B450,read_prefix=0x1002E450,
            read_all=0x1002EFC0,mix_hook=0x1002FA00,yuri_helper=0x1002EAA0,archive_controller=0x534FA0,
            archive_suffix=0x535003,ui_reload=0x72F350,palette_retire=0x730400,cache_key_retire=0x5B4270))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ares-dll',type=Path,default=os.environ.get('VERA20K_ARES_DLL'))
    parser.add_argument('--gamemd-exe',type=Path)
    args,remaining=parser.parse_known_args()
    if args.ares_dll is None:parser.error('Set VERA20K_ARES_DLL or supply --ares-dll')
    if args.gamemd_exe:os.environ['VERA20K_GAMEMD_EXE']=str(args.gamemd_exe)
    ares=identified_bytes(args.ares_dll,ARES_SHA256)
    finish_vectors(lambda:generate(ares),Path(__file__).with_suffix('.json'),provenance=metadata,argv=remaining,
        source_paths={'producer':Path(__file__),'runner':Path(native.__file__),
            'vm_owner':Path(__file__).with_name('ares_prerequisites.py'),'return_owner':Path('tools/input_oracle/fast_scroll.py'),'crt_owner':Path('tools/rules_oracle/ini_token_readers.py')})


if __name__=='__main__':main()
