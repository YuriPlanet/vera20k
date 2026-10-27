"""The opt-in package cannot leak networking into a later single-player build."""
from pathlib import Path
import tempfile
import unittest

from tools.wasm.build import package_host, validate_proxy_url


class WasmBuildTests(unittest.TestCase):
    def test_proxy_override_requires_an_origin_and_secure_remote_transport(self):
        validate_proxy_url(None)
        validate_proxy_url('wss://proxy.example:9443')
        validate_proxy_url('ws://localhost:8788')
        for invalid in ('', 'ws://proxy.example', 'wss://proxy.example/game',
                        'wss://user@proxy.example', 'wss://proxy.example?',
                        'wss://proxy.example:0', 'wss://proxy.example:bad'):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                validate_proxy_url(invalid)

    def test_single_player_rebuild_retires_network_scripts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            network, single, output = (root / name for name in ('network', 'single', 'output'))
            network.mkdir()
            single.mkdir()
            package_host(network, output, True)
            self.assertIn("import './network.js';", (output / 'host.js').read_text())
            self.assertTrue((output / 'lobby.js').is_file())
            self.assertIn('Online play can share selected maps;',
                          (output / 'index.html').read_text(encoding='utf-8'))
            package_host(single, output, False)
            self.assertNotIn('network.js', (output / 'host.js').read_text())
            self.assertFalse((output / 'network.js').exists())
            self.assertFalse((output / 'lobby.js').exists())
            self.assertIn('Single player.', (output / 'index.html').read_text())


if __name__ == '__main__':
    unittest.main()
