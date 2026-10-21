"""RGB565-unit comparison of shell captures against native screenshots."""

import copy
import hashlib
import json
import struct
import tempfile
import unittest
import zlib
from pathlib import Path

from tools import shell_capture_diff as diff


def png_bytes(width, height, depth, color, rows, palette=None):
    def chunk(kind, body):
        return (
            struct.pack(">I", len(body))
            + kind
            + body
            + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
        )

    body = chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, depth, color, 0, 0, 0))
    if palette is not None:
        body += chunk(b"PLTE", bytes(v for rgb in palette for v in rgb))
    raw = b"".join(bytes([kind]) + bytes(line) for kind, line in rows)
    return diff.PNG_SIGNATURE + body + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def write_bundle(directory, width, height, rgb_pixels):
    raw = bytearray()
    for r, g, b in rgb_pixels:
        raw.extend((b, g, r, 255))
    (directory / "frame.bgra").write_bytes(bytes(raw))
    manifest = {
        "checkpoint": "test",
        "capture_frame": 7,
        "frame": {"path": "frame.bgra", "sha256": hashlib.sha256(raw).hexdigest()},
        "surface": {
            "width": width,
            "height": height,
            "row_stride": width * 4,
            "pixel_layout": "BGRA8",
            "row_order": "top-left",
        },
    }
    (directory / "capture.json").write_text(json.dumps(manifest), encoding="utf-8")


def campaign_bundle(directory, campaign="all1", phase="first-live-frame"):
    """Synthetic protocol inputs, not native gameplay or rendered goldens."""
    write_bundle(directory, 1, 1, [(0, 0, 0)])
    manifest = json.loads((directory / "capture.json").read_text(encoding="utf-8"))
    emblem = {"all1": 0x6EA, "sov1": 0x6EC}[campaign]
    scenario = {"all1": "ALL01UMD.MAP", "sov1": "SOV01UMD.MAP"}[campaign]
    manifest.update({
        "schema_version": diff.CAMPAIGN_SCHEMA,
        "checkpoint": f"campaign-{campaign}-{phase}",
        "checkpoint_phase": phase,
        "parity_certification": "NONE",
        "route": [
            {"frame": 1, "dialog": 0xE2, "action": "SinglePlayer"},
            {"frame": 2, "dialog": 0x100, "action": "NewCampaign"},
            {"frame": 3, "dialog": 0x94, "action": "emblem press", "emblem": emblem,
             "campaign": campaign, "difficulty": 1},
            {"frame": 4, "dialog": 0x94, "action": "emblem release", "emblem": emblem,
             "campaign": campaign, "difficulty": 1},
            {"frame": 5, "action": "campaign loading admitted"},
        ],
        "startup": {
            "campaign": {"id": campaign.upper(), "index": 0, "cd": 2, "scenario": scenario},
            "difficulty": 1,
            "seed": {"value": 9, "source": "Controlled", "seed_authority_certifying": False},
        },
        "map_admission": "prepared",
        "selected_map_source": {"kind": "mix", "logical_name": scenario,
                                "source_archive": "mapsmd03.mix", "entry_id": -42,
                                "payload_len": 1, "source_sha256": "0" * 64},
        "process_source_ini_hashes": {
            "domain": "IniFile.content_hash parsed cache; not retail byte SHA256",
            "sources": [{"name": name, "parsed_cache_hash": value}
                        for name, value in [("RULESMD.INI", "0" * 16),
                                            ("LANGRULE.INI", None), ("ARTMD.INI", "1" * 16)]],
        },
        "coverage": {"mission_opening_1308": "unresolved"},
        "runtime": {
            "tick": 0, "game_mode_nonzero": False, "campaign_mission_counter": 1,
            "campaign_difficulty_rows": {"player": 1, "computer": 1},
            "current_house": {"name": "House", "native_unique_id": 2},
            "house_roster": [{"name": "House", "native_unique_id": 2,
                              "scalar_difficulty_bits": ["0" * 16] * 9}],
            "native_identity_cursor": 3,
            "rng": {stream: {"disabled": 0, "index_a": 0, "index_b": 103, "words": list(range(250))}
                    for stream in ("main", "scenario", "mapgen")},
            "active_rules": {"source_ini_hash": "0" * 16, "simulation_config_hash": "1" * 16},
            "camera": {"world_pixel_origin": [0, 0], "tactical_centre_cell": [1, 2],
                       "view_bookmarks": [[1, 2]] * 4},
        } if phase == "first-live-frame" else None,
    })
    manifest["frame"]["byte_length"] = 4
    (directory / "capture.json").write_text(json.dumps(manifest), encoding="utf-8")
    return manifest


