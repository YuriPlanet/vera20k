"""The opt-in package cannot leak networking into a later single-player build,
and every pull request that can break the browser build builds it."""
from pathlib import Path
import re
import tempfile
import unittest

from tools.wasm.build import package_host, validate_proxy_url

ROOT = Path(__file__).resolve().parents[2]


def github_path_filter(pattern):
    """GitHub filter-pattern semantics: `**` crosses directories, `*` does not."""
    parts = re.split(r'(\*\*|\*)', pattern)
    return re.compile(''.join('.*' if part == '**' else '[^/]*' if part == '*' else re.escape(part)
                              for part in parts) + r'\Z')


class WasmBuildTests(unittest.TestCase):
    def test_pull_request_paths_cover_every_browser_switch(self):
        workflow = (ROOT / '.github/workflows/wasm.yml').read_text(encoding='utf-8')
        filters = [github_path_filter(pattern)
                   for pattern in re.findall(r"^      - '([^']+)'$", workflow, re.MULTILINE)]
        self.assertTrue(filters)
        switch = re.compile(r'feature = "wasm|target_arch = "wasm32"')
        uncovered = [path.relative_to(ROOT).as_posix() for path in sorted((ROOT / 'src').rglob('*.rs'))
                     if switch.search(path.read_text(encoding='utf-8'))
                     and not any(f.match(path.relative_to(ROOT).as_posix()) for f in filters)]
        self.assertEqual(uncovered, [])

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
            self.assertFalse((output / 'lan_rtc.js').exists())
            package_host(network, output, True, lan=True)
            self.assertIn("import './lan_rtc.js';", (output / 'host.js').read_text())
            package_host(single, output, False)
            self.assertFalse((output / 'lan_rtc.js').exists())
            self.assertNotIn('network.js', (output / 'host.js').read_text())
            self.assertFalse((output / 'network.js').exists())
            self.assertFalse((output / 'lobby.js').exists())
            self.assertIn('Single player.', (output / 'index.html').read_text())


if __name__ == '__main__':
    unittest.main()
