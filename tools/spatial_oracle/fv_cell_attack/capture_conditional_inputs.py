"""Extract only declared constructor inputs and outside raw calls from a retained build.

Never imported by the native oracle or its output projection. The caller supplies
case seed/physical-stage/command declarations; production output supplies only
identity, options, assertion hashes and observed external RNG calls. Preserve the
retained build receipt: source-line receipts describe this historical build, not
arbitrary later checkout bytes.
"""
from pathlib import Path
import argparse,hashlib,json
from tools.cargo_run import source_identity
from tools.native_oracle import first_difference
from tools.spatial_oracle.fv_cell_attack.paid_conditional_input_tools import REPO,schedule,selected_logic_ids

sha=lambda raw:hashlib.sha256(raw).hexdigest()

def producer_key(proof):
    return sha(json.dumps(proof,sort_keys=True,separators=(',',':')).encode())

def extract(production_dir, manifest_path, declarations, *, historical=None, source_root=REPO):
    raw_manifest=manifest_path.read_bytes();manifest=json.loads(raw_manifest)
    assert len(manifest['artifacts'])==1
    build=dict(label=manifest_path.parent.name,manifest_sha256=sha(raw_manifest),
        head=manifest['source']['head'],binary_sha256=manifest['artifacts'][0]['sha256'],command=manifest['command'],rustc=manifest['rustc'])
    if historical is None:
        current = source_identity(source_root)
        assert current['head'] == manifest['source']['head']
        assert current['source_sha256'] == manifest['source']['source_sha256'], 'Capture source differs from retained build; use sealed receipts for historical auditing'
        receipts = None
    else:
        assert historical['provenance']['historical_build'] == build
        assert historical['provenance']['declarations'] == declarations
        receipts = {}
        for producer in historical['producers'].values():
            proof = producer['source_proof']
            key = (proof['file'], proof['line'], proof['column'])
            assert key not in receipts or receipts[key] == proof
            receipts[key] = proof
    producers={};cases=[]
    for declaration in declarations:
        stage=declaration['stage'];path=production_dir/(stage+'.json');raw=path.read_bytes();production=json.loads(raw)
        previous = next((c for c in historical['cases'] if c['stage'] == stage), None) if historical else None
        if historical:
            assert previous is not None and previous['production_source']['sha256'] == sha(raw)
        rows=schedule(production,receipts=receipts,source_root=source_root);first=production['diagnostics'][0]
        assert first['frame']==production['frames'][0]['next_frame']==1
        assert len(production['frames'])==len(rows)+1
        boundary=dict(source_native_id=first['native_id'],scenario_cursor_before_constructor=first['native_id']-1,
            scenario_seed=declaration['scenario_seed'],game_speed=first['game_speed'],command_frame=first['frame'],
            command_rng_sha256={k:sha(bytes.fromhex(v)) for k,v in first['rng'].items()})
        frames=[]
        for step in rows:
            def external(source):
                result=[]
                for call in source:
                    assert call['supplied_external']
                    proof={k:call[k] for k in ('reason','callers','source_proof')}
                    key=producer_key(proof)
                    assert key not in producers or producers[key]==proof
                    producers[key]=proof
                    result.append({**{k:call[k] for k in ('stream','logic_object','before_indices','value')},'producer_ref':key})
                return result
            a=step['prefix'];b=a+step['selected']
            frames.append(dict(logic_frame=step['logic_frame'],prefix=external(step['rows'][:a]),suffix=external(step['rows'][b:])))
        case=dict(stage=stage,physical_stage=declaration['physical_stage'],boundary=boundary,
            production_source=dict(file=path.name,sha256=sha(raw),build=build),
            attribution=dict(method='Observed Logic object identity separates the selected FV, its Bullets and newly created Anims from outside producers. Null contexts require an established outside global producer. Selected Guard calls remain native. Caller/source receipts are historical input provenance for this retained build.',
                selected_logic_ids=selected_logic_ids(production)),frames=frames)
        if declaration.get('followup_commands'):
            case['followup_commands']=declaration['followup_commands']
        cases.append(case)
        print(stage,'states',len(production['frames']),'outside',sum(len(f['prefix'])+len(f['suffix']) for f in frames),'selected',sum(f['selected'] for f in rows),flush=True)
    return dict(schema=2,boundary='Selected native owner continuation under declared constructor ID/options/seed and outside raw-call schedule. Does not prove whole-Scenario population or external producer cadence.',
        provenance=dict(declarations=declarations,historical_build=build),producers=producers,cases=cases)

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--production-dir',type=Path,required=True)
    p.add_argument('--manifest',type=Path,required=True)
    p.add_argument('--declarations',type=Path)
    p.add_argument('--source-repo',type=Path,default=REPO)
    mode=p.add_mutually_exclusive_group(required=True)
    mode.add_argument('--out',type=Path,help='Write a new declared input from matching build sources')
    mode.add_argument('--check-input',type=Path,help='Check historical capture using its sealed source receipts')
    a=p.parse_args()
    historical=json.loads(a.check_input.read_bytes()) if a.check_input else None
    declarations=json.loads(a.declarations.read_bytes()) if a.declarations else historical['provenance']['declarations'] if historical else None
    assert declarations is not None, '--declarations is required for a new capture'
    result=extract(a.production_dir,a.manifest,declarations,historical=historical,source_root=a.source_repo)
    if historical:
        actual=json.loads(json.dumps(result))
        assert (difference:=first_difference(historical,actual)) is None, difference
        print('PASS historical constructor inputs/outside schedule and sealed build/source receipts; no files written')
    else:
        a.out.write_text(json.dumps(result,indent=2,sort_keys=True)+'\n')