def opening_bundle(directory, phase="opening-unlocked"):
    receipt = campaign_bundle(directory, "sov1")
    receipt.update(checkpoint=f"campaign-sov1-{phase}", checkpoint_phase=phase)
    receipt["runtime"]["scenario_started"] = True
    receipt["runtime"]["startup_admission"] = dict(diff.CAMPAIGN_INSTALLED)
    initial = copy.deepcopy(receipt["runtime"])
    initial["binary_frame"] = 0
    receipt["runtime"].update(tick=3, binary_frame=3)
    view = {"view_center_world_pixels": [0, 0], "focus_rate_bits": 1,
            "focus_progress_bits": struct.unpack("<I", struct.pack("<f", .5))[0]}
    locked = phase not in ("opening-unlocked", "opening-restore-unlocked",
                           "opening-timer-running", "opening-timer-expired")
    current = {"tick": 3, "binary_frame": 3, "input_locked": locked,
               "cursor_depth": -1 if locked else 0, "camera": view,
               "fog_viewer": receipt["runtime"]["current_house"]["name"],
               "flashing": [{"id": 1, "type": "MTNK", "cell_shrouded": False}],
               "no_effects_anims": [{"id": 2, "type": "ARRWSE", "cell_shrouded": False}],
               "messages": [{"text": "Synthetic protocol text"}]}
    saved = copy.deepcopy(initial)
    saved.update(tick=2, binary_frame=2)
    loaded = copy.deepcopy(saved)
    if phase == "opening-restore-unlocked":
        loaded["startup_admission"] = dict(diff.SAVED_WORLD_RESTORED)
        receipt["runtime"]["startup_admission"] = dict(diff.SAVED_WORLD_RESTORED)
    receipt["opening"] = {
        "target": phase, "initial_runtime": initial, "saw_lock": True,
        "saw_unlock": not locked,
        "exact_steps": [{"tick_before": tick, "tick_after": tick + 1,
                         "binary_frame_before": tick, "binary_frame_after": tick + 1}
                        for tick in range(3)],
        "current": current, "observations": [copy.deepcopy(current)],
        "locked_arrow_probe": {"locked": True, "custom_binding": False,
                               "press_admitted": False, "release_cleared": True},
        "unlocked_arrow_probe": {"locked": False, "custom_binding": False,
                                 "press_admitted": True, "release_cleared": True},
        "locked_mouse_probe": {
            "pre_lock_capture_cancelled": True, "locked_release_cleared": True,
            "locked_press_recorded": True, "left_held_until_unlock": True,
            "locked_press_captured": False,
        },
        "unlocked_mouse_probe": {
            "held_left_blocks_right_pan": True,
            "released_buttons_admit_right_pan": True, "capture_released": True,
            "camera_request_committed": False,
        },
        "save_restore": {"view_before": view, "view_after": view,
                         "cursor_depth_before": -1, "cursor_depth_after": -1,
                         "saved_runtime": saved, "loaded_runtime": loaded}
                        if phase == "opening-restore-unlocked" else None,
    }
    return receipt


