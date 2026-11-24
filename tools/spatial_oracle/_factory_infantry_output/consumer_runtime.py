"""UnitReady physical inputs and complete-observation comparisons.

The separate41-entry consumer profile never changes the P10 input closure.
Complete original consumer and retained movement comparisons are required first.
"""
from pathlib import Path
import functools, gzip, hashlib, json, os, sys
from . import runtime as rt


@functools.cache
def metadata():
    return json.loads((rt.HERE / 'consumer-meta.json').read_text(encoding='utf-8'))


def inputs(root, wave):
    value=root or os.environ.get('VERA20K_UNIT_READY_CONSUMER_ASSETS')
    rt.require(value, 'Provide --consumer-assets: separate exact41-entry AnyTown-alias input root')
    root=Path(value).expanduser().resolve()
    rt.require(root.is_dir(), 'Provide --consumer-assets: separate exact41-entry AnyTown-alias input root')
    meta=metadata()
    actual={p.name:p for p in root.iterdir()}
    expected={name for name,row in meta['physical_inputs'].items() if not row.get('absent')}
    rt.require(set(actual)==expected, 'Consumer physical-input census differs')
    for name,row in meta['physical_inputs'].items():
        if row.get('absent'):
            rt.require(name.upper() not in {n.upper()for n in actual}, 'Recorded absent consumer input appeared: '+name)
        else:
            p=actual[name]
            rt.require(p.is_file()and p.stat().st_size==row['bytes']and rt.sha(p)==row['sha256'], 'Consumer physical input changed: '+name)
    value=wave or os.environ.get('VERA20K_UNIT_READY_CONSUMER_WAVE')
    rt.require(value, 'Provide --wave: exact production-selected ceva062.wav')
    path=Path(value).expanduser().resolve()
    row=meta['wave']
    rt.require(path.is_file()and path.stat().st_size==row['bytes']and rt.sha(path)==row['sha256'], 'Provide --wave: exact production-selected ceva062.wav bytes')
    return root,path.read_bytes()


def physical_eva():
    from tools import native_oracle as native
    from tools.sidebar_oracle.stock import mix, mix_hash
    from tools.rules_oracle.bridge_child_sound import sections
    master=(native.configured_gamemd().parent/'ra2md.mix').read_bytes()
    local=mix(master)[mix_hash('localmd.mix')]
    raw=mix(local)[mix_hash('EVAMD.INI')]
    full=sections(raw);name='EVA_UnitReady'
    selected={name:full[name], 'DialogList':{k:v for k,v in full['DialogList'].items()if v==name}}
    actual=dict(archive='ra2md.mix -> localmd.mix -> EVAMD.INI',
        master_sha256=hashlib.sha256(master).hexdigest(),local_sha256=hashlib.sha256(local).hexdigest(),
        eva_sha256=hashlib.sha256(raw).hexdigest(),eva_bytes=len(raw),
        physical_dialog_count=len(full['DialogList']),selected=selected)
    rt.require(actual==metadata()['physical_eva'], 'Fixed physical EVAMD winner/selected strings differ')
    return actual


def compare(control, actual):
    identity=metadata()['controls'][control]
    observed=rt.canonical_sha(actual)
    rt.require(actual['success']is True and actual['original_code_unchanged']is True and not actual['requests']and not actual['advances'], 'Current original consumer did not complete')
    rt.require(observed==identity['full_native_observation_sha256'], 'Complete original consumer observations changed: '+control+' observed='+observed+' expected='+identity['full_native_observation_sha256'])
    return dict(native_observations_equal=True, normalized_full_native_sha256=observed,
        historical_receipt_sha256=identity['sha256'])


