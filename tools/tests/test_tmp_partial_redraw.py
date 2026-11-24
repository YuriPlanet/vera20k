"""Partial-redraw publication boundaries; synthetic records are not native evidence."""
from copy import deepcopy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from tools.spatial_oracle import tmp_clear_render as tmp


class PartialRedrawTests(unittest.TestCase):
    def test_compaction_preserves_consumed_surfaces_and_complete_rng_states(self):
        # A maintenance surface can also be a later final surface. Only states
        # exclusively intermediate may lose their runs; never alter the input.
        phases = [dict(name=name, surface_ref=ref,
                       rng=dict(main='abcd', scenario='ef01', mapgen='ef01'))
                  for name, ref in [('prior', 0), ('after_maintenance', 1),
                                    ('after_terrain', 2), ('after_cleanup', 2)]]
        raw = dict(schema_version=1, histories=[dict(phases=phases)],
                   surface_states=[dict(runs=[[i]], color_sha256=str(i),
                                        depth_sha256=str(i)) for i in range(3)])
        original = deepcopy(raw)
        compact = tmp.partial_redraw_projection(raw)
        self.assertEqual(raw, original)
        self.assertEqual(compact['surface_states'][0], raw['surface_states'][0])
        self.assertEqual(compact['surface_states'][2], raw['surface_states'][2])
        self.assertEqual(compact['surface_states'][1],
                         dict(color_sha256='1', depth_sha256='1'))
        for before, after in zip(phases, compact['histories'][0]['phases']):
            self.assertEqual(before['rng'], {name: compact['rng_streams'][ref]
                                            for name, ref in after['rng'].items()})

    def test_scratch_guard_precedes_native_execution_or_publication(self):
        repo = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as external:
            commands = [
                ['--partial-output-dir', str(repo / 'target' / 'partial')],
                ['--partial-output-dir', external, '--output', external + '/escape.json'],
                ['--partial-output-dir', external, '--output=' + external + '/escape.json'],
            ]
            with patch.object(tmp, 'finish_vectors') as publish:
                for command in commands:
                    with self.subTest(command=command), self.assertRaises(ValueError):
                        tmp.partial_redraw_main(command)
                publish.assert_not_called()
            self.assertEqual(list(Path(external).iterdir()), [])


if __name__ == '__main__':
    unittest.main()
