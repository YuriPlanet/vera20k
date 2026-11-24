"""The opt-in package cannot leak networking into a later single-player build."""
from pathlib import Path
import tempfile
import unittest

from tools.wasm.build import package_fonts, package_host, validate_proxy_url

ROOT = Path(__file__).resolve().parents[2]


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
            self.assertFalse((output / 'lan_rtc.js').exists())
            package_host(network, output, True, lan=True)
            self.assertIn("import './lan_rtc.js';", (output / 'host.js').read_text())
            package_host(single, output, False)
            self.assertFalse((output / 'lan_rtc.js').exists())
            self.assertNotIn('network.js', (output / 'host.js').read_text())
            self.assertFalse((output / 'network.js').exists())
            self.assertFalse((output / 'lobby.js').exists())
            self.assertIn('Single player.', (output / 'index.html').read_text())

    def test_package_names_its_build_in_the_footer(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package_host(root, root / 'output', False, label='Build v1 · fast · single player')
            page = (root / 'output' / 'index.html').read_text(encoding='utf-8')
            self.assertIn('<span id="build">Build v1 · fast · single player</span>', page)
            self.assertNotIn('Unpackaged page', page)

    def test_host_serves_the_skirmish_map_as_a_separate_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package_host(root, root / 'output', False)
            name = 'Arctic_Circle_16_Ice_Passages.map'
            self.assertEqual((root / 'output' / 'Maps' / name).read_bytes(),
                             (ROOT / 'maps' / name).read_bytes())

    def test_engine_package_ships_the_ice_terrain_files(self):
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            package_host(stage, stage / 'out', False)
            for name in ('vera_ice.mix', 'vera_ice_tiles.ini'):
                self.assertEqual((stage / 'out/pkg/Terrain' / name).read_bytes(),
                                 (ROOT / 'Terrain' / name).read_bytes())

    def test_package_carries_the_files_the_engine_fetches_before_starting(self):
        # src/render/text_engine/web_fonts.rs PRELOADS: the engine embeds neither.
        with tempfile.TemporaryDirectory() as directory:
            fonts = Path(directory) / 'fonts'
            package_fonts(fonts)
            for name in ('NotoSans-VF.ttf', 'Unifont-CJK-15.bin', 'OFL-Unifont.txt'):
                self.assertTrue((fonts / name).is_file(), name)


if __name__ == '__main__':
    unittest.main()
