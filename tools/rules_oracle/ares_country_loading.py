"""Country loading GUI reads through the existing CountryFixture owner.
All inputs are synthetic. No mod data, artwork or resource extraction is used.
"""
import argparse
import os
from pathlib import Path
from tools import native_oracle as native
from tools.native_oracle import finish_vectors, provenance
from tools.rules_oracle.ares_prerequisites import ARES_SHA256, identified_bytes
from tools.rules_oracle.ares_country_veterancy import CountryFixture
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ESI, UC_X86_REG_EAX, UC_X86_REG_ECX
FIELDS={'file':0x2C,'palette':0x4C,'name':0x8C,'special':0xB0,'brief':0xD4}
COLOR_NAMES=['SCHEME_A','SCHEME_B','AlliedLoad','SovietLoad','YuriLoad','Purple']

def fixture(images,color_names=None):
 p=CountryFixture(images);u=p.u
 c,e,pc,pe,ini,other=(p.alloc(n) for n in [0x200,0x200,0x200,0x200,0x40,0x40])
 p.put(e,c,2);p.put(pe,pc,2);p.put(c+0xC4,e);p.put(pc+0xC4,pe)
 u.mem_write(c+0x24,b'CHILD_A\0');u.mem_write(pc+0x24,b'PARENT_A\0');u.mem_write(c+0x98,b'CHILD_A\0')
 p.put(0x887048,ini);p.put(0xB1CF74,0)
 arr=p.alloc(0x20);p.put(arr,c,pc);p.put(0xA83C9C,arr);p.put(0xA83CA8,2)
 p.put(e+0x11C,1);p.put(pe+0x11C,3)
 for label,ext in [('CHILD',e),('PARENT',pe)]:
  for field,off in FIELDS.items():u.mem_write(ext+off,(label+'_'+field).encode()+b'\0')
 colors=[]
 for name in COLOR_NAMES if color_names is None else color_names:
  for kind in [1,53]:
   col=p.alloc(0x320);p.put(col+0x304,p.text(name));p.put(col+0x310,kind);colors.append(col)
 ca=p.alloc(0x80);p.put(ca,*colors);p.put(0xB054D4,ca);p.put(0xB054E0,len(colors))
 values={};events=[]
 def service(_u,at,_size,_data):
  if at==0x528A10:
   sp=u.reg_read(UC_X86_REG_ESP)
   section,key,default,target,capacity=[p.read(sp+4+i*4) for i in range(5)]
   k=p.string(key);val=values.get(k);raw=(val or '').encode('ascii')[:capacity-1]
   u.mem_write(target,raw+b'\0');events.append(dict(kind='ReadString',section=p.string(section),key=k,default=p.string(default),capacity=capacity,supplied=val))
   p.service_return(20,len(raw))
  elif at in [0x10061B00,0x10061960]:
   # Diagnostics do not decide a selected field; explicit transport boundary.
   p.service_return(0 if at==0x10061B00 else 16,0)
  elif at==0x10076470:
   events.append(dict(kind='PCXResourceLoadBoundary',name=p.string(u.reg_read(UC_X86_REG_ECX))))
   p.service_return(0,0)
  elif at==0x10005970:
   events.append(dict(kind='SHPResourceLoadBoundary',name=p.string(u.reg_read(UC_X86_REG_ECX))))
   p.service_return(0,0)
 u.hook_add(UC_HOOK_CODE,service)
 def snapshot():return dict(phase=p.read(e+4),parent_initialized=u.mem_read(e+0x1B9,1)[0],**{k:p.string(e+off) for k,off in FIELDS.items()},color=p.read(e+0x11C),cached_csf={k:p.read(e+off+0x20) for k,off in FIELDS.items() if k in ['name','special','brief']})
 return p,c,e,pc,pe,ini,other,values,events,snapshot

