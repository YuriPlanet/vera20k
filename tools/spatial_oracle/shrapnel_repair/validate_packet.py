"""Regenerate or independently check every composed Shrapnel witness and manifest."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PACKAGE = 'tools.spatial_oracle.shrapnel_repair'
RUNNERS = ('shrapnel_repair', 'zone_composition', 'hierarchy_composition', 'compare_production')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def retail_inputs():
    from .retail_inputs import ASSETS
    expected = json.loads((HERE / 'retail_manifest.json').read_bytes())['files']
    actual = {}
    for name, record in expected.items():
        path = ASSETS / name
        if record.get('absent'):
            assert not path.exists(), f'Frozen retail input requires absent {name}'
            actual[name] = {'absent': True}
        else:
            assert path.is_file(), f'Extract {name} into VERA20K_SHRAPNEL_INPUTS'
            value = {'sha256': sha(path), 'bytes': path.stat().st_size}
            assert all(value[k] == record[k] for k in value), f'Retail input mismatch: {name}'
            actual[name] = value
    return actual


def manifest(inputs):
    # Import the same owners; record transitive repository sources by relative
    # name. No host paths, retail bytes, generated logs, or duplicate captures.
    from . import hierarchy_composition, map_facts
    from tools.native_oracle import image_sha256
    import capstone
    import unicorn
    owned = {
        p.relative_to(HERE).as_posix(): sha(p)
        for p in sorted(HERE.rglob('*'))
        if p.is_file() and p.name != 'receipt.json'
        and p.suffix not in ('.log', '.pyc') and '__pycache__' not in p.parts
    }
    dependencies = {}
    for module in list(sys.modules.values()):
        file = getattr(module, '__file__', None)
        if not file:
            continue
        path = Path(file).resolve()
        if path.suffix == '.py' and path.is_relative_to(ROOT / 'tools') and not path.is_relative_to(HERE):
            dependencies[path.relative_to(ROOT).as_posix()] = sha(path)
    return {
        'schema': 2,
        'native_sha256': image_sha256(),
        'runtime': {'python_requires': '>=3.10', 'unicorn': unicorn.__version__,
                    'capstone': capstone.__version__, 'lzo_version': map_facts.lib.lzo_version()},
        'runners': [f'{PACKAGE}.{runner}' for runner in RUNNERS],
        'owned': owned,
        'dependencies': dict(sorted(dependencies.items())),
        'external_inputs': inputs,
        'validation': 'Separate --write then --check invocations; four native/comparison outputs and exact source/input/artifact manifest. Check is read-only.',
        'scope': 'Two physical hut selectors; empty low repair, resident Recalc, native full connectivity/hierarchy from supplied production planes. No native scenario loader, Engineer prefix, or occupied repair claim.',
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--write', action='store_true')
    mode.add_argument('--check', action='store_true')
    args = parser.parse_args()
    assert sys.version_info >= (3, 10)
    inputs = retail_inputs()
    flag = '--write' if args.write else '--check'
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1', PYTHONPATH=str(ROOT))
    for runner in RUNNERS:
        print(f'{runner} {flag}', flush=True)
        result = subprocess.run([sys.executable, '-m', f'{PACKAGE}.{runner}', flag],
                                cwd=ROOT, env=env, check=False)
        if result.returncode:
            raise SystemExit(result.returncode)
    # The uncompressed comparison is small and must retain its original bytes.
    promotion = json.loads((HERE / 'promotion.json').read_bytes())
    name = 'production_comparison.json'
    assert sha(HERE / name) == promotion['results'][name]['original_file_sha256']
    data = manifest(inputs)
    path = HERE / 'receipt.json'
    if args.write:
        path.write_text(json.dumps(data, indent=2, sort_keys=True) + '\n', encoding='utf-8')
        print('WROTE receipt; run separate --check before citing the packet.', flush=True)
    else:
        from tools.native_oracle import first_difference
        difference = first_difference(json.loads(path.read_bytes()), data)
        assert difference is None, f'Source/input/artifact manifest changed: {difference}'
        print('PASS independent packet check: four outputs and manifest match; no files written.', flush=True)


if __name__ == '__main__':
    main()
