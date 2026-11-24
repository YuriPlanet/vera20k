"""Two new bounded loading-score histories; sole EmpProbe VM and INI/CRT transport.
No corpus is regenerated. Actual Ares hooks, ReadTheme and From_Name run intact;
Play_Song is a request recording boundary, not a device/stream parity claim.
"""
from pathlib import Path
import sys,json
sys.dont_write_bytecode=True
from tools.rules_oracle.ares_emp import EmpProbe,identified_bytes,ARES_SHA256,native,signed
from tools.native_oracle import run_checked,finish_vectors,provenance
from tools.input_oracle.fast_scroll import return_from_sink
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ESP,UC_X86_REG_ECX
FIXTURE=Path(__file__).with_suffix(".json")
class LoadingScoreProbe(EmpProbe):
 def __init__(self,images):
  self.read_observations=[];self.play_requests=[];super().__init__(images)
  self.mapini=self.alloc(0x100);self.rulesini=self.alloc(0x100);self.frame=self.alloc(0x40)
  self.write(self.frame+0x14,self.mapini-0x1c);self.write(0x887048,self.rulesini)
  self.keys=['LOADING','SIDE_SCORE','COUNTRY_SCORE','MISSION_SCORE']
  entries=[self.alloc(0x2a0) for _ in self.keys];array=self.alloc(len(entries)*4)
  for i,(entry,key) in enumerate(zip(entries,self.keys)):
   self.u.mem_write(entry,key.encode()+b'\0');self.write(array+i*4,entry)
  self.write(0xa83d10+0x18,array);self.write(0xa83d10+0x24,len(entries))
  country=self.alloc(0x200);side=self.alloc(0x100);spot=self.alloc(0x80)
  self.u.mem_write(country+0x24,b'COUNTRY\0');self.u.mem_write(side+0x24,b'SIDE\0');self.write(country+0xbc,0);self.write(spot+0x4b,0)
  for global_address,item in [(0xa8da78,spot),(0xa83c9c,country),(0x8b4124,side)]:
   array=self.alloc(4);self.write(array,item);self.write(global_address,array)
  self.write(0xa8da84,1)
 def hook(self,u,pc,size,data):
  sp=u.reg_read(UC_X86_REG_ESP)
  if pc==0x528a10:
   args=[self.read(sp+4+i*4) for i in range(5)]
   source=u.reg_read(UC_X86_REG_ECX)
   self.read_observations.append(dict(source='map' if source==self.mapini else 'global_rules' if source==self.rulesini else f'{source:08X}',section=self.string(args[0]),key=self.string(args[1]),capacity=args[4],default=self.string(args[2])))
  if pc==0x720bb0:
   self.play_requests.append(signed(self.read(sp+4)));return return_from_sink(u,4,0)
  return super().hook(u,pc,size,data)
 def step(self,label,mode,rows):
  self.write(0xa8b238,mode);self.reads=dict(rows);self.events=[];self.read_observations=[];self.play_requests=[]
  a=self.call(0x1002fb80,self.frame);selected=signed(self.read(0x100c0130));b=self.call(0x1002fc20,self.frame);captured=signed(self.read(self.frame+0x24))
  sp=0x2000e000;self.write(sp,0x30000000);self.u.reg_write(UC_X86_REG_ESP,sp);self.u.reg_write(UC_X86_REG_EAX,self.read(self.frame+0x24))
  run_checked(self.u,0x683d05 if b==0 else b,0x683d1f,count=100000,context={'scope':'Ares loading score retail call-tail'})
  self.check_code()
  return dict(label=label,input=dict(game_mode=mode,rows=[dict(section=s,key=k,value=v) for (s,k),v in rows.items()]),hook_a_return=f'{a:08X}',selected=selected,hook_b_return=f'{b:08X}',captured_eax=captured,read_calls=self.read_observations,play_requests=self.play_requests)