def replay(control,root,wave):
    rt.require(sys.flags.optimize==0, 'Whole original consumer emulation requires normal Python')
    root,wave=inputs(root,wave)
    physical=physical_eva()
    from . import consumer
    result={}
    observer=None
    try:
        if control=='buffer':
            from .pcm import NativePcmObserver
            observer=NativePcmObserver()
            actual=consumer.buffer(root,physical,metadata(),wave,observer=observer.install)
        else:
            actual=getattr(consumer,control)(root,physical,metadata(),wave)
        result=dict(schema=1,status='PASS',control=control,
            full_original_consumer=actual,comparison=compare(control,actual),whole_object_completion_claimed=False)
        if observer is not None:
            witness=observer.finish(wave,actual)
            identity=metadata()['pcm_witness']
            observed=rt.canonical_sha(witness['observation'])
            rt.require(observed==identity['complete_original_observation_sha256'],
                'Complete original PCM return/write/copy observations changed')
            rt.require(witness['source_boundary']==identity['source_boundary'] and
                {k:v for k,v in witness['pcm'].items() if k!='bytes_hex'}==identity['pcm'],
                'Original complete PCM count/format/source/ring differs')
            result['full_original_pcm_witness']=witness
            result['pcm_comparison']=dict(complete_original_observation_equal=True,
                complete_original_observation_sha256=observed,bytes=witness['pcm']['bytes'],
                pcm_sha256=witness['pcm']['sha256'],historical_witness_sha256=identity['source_receipt']['sha256'])
    except Exception as exc:
        import traceback
        tb=exc.__traceback__; partial=None; f=None
        while tb:
            scope=tb.tb_frame.f_locals
            if isinstance(scope.get('result'),dict):partial=scope['result']
            if 'f'in scope and hasattr(scope['f'],'u'):f=scope['f']
            tb=tb.tb_next
        result=dict(schema=1,status='FAIL',control=control,
            failure=dict(type=type(exc).__name__,message=str(exc),traceback=traceback.format_exc()),
            full_original_consumer=locals().get('actual',partial),whole_object_completion_claimed=False)
        if observer is not None:
            result['partial_original_pcm_observation']={k:v for k,v in observer.state.items() if k!='fixture'}
        if f is not None:
            from unicorn.x86_const import UC_X86_REG_EIP
            result['failure'].update(original_pc=f'0x{f.u.reg_read(UC_X86_REG_EIP):08X}',
                original_code_unchanged=f.code_unchanged(),rng=f.rng(),requests=f.draws,advances=f.advances,
                executed_pcs=[f'0x{x:08X}'for x in sorted(f.executed)],native_trail=[f'0x{x:08X}'for x in f.trail])
    return result


