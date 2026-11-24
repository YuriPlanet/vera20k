"""Read-only comparison of saved original Anytown graphs to production exports."""
import argparse,hashlib,json,gzip
from pathlib import Path

sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def summary(a,b):
 if a==b:return {'equal':True}
 if isinstance(a,list) and isinstance(b,list):
  examples=[dict(index=i,native=x,production=y) for i,(x,y) in enumerate(zip(a,b)) if x!=y]
  return dict(equal=False,native_length=len(a),production_length=len(b),different_entries=len(examples),first=examples[:5])
 return dict(equal=False,native=a,production=b)

def compare(native,prefix,repaired=None,*,stage_indices=None):
 data=json.loads(gzip.decompress(native.read_bytes()));rows=[]
 overlays={(r[0],r[1]):r[5] for r in data['case']['cells']}
 if stage_indices is None:
  stages=[('loaded',data['initial'],None),('damaged',data['stages'][0]['state'],data['stages'][0]),('collapsed',data['stages'][1]['state'],data['stages'][1])]
  if repaired is not None:stages.append(('repaired',data['stages'][2]['state'],data['stages'][2]))
 else:
  stages=[(label,data['initial'] if index is None else data['stages'][index]['state'],None if index is None else data['stages'][index])for label,index in stage_indices]
 for label,state,stage in stages:
  if stage:
   for event in stage['trace']:
    if event['kind']=='overlay':overlays[tuple(event['coord'])]=event['value']
  path=repaired if label=='repaired' and repaired is not None else Path(str(prefix)+'.'+label+'.json');root=json.loads(path.read_bytes());actual=root['navigation'];checks={}
  for key,value in state['navigation'].items():checks['navigation.'+key]=summary(value,actual.get(key,actual['rust'].get(key)))
  for level,graph in enumerate(state['graphs']):
   for key,value in graph.items():checks[f'graphs.{level}.{key}']=summary(value,actual['graphs'][level][key])
  width=actual['width'];height=actual['height'];cells={tuple(c['coord']):c for c in state['cells']}
  checks['native_size']=summary(data['case']['size'],actual['native_size'])
  checks['live_cell_allocated']=summary([(x,y) in cells for y in range(height) for x in range(width)],actual['live_cell_allocated'])
  for key in ['level','slope']:
   checks['live_cell_'+key+'s_allocated']=summary([c[key]for c in state['cells']],[actual['live_cell_'+key+'s'][c['coord'][1]*width+c['coord'][0]]for c in state['cells']])
  selected=[]
  for cell in actual['cells']:
   coord=tuple(cell['coord']);nc=cells[coord];index=coord[1]*width+coord[0];nid=state['navigation']['base_ids'][index]
   expected={k:nc[k]for k in ['coord','tile','subtile','level','slope','land','zone_type']}
   expected.update(bridge_flags=nc['flags'],overlay=overlays[coord],cached_class=state['navigation']['classes'][index],cached_height=state['navigation']['levels'][index],base_id=nid,raw_row7=state['navigation']['raw_rows'][7][nid])
   selected.append(dict(coord=list(coord),comparison=summary(expected,cell)))
  checks['representative_cells']={'equal':all(c['comparison']['equal']for c in selected),'count':len(selected),'mismatches':[c for c in selected if not c['comparison']['equal']]}
  checks['dummy']='Not compared: production uses Debug text, native stores structured fields; both inspected separately.'
  rows.append(dict(state=label,production_file=path.name,production_sha256=sha(path),frame=root['frame'],checks=checks,all_compared_equal=all(v.get('equal',True) for v in checks.values()if isinstance(v,dict))))
 scope='All class/level/base-ID planes,13 complete movement rows,zone count,three graph IDs/padding IDs/full ordered records; allocated live level/slope planes and77 representative cells per state. Runtime objects/occupancy and RNG are not compared across these different caller boundaries. '+('Repaired production export included.' if repaired is not None else 'Production repair export not supplied.')
 if stage_indices is not None:scope='All class/level/base-ID planes,13 complete movement rows,zone count,three graph IDs/padding IDs/full ordered records; allocated live level/slope planes and every exported representative cell at each explicitly mapped stage. Runtime objects/occupancy and RNG are not compared across these different caller boundaries.'
 return dict(native_file=native.name,native_sha256=sha(native),production_prefix=prefix.name,states=rows,all_compared_equal=all(r['all_compared_equal']for r in rows),scope=scope)

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('prefix',type=Path);p.add_argument('--native',type=Path,default=Path(__file__).with_name('navigation.json.gz'));p.add_argument('--output',type=Path);p.add_argument('--repaired',type=Path,help='explicit repaired production export; compares the frozen fourth native state');a=p.parse_args();result=compare(a.native,a.prefix,a.repaired);text=json.dumps(result,indent=2)+'\n'
 if a.output:a.output.write_text(text)
 else:print(text,end='')
 assert result['all_compared_equal'], 'Native/production navigation mismatch; inspect receipt.'