def abort_bundle(directory, campaign="all1"):
    receipt = campaign_bundle(directory, campaign)
    receipt.update(checkpoint=f"campaign-{campaign}-abort-return", checkpoint_phase="abort-return",
                   capture_frame=23, runtime_observation="first-live-frame-before-Escape-press")
    receipt["route"] += [
        {"frame": 6, "action": "first live observed", "tick": 0},
        {"frame": 6, "action": "Escape press", "dialog": 0xB5},
        {"frame": 7, "action": "Escape release", "dialog": 0xB5},
        {"frame": 7, "action": "pause menu presented", "dialog": 0xB5},
        {"frame": 7, "action": "Abort press", "dialog": 0xB5, "point": [5, 5], "rect": [0, 0, 10, 10]},
        {"frame": 8, "action": "Abort release", "dialog": 0xB5},
        {"frame": 9, "action": "abort modal presented", "dialog": 0xB6},
        {"frame": 9, "action": "Leave press", "dialog": 0xB6, "point": [5, 5], "rect": [0, 0, 10, 10]},
        {"frame": 10, "action": "Leave release", "dialog": 0xB6},
        {"frame": 10, "action": "EXIT queued", "queued_at_tick": 0, "execute_tick": 0,
         "owner": "House", "owner_native_unique_id": 2, "modal_closed": True,
         "diagnostic_simulation_freeze": False, "quit_requested": False},
        {"frame": 11, "action": "EXIT consumed", "tick": 0, "scenario_exit_present": True,
         "scenario_outcome_present": False, "quit_requested": True},
        {"frame": 18, "action": "abort returned", "dialog": 0xE2, "tick": 0,
         "campaign_mission_counter": 1},
    ]
    receipt["cleanup"] = {
        "screen": "MainMenu", "route": "MainMenu", "loading_session_present": False,
        "loading_campaign_startup_present": False, "accepted_match_startup_present": False,
        "match_startup_receipt_present": False, "asset_manager_leased": False,
        "asset_manager_available": True, "campaign_page_present": False,
        "fullscreen_movie_present": False, "scenario_exit_present": False,
        "scenario_outcome_present": False, "in_game_menu": "Closed",
        "retained_runtime_present": True, "retained_tick": 0, "retained_quit_requested": True,
        "campaign_mission_counter": 1, "pending_exit_commands": 0,
    }
    (directory / "capture.json").write_text(json.dumps(receipt), encoding="utf-8")
    return receipt


def all01_load_bundle(directory):
    opening = opening_bundle(directory)["opening"]
    receipt = campaign_bundle(directory, "all1")
    phase = "opening-load-unlocked-while-locked"
    receipt.update(checkpoint="campaign-all1-opening-load-unlocked",
                   checkpoint_phase=phase, capture_frame=100, opening=opening)
    opening.update(target=phase, pre_lock_save="pre-lock.bin", unlocked_save="unlocked.bin",
                   load_while_locked={"depth_before": -1, "locked_after": False,
                       "depth_after": 0, "cursor_instances_after": 1,
                       "right_drag_pans_after": True})
    # Two real load boundaries: replay frame0, then load the frame3 unlock
    # while frame1 of that replay is locked. Following steps are ordinary.
    opening["exact_steps"] = [
        {"tick_before": tick, "tick_after": tick + 1,
         "binary_frame_before": tick, "binary_frame_after": tick + 1}
        for tick in [0, 1, 2, 0, *range(3, 49)]
    ]
    opening["current"].update(tick=49, binary_frame=49)
    observed = copy.deepcopy(opening["current"])
    opening["observations"] = []
    for tick in (0, 1, 3, 0, 1, 3, 49):
        row = copy.deepcopy(observed)
        row.update(tick=tick, binary_frame=tick,
                   input_locked=tick == 1, cursor_depth=-1 if tick == 1 else 0)
        opening["observations"].append(row)
    receipt["runtime"].update(tick=49, binary_frame=49, scenario_started=True,
                             startup_admission=dict(diff.SAVED_WORLD_RESTORED))
    return receipt


class PngDecodeTests(unittest.TestCase):
    def test_every_filter_reconstructs_rgb(self):
        # Row 1 (target 1..6 under row 0 = 10..60) encoded by hand with each
        # PNG filter type (RFC 2083 section 6).
        row0 = [10, 20, 30, 40, 50, 60]
        for kind, encoded in (
            (0, [1, 2, 3, 4, 5, 6]),
            (1, [1, 2, 3, 3, 3, 3]),
            (2, [247, 238, 229, 220, 211, 202]),
            (3, [252, 248, 244, 240, 235, 231]),
            (4, [247, 238, 229, 220, 241, 232]),
        ):
            data = png_bytes(2, 2, 8, 2, [(0, row0), (kind, encoded)])
            width, height, pixels = diff.read_png_rgb(data)
            self.assertEqual((width, height), (2, 2))
            self.assertEqual(
                pixels, [(10, 20, 30), (40, 50, 60), (1, 2, 3), (4, 5, 6)], f"filter {kind}"
            )

    def test_sub_byte_palette_indices_unpack_most_significant_first(self):
        palette = [(0, 0, 0), (255, 0, 0), (0, 255, 0), (0, 0, 255)]
        # Depth 2: indices 1, 2, 3, 0 in one byte.
        data = png_bytes(4, 1, 2, 3, [(0, [0b01101100])], palette)
        _, _, pixels = diff.read_png_rgb(data)
        self.assertEqual(pixels, [palette[1], palette[2], palette[3], palette[0]])