def history(images,name,steps,parent=False,phase=2,side=3,color_names=None):
 p,c,e,pc,pe,ini,other,values,events,snapshot=fixture(images,color_names)
 p.put(e+4,phase);p.put(c+0xBC,side)
 if parent:p.u.mem_write(c+0x98,b'PARENT_A\0')
 initial=snapshot();rows=[]
 for step in steps:
  values.clear();values.update(step.get('values',{}));start=len(events)
  p.call(0x1001C530,(ini if step['primary'] else other,),e)
  rows.append(dict(input=step,after=snapshot(),reads=events[start:]))
 return dict(name=name,color_registry=COLOR_NAMES if color_names is None else color_names,declared_initial=initial,declared_parent=dict(**{k:'PARENT_'+k for k in FIELDS},color=3),parent_country='PARENT_A' if parent else 'CHILD_A',country_side=side,rows=rows)
def generate(ares):
 images=(ares,native.image_bytes())
 controls=[
  history(images,'primary_then_nonprimary_then_missing_empty',[
   dict(primary=True,values={'File.LoadScreen':'ls%sfixture.shp','File.LoadScreenPAL':'fixture.pal','LoadScreenText.Name':'Name:Fixture','LoadScreenText.SpecialName':'Name:Unit','LoadScreenText.Brief':'Brief:Fixture','LoadScreenText.Color':'SCHEME_B'}),
   dict(primary=False,values={'File.LoadScreen':'ignored.shp','LoadScreenText.Name':'Ignored:Name','LoadScreenText.Color':'SCHEME_A'}),
   dict(primary=True,values={}),dict(primary=True,values={k:'' for k in ['File.LoadScreen','File.LoadScreenPAL','LoadScreenText.Name','LoadScreenText.SpecialName','LoadScreenText.Brief','LoadScreenText.Color']})]),
  history(images,'sentinels_files_clear_csf_labels_literal_color_minus_one',[dict(primary=True,values={'File.LoadScreen':'NoNe','File.LoadScreenPAL':'<NoNe>','LoadScreenText.Name':'none','LoadScreenText.SpecialName':'<none>','LoadScreenText.Brief':'none','LoadScreenText.Color':'none'})]),
  history(images,'unknown_color_retains',[dict(primary=False,values={'LoadScreenText.Color':'MISSING_SCHEME'})]),
  history(images,'fixed31byte_destinations',[dict(primary=True,values={'File.LoadScreen':'A'*50+'.shp','File.LoadScreenPAL':'B'*50+'.pal','LoadScreenText.Name':'N'*50,'LoadScreenText.SpecialName':'S'*50,'LoadScreenText.Brief':'T'*50})]),
  history(images,'first_primary_inherit_overwrites_gui_then_later_primary_child_wins',[
   dict(primary=True,values={'File.LoadScreen':'child_first.shp','File.LoadScreenPAL':'child_first.pal','LoadScreenText.Name':'Name:First','LoadScreenText.Color':'SCHEME_A'}),
   dict(primary=True,values={'File.LoadScreen':'child_second.shp','LoadScreenText.Name':'Name:Second'})],parent=True),
  history(images,'nonprimary_can_inherit_gui',[dict(primary=False,values={'File.LoadScreen':'ignored.shp','LoadScreenText.Color':'SCHEME_A'})],parent=True),
  history(images,'negative_phase_skips_all',[dict(primary=True,values={'File.LoadScreen':'ignored.shp','LoadScreenText.Color':'SCHEME_B'})],phase=-1),
  history(images,'phase3_skips_all',[dict(primary=True,values={'File.LoadScreen':'ignored.shp','LoadScreenText.Color':'SCHEME_B'})],phase=3),
  history(images,'phase0_side2_default_color_then_common',[dict(primary=False,values={})],phase=0,side=2),
  history(images,'phase0_side3_keeps_prior_color',[dict(primary=False,values={})],phase=0,side=3),
 history(images,'phase0_missing_allied_default_writes_minus_one',[dict(primary=False,values={})],phase=0,side=0,color_names=[]),
 history(images,'phase0_missing_yuri_uses_current_purple',[dict(primary=False,values={})],phase=0,side=2,color_names=[name for name in COLOR_NAMES if name!='YuriLoad']),
 ]
 initializers=[]
 for name in ['Americans','Alliance','French','Germans','British','Africans','Arabs','Confederation','Russians','YuriCountry','COUNTRY_A']:
  p,c,e,pc,pe,ini,other,values,events,snapshot=fixture(images);p.u.mem_write(c+0x24,name.encode()+b'\0')
  # Original inlined ctor scalar initialization prefix, with enclosing allocation
  # and registry insertion outside scope. Actual subsequent ID-default initializer.
  u=p.u;u.reg_write(UC_X86_REG_ESI,e);u.reg_write(UC_X86_REG_EAX,c);u.reg_write(UC_X86_REG_ECX,e)
  native.run_checked(u,0x1001B767,0x1001B911,count=10000,timeout_us=1000000);p.check_code()
  ctor=snapshot();p.call(0x1001C0F0,ecx=e)
  initializers.append(dict(country_id=name,constructor_scalar_prior=ctor,after_id_defaults=snapshot(),resource_boundaries=events))
 formats=[]
 for template,width in [('ls%sfixture.shp','640'),('ls%sfixture.shp','800'),('ls800fixture.shp','640'),('ls%%sfixture.shp','800')]:
  p=CountryFixture(images);out=p.alloc(256)
  length=p.call(0x7C8EF4,(out,p.text(template),p.text(width)))
  formats.append(dict(template=template,width=width,output=p.string(out),return_length=length))
 return dict(fields=FIELDS,initializers=initializers,controls=controls,filename_formats=formats)

