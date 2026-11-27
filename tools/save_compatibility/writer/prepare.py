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
    commits["ffe8b25afb8cb45050a0097bd5578d41bdb0bba6"] = "main-ffe8b25af"
    commits["93ab2ce86bdc5a4a6cfd97651ff628c68db7cfb8"] = "main-93ab2ce86"
    commits["06789b62bf74ce8d782e0b734a8568561f52c687"] = "main-06789b62b"
    commits["8d55b4a73e9dca60dae7efdcfdf3221823f00ef3"] = "main-8d55b4a73"
    commits["8852f7a762edfd4097a53b1e35bc44393fd1fc19"] = "main-8852f7a76"
    commits["bc55c35e740e8cfeb56973f784b762a908c318bb"] = "main-bc55c35e7"
    commits["30e9a0f3839bf78e5cbba2dbd0a4c85995701b89"] = "main-30e9a0f38"
    commits["a33fcac81db63ff3dc4d39000b4d87e0930204e4"] = "main-a33fcac81"
    commits["d17e45e42866f3506baf74ee31c22f3471f16e61"] = "main-d17e45e42"
    commits["2e39b88dde2230909dffc491638fc831e9232b30"] = "main-2e39b88dd"
    commits["2e67ebaa84545e3317e2273162ac5c2a72b3e1f1"] = "main-2e67ebaa8"
    commits["fb29ff47307cea7f0615774ab49932841156799d"] = "main-fb29ff473"
    commits["b268ba798a3f24d88c14218f82a5222165f841f7"] = "main-b268ba798"
    commits["0b918d0535c0944b07c013b5921aec44b2c59228"] = "main-0b918d053"
    commits["c3609b58aea0f0e261252dc89e6ec78c287073b8"] = "main-c3609b58a"
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