def generate(ares_dll):
 images=(identified_bytes(ares_dll,ARES_SHA256),native.image_bytes());histories=[]
 for name,mode,stages in [('campaignBasic',0,[('missing',{}),('named', {('Basic','LoadingTheme'):'mIsSiOn_ScOrE'}),('unknown',{('Basic','LoadingTheme'):'UNKNOWN_SCORE'}),('empty',{('Basic','LoadingTheme'):''})]),('skirmishSideCountry',1,[('side_and_missing_country',{('SIDE','LoadingTheme'):'SIDE_SCORE'}),('country_overrides_side',{('SIDE','LoadingTheme'):'SIDE_SCORE',('COUNTRY','LoadingTheme'):'COUNTRY_SCORE'}),('unknown_country_overrides_side',{('SIDE','LoadingTheme'):'SIDE_SCORE',('COUNTRY','LoadingTheme'):'UNKNOWN_SCORE'}),('both_missing',{}),('valid_country_overrides_unknown_side',{('SIDE','LoadingTheme'):'UNKNOWN_SCORE',('COUNTRY','LoadingTheme'):'COUNTRY_SCORE'})])]:
  m=LoadingScoreProbe(images);rows=[m.step(label,mode,inputs) for label,inputs in stages];m.check_code();histories.append(dict(name=name,theme_keys=m.keys,stages=rows,code_guard=True,three_rng_unchanged=True))
 return dict(schema=1,status='Two new synthetic histories executed',binary_identity=dict(ares_sha256=ARES_SHA256),histories=histories,limits=['Original Ares hooks/retailReadTheme4758F0/From_Name721210 execute; registry and parsed ReadString input are supplied existing world/INI services.','Play_Song720BB0 request index recorded; no music decode/device/stream output parity.','Sole EmpProbe VM/CRT/INI allocation owner reused; no parser/alternative decision formula or old corpus replay.'])
def metadata():
 return dict(provenance(
  scope='Installed Ares loading-score selection and original retail ReadTheme/From_Name/call-tail on two bounded synthetic histories.',
  assumptions=[
   'Campaign mode0 uses selected mapBasic; noncampaign mode1 uses retained globalRulesSide thenCountry. Existing Country/Side/StartSpot andTheme registries are supplied world inputs, not fullDLL/session/Scenario initialization.',
   'ReadString128 inputs are supplied existing parsed ASCII values; actual ReadTheme4758F0, From_Name721210, both Areshooks andretail683D05..683D1F call-tail execute. Missing/empty defaults andunknown-name replacement are observed, not Python decisions.',
   'Theme registry layout reuses trigger_action_values.music_theme_case. Play_Song720BB0 records requestedindex; audio decoding/device/stream execution is excluded.',
   'All3nativeRNGstreams are seeded by existingEmpProbe; observedRNG/detach calls are rejected andfullstreamstates/codebytes remain unchanged. No timer producers on selected route.',
   'Exactly2new histories; public observations are lossless projections of their receipts, apart from private timing/executable identity. No old corpus is replayed.'
  ],
  substitutions=[
   'Existing EmpProbe/Probe ownVM/call/arena/CRT/ReadString; no secondmapper orparser.',
   'Only explicit parsedINI input, existingregistryworld priors andPlay_Song request-recording service. Selection/read/name-lookup/call-order bodies are not replaced.'
  ],entry_points=dict(select=0x1002FB80,apply=0x1002FC20,read_theme=0x4758F0,from_name=0x721210,retail_tail=0x683D05,play_request_boundary=0x720BB0)),ares_sha256=ARES_SHA256)

def main():
 import argparse,os
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--ares-dll',type=Path,default=os.environ.get('VERA20K_ARES_DLL'))
 p.add_argument('--gamemd-exe',type=Path)
 a,remaining=p.parse_known_args()
 if a.ares_dll is None:p.error('Set VERA20K_ARES_DLL or supply --ares-dll')
 if a.gamemd_exe:os.environ['VERA20K_GAMEMD_EXE']=str(a.gamemd_exe)
 finish_vectors(lambda:generate(a.ares_dll),FIXTURE,provenance=metadata,argv=remaining,
  source_paths={'producer':Path(__file__),'runner':Path(native.__file__),
    'vm_owner':Path(__file__).with_name('ares_prerequisites.py'),
    'emp_owner':Path(__file__).with_name('ares_emp.py'),
    'return_owner':Path('tools/input_oracle/fast_scroll.py'),
    'crt_owner':Path('tools/rules_oracle/ini_token_readers.py')})
if __name__=='__main__':main()