def check_saved():
    """Check original bytes and both serializations; never emulate under -O."""
    from tools import native_oracle as native
    from .fixture import unit_ready_consumer
    meta=metadata()
    image=native.image_bytes()
    for row in meta['original_static_bytes']:
        _,raw=native.file_span(image,int(row['start'],16),row['bytes'])
        rt.require(hashlib.sha256(raw).hexdigest()==row['sha256'], 'Original consumer byte range changed: '+row['name'])
    data={}
    for name,row in meta['retained_files'].items():
        path=rt.HERE/name;raw=path.read_bytes()
        rt.require(len(raw)==row['bytes']and hashlib.sha256(raw).hexdigest()==row['sha256'], 'Retained consumer export changed: '+name)
        if path.suffix=='.gz':
            raw=gzip.decompress(raw)
            rt.require(len(raw)==row['uncompressed_bytes']and hashlib.sha256(raw).hexdigest()==row['uncompressed_sha256'], 'Consumer gzip decoded bytes changed')
        data[name]=json.loads(raw)
    full=data['fixtures/unit-ready-consumer.json.gz']
    lean=data['fixtures/unit-ready-consumer-rust.json']
    rt.require(unit_ready_consumer(None,None,lean=True,projection=full)==lean,
        'Plain Rust export differs from the single full-projection selector')
    from .pcm import decoded_ranges
    import copy
    pcm_identity=meta['pcm_witness']
    witness=data[pcm_identity['retained_path']]
    rt.require(witness['success'] is True and witness['original_code_unchanged'] is True,
        'Retained original PCM witness failed')
    pcm_raw,source=decoded_ranges(witness['observation'])
    rt.require(rt.canonical_sha(witness['observation'])==pcm_identity['complete_original_observation_sha256'] and
        witness['source_boundary']==pcm_identity['source_boundary'], 'Complete original PCM witness differs')
    rt.require({k:v for k,v in witness['pcm'].items() if k!='path'}==pcm_identity['pcm'] and
        len(pcm_raw)==pcm_identity['pcm']['bytes'] and hashlib.sha256(pcm_raw).hexdigest()==pcm_identity['pcm']['sha256'],
        'Literal native PCM count/format/bytes differ')
    rt.require(len(source)==pcm_identity['source_boundary']['native_returned_source_bytes'] and
        hashlib.sha256(source).hexdigest()==pcm_identity['source_boundary']['native_returned_source_sha256'],
        'Original complete compressed-source bytes differ')
    ring=bytes.fromhex(full['complete_device_payloads'][pcm_identity['pcm']['initial_native_ring_sha256']]['bytes'])
    rt.require(ring[:len(pcm_raw)]==pcm_raw and not any(ring[len(pcm_raw):]), 'Returned PCM differs from retained native ring')
    regenerated=unit_ready_consumer(None,None,projection=copy.deepcopy(full),
        pcm=dict(observation=witness,source_receipt=pcm_identity['source_receipt']))
    rt.require(regenerated==full, 'PCM export differs from the single original witness selector')
    rt.require(set(full['controls'])==set(meta['controls']), 'Consumer control census differs')
    for control,row in full['controls'].items():
        identity=meta['controls'][control]
        rt.require(row['full_original_receipt_sha256']==identity['canonical_historical_receipt_sha256'],
            'Original complete consumer receipt pin differs: '+control)
        rt.require(full['source_receipts'][control]['sha256']==identity['sha256']and
            full['source_receipts'][control]['bytes']==identity['bytes'], 'Consumer original receipt identity differs')
        steps=row['steps']+row['registered_prior']['clock_steps']+row['registered_prior']['notification_suffix']
        if control=='radar':
            steps+=row['cleanup']+[s for f in row['frames']for s in (f['tick'],f['cleanup'])]
            rt.require([f['frame']for f in row['frames']]==list(range(row['expiry_frame']+1)), 'Native sequential marker visits missing')
        for step in steps:
            rt.require(step['before']['rng']==step['after']['rng'], 'Consumer selected complete RNG state differs')
            for stream,digest in step['before']['rng'].items():
                rt.require(digest in full['complete_rng_states']and stream in full['complete_rng_states'][digest]['streams'],
                    'Consumer RNG stream reference differs')
    for digest,row in full['complete_rng_states'].items():
        raw=bytes.fromhex(row['bytes'])
        rt.require(len(raw)==row['length']==1012 and hashlib.sha256(raw).hexdigest()==digest,
            'Consumer original full RNG bytes changed')
    for digest,row in full['complete_device_payloads'].items():
        raw=bytes.fromhex(row['bytes'])
        rt.require(len(raw)==row['length']and hashlib.sha256(raw).hexdigest()==digest,
            'Consumer original complete device storage changed')
    return dict(schema=1,status='PASS',
        source_seal=meta['source_seal']['manifest_sha256'],
        original_byte_ranges=len(meta['original_static_bytes']),
        control_complete_native_sha256={k:v['full_native_observation_sha256']for k,v in meta['controls'].items()},
        one_selector_full_and_rust_exports_equal=True,
        complete_rng_buffers=len(full['complete_rng_states']),
        complete_device_storage_payloads=len(full['complete_device_payloads']),
        literal_original_pcm_bytes=len(pcm_raw),literal_original_pcm_sha256=hashlib.sha256(pcm_raw).hexdigest(),
        complete_original_pcm_observation_sha256=pcm_identity['complete_original_observation_sha256'],
        whole_original_emulation_run=False,whole_object_completion_claimed=False)