class CompareTests(unittest.TestCase):
    def test_units_ignore_expansion_bits_and_masks_exclude_pixels(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            # Same units with different low bits, one 1-unit and one 3-unit
            # red difference, and a masked difference.
            rust = [(247, 255, 8), (8, 0, 0), (24, 0, 0), (200, 200, 200)]
            native = [(240, 252, 15), (16, 0, 0), (0, 0, 0), (0, 0, 0)]
            write_bundle(temp, 2, 2, rust)
            rows = [(0, [v for p in native[:2] for v in p]), (0, [v for p in native[2:] for v in p])]
            (temp / "native.png").write_bytes(png_bytes(2, 2, 8, 2, rows))
            report_path = temp / "report.json"
            status = diff.main(
                [
                    "--capture",
                    str(temp),
                    "--native",
                    str(temp / "native.png"),
                    "--mask",
                    "cursor=1,1,1,1",
                    "--output",
                    str(report_path),
                    "--max-differing",
                    "1",
                ]
            )
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertEqual(status, 1)
            self.assertEqual(report["compared_pixels"], 3)
            self.assertEqual(report["differing_pixels"], 2)
            self.assertEqual(report["max_unit_difference"], 3)
            self.assertEqual(report["unit_difference_histogram"], {"1": 1, "3": 1})
            self.assertEqual(report["differing_bounds"], [0, 0, 1, 1])
            self.assertEqual(report["masks"], [{"label": "cursor", "rect": [1, 1, 1, 1]}])

    def test_manifest_without_frame_hash_is_checked_by_length(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            write_bundle(temp, 1, 1, [(0, 0, 0)])
            manifest = json.loads((temp / "capture.json").read_text(encoding="utf-8"))
            del manifest["frame"]["sha256"]
            manifest["frame"]["byte_length"] = 4
            (temp / "capture.json").write_text(json.dumps(manifest), encoding="utf-8")
            (temp / "native.png").write_bytes(png_bytes(1, 1, 8, 2, [(0, [0, 0, 0])]))
            report_path = temp / "report.json"
            status = diff.main(
                [
                    "--capture",
                    str(temp),
                    "--native",
                    str(temp / "native.png"),
                    "--output",
                    str(report_path),
                ]
            )
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertEqual(status, 0)
            self.assertEqual(
                report["capture"]["frame_sha256"],
                hashlib.sha256(b"\x00\x00\x00\xff").hexdigest(),
            )
            manifest["frame"]["byte_length"] = 8
            (temp / "capture.json").write_text(json.dumps(manifest), encoding="utf-8")
            status = diff.main(["--capture", str(temp), "--native", str(temp / "native.png")])
            self.assertEqual(status, 2)


    def test_tampered_frame_is_invalid_input(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            write_bundle(temp, 1, 1, [(0, 0, 0)])
            (temp / "frame.bgra").write_bytes(b"\x01\x00\x00\xff")
            (temp / "native.png").write_bytes(png_bytes(1, 1, 8, 2, [(0, [0, 0, 0])]))
            status = diff.main(["--capture", str(temp), "--native", str(temp / "native.png")])
            self.assertEqual(status, 2)


class CampaignReceiptTests(unittest.TestCase):
    def test_both_abort_returns_validate_the_real_route_protocol(self):
        with tempfile.TemporaryDirectory() as temp:
            for campaign in ("all1", "sov1"):
                receipt = abort_bundle(Path(temp), campaign)
                self.assertEqual(diff.validate_campaign_capture(receipt)["phase"], "abort-return")

    def test_abort_return_rejects_shortcuts_and_unretired_owners(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = abort_bundle(Path(temp))
            mutations = [
                lambda value: value["route"].pop(15),  # The EXIT-consumed observation.
                lambda value: value["route"][14].update(diagnostic_simulation_freeze=True),
                lambda value: value["route"][12].update(point=[10, 10]),
                lambda value: value["route"][10].update(frame=7),
                lambda value: value["cleanup"].update(asset_manager_leased=True),
                lambda value: value["cleanup"].update(loading_session_present=True),
                lambda value: value["cleanup"].update(campaign_mission_counter=2),
                lambda value: value["cleanup"].update(scenario_exit_present=True),
                lambda value: value["cleanup"].update(campaign_installation_present=True),
                lambda value: value["cleanup"].update(retained_scenario_started=True),
                lambda value: value["cleanup"].update(retained_quit_requested=False),
                lambda value: value["route"][14].update(quit_requested=True),
                lambda value: value.update(runtime_observation="after-return"),
            ]
            for mutate in mutations:
                invalid = copy.deepcopy(receipt)
                mutate(invalid)
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_four_start_checkpoints_validate_bytes_without_asserting_parity(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            for campaign in ("all1", "sov1"):
                for phase in ("loading-first-frame", "first-live-frame"):
                    campaign_bundle(directory, campaign, phase)
                    output = directory / "validation.json"
                    status = diff.main(["--capture", str(directory), "--validate-only", "--output", str(output)])
                    self.assertEqual(status, 0)
                    report = json.loads(output.read_text(encoding="utf-8"))
                    self.assertEqual(report["campaign"]["campaign"], campaign)
                    self.assertEqual(report["campaign"]["phase"], phase)
                    self.assertEqual(report["parity_certification"], "NONE")

    def test_explicit_campaign_capture_keeps_the_selected_map(self):
        with tempfile.TemporaryDirectory() as temp:
            for checkpoint, scenario in diff.EXPLICIT_CAMPAIGN_MAPS.items():
                receipt = campaign_bundle(Path(temp))
                receipt.update(checkpoint=checkpoint, checkpoint_phase="explicit-first-live-frame")
                receipt["startup"]["scenario"] = scenario
                receipt["selected_map_source"]["logical_name"] = scenario
                receipt["startup"]["seed"]["value"] = 1
                if scenario == "ALL02UMD.MAP":
                    receipt["runtime"]["player_buildings"] = [{
                        "type": "NAFLAK", "cell": [159, 165], "in_playfield": False,
                        "discovered": False, "opened": False, "ready": 0,
                        "mission": -1, "queued": 5,
                    }]
                receipt["route"][2:4] = [{"frame": 3, "action": "explicit campaign spawn",
                                         "scenario": scenario, "difficulty": 1, "seed": 1}]
                diff.validate_campaign_capture(receipt)
                if scenario == "ALL02UMD.MAP":
                    receipt["runtime"]["player_buildings"][0]["ready"] = 1
                    with self.assertRaises(diff.InputError):
                        diff.validate_campaign_capture(receipt)
                    receipt["runtime"]["player_buildings"][0]["ready"] = 0
                receipt["startup"]["scenario"] = "SOV01UMD.MAP"
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(receipt)

    def test_process_source_filenames_allow_their_physical_case(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = campaign_bundle(Path(temp))
            sources = receipt["process_source_ini_hashes"]["sources"]
            for source in sources:
                source["name"] = source["name"].lower()
            diff.validate_campaign_capture(receipt)
            sources.reverse()
            with self.assertRaises(diff.InputError):
                diff.validate_campaign_capture(receipt)

    def test_first_loading_readback_requires_prepared_map_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = campaign_bundle(Path(temp), phase="loading-first-frame")
            self.assertEqual(diff.validate_campaign_capture(receipt)["selected_map_sha256"], "0" * 64)
            mutations = [
                lambda value: value.update(map_admission="pending", selected_map_source=None),
                lambda value: value.update(selected_map_source=None),
                lambda value: value.update(map_admission="pending"),
                lambda value: value.pop("map_admission"),
            ]
            for mutate in mutations:
                invalid = copy.deepcopy(receipt)
                mutate(invalid)
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_first_live_rejects_a_running_or_skirmish_runtime(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            receipt = campaign_bundle(directory)
            for key, value in (("tick", 1), ("tick", False), ("game_mode_nonzero", True)):
                invalid = copy.deepcopy(receipt)
                invalid["runtime"][key] = value
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_opening_receipts_require_contiguous_steps_and_connected_consumers(self):
        with tempfile.TemporaryDirectory() as temp:
            for phase in ("opening-locked", "opening-pan", "opening-flash", "opening-arrows",
                          "opening-unlocked", "opening-restore-unlocked"):
                receipt = opening_bundle(Path(temp), phase)
                self.assertEqual(diff.validate_campaign_capture(receipt)["phase"], phase)
            receipt = opening_bundle(Path(temp))
            mutations = [
                lambda value: value["opening"]["initial_runtime"].update(scenario_started=False),
                lambda value: value["runtime"].update(scenario_started=False),
                lambda value: value["opening"]["exact_steps"][1].update(tick_before=0),
                lambda value: value["opening"]["exact_steps"][1].update(binary_frame_after=1),
                lambda value: value["opening"]["current"].update(tick=4),
                lambda value: value["opening"].update(saw_lock=False),
                lambda value: value["opening"].update(locked_mouse_probe=None),
                lambda value: value["opening"]["locked_mouse_probe"].update(locked_release_cleared=False),
                lambda value: value["opening"]["locked_mouse_probe"].update(locked_press_captured=True),
                lambda value: value["opening"]["unlocked_mouse_probe"].update(held_left_blocks_right_pan=False),
                lambda value: value["opening"]["unlocked_mouse_probe"].update(released_buttons_admit_right_pan=False),
                lambda value: value["opening"]["current"].update(input_locked=True),
                lambda value: value["opening"]["current"].update(fog_viewer="wrong player"),
                lambda value: value["opening"]["observations"][0].update(no_effects_anims=[]),
                lambda value: value["opening"]["observations"][0].update(messages=[]),
                lambda value: value["opening"]["observations"][0].update(flashing=[]),
                lambda value: value["opening"]["observations"][0]["flashing"][0].update(cell_shrouded=True),
                lambda value: value["opening"]["observations"][0]["no_effects_anims"][0].update(cell_shrouded=True),
            ]
            for mutate in mutations:
                invalid = copy.deepcopy(receipt)
                mutate(invalid)
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_countdown_capture_requires_running_then_hidden_timer(self):
        with tempfile.TemporaryDirectory() as temp:
            for phase in ("opening-timer-running", "opening-timer-expired"):
                receipt = opening_bundle(Path(temp), phase)
                opening = receipt["opening"]
                opening["saw_countdown"] = True
                opening["timer_action_replay"] = {
                    "mode": "retail-timer-actions-after-opening-unlock",
                    "trigger": "0952F87C", "action_count": 3, "tick": 0, "binary_frame": 0,
                }
                running = {"remaining": 1800, "caption": "MISSION:TmeTimer"}
                opening["observations"][0]["scenario_timer"] = running
                opening["current"]["scenario_timer"] = (
                    running if phase == "opening-timer-running" else None)
                opening["last_running_countdown"] = {
                    "tick": 2, "binary_frame": 2,
                    "timer": {"remaining": 0, "caption": "MISSION:TmeTimer"},
                }
                diff.validate_campaign_capture(receipt)
                invalid = copy.deepcopy(receipt)
                invalid["opening"]["current"]["scenario_timer"] = (
                    None if phase == "opening-timer-running" else
                    {"remaining": 0, "caption": "MISSION:TmeTimer"})
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)
                invalid = copy.deepcopy(receipt)
                invalid["opening"]["observations"][0]["scenario_timer"] = None
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_opening_load_preserves_frame_pan_and_retained_rng_owners(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = opening_bundle(Path(temp), "opening-restore-unlocked")
            for mutate in (
                lambda value: value["opening"]["save_restore"].update(view_after={}),
                lambda value: value["opening"]["save_restore"].update(cursor_depth_after=0),
                lambda value: value["opening"]["save_restore"]["loaded_runtime"].update(tick=0),
                lambda value: value["opening"]["save_restore"]["loaded_runtime"]["rng"]["main"].update(index_a=1),
                lambda value: value["opening"]["save_restore"]["loaded_runtime"].update(
                    startup_admission=dict(diff.CAMPAIGN_INSTALLED)),
                lambda value: value["runtime"].update(startup_admission=dict(diff.CAMPAIGN_INSTALLED)),
            ):
                invalid = copy.deepcopy(receipt)
                mutate(invalid)
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_all01_registered_load_checkpoint_accepts_its_two_load_boundaries(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = all01_load_bundle(Path(temp))
            self.assertEqual(diff.validate_campaign_capture(receipt)["phase"],
                             "opening-load-unlocked-while-locked")

    def test_all01_load_requires_a_locked_replay_owner_before_forward_load(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = all01_load_bundle(Path(temp))
            for remove_witness in (False, True):
                invalid = copy.deepcopy(receipt)
                rows = invalid["opening"]["observations"]
                if remove_witness:
                    del rows[4]
                else:
                    rows[4].update(input_locked=False, cursor_depth=0)
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_opening_observations_belong_to_their_actual_production_segment(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = all01_load_bundle(Path(temp))
            for change in ({"binary_frame": 90000}, {"binary_frame": 2},
                           {"tick": 2, "binary_frame": 2}):
                invalid = copy.deepcopy(receipt)
                invalid["opening"]["observations"][4].update(change)
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)
            invalid = opening_bundle(Path(temp))
            invalid["opening"]["observations"][0]["binary_frame"] = 90000
            with self.assertRaises(diff.InputError):
                diff.validate_campaign_capture(invalid)

    def test_all01_load_checkpoint_rejects_unexplained_jumps_and_hidden_cursor(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = all01_load_bundle(Path(temp))
            for mutate in (
                lambda value: value["opening"]["exact_steps"][3].update(tick_before=2),
                lambda value: value["opening"]["exact_steps"][4].update(binary_frame_before=4),
                lambda value: value["opening"]["load_while_locked"].update(depth_after=-1),
                lambda value: value["opening"]["load_while_locked"].update(locked_after=True),
                lambda value: value["opening"]["load_while_locked"].update(right_drag_pans_after=False),
                lambda value: value["runtime"].update(startup_admission=dict(diff.CAMPAIGN_INSTALLED)),
            ):
                invalid = copy.deepcopy(receipt)
                mutate(invalid)
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_rejects_wrong_emblem_map_identity_and_missing_native_owners(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = campaign_bundle(Path(temp))
            mutations = [
                lambda value: value["route"][3].update(emblem=0x6EC),
                lambda value: value["route"][3].update(frame=3),
                lambda value: value["selected_map_source"].update(logical_name="SOV01UMD.MAP"),
                lambda value: value["selected_map_source"].update(kind="legacy_fallback"),
                lambda value: value["selected_map_source"].update(source_sha256=""),
                lambda value: value["runtime"]["current_house"].update(native_unique_id=3),
                lambda value: value["runtime"]["rng"]["scenario"].update(words=[]),
                lambda value: value["process_source_ini_hashes"]["sources"][0].update(parsed_cache_hash=None),
            ]
            for mutate in mutations:
                invalid = copy.deepcopy(receipt)
                mutate(invalid)
                with self.assertRaises(diff.InputError):
                    diff.validate_campaign_capture(invalid)

    def test_loading_does_not_claim_installed_runtime_and_back_retires_route(self):
        with tempfile.TemporaryDirectory() as temp:
            receipt = campaign_bundle(Path(temp), phase="loading-first-frame")
            receipt["runtime"] = {"tick": 0}
            with self.assertRaises(diff.InputError):
                diff.validate_campaign_capture(receipt)
            receipt.update(checkpoint="campaign-back-return", checkpoint_phase="campaign-back-return")
            receipt["route"] = receipt["route"][:2] + [
                {"frame": 3, "dialog": 0x94, "action": "Back"},
                {"frame": 4, "dialog": 0x100, "action": "Back returned"},
            ]
            receipt["shell_return"] = {"screen": "MainMenu", "route": "SinglePlayer",
                                       "campaign_page_present": False, "loading_session_present": False}
            self.assertEqual(diff.validate_campaign_capture(receipt)["phase"], "campaign-back-return")
            receipt["shell_return"]["loading_session_present"] = True
            with self.assertRaises(diff.InputError):
                diff.validate_campaign_capture(receipt)

if __name__ == "__main__":
    unittest.main()
