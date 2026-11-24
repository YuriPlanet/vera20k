"""Original InfantryType sequence constructor loop and complete ReadSequenceData.

The fixture supplies cached INI indexes at ReadString entry; original ReadString,
sscanf, case-sensitive facing comparisons and 42-action traversal execute intact.
No sequence Sounds values are supplied: sound registry population is out of scope.
"""
import argparse
import hashlib
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import (load_image, run_checked, call, SCRATCH,
    STACK_BASE, STACK_SIZE, RET_MAGIC, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

TYPE, RECORDS, SECTION, INDEX, ENTRY, RAW = [SCRATCH+n*0x2000 for n in range(6)]
SP = STACK_BASE+STACK_SIZE-0x1000
INI = 0x887180
SPANS = [(0x52392C,0x523970),(0x523D00,0x524097)]
DISCHARGE_FIELDS = [('FireUp',0xE40),('FireProne',0xE44),
                    ('SecondaryFire',0xE48),('SecondaryProne',0xE4C)]
DISCHARGE_SPANS = [(0x5236A7,0x5236A9),(0x5236D7,0x5236F9),
                   (0x5246BE,0x52473A),(0x5276D0,0x5278E4)]
DEFAULT_PAYLOAD_SHA256 = '03d8276eaee8ac553333412005bdc0602387ddbe6ded9e4fd06295c66871055f'


class Fixture:
    def __init__(self, *, discharge=False):
        self.u=Uc(UC_ARCH_X86,UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH,0x10000)
        self.u.mem_map(STACK_BASE,STACK_SIZE)
        self.u.mem_map(RET_MAGIC,0x1000)
        self.u.reg_write(UC_X86_REG_FPCW,0x0E7F)
        self.original=[bytes(self.u.mem_read(a,b-a)) for a,b in SPANS]
        self.crcs={}
        self.values={}
        self.reads=[]
        self.names=[self.string(self.word(0x8255C8+4*n)) for n in range(42)]
        # Prepare fixture CRCs before emulation. Creating/collecting a nested
        # Unicorn VM inside a callback can stop the active parent VM.
        for key in ['Sequence',*self.names,*(name+'Sounds' for name in self.names),
                    *(key for key,_ in DISCHARGE_FIELDS)]:
            if key not in self.crcs:
                name=key.encode('ascii')
                self.crcs[key]=call(0x4A1DE0,ecx=SCRATCH,
                    stack_args=[SCRATCH+0x100,len(name)],
                    writes={SCRATCH:bytes(16),SCRATCH+0x100:name})['eax']
        self.u.hook_add(UC_HOOK_CODE,self.read_boundary,begin=0x528A10,end=0x528A10)
        self.discharge_reads=[]
        if discharge:
            self.discharge_original=[bytes(self.u.mem_read(a,b-a)) for a,b in DISCHARGE_SPANS]
            self.u.hook_add(UC_HOOK_CODE,self.discharge_read_boundary,begin=0x5276D0,end=0x5276D0)
            for address in (0x5246DB,0x5246F8,0x524715,0x524732):
                self.u.hook_add(UC_HOOK_CODE,self.discharge_read_return,begin=address,end=address)

    def word(self,addr):
        return struct.unpack('<I',self.u.mem_read(addr,4))[0]

    def string(self,addr):
        return bytes(self.u.mem_read(addr,256)).split(b'\0')[0].decode('ascii')

    def read_boundary(self,_u,_a,_size,_data):
        u=self.u
        sp=u.reg_read(UC_X86_REG_ESP)
        section_ptr=self.word(sp+4)
        section,key,raw=self.prepare_cached_read(section_ptr,self.word(sp+8))
        self.reads.append([section,key,raw])

    def prepare_cached_read(self,section_ptr,key_ptr):
        """One fixture owner supplies cache backing for the original INI readers."""
        u=self.u
        section=self.string(section_ptr)
        key=self.string(key_ptr)
        raw=self.values.get((section,key))
        assert key in self.crcs,('Unprepared original reader key',key)
        # Only the section cache/index backing data are supplied. ReadString
        # and ReadInt execute their original CRC, entry search and parsing.
        u.mem_write(INI,bytes(0x40))
        u.mem_write(INI+4,dwords(section_ptr,SECTION))
        u.mem_write(SECTION+0x2C,dwords(INDEX,int(raw is not None)))
        u.mem_write(SECTION+0x38,dwords(1,INDEX))
        u.mem_write(INDEX,dwords(self.crcs[key],ENTRY))
        u.mem_write(ENTRY+0x10,dwords(RAW))
        u.mem_write(RAW,(raw or '').encode('ascii')+b'\0')
        return section,key,raw

    def discharge_read_boundary(self,_u,_a,_size,_data):
        sp=self.u.reg_read(UC_X86_REG_ESP)
        section,key,raw=self.prepare_cached_read(self.word(sp+4),self.word(sp+8))
        default=struct.unpack('<i',self.u.mem_read(sp+12,4))[0]
        self.discharge_reads.append(dict(section=section,key=key,raw=raw,default=default))

    def discharge_read_return(self,_u,_a,_size,_data):
        result=self.u.reg_read(UC_X86_REG_EAX)
        self.discharge_reads[-1]['result']=struct.unpack('<i',dwords(result))[0]

    def discharge_state(self):
        return [struct.unpack('<i',self.u.mem_read(TYPE+offset,4))[0]
                for _,offset in DISCHARGE_FIELDS]

    def discharge_constructor(self):
        u=self.u
        u.mem_write(TYPE+0xE40,b'\xCD'*16)
        u.reg_write(UC_X86_REG_EBX,0x12345678)
        run_checked(u,0x5236A7,0x5236A9)
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ESI,TYPE)
        run_checked(u,0x5236D7,0x5236F9)
        return dict(before=[-842150451]*4,after=self.discharge_state(),
                    ebx=u.reg_read(UC_X86_REG_EBX),stack_bytes_pushed=SP-u.reg_read(UC_X86_REG_ESP))

    def execute_discharge(self,layers,initial=None,section='TEST',crawls=False):
        u=self.u
        constructor=self.discharge_constructor()
        if initial is not None:
            u.mem_write(TYPE+0xE40,dwords(*initial))
        before=self.discharge_state()
        u.mem_write(TYPE+0x1F8,section.encode('ascii')+b'\0')
        self.discharge_reads=[]
        after_layers=[]
        for layer in layers:
            self.values={(section,key):raw for key,raw in layer.items()}
            u.reg_write(UC_X86_REG_ESP,SP)
            u.reg_write(UC_X86_REG_ESI,TYPE)
            u.reg_write(UC_X86_REG_EDI,TYPE+0x1F8)
            u.reg_write(UC_X86_REG_EAX,int(crawls))
            run_checked(u,0x5246BE,0x52473A,count=100000)
            assert u.reg_read(UC_X86_REG_ESP)==SP
            after_layers.append(self.discharge_state())
        assert self.discharge_original==[bytes(u.mem_read(a,b-a)) for a,b in DISCHARGE_SPANS]
        assert [read['key'] for read in self.discharge_reads]==[
            key for _ in layers for key,_ in DISCHARGE_FIELDS]
        return dict(section=section,layers=layers,initial=initial,before=before,
                    after_layers=after_layers,after=self.discharge_state(),
                    reads=self.discharge_reads,crawls=bool(u.mem_read(TYPE+0xEBD,1)[0]),
                    constructor=constructor)

    def constructor(self):
        u=self.u
        u.mem_write(RECORDS,b'\xCD'*0x5E8)
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ESI,TYPE)
        u.reg_write(UC_X86_REG_EAX,RECORDS)
        u.reg_write(UC_X86_REG_EBX,0)
        u.reg_write(UC_X86_REG_EDI,0xFFFFFFFF)
        run_checked(u,0x52392C,0x523970)

    def records(self):
        return [list(struct.unpack('<5i',self.u.mem_read(RECORDS+n*36,20)))
                for n in range(42)]

    def execute(self,layers,initial=None,sequence='ArbitraryActions'):
        u=self.u
        self.constructor()
        if initial is not None:
            for n in range(42):
                u.mem_write(RECORDS+n*36,dwords(*initial,0))
        u.mem_write(TYPE+0x1F8,b'TEST\0')
        before=self.records()
        self.reads=[]
        for layer in layers:
            self.values={('TEST','Sequence'):sequence}
            self.values.update({(sequence,k):v for k,v in layer.items()})
            u.mem_write(SP,dwords(RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP,SP)
            u.reg_write(UC_X86_REG_ECX,TYPE)
            run_checked(u,0x523D00,RET_MAGIC,count=200000)
        assert self.original==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
        return dict(layers=layers,initial=initial,sequence=sequence,
                    before=before,after=self.records(),reads=self.reads)


def generate():
    f=Fixture()
    rows=[]
    values=[None,'','1,2,3','-4,-5,-6','2147483648,4294967295,4294967296',
        '99999999999999999999999999999999,2,3','1','1,','1,,3','1,2x,3',
        '1 ,2,3','1, 2, 3','+,-1,2','x,2,3','1,0x10,3','1,\t-2,3',
        '1,\x1f2,3','1,2,3junk','1,2,3,4','"1,2,3"',' 1,2,3 ',
        ' '*30+'1,2,3','\v1,2,3','1,\f2,3','1,2,3,N','1,2,3,n',
        '1,2,3,NE','1,2,3, E','1,2,3,SE tail','1,2,3,S','1,2,3,SW',
        '1,2,3,W','1,2,3,NW','1,2,3,BAD','1,2,3,S; tail']
    for initial in (None,[7,-9,11,5]):
        for raw in values:
            rows.append(f.execute([{'Deploy':raw}],initial))
    rows += [f.execute([{'Ready':'1,2,3','Guard':'4,5,6'}]),
             f.execute([{'Deploy':'1,2,3,S'},{'Deploy':'9'}]),
             f.execute([{'Deploy':'1,2,3,S'},{'Deploy':'9,8,7,n'}]),
             f.execute([{'Deploy':'1,2,3,S'},{}]),
             f.execute([{'Deploy':'1,2,3'}],sequence=None),
             f.execute([{name:f'{n},{n+1},{n+2}' for n,name in enumerate(f.names)}])]
    return dict(schema_version=1,names=f.names,cases=rows)


def metadata():
    f=Fixture()
    out=provenance(scope='InfantryType sequence constructor and complete42-action ReadSequenceData, numeric/facing fields',
        assumptions=['Supplied cached section/index backing data; original ReadString and sscanf retained',
                     'No sequence Sounds entries supplied; sound registry and successful sound reads excluded',
                     'Initial retained records are explicitly supplied for reload cases; constructor runs first',
                     'Constructor allocation supplied; original initialization loop runs on all42 records'],
        substitutions=[],entry_points={'constructor_loop':0x52392C,'read_sequence':0x523D00})
    out['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),
        sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return out


def generate_discharge():
    from tools.projectile_oracle.bridge_render_inputs import assets_root,lexical
    f=Fixture(discharge=True)
    values=[None,'','-9','256','2147483647','-2147483648','2147483648',
            '4294967295','4294967296','9999999999999999999999999999999',
            'junk','+17tail','$FF','100h','0x10','  -513  ','$invalid','invalidh']
    rows=[]
    for initial in (None,[7,-9,300,-400]):
        for key,_ in DISCHARGE_FIELDS:
            for raw in values:
                row=f.execute_discharge([{key:raw}],initial)
                row['case']=f'{key}/{raw!r}/{initial!r}'
                rows.append(row)
    controls=[('all_independent',[{'FireUp':'-9','FireProne':'256',
                    'SecondaryFire':'2147483647','SecondaryProne':'-2147483648'}]),
        ('case_sensitive',[{'fireup':'9','FIREPRONE':'10','secondaryfire':'11','secondaryprone':'12'}]),
        ('reload_omit',[{'FireUp':'2','FireProne':'-7','SecondaryFire':'300','SecondaryProne':'-400'},{}]),
        ('reload_independent',[{'FireUp':'2','FireProne':'-7','SecondaryFire':'300','SecondaryProne':'-400'},
                               {'FireUp':'256','SecondaryFire':'-8'}]),
        ('reload_empty_and_failed_hex',[{'FireUp':'2','FireProne':'-7','SecondaryFire':'300','SecondaryProne':'-400'},
                                        {'FireUp':'','FireProne':'$invalid','SecondaryFire':'invalidh','SecondaryProne':'junk'}])]
    for name,layers in controls:
        row=f.execute_discharge(layers);row['case']=name;rows.append(row)
    path=assets_root()/'ARTMD.INI'
    raw=path.read_bytes();sections,lines=lexical(raw,{'GI'})
    row=f.execute_discharge([sections['GI']],section='GI',crawls=True)
    row['case']='physical_GI';row['retail_source']={
        'file':'ARTMD.INI','sha256':hashlib.sha256(raw).hexdigest(),'section_lines':lines}
    rows.append(row)
    return dict(schema_version=1,field_order=[key for key,_ in DISCHARGE_FIELDS],
                field_offsets=[offset for _,offset in DISCHARGE_FIELDS],cases=rows)


def discharge_metadata():
    f=Fixture(discharge=True)
    out=provenance(scope='Original InfantryType ART discharge-frame prefix and signed ReadInt, independent retained defaults',
        assumptions=['Existing fixture supplies cached section/index backing data; original ReadInt and CRT parsing execute',
                     'Constructor receipt executes original EBX zeroing and four stores, excluding allocation/base constructor',
                     'ReadINI is entered at 5246BE; preceding Crawls result in AL and ESI/EDI caller registers are supplied',
                     'Physical GI ART lexical values come from the existing retail input owner; archive/file loading is supplied',
                     'Retained four-DWORD initial values are supplied explicitly after the original constructor stores'],
        substitutions=[],entry_points={'constructor_zero':0x5236A7,'constructor_stores':0x5236D7,
                                        'read_art_prefix':0x5246BE,'read_int':0x5276D0})
    out['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
        hex=bytes(f.u.mem_read(a,b-a)).hex(),
        sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in DISCHARGE_SPANS]
    out['preserved_default_payload_sha256']=DEFAULT_PAYLOAD_SHA256
    return out


if __name__=='__main__':
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--discharge',action='store_true')
    selected,remaining=parser.parse_known_args()
    if selected.discharge:
        finish_vectors(generate_discharge,Path(__file__).with_name('infantry_discharge_rules.json'),
            provenance=discharge_metadata,argv=remaining,
            source_paths={'generator':Path(__file__),
                          'retail_lexical_owner':Path(__file__).parents[1]/'projectile_oracle'/'bridge_render_inputs.py'})
    else:
        finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata,argv=remaining)
