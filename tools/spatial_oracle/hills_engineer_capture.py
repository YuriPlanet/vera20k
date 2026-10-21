"""Project an actual Hills Engineer export for the bounded native entry runners.

Run the ignored retail_hills_engineer_enters_hut_and_repairs Rust test with
VERA20K_BRIDGE_ZONE_EXPORT set, then pass one mark0 or walk-prehead JSON file.
This maps captured inputs only; AStar local search state remains supplied.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def project(path):
    raw = path.read_bytes()
    source = json.loads(raw)
    boundary = source["boundary"]
    assert boundary in ("mark0", "walk-prehead"), boundary
    entities = {entry["id"]: entry for entry in source["entities"]}
    actor_id = source["actor"]
    actor_entry = entities[actor_id]
    actor = actor_entry["entity"]
    assert actor_entry["type_name"] == "ENGINEER"
    hut_id = actor["navigation"]["nav_com"]["Object"]["id"]
    hut_entry = entities[hut_id]
    hut = hut_entry["entity"]
    assert hut_entry["type_name"] == "CABHUT"
    assert not actor_entry["team_present"] and not hut_entry["team_present"]
    assert actor["attack_target"] is None and actor["slave"]["owner"] is None
    assert actor["dock_entered_with"] is None and actor["in_playfield"]
    assert not actor["on_bridge"]
    assert hut["teleport_state"] is None and hut["invulnerability"] is None
    assert re.search(r"\bspeed_type: Foot,", actor_entry["type"])
    assert re.search(r"\bis_train: false,", actor_entry["type"])

    def xyz(entity):
        position = entity["position"]
        # Production positions can retain fractions; native physical Coord is
        # the integer part selected by the Rust position owner, not a rounding.
        return [position["rx"] * 256 + position["sub_x"]["bits"] // 65536,
                position["ry"] * 256 + position["sub_y"]["bits"] // 65536,
                position["exact_z_leptons"] if position["exact_z_leptons"] is not None
                else position["z"] * 104]

    def object_ids(items):
        result = []
        for item in items:
            match = re.fullmatch(r"Entity\((\d+)\)", item)
            assert match, item
            ident = int(match.group(1))
            assert ident in (actor_id, hut_id), (ident, item)
            result.append(ident)
        return result

    indexed = {tuple(cell["coord"]): cell for cell in source["cells"]}
    cells = []
    for at in ((66, 76), (67, 75), (68, 74)):
        cell = indexed[at]
        assert "tube_index: None," in cell["full_cell_debug"]
        assert cell["ground_owner"] in (None, actor["owner"])
        assert cell["deck_owner"] is None
        if cell["ground_owner"] is not None:
            assert cell["ground_owner"] == 0 and at == (67, 75)
        cells.append(dict(
            coord=cell["coord"], overlay=cell["bridge_facts"]["overlay_id"]
            if cell["bridge_facts"]["overlay_id"] is not None else -1,
            occupation_owners=[cell["ground_owner"] if cell["ground_owner"] is not None else -1, -1],
            land=cell["land"], tube_index=-1,
            level=cell["level"], slope=cell["slope"],
            occupation=[cell["ground_bits"], cell["deck_bits"]],
            bridge_flags=cell["bridge_facts"]["raw_flags"],
            ground_objects=object_ids(cell["ground_list"]),
            upper_objects=object_ids(cell["deck_list"])))
    assert all(cell["level"] == 10 and cell["bridge_flags"] == 0 for cell in cells)
    physical = xyz(actor)
    if boundary == "walk-prehead":
        assert (actor["position"]["rx"], actor["position"]["ry"]) == (67, 75)
        replay = actor["navigation"]["path_replay"]
        assert replay["directions"][replay["cursor"]] == 1
    row = dict(
        name=f"hills_{boundary}_{source['binary_frame']}_foot_capture_hut_neighbor",
        origin="Projected actual production fields; original entry executes on supplied storage",
        native_size=source["native_size"], local_size=source["local_size"],
        frame=source["binary_frame"], cells=cells,
        actor=dict(id=actor_id, coord=physical,
                   current_mission=actor["mission"]["current"],
                   queued_mission=actor["mission"]["queued"],
                   in_playfield=actor["in_playfield"], terrain_bypass=False,
                   team=None, slave_owner=None, nav_is_hut=True,
                   attack_is_hut=False, speed_type=0, is_train=False, on_bridge=False),
        hut=dict(id=hut_id, coord=xyz(hut), warping_out=False,
                 iron_curtain_start=-1, iron_curtain_duration=0),
        candidate=[68, 74], direction=1, height=10, goal_height=10,
        urgency=0, bridge_list=False, **{"from": [67, 75]})
    return dict(
        schema_version=1, cases=[row], capture=path.name,
        capture_sha256=sha(raw),
        supplied_vs_executed=[
            "Physical state, mission/NavCom, lists, occupation, terrain and normalized map dimensions are captured from Rust, not native whole-object constructors.",
            "SpeedType Foot0 is checked against the captured type diagnostic; its constructor/reader proof is the separate infantry_speed_type corpus.",
            "The occupied source retains Rust owner identity0. It is supplied as native slot0; walk_capture_entry asserts that this source's occupation-owner fields are not read in the admitted hut corridor. The hut cell's empty owner remains native -1.",
            "Inactive IronCurtain and warping flags map from absent Rust lifecycle state. These lifecycle transitions do not execute in this corridor.",
            "AStar from67,75 to68,74, NE direction1, heights10 and list selection are supplied interior state. Whole-route hierarchy/closed-list/reconstruction do not execute.",
            "For walk-prehead, original75B59C produces prospective coordinates and all CanEnter arguments from the captured physical pose and retained direction.",
            "Both runners execute unmodified concrete Infantry51BF90; native applicable land-speed readers execute during setup."])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    data = project(args.capture)
    text = json.dumps(data, indent=2, allow_nan=False) + "\n"
    if args.output.exists():
        assert args.output.read_text() == text, "Refusing to replace a different capture projection"
    else:
        args.output.write_text(text)
    print(json.dumps(dict(output=str(args.output), sha256=sha(text.encode()),
                          capture_sha256=data["capture_sha256"])))


if __name__ == "__main__":
    main()
