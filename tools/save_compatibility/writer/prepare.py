"""Stage the historical save fixture writer in a disposable release checkout.

Run this file from the root of the fixture's clean, detached checkout.
Build and invoke the resulting snapshot-layout binary as in README.md.
"""
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = Path.cwd()


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def main():
    commits = {
        git("rev-parse", f"{tag}^{{commit}}"): tag
        for tag in ("v0.3.8", "v0.3.9", "v0.4.0")
    }
    commits["eb05f451e965337534a68d631a2217a242271b36"] = "main-eb05f451e"
    commits["41f140f93db4d28dcc5b8f4f4a839593bf8de787"] = "main-41f140f93"
    commits["6d6caa108e36043ed690d07a12923d15b3cdc3de"] = "main-6d6caa108"
    tag = commits.get(git("rev-parse", "HEAD"))
    if not tag or git("branch", "--show-current") or git("status", "--porcelain", "-uno"):
        raise SystemExit("Run in a clean, detached checkout of a listed fixture revision.")

    staged = ROOT / "target" / "save-fixture-writer"
    staged.mkdir(parents=True, exist_ok=True)
    helpers = (ROOT / "src/app/loading/init_helpers.rs").read_text(encoding="utf-8")
    begin = helpers.index("pub(crate) fn bind_rules_art_metadata(")
    end = helpers.index("/// Palette variants", helpers.index("pub(crate) fn bind_rules_asset_closure("))
    imports = (
        "use vera20k_core::assets::asset_manager::AssetManager;\n"
        "use vera20k_core::rules::{ini_parser::IniFile, ruleset::RuleSet,"
        "effect_asset_catalog::tolerant_anim_class_roots};\n"
    )
    (staged / "bindings.rs").write_text(imports + helpers[begin:end], encoding="utf-8")
    (staged / "scenario.rs").write_bytes((HERE / "scenario.rs").read_bytes())
    assets = staged / "assets"
    assets.mkdir(exist_ok=True)
    (assets / "COMPATGLOW.SHP").write_bytes((HERE / "COMPATGLOW.SHP").read_bytes())
    tube_body = 'panic!("this release has no tunnel fixture at {path:?}")'
    if tag == "v0.3.9":
        source = ROOT / "src/sim/movement/tube_movement.rs"
        text = source.read_text(encoding="utf-8")
        marker = "#[cfg(all(test, tests_sim_movement))]\nmod tests {"
        if text.count(marker) != 1:
            raise SystemExit("Cannot locate the historical tube movement fixture.")
        tube = (HERE / "tunnel.rs").read_text(encoding="utf-8")
        fixture_module = '#[cfg(feature = "test-support")]\npub mod fixture {\n' + tube
        source.write_text(text.replace(marker, fixture_module), encoding="utf-8")
        tube_body = (
            "let (sim, terrain) = sim::movement::tube_movement::fixture::historical_tunnel();\n"
            "writer::write_world(sim, terrain, path, sim::scenario_session::ScenarioGameMode::Skirmish, writer::STANDARD)"
        )
    dispatch = "fn write_tunnel(path: &std::path::Path) {\n" + tube_body + "\n}\n"
    if tag == "v0.4.0":
        dispatch += (HERE / "alpha.rs").read_text(encoding="utf-8")
    else:
        dispatch += 'fn write_alpha(_: &std::path::Path) { panic!("alpha requires v0.4.0") }\n'
    (staged / "revision_cases.rs").write_text(dispatch, encoding="utf-8")
    (ROOT / "src/bin/snapshot-layout.rs").write_bytes((HERE / "main.rs").read_bytes())


if __name__ == "__main__":
    main()
