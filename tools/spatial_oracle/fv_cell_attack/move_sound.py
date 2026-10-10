"""Original FV MoveSound binding and first native paid AI Main-RNG receipt.

This composes the existing selected sound reader with Paid before its original
full UnitType reader. Sound playback remains the inherited device boundary.
"""
from pathlib import Path
import hashlib, json, os, struct, sys
from tools.spatial_oracle.anytown_damage import mtnk_attack
from tools.native_oracle import finish_vectors, provenance

HERE = Path(__file__).resolve().parent
SOUND_ROOT = Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])

def install(q):
    from tools.spatial_oracle.fv_cell_attack import paid_world as owner
    # This selects input sections only. Native 7510D0/750440 own the SoundList
    # records, and the original full UnitType reader owns MoveSound parsing.
    declared = []
    for name, path in mtnk_attack.layers():
        if not path.exists():
            continue
        sections, _ = owner.frozen.proof.lexical(path.read_bytes(), {'FV'})
        value = sections.get('FV', {}).get('MoveSound')
        if value is not None:
            declared.append(dict(file=name, value=value))
    names = {value.strip() for row in declared for value in row['value'].split(',')
             if value.strip() and value.strip().lower() != '<none>'}
    assert names, 'The supplied retail FV layers must declare a MoveSound'
    art_names = {'FV', 'AAHeatSeeker2', 'DRAGON'} | {
        row['name'] for row in q.inputs['impact_anim_art']}
    art, _ = owner.frozen.proof.lexical(
        (owner.frozen.proof.assets_root() / 'ARTMD.INI').read_bytes(), art_names)
    result = mtnk_attack.sound_inputs(q.m, SOUND_ROOT, art, wanted_names=names)
    return dict(declared=declared, registry=result)

def bound(q):
    m = q.m
    count = m.read32(q.typ + 0x504)
    data = m.read32(q.typ + 0x4F8)
    indices = [m.read32(data + i * 4) for i in range(count)]
    registry = m.read32(0xB1D37C)
    names = [m.string(m.read32(m.read32(registry + i * 4)) + 0x6C) for i in indices]
    return dict(count=count, indices=indices, names=names)

def generate():
    from tools.spatial_oracle.fv_cell_attack import paid_world as owner
    world = owner.native_worlds()[0]
    original = owner.Paid
    class WithSound(original):
        def setup(self):
            self.inputs['move_sound_install'] = install(self)
            super().setup()
            self.inputs['move_sound_binding'] = bound(self)
    owner.Paid = WithSound
    try:
        # The existing run owner controls the mapped-heap boundary and cleanup.
        result = owner.run(world, frames=1)
        assert result['failure'] is None, result['failure']
        inputs = result['inputs']['move_sound_install']
        binding = result['inputs']['move_sound_binding']
        assert binding['names'] == [r['value'] for r in inputs['declared']][-1].split(',')
        before, after = result['states']
        calls = [r for r in result['events'] if r.get('phase') == 'logic']
        main = [r for r in calls if r.get('kind') == 'rng' and r.get('stream') == 'main']
        assert len(main) == 1 and main[0]['return_pc'] == '0x4daad0', main
        assert struct.unpack_from('<ii', bytes.fromhex(before['rng']['main']), 4) == (0, 103)
        assert struct.unpack_from('<ii', bytes.fromhex(after['rng']['main']), 4) == (1, 104)
        return dict(inputs=inputs, binding=binding, before=before, after=after,
                    calls=calls)
    finally:
        owner.Paid = original

def metadata():
    from tools.spatial_oracle.fv_cell_attack import paid_world as owner
    result = provenance(scope=__doc__, entry_points={
        'sound_registry':0x7510D0, 'sound_reader':0x750440,
        'unit_type_reader':0x747620, 'foot_ai':0x4DA530, 'move_draw':0x4DAACB},
        assumptions=['Original native SoundList/types bind the physical FV MoveSound before the full UnitType reader. Original paid Unit/Foot/Drive/Fire AI executes unchanged. Only the isolated healthy native physical-map continuation is measured.'],
        substitutions=['Inherited OS, asset IO and sound playback transport boundaries. No native sound index, RNG value, movement or firing result is replaced. Audio decoding/mixing and global audio RNG after playback entry are outside this receipt.'])
    result['harness_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    result['sound_reader_owner_sha256'] = hashlib.sha256(Path(mtnk_attack.__file__).read_bytes()).hexdigest()
    result['source_pins'] = owner.sources()
    return result

if __name__ == '__main__':
    if '--foot-tail' in sys.argv:
        from tools.spatial_oracle.fv_cell_attack import foot_move_sound
        finish_vectors(foot_move_sound.generate, HERE/'foot_move_sound.json',
                       provenance=foot_move_sound.metadata,
                       source_paths=foot_move_sound.source_paths(),
                       argv=[arg for arg in sys.argv[1:] if arg != '--foot-tail'])
    else:
        finish_vectors(generate, HERE/'move_sound.json', provenance=metadata)
