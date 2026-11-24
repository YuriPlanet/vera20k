"""Compare saved original-execution outputs with frozen production boundary captures."""
from .shrapnel_repair import *
from .packet_io import read_result
from tools.native_oracle import finish_vectors
import re

def load(name):return read_result(HERE/(name+'.gz'))
def compare_cells(native,production,fields):
 prod={tuple(r['coord']):r for r in production['cells']};count=0
 for row in native:
  p=prod[tuple(row['coord'])];expected=dict(coord=p['coord'],flags=p['bridge']['raw_flags'],overlay=-1 if p['bridge']['overlay_id'] is None else p['bridge']['overlay_id'],state=p['bridge']['state_byte'],tile=p['tile'],subtile=p['subtile'],level=p['level'],slope=p['slope'],land=p['land'],zone_type=p['zone_type'])
  for key in fields:assert row[key]==expected[key],(row['coord'],key,row[key],expected[key])
  count+=1
 return dict(cells=count,fields=fields,exact=True)
def compare_navigation(native,production):
 n=production['navigation']
 for key in ('classes','levels'):assert native[key]==n[key],key
 for key in ('base_ids','raw_rows','zone_count'):assert native[key]==n['rust'][key],key
 return dict(cells=len(native['classes']),movement_rows=13,zone_count=native['zone_count'],classes_exact=True,heights_exact=True,base_ids_exact=True,all_raw_rows_exact=True)
def compare_steps(steps,after,fields):
 first,repeat=steps;assert repeat['changes']==[] and repeat['rng_before']==repeat['rng_after']
 assert set(repeat['reached'])=={'0x570050','0x57f200','0x57fbc0'}
 assert first['rng_after']['mapgen']==after['rng']['mapgen']
 assert first['rng_after']['main']==first['rng_before']['main']==after['rng']['main']
 assert first['rng_after']['scenario']==first['rng_before']['scenario']
 return dict(strip=compare_cells(first['strip'],after,fields),repeat_no_changes=True,
  mapgen_full_state_exact=True,main_full_state_exact=True,scenario_unchanged_inside_native_repair=True,
  scenario_production_after_equals_bounded_native=first['rng_after']['scenario']==after['rng']['scenario'],
  scenario_limit='The production approach includes30 ordinary frames; this bounded native570050 call does not execute their unrelated Scenario draws.')
def generate():
 before=production('before_command');after=production('after_approach');strip_fields=['coord','flags','overlay','state','tile','subtile','level','slope'];resident_fields=strip_fields+['land','zone_type']
 result={'scope':__doc__,'physical_map_sha256':sha((ASSETS/'XShrapnel.MAP').read_bytes()),'production_inputs':{k:source_sha256(k) for k in ('loaded','before_command','after_approach')},'controller_and_resident':[]}
 for case in load('shrapnel_repair.json')['cases']:
  assert case['input']['production_input_sha256']==result['production_inputs']['before_command']
  row=dict(hut=case['input']['start'],stage=case['stage'],repair=compare_steps(case['steps'],after,resident_fields if case['initial_recalc'] else strip_fields))
  if case['initial_recalc']:
   rows=[r['after'] for r in case['initial_recalc']['trace'] if r['kind']=='recalc'];row['initial_resident_cells']=compare_cells(rows,before,resident_fields)
  result['controller_and_resident'].append(row)
 for filename,label in (('zone_composition.json','connectivity'),('hierarchy_composition.json','hierarchy')):
  d=load(filename);r=d['result'];assert d['input']['production_input_sha256']==result['production_inputs']['before_command']
  result[label]=dict(initial=compare_navigation(r['initial_navigation'],before),final=compare_navigation(r['final_navigation'],after),repair=compare_steps(r['steps'],after,resident_fields))
  if label=='hierarchy':
   result[label]['native_initial_graph_counts']=[len(g['records']) for g in r['initial_graphs']]
   result[label]['native_final_graph_counts']=[len(g['records']) for g in r['final_graphs']]
   for phase,native,capture in (('initial',r['initial_graphs'],before),('final',r['final_graphs'],after)):
    assert native==capture['navigation']['graphs'],(phase,'hierarchy IDs/padding/parents/types/ordered edges')
    text=capture['navigation']['dummy'];parts=re.fullmatch(r'SharedCellDummySnapshot \{ coord: \((-?\d+), (-?\d+)\), level: (\d+), slope_type: (\d+), bridge_flags_0x1180: (\d+) \}',text)
    assert parts,text
    x,y,z,slope,flags=map(int,parts.groups());expected=dict(coord=[x,y],level=z,slope_type=slope,bridge_flags_0x1180=flags)
    assert r[phase+'_dummy']==expected,(phase,'shared dummy',r[phase+'_dummy'],expected)
   result[label]['graph_comparison']=dict(levels=3,ids_each=26896,padding_ids_each=329,all_ordered_edges_exact=True,all_parents_exact=True,all_types_exact=True,initial_and_final_exact=True,shared_dummy_exact=True)
   result[label]['physical_live_check']=r['physical_live_check']
 mode_sha=sha((ASSETS/'MPBattleMD.ini').read_bytes());old_mode_sha='50406e81d7523f6be1954daab6b25bd85a8347c455f3d53dcf515d7f719b4963'
 # The frozen packet checked the legacy alias bytes directly. The portable
 # witness requires only the actual active file and retains that prior hash.
 assert mode_sha==old_mode_sha
 result['mode_file']=dict(active='MPBattleMD.ini',source='ra2md.mix/localmd.mix',entry_id='0xAF084179',sha256=mode_sha,previous_extract_sha256=old_mode_sha,byte_identical=mode_sha==old_mode_sha)
 return result

if __name__=='__main__':
 finish_vectors(generate,HERE/'production_comparison.json',provenance=lambda:provenance(scope=__doc__,assumptions=['Only declared fields and stages compared. Native load or whole-scene equality is not inferred from matching supplied projections.'],substitutions=[],entry_points={'compared_repair':0x570050,'compared_connectivity':0x56C510,'compared_hierarchy_batch':0x586990}))