def metadata():
    return provenance(
        scope="CountryExt loading GUI retained reads, one-shot parent ordering, color Find, and literal-ID defaults; not rendered loading-screen parity.",
        assumptions=[
            "Synthetic Country/ColorScheme registries, primary Rules pointer and phase priors; no physical INI schedule or whole startup.",
            "Actual dispatcher1001C530, GUI1001C770, common1001C5D0, parent1001BDC0, Color Find1001BB30 and copy/sentinel instructions execute unchanged.",
            "ReadString lexical transport and diagnostics are declared sinks. Actual CSF lookup734E60 sees an empty catalog; no localized text/font output is certified.",
            "Constructor scalar prefix1001B767..1001B911 and ID initializer1001C0F0 execute for ten built-in IDs and one unknown ID. PCX/SHP initializer resource requests are named sinks, not asset decode or file loading evidence.",
            "Color fixture has paired kind1/kind53 entries like the production registry. Unknown color retains, none becomes signed-1.",
            "CountryFixture rejects monitored RNG/detach and verifies original code bytes. Native sprintf7C8EF4 executes four filename controls with one declared width argument. No full save/load, palette Convert or loading capture comparison is certified.",
        ], substitutions=[
            "Reuse ares_country_veterancy.CountryFixture and ares_prerequisites.Probe; no new VM, mapper, parser or heap owner.",
            "ReadString528A10 supplies declared ASCII strings. Diagnostic10061B00 uses cdecl cleanup0; parse diagnostic10061960 uses cleanup16.",
            "Named resource initializer sinks10076470/10005970 return null; they do not supply a loading field or inheritance decision.",
        ], entry_points=dict(dispatch=0x1001C530,gui=0x1001C770,common=0x1001C5D0,parent=0x1001BDC0,initialize=0x1001C0F0,color_find=0x1001BB30,filename_format=0x7C8EF4))

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ares-dll',type=Path,default=os.environ.get('VERA20K_ARES_DLL'))
    parser.add_argument('--gamemd-exe',type=Path)
    args,remaining=parser.parse_known_args()
    if args.ares_dll is None: parser.error('Set VERA20K_ARES_DLL or supply --ares-dll')
    if args.gamemd_exe: os.environ['VERA20K_GAMEMD_EXE']=str(args.gamemd_exe)
    ares=identified_bytes(args.ares_dll,ARES_SHA256)
    finish_vectors(lambda:generate(ares),Path(__file__).with_suffix('.json'),provenance=metadata,argv=remaining,
        source_paths={'producer':Path(__file__),'runner':Path(native.__file__),
          'country_owner':Path(__file__).with_name('ares_country_veterancy.py'),
          'vm_owner':Path(__file__).with_name('ares_prerequisites.py')})

if __name__=='__main__': main()
