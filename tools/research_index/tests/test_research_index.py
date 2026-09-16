from __future__ import annotations

import json
import os
import secrets
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


TOOL_ROOT = Path(__file__).resolve().parents[1]
if str(TOOL_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOL_ROOT))

from research_index.chunking import chunk_file, ini_section_name
from research_index.database import rebuild_database, search
from research_index.brief import research_brief
from research_index.formatting import (
    format_document_graph,
    format_graph_view,
    format_index_health,
    format_parity_handoff,
    format_research_brief,
    format_system_map,
    format_validation,
)
from research_index.graph import document_graph, evidence_view, graph_document_score, implementation_view
from research_index.handoff import implementation_handoff_candidates, parity_handoff
from research_index.lifecycle import (
    IndexLifecycleError,
    ensure_fresh,
    inspect_index,
    manifest_path,
    refresh_index,
)
from research_index.metadata import document_metadata, extract_terms
from research_index.ranking import final_score
from research_index.system_map import system_map
from research_index.validation import validate_index


class ExtractionTests(unittest.TestCase):
    def test_extract_terms_collects_citable_evidence_and_filters_report_labels(self) -> None:
        text = """
        BridgeRepairHutClass__Repair calls UnitClass::Try_To_Deploy at 0x00574000.
        Rust touchpoint: src/sim/bridge_state/repair.rs.
        See [bridge system](../00-system-models/BRIDGE_SYSTEM.md#repair).
        Backtick INI key `RepairBridge=`.
        ADDRESS_MAP and BRIDGE_REPAIR_GHIDRA_REPORT are report labels, not symbols.
        """

        terms = extract_terms(text, ".md")

        self.assertEqual(terms.addresses, ("0x00574000",))
        self.assertIn("BridgeRepairHutClass__Repair", terms.symbols)
        self.assertIn("UnitClass::Try_To_Deploy", terms.symbols)
        self.assertNotIn("ADDRESS_MAP", terms.symbols)
        self.assertNotIn("BRIDGE_REPAIR_GHIDRA_REPORT", terms.symbols)
        self.assertEqual(terms.ini_keys, ("RepairBridge",))
        self.assertEqual(terms.rust_paths, ("src/sim/bridge_state/repair.rs",))
        self.assertEqual(terms.links, ("../00-system-models/BRIDGE_SYSTEM.md",))

    def test_ini_assignment_keys_are_only_collected_from_ini_files(self) -> None:
        text = "RepairBridge=yes\nBridgeRepairHut=GACNST\n"

        self.assertEqual(extract_terms(text, ".ini").ini_keys, ("BridgeRepairHut", "RepairBridge"))
        self.assertEqual(extract_terms(text, ".md").ini_keys, ())


class IniSectionHeaderTests(unittest.TestCase):
    """Header detection must match `INIClass__LoadFromStraw @ 0x00525A60`.

    The name is the text between `[` and the *first* `]`; the rest of the line
    is discarded (`MOV byte ptr [EAX],0x0` @ 0x00525DDB). A `]` somewhere on
    the line is required (`strchr` via `PUSH 0x5d` @ 0x00525DCC), so `[Foo`
    alone opens nothing.
    """

    def test_plain_header(self) -> None:
        self.assertEqual(ini_section_name("[NACLON]"), "NACLON")

    def test_leading_whitespace_is_trimmed_before_the_bracket_test(self) -> None:
        self.assertEqual(ini_section_name("\t  [NACLON]  \r"), "NACLON")

    def test_trailing_semicolon_comment_is_discarded(self) -> None:
        self.assertEqual(ini_section_name("[CAHOSP];copypasted the good one on top of it"), "CAHOSP")
        self.assertEqual(ini_section_name("[ROCK] ; Rocketeer"), "ROCK")

    def test_trailing_second_bracket_group_is_discarded(self) -> None:
        # Retail rulesmd.ini:13561.
        self.assertEqual(ini_section_name("[GAFWLL];temp wall for yuri[YAWALL]"), "GAFWLL")

    def test_closing_bracket_inside_a_trailing_comment_does_not_extend_the_name(self) -> None:
        self.assertEqual(ini_section_name("[DISK];gs changed to K [see the Master Name Doc]"), "DISK")

    def test_open_bracket_without_a_closing_bracket_is_not_a_header(self) -> None:
        self.assertIsNone(ini_section_name("[Foo"))
        self.assertIsNone(ini_section_name("  [JumpjetControls ;gs no bracket"))

    def test_non_header_lines_are_not_headers(self) -> None:
        self.assertIsNone(ini_section_name(""))
        self.assertIsNone(ini_section_name("Wall=yes"))
        self.assertIsNone(ini_section_name(";[NotASection]"))
        self.assertIsNone(ini_section_name("Prerequisite=YABRCK ; [not a section]"))

    def test_decorated_headers_start_their_own_chunk_instead_of_merging_upward(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "rules.ini"
            path.write_text(
                "[NACLON]\nStrength=1000\n"
                "[GAFWLL];temp wall for yuri[YAWALL]\nWall=yes\n"
                "[Parasite] ; special warhead\nVersus=100%%\n",
                encoding="utf-8",
            )

            chunks = chunk_file(path)

            self.assertEqual(
                [(chunk.heading_path, chunk.start_line, chunk.end_line) for chunk in chunks],
                [("INI [NACLON]", 1, 2), ("INI [GAFWLL]", 3, 4), ("INI [Parasite]", 5, 6)],
            )


class RankingTests(unittest.TestCase):
    def test_verified_binary_evidence_beats_stronger_unknown_lexical_match(self) -> None:
        ghidra_score = final_score(-1.0, "ghidra", "verified")
        unknown_score = final_score(-8.0, "unknown", "unknown")

        self.assertGreater(ghidra_score, unknown_score)

    def test_graph_document_score_prefers_focused_docs_over_catch_all_maps(self) -> None:
        focused = _row(
            """
            SELECT 1 AS match_count,
                   'docs/research/bridges/repair/BRIDGE_REPAIR_GHIDRA_REPORT.md' AS path,
                   'BridgeRepairHut repair' AS title,
                   'bridges' AS system,
                   'repair' AS subsystem,
                   'ghidra' AS source_kind,
                   'verified' AS status
            """
        )
        catch_all = _row(
            """
            SELECT 4 AS match_count,
                   'docs/research/bridges/ADDRESS_MAP.md' AS path,
                   'Address map' AS title,
                   'bridges' AS system,
                   'root' AS subsystem,
                   'unknown' AS source_kind,
                   'unknown' AS status
            """
        )

        self.assertGreater(graph_document_score(focused, "BridgeRepairHut"), graph_document_score(catch_all, "BridgeRepairHut"))

    def test_graph_document_score_keeps_verified_evidence_above_unknown_title_hits(self) -> None:
        verified = _row(
            """
            SELECT 1 AS match_count,
                   'docs/research/miner/MISSION_HARVEST_GHIDRA_REPORT.md' AS path,
                   'UnitClass Mission Harvest' AS title,
                   'miner' AS system,
                   'root' AS subsystem,
                   'ghidra' AS source_kind,
                   'verified' AS status
            """
        )
        unknown_title_hit = _row(
            """
            SELECT 1 AS match_count,
                   'docs/research/chronominer-locomotion/string-teleporter.md' AS path,
                   'Teleporter string note' AS title,
                   'unknown' AS system,
                   'unknown' AS subsystem,
                   'unknown' AS source_kind,
                   'unknown' AS status
            """
        )

        self.assertGreater(graph_document_score(verified, "Teleporter"), graph_document_score(unknown_title_hit, "Teleporter"))


class GraphCitationTests(unittest.TestCase):
    def test_graph_views_preserve_file_line_citations_and_rust_touchpoints(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            repair_doc = workspace / "docs/research/bridges/repair/BRIDGE_REPAIR_GHIDRA_REPORT.md"
            system_doc = workspace / "docs/research/bridges/00-system-models/BRIDGE_SYSTEM.md"
            repair_doc.parent.mkdir(parents=True)
            system_doc.parent.mkdir(parents=True)

            repair_doc.write_text(
                "\n".join(
                    [
                        "# Bridge Repair",
                        "See [system](../00-system-models/BRIDGE_SYSTEM.md).",
                        "BridgeRepairHutClass__Repair at 0x00574000 uses `RepairBridge=`.",
                        "Rust touchpoint src/sim/bridge_state/repair.rs.",
                    ]
                ),
                encoding="utf-8",
            )
            system_doc.write_text("# Bridge System\nBridgeRepairHut repair overview.\n", encoding="utf-8")

            db_path = workspace / "research.db"
            documents = _documents(workspace, [repair_doc, system_doc])
            rebuild_database(db_path, workspace, documents)

            graph = document_graph(db_path, "docs/research/bridges/00-system-models/BRIDGE_SYSTEM.md")
            self.assertTrue(graph["found"])
            self.assertEqual(graph["incoming"][0]["path"], "docs/research/bridges/repair/BRIDGE_REPAIR_GHIDRA_REPORT.md")
            self.assertEqual(graph["incoming"][0]["source_start_line"], 1)
            self.assertEqual(graph["incoming"][0]["source_end_line"], 4)
            self.assertIn(":1-4", format_document_graph(graph))

            evidence = evidence_view(db_path, "BridgeRepairHutClass__Repair")
            self.assertEqual(evidence["documents"][0]["line_ranges"], ["1-4"])
            self.assertIn("lines: 1-4", format_graph_view(evidence))

            implementation = implementation_view(db_path, "BridgeRepairHutClass__Repair")
            self.assertEqual(implementation["rust_paths"][0]["rust_path"], "src/sim/bridge_state/repair.rs")
            self.assertEqual(implementation["rust_paths"][0]["doc_count"], 1)
            self.assertEqual(
                implementation["rust_paths"][0]["citations"],
                ["docs/research/bridges/repair/BRIDGE_REPAIR_GHIDRA_REPORT.md:1-4"],
            )
            self.assertIn(
                "citations: docs/research/bridges/repair/BRIDGE_REPAIR_GHIDRA_REPORT.md:1-4",
                format_graph_view(implementation),
            )

    def test_graph_view_falls_back_to_full_text_for_phrase_queries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            report = workspace / "docs/research/bridges/03-traversal/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md"
            report.parent.mkdir(parents=True)
            report.write_text(
                "\n".join(
                    [
                        "# Unit Can Enter Cell",
                        "For normal directions, if a tube exists on the candidate cell,",
                        "compute abs(direction - tube.Direction_0x2C). Results 3, 4, or 5 return 7.",
                        "Rust touchpoint src/sim/pathfinding/core.rs.",
                    ]
                ),
                encoding="utf-8",
            )

            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [report]))

            query = "normal direction tube exclusion abs direction tube Direction 3 5"
            evidence = evidence_view(db_path, query, limit=4)
            text = format_graph_view(evidence)

            self.assertFalse(evidence["documents"])
            self.assertEqual(
                evidence["fallback_documents"][0]["path"],
                "docs/research/bridges/03-traversal/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md",
            )
            self.assertIn("Full-text fallback:", text)
            self.assertIn(":1-4", text)

            implementation = implementation_view(db_path, query, limit=4)
            self.assertEqual(implementation["rust_paths"][0]["rust_path"], "src/sim/pathfinding/core.rs")

    def test_parity_handoff_collects_handoff_sections_evidence_and_touchpoints(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            report = workspace / "docs/research/miner/CMIN_EXIT_GHIDRA_REPORT.md"
            report.parent.mkdir(parents=True)
            report.write_text(
                "\n".join(
                    [
                        "# CMIN Exit",
                        "UnitClass::Mission_Harvest at 0x0073E5E0 reaches the chrono miner exit path.",
                        "Rust surface src/sim/miner/miner_dock_sequence.rs.",
                        "## Implementation Handoff",
                        "| Verified behavior | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario |",
                        "|---|---|---|---|---|",
                        "| Exit uses Force_Track 0x47 | mismatch | src/sim/miner/miner_dock_sequence.rs | issue exact track command | CMIN exits refinery visibly |",
                    ]
                ),
                encoding="utf-8",
            )

            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [report]))

            result = parity_handoff(db_path, "UnitClass::Mission_Harvest CMIN exit", limit=4)
            text = format_parity_handoff(result)

            self.assertFalse(result["warnings"])
            self.assertEqual(result["handoff_candidates"][0]["heading_path"], "CMIN Exit > Implementation Handoff")
            self.assertEqual(result["rust_touchpoints"][0]["rust_path"], "src/sim/miner/miner_dock_sequence.rs")
            self.assertIn("Implementation handoff candidates:", text)
            self.assertIn("Rust touchpoints:", text)

    def test_parity_handoff_clusters_authoritative_gate_docs_and_flags_stale_wording(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            writer = workspace / "docs/research/GATE_WRITER_STATE_MACHINE_GHIDRA_REPORT.md"
            contract = workspace / "docs/research/INFANTRY_GATE_CANGARRISON_RESULT_CONTRACT_GHIDRA_REPORT.md"
            trace = workspace / "docs/research/traces/GATE_BUNKER_BUILDING_BLOCKER_ENTRY_TRACE.md"
            stale = workspace / "docs/research/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md"
            trace.parent.mkdir(parents=True)
            writer.parent.mkdir(parents=True, exist_ok=True)

            writer.write_text(
                "\n".join(
                    [
                        "# Gate Writer State Machine",
                        "BuildingClass::CanGarrison at 0x004525F0 reads the `Gate=` open state.",
                        "The live writer sets mission 0x18 and helper bytes for open gate passability.",
                        "Rust touchpoint src/sim/pathfinding/cell_entry.rs.",
                    ]
                ),
                encoding="utf-8",
            )
            contract.write_text(
                "\n".join(
                    [
                        "# Infantry Gate CanGarrison Result Contract",
                        "BuildingClass::CanGarrison at 0x004525F0 is gate passability, not boarding.",
                        "`Gate=` and CanGarrison decide open gate passability result codes.",
                        "## Implementation Handoff",
                        "| Verified behavior | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario |",
                        "|---|---|---|---|---|",
                        "| Gate=yes open helper permits continuation | missing | src/sim/pathfinding/cell_entry.rs | add gate passability predicate | open gate passability matches |",
                        "Replacement wording corrects older UNIT_CAN_ENTER_CELL wording.",
                    ]
                ),
                encoding="utf-8",
            )
            trace.write_text(
                "\n".join(
                    [
                        "# Gate Bunker Building Blocker Entry Trace",
                        "Trace verifies Gate=yes CanGarrison open gate passability.",
                        "Rust touchpoint src/sim/movement/movement_occupancy.rs.",
                    ]
                ),
                encoding="utf-8",
            )
            stale.write_text(
                "\n".join(
                    [
                        "# Unit Can Enter Cell",
                        "CanBeGarrisoned check calls BuildingClass::CanGarrison.",
                        "Field table labels `IsGate=` around the gate branch.",
                        "Rust touchpoint src/sim/pathfinding/cell_entry.rs.",
                    ]
                ),
                encoding="utf-8",
            )

            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [writer, contract, trace, stale]))

            result = parity_handoff(db_path, "Gate=yes CanGarrison open gate passability", limit=6)
            text = format_parity_handoff(result)
            clusters = result["authority_clusters"]
            trusted_or_supporting = {row["path"] for row in [*clusters["trust_first"], *clusters["supporting"]]}
            risky = {row["path"] for row in clusters["risky"]}

            self.assertIn("docs/research/GATE_WRITER_STATE_MACHINE_GHIDRA_REPORT.md", trusted_or_supporting)
            self.assertIn("docs/research/INFANTRY_GATE_CANGARRISON_RESULT_CONTRACT_GHIDRA_REPORT.md", trusted_or_supporting)
            self.assertIn("docs/research/traces/GATE_BUNKER_BUILDING_BLOCKER_ENTRY_TRACE.md", trusted_or_supporting)
            self.assertIn("docs/research/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md", risky)
            self.assertTrue(any("legacy gate wording" in row["risk_flags"] for row in clusters["risky"]))
            self.assertIn("Trust these first:", text)
            self.assertIn("Supporting docs:", text)
            self.assertIn("Risky / superseded docs:", text)
            self.assertIn("Confidence notes:", text)

    def test_handoff_candidates_reject_unrelated_generic_handoff_sections(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            target = workspace / "docs/research/bridges/BRIDGE_C4_GHIDRA_REPORT.md"
            unrelated = workspace / "docs/research/render/SPARK_LIGHT_GHIDRA_REPORT.md"
            target.parent.mkdir(parents=True)
            unrelated.parent.mkdir(parents=True)
            target.write_text(
                "# Bridge C4\n## Implementation Handoff\nBridgeRepairHut C4 collapse affects src/sim/world/bridge_orchestrator.rs.\n",
                encoding="utf-8",
            )
            unrelated.write_text(
                "# Spark Light\n## Implementation Handoff\nParticle spark light affects src/render/particle.rs.\n",
                encoding="utf-8",
            )

            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [target, unrelated]))

            rows = implementation_handoff_candidates(db_path, "BridgeRepairHut C4 collapse", limit=4)

            self.assertEqual([row["path"] for row in rows], ["docs/research/bridges/BRIDGE_C4_GHIDRA_REPORT.md"])

    def test_parity_handoff_system_filter_limits_evidence_and_handoffs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            bridge = workspace / "docs/research/bridges/BRIDGE_COLLAPSE_GHIDRA_REPORT.md"
            render = workspace / "docs/research/render/BRIDGE_COLLAPSE_RENDER_GHIDRA_REPORT.md"
            bridge.parent.mkdir(parents=True)
            render.parent.mkdir(parents=True)
            bridge.write_text(
                "# Bridge Collapse\n## Implementation Handoff\nBridge collapse updates src/sim/world/bridge_orchestrator.rs.\n",
                encoding="utf-8",
            )
            render.write_text(
                "# Bridge Collapse Render\n## Implementation Handoff\nBridge collapse render updates src/render/bridges.rs.\n",
                encoding="utf-8",
            )

            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [bridge, render]))

            result = parity_handoff(db_path, "bridge collapse", limit=4, system="bridges")

            self.assertEqual({row["path"] for row in result["evidence"]}, {"docs/research/bridges/BRIDGE_COLLAPSE_GHIDRA_REPORT.md"})
            self.assertEqual({row["path"] for row in result["handoff_candidates"]}, {"docs/research/bridges/BRIDGE_COLLAPSE_GHIDRA_REPORT.md"})
            self.assertEqual(result["rust_touchpoints"][0]["rust_path"], "src/sim/world/bridge_orchestrator.rs")

    def test_system_map_groups_docs_and_surfaces_handoff_and_uncertainty_signals(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            report = workspace / "docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_COLLAPSE_GHIDRA_REPORT.md"
            trace = workspace / "docs/research/bridges/08-traces/BRIDGE_COLLAPSE_TRACE.md"
            report.parent.mkdir(parents=True)
            trace.parent.mkdir(parents=True)
            report.write_text(
                "\n".join(
                    [
                        "# Bridge Collapse",
                        "Verified collapse behavior.",
                        "## Implementation Handoff",
                        "Current Rust delta affects src/sim/world/bridge_orchestrator.rs.",
                        "## Remaining Uncertainty",
                        "Exact route choice after collapse remains unknown.",
                    ]
                ),
                encoding="utf-8",
            )
            trace.write_text("# Bridge Collapse Trace\nCollapse trace confirms fallout ordering.\n", encoding="utf-8")

            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [report, trace]))

            result = system_map(db_path, system="bridges", topic="collapse", limit=10)
            text = format_system_map(result)

            self.assertEqual(result["document_count"], 2)
            self.assertTrue(any(group["subsystem"] == "05-damage-collapse-repair-cabhut" for group in result["groups"]))
            self.assertEqual(result["handoff_sections"][0]["path"], "docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_COLLAPSE_GHIDRA_REPORT.md")
            self.assertTrue(any(row["heading_path"] == "Bridge Collapse > Remaining Uncertainty" for row in result["signals"]))
            self.assertIn("Implementation handoff sections:", text)
            self.assertIn("Contradiction / uncertainty signals:", text)

    def test_validate_index_reports_changed_docs_and_missing_links(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            doc = workspace / "docs/research/bridges/BRIDGE_VALIDATION_GHIDRA_REPORT.md"
            doc.parent.mkdir(parents=True)
            doc.write_text("# Bridge Validation\nSee [missing](MISSING.md).\n", encoding="utf-8")

            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [doc]))
            doc.write_text("# Bridge Validation\nChanged after indexing.\nSee [missing](MISSING.md).\n", encoding="utf-8")

            result = validate_index(db_path, workspace, system="bridges", topic="validation", limit=10)
            text = format_validation(result)

            self.assertFalse(result["valid"])
            self.assertEqual(result["counts"]["checksum_mismatches"], 1)
            self.assertEqual(result["counts"]["missing_links"], 1)
            self.assertIn("Checksum mismatches:", text)
            self.assertIn("Missing links:", text)

    def test_research_brief_combines_validation_map_handoff_and_anchors(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            doc = workspace / "docs/research/miner/MISSION_HARVEST_GHIDRA_REPORT.md"
            doc.parent.mkdir(parents=True)
            doc.write_text(
                "\n".join(
                    [
                        "# Mission Harvest",
                        "UnitClass::Mission_Harvest at 0x0073E5E0 is verified.",
                        "Rust touchpoint src/sim/miner/miner_system.rs.",
                        "## Implementation Handoff",
                        "Current Rust delta affects src/sim/miner/miner_system.rs.",
                    ]
                ),
                encoding="utf-8",
            )

            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [doc]))

            result = research_brief(db_path, workspace, "Mission_Harvest", system="miner", anchors=["0x0073e5e0"], limit=4)
            text = format_research_brief(result)

            self.assertTrue(result["validation"]["valid"])
            self.assertEqual(result["map"]["document_count"], 1)
            self.assertEqual(result["handoff"]["rust_touchpoints"][0]["rust_path"], "src/sim/miner/miner_system.rs")
            self.assertEqual(result["anchors"][0]["anchor"], "0x0073e5e0")
            self.assertIn("Pre-implementation brief:", text)
            self.assertIn("Exact anchors:", text)


class ReliabilityContractTests(unittest.TestCase):
    def test_blank_search_is_not_a_database_wide_match(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            doc = workspace / "docs/research/miner/MINER_GHIDRA_REPORT.md"
            doc.parent.mkdir(parents=True)
            doc.write_text("# Miner\nMission_Harvest evidence.\n", encoding="utf-8")
            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [doc]))

            self.assertEqual(search(db_path, "", limit=5), [])
            self.assertEqual(search(db_path, "   ", limit=5), [])

    def test_lowercase_miss_cannot_inherit_generic_handoff_sections(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            generic = workspace / "docs/research/GENERIC_GHIDRA_REPORT.md"
            generic.parent.mkdir(parents=True)
            generic.write_text(
                "\n".join(
                    [
                        "# Generic",
                        "## Implementation Handoff",
                        "Current Rust delta affects src/sim/unrelated.rs.",
                    ]
                ),
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [generic]))

            result = parity_handoff(
                db_path,
                "utterlyabsent lowercaseconcept",
                limit=4,
                workspace=workspace,
            )

            self.assertFalse(result["matched"])
            self.assertEqual(result["evidence"], [])
            self.assertEqual(result["handoff_candidates"], [])
            self.assertEqual(result["rust_touchpoints"], [])
            self.assertEqual(result["authority_clusters"]["trust_first"], [])
            self.assertTrue(
                any("No relevant research scope" in warning for warning in result["warnings"])
            )

    def test_handoff_keeps_relevant_multiterm_doc_and_rejects_generic_neighbor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            target = workspace / "docs/research/bridges/BRIDGE_COLLAPSE_GHIDRA_REPORT.md"
            generic = workspace / "docs/research/render/GENERIC_GHIDRA_REPORT.md"
            target.parent.mkdir(parents=True)
            generic.parent.mkdir(parents=True)
            target.write_text(
                "\n".join(
                    [
                        "# Bridge Collapse",
                        "Bridge collapse evidence.",
                        "## Implementation Handoff",
                        "Bridge collapse affects src/sim/world/bridge_orchestrator.rs.",
                    ]
                ),
                encoding="utf-8",
            )
            generic.write_text(
                "\n".join(
                    [
                        "# Generic Render",
                        "## Implementation Handoff",
                        "Current Rust delta affects src/render/unrelated.rs.",
                    ]
                ),
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            rebuild_database(
                db_path,
                workspace,
                _documents(workspace, [target, generic]),
            )

            result = parity_handoff(
                db_path,
                "bridge collapse",
                limit=4,
                workspace=workspace,
            )

            self.assertTrue(result["matched"])
            paths = {row["path"] for row in result["handoff_candidates"]}
            self.assertEqual(
                paths,
                {"docs/research/bridges/BRIDGE_COLLAPSE_GHIDRA_REPORT.md"},
            )
            self.assertEqual(
                [row["rust_path"] for row in result["rust_touchpoints"]],
                ["src/sim/world/bridge_orchestrator.rs"],
            )

    def test_touchpoint_counts_unique_documents_and_reports_path_existence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            report = workspace / "docs/research/miner/MISSION_HARVEST_GHIDRA_REPORT.md"
            existing = workspace / "src/sim/miner/miner_system.rs"
            report.parent.mkdir(parents=True)
            existing.parent.mkdir(parents=True)
            existing.write_text("// fixture\n", encoding="utf-8")
            report.write_text(
                "\n".join(
                    [
                        "# Mission Harvest",
                        "UnitClass::Mission_Harvest at 0x0073E5E0.",
                        "Rust paths src/sim/miner/miner_system.rs and src/sim/miner/removed.rs.",
                        "## Implementation Handoff",
                        "Mission_Harvest 0x0073E5E0 uses both cited Rust paths.",
                    ]
                ),
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [report]))

            result = parity_handoff(
                db_path,
                "Mission_Harvest 0x73E5E0",
                limit=8,
                workspace=workspace,
            )
            touchpoints = {
                row["rust_path"]: row
                for row in result["rust_touchpoints"]
            }

            self.assertTrue(result["matched"])
            self.assertGreaterEqual(result["evidence"][0]["query_hit_count"], 2)
            self.assertEqual(touchpoints["src/sim/miner/miner_system.rs"]["doc_count"], 1)
            self.assertEqual(
                touchpoints["src/sim/miner/miner_system.rs"]["documents"],
                ["docs/research/miner/MISSION_HARVEST_GHIDRA_REPORT.md"],
            )
            self.assertTrue(touchpoints["src/sim/miner/miner_system.rs"]["exists"])
            self.assertFalse(touchpoints["src/sim/miner/removed.rs"]["exists"])
            self.assertTrue(
                any("may be stale citations or planned paths" in warning for warning in result["warnings"])
            )

            anchor_only = parity_handoff(
                db_path,
                "0x73E5E0",
                limit=8,
                workspace=workspace,
            )
            self.assertTrue(anchor_only["matched"])
            self.assertTrue(anchor_only["authority_clusters"]["trust_first"])
            self.assertIn(
                "address:0x0073e5e0",
                anchor_only["authority_clusters"]["matched_anchors"],
            )

    def test_empty_scope_is_not_a_valid_validation_or_successful_map(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            doc = workspace / "docs/research/bridges/BRIDGE_GHIDRA_REPORT.md"
            doc.parent.mkdir(parents=True)
            doc.write_text("# Bridge\nBridge evidence.\n", encoding="utf-8")
            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [doc]))

            validation = validate_index(db_path, workspace, topic="absenttopic")
            research_map = system_map(db_path, topic="absenttopic")

            self.assertFalse(validation["scope_matched"])
            self.assertFalse(validation["valid"])
            self.assertFalse(research_map["matched"])
            self.assertIn("scope matched: False", format_validation(validation))
            self.assertIn("No documents matched", format_system_map(research_map))

    def test_handoff_text_is_bounded_and_names_omissions(self) -> None:
        long_snippet = "detailed evidence " * 100
        evidence_row = {
            "path": "docs/research/miner/REPORT.md",
            "title": "Report",
            "source_kind": "ghidra",
            "status": "verified",
            "heading_path": "Report > Implementation Handoff",
            "start_line": 1,
            "end_line": 20,
            "score": 3.4,
            "snippet": long_snippet,
        }
        authority_row = {
            "path": "docs/research/miner/REPORT.md",
            "source_kind": "ghidra",
            "status": "verified",
            "score": 12.0,
            "anchors": ["symbol:Mission_Harvest"],
            "notes": ["exact anchor"],
            "citations": ["docs/research/miner/REPORT.md:1-20"],
        }
        rust_row = {
            "rust_path": "src/sim/miner/miner_system.rs",
            "doc_count": 1,
            "terms": ["Mission_Harvest"],
            "documents": ["docs/research/miner/REPORT.md"],
            "citations": ["docs/research/miner/REPORT.md:1-20"],
            "exists": True,
        }
        result = {
            "query": "Mission_Harvest",
            "system": "miner",
            "source_kind": None,
            "matched": True,
            "warnings": [],
            "authority_clusters": {
                "trust_first": [dict(authority_row) for _ in range(8)],
                "supporting": [dict(authority_row) for _ in range(8)],
                "risky": [dict(authority_row) for _ in range(8)],
                "confidence_notes": ["fixture"],
            },
            "handoff_candidates": [dict(evidence_row) for _ in range(8)],
            "rust_touchpoints": [dict(rust_row) for _ in range(8)],
            "evidence": [dict(evidence_row) for _ in range(8)],
            "implementation_terms": [
                {"term": f"term_{index}", "documents": [{}], "rust_paths": [{}]}
                for index in range(8)
            ],
        }

        text = format_parity_handoff(result)

        self.assertLess(len(text), 10_000)
        self.assertIn("more omitted from text", text)
        self.assertIn("docs/research/miner/REPORT.md:1-20", text)
        self.assertIn("exists=yes", text)

    def test_zero_match_cli_contracts_return_exit_one(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            doc = workspace / "docs/research/bridges/BRIDGE_GHIDRA_REPORT.md"
            doc.parent.mkdir(parents=True)
            doc.write_text("# Bridge\nBridge evidence.\n", encoding="utf-8")
            db_path = workspace / "research.db"
            rebuild_database(db_path, workspace, _documents(workspace, [doc]))

            commands = [
                [
                    sys.executable,
                    str(TOOL_ROOT / "validate.py"),
                    "--db",
                    str(db_path),
                    "--workspace",
                    str(workspace),
                    "absenttopic",
                ],
                [
                    sys.executable,
                    str(TOOL_ROOT / "map.py"),
                    "--db",
                    str(db_path),
                    "absenttopic",
                ],
                [
                    sys.executable,
                    str(TOOL_ROOT / "handoff.py"),
                    "--db",
                    str(db_path),
                    "--workspace",
                    str(workspace),
                    "absenttopic",
                ],
            ]

            for command in commands:
                completed = subprocess.run(
                    command,
                    cwd=workspace,
                    capture_output=True,
                    text=True,
                    encoding="utf-8",
                    check=False,
                )
                self.assertEqual(completed.returncode, 1, completed.stdout)


class LifecycleTests(unittest.TestCase):
    def test_default_corpus_excludes_plans_and_cli_resets_an_opt_in_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            files = {
                "docs/research/CURRENT.md": "# Current\nRetainedEvidenceMarker\n",
                "docs/plans/OLD.md": "# Plan\nHistoricalPlanMarker\n",
                "ini/rulesmd.ini": "[General]\nRetailRulesMarker=yes\n",
            }
            for relative, content in files.items():
                path = workspace / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8")
            db_path = workspace / "research.db"

            refresh_index(db_path, workspace)
            self.assertTrue(search(db_path, "RetainedEvidenceMarker"))
            self.assertTrue(search(db_path, "RetailRulesMarker"))
            self.assertEqual(search(db_path, "HistoricalPlanMarker"), [])

            refresh_index(db_path, workspace, roots=["docs/plans"])
            self.assertTrue(search(db_path, "HistoricalPlanMarker"))
            self.assertEqual(ensure_fresh(db_path, workspace)["roots"], ["docs/plans"])

            reset = subprocess.run(
                [
                    sys.executable,
                    str(TOOL_ROOT / "index.py"),
                    "--workspace", str(workspace),
                    "--db", str(db_path),
                ],
                capture_output=True,
                text=True,
                encoding="utf-8",
                check=False,
            )
            self.assertEqual(reset.returncode, 0, reset.stderr)
            self.assertEqual(search(db_path, "HistoricalPlanMarker"), [])
            self.assertTrue(search(db_path, "RetainedEvidenceMarker"))
            self.assertTrue(search(db_path, "RetailRulesMarker"))
            self.assertEqual(inspect_index(db_path, workspace)["roots"], ["docs/research", "ini"])

    def test_refresh_detects_and_repairs_changed_added_and_removed_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research"
            doc = root / "SYSTEM_GHIDRA_REPORT.md"
            doc.parent.mkdir(parents=True)
            doc.write_text("# System\nInitial evidence.\n", encoding="utf-8")
            db_path = workspace / "research.db"

            refreshed = refresh_index(
                db_path,
                workspace,
                roots=["docs/research"],
            )
            self.assertTrue(refreshed["fresh"])
            self.assertTrue(refreshed["refreshed"])
            self.assertTrue(manifest_path(db_path).is_file())

            doc.write_text(
                "# System\nInitial evidence with a material change.\n",
                encoding="utf-8",
            )
            changed = inspect_index(db_path, workspace)
            self.assertFalse(changed["fresh"])
            self.assertEqual(changed["changes"]["counts"]["changed"], 1)

            repaired = ensure_fresh(db_path, workspace)
            self.assertTrue(repaired["fresh"])
            self.assertTrue(repaired["refreshed"])

            added_doc = root / "ADDED_TRACE.md"
            added_doc.write_text("# Added\nTrace evidence.\n", encoding="utf-8")
            added = inspect_index(db_path, workspace)
            self.assertEqual(added["changes"]["counts"]["added"], 1)

            added_doc.unlink()
            doc.unlink()
            removed = inspect_index(db_path, workspace)
            self.assertEqual(removed["changes"]["counts"]["removed"], 1)

    def test_custom_root_persists_and_ignores_out_of_scope_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            custom = workspace / "notes"
            custom.mkdir()
            (custom / "CUSTOM.md").write_text(
                "# Custom\nScoped evidence.\n",
                encoding="utf-8",
            )
            db_path = workspace / "research.db"

            refresh_index(db_path, workspace, roots=["notes"])
            unrelated = workspace / "docs/research/UNRELATED.md"
            unrelated.parent.mkdir(parents=True)
            unrelated.write_text("# Unrelated\nNot in scope.\n", encoding="utf-8")

            health = inspect_index(db_path, workspace)
            self.assertTrue(health["fresh"])
            self.assertEqual(health["roots"], ["notes"])
            self.assertEqual(health["current_file_count"], 1)

    def test_fresh_ensure_does_not_take_publication_lock(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research"
            root.mkdir(parents=True)
            (root / "VALID.md").write_text(
                "# Valid\nEvidence.\n",
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            refresh_index(db_path, workspace, roots=["docs/research"])

            with mock.patch(
                "research_index.lifecycle.index_lock",
                side_effect=AssertionError(
                    "fresh reads must not acquire the publication lock"
                ),
            ):
                health = ensure_fresh(db_path, workspace)

            self.assertTrue(health["fresh"])
            self.assertFalse(health["refreshed"])

    def test_unsafe_and_empty_roots_cannot_replace_a_valid_generation(self) -> None:
        with (
            tempfile.TemporaryDirectory() as tmp,
            tempfile.TemporaryDirectory() as outside_tmp,
        ):
            workspace = Path(tmp)
            root = workspace / "docs/research"
            root.mkdir(parents=True)
            (root / "VALID.md").write_text(
                "# Valid\nEvidence.\n",
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            refresh_index(db_path, workspace, roots=["docs/research"])
            original_db = db_path.read_bytes()
            original_manifest = manifest_path(db_path).read_bytes()

            outside = Path(outside_tmp) / "OUTSIDE.md"
            outside.write_text("# Outside\nEvidence.\n", encoding="utf-8")
            with self.assertRaises(IndexLifecycleError):
                refresh_index(db_path, workspace, roots=[outside])

            empty = workspace / "empty"
            empty.mkdir()
            with self.assertRaises(IndexLifecycleError):
                refresh_index(db_path, workspace, roots=[empty])

            self.assertEqual(db_path.read_bytes(), original_db)
            self.assertEqual(
                manifest_path(db_path).read_bytes(),
                original_manifest,
            )

    def test_failed_build_keeps_last_good_database_and_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research"
            root.mkdir(parents=True)
            doc = root / "VALID.md"
            doc.write_text("# Valid\nEvidence.\n", encoding="utf-8")
            db_path = workspace / "research.db"
            refresh_index(db_path, workspace, roots=["docs/research"])
            original_db = db_path.read_bytes()
            original_manifest = manifest_path(db_path).read_bytes()
            doc.write_text("# Valid\nChanged evidence.\n", encoding="utf-8")

            with (
                mock.patch(
                    "research_index.database.insert_document",
                    side_effect=sqlite3.OperationalError("fixture failure"),
                ),
                self.assertRaises(IndexLifecycleError),
            ):
                refresh_index(db_path, workspace)

            self.assertEqual(db_path.read_bytes(), original_db)
            self.assertEqual(
                manifest_path(db_path).read_bytes(),
                original_manifest,
            )

    def test_database_identity_change_invalidates_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research"
            root.mkdir(parents=True)
            (root / "VALID.md").write_text(
                "# Valid\nEvidence.\n",
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            refresh_index(db_path, workspace, roots=["docs/research"])

            stat = db_path.stat()
            os.utime(
                db_path,
                ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000),
            )
            health = inspect_index(db_path, workspace)

            self.assertFalse(health["fresh"])
            self.assertIn("database identity differs", health["reasons"])

    def test_builder_signature_change_invalidates_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research"
            root.mkdir(parents=True)
            (root / "VALID.md").write_text(
                "# Valid\nEvidence.\n",
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            refresh_index(db_path, workspace, roots=["docs/research"])
            metadata_path = manifest_path(db_path)
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            metadata["builder_signature"] = "obsolete-builder"
            metadata_path.write_text(
                json.dumps(metadata),
                encoding="utf-8",
            )

            health = inspect_index(db_path, workspace)

            self.assertFalse(health["fresh"])
            self.assertIn(
                "index builder signature differs",
                health["reasons"],
            )

    def test_validation_checks_links_against_live_filesystem(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research/links"
            root.mkdir(parents=True)
            source = root / "SOURCE_GHIDRA_REPORT.md"
            target = root / "TARGET.md"
            source.write_text(
                "# Source\nSee [target](TARGET.md).\n",
                encoding="utf-8",
            )
            target.write_text("# Target\nEvidence.\n", encoding="utf-8")
            db_path = workspace / "research.db"
            refresh_index(db_path, workspace, roots=["docs/research"])

            target.unlink()
            missing = validate_index(
                db_path,
                workspace,
                topic="Source",
            )
            self.assertEqual(missing["counts"]["missing_links"], 1)

            target.write_text("# Target\nRestored.\n", encoding="utf-8")
            restored = validate_index(
                db_path,
                workspace,
                topic="Source",
            )
            self.assertEqual(restored["counts"]["missing_links"], 0)

    def test_unscoped_validation_reports_unindexed_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research"
            root.mkdir(parents=True)
            (root / "INDEXED.md").write_text(
                "# Indexed\nEvidence.\n",
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            refresh_index(db_path, workspace, roots=["docs/research"])
            (root / "NEW.md").write_text(
                "# New\nNot indexed yet.\n",
                encoding="utf-8",
            )

            result = validate_index(db_path, workspace)
            text = format_validation(result)

            self.assertFalse(result["valid"])
            self.assertEqual(result["counts"]["unindexed_files"], 1)
            self.assertIn("Unindexed files:", text)

    def test_health_cli_is_read_only_unless_refresh_is_requested(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research"
            root.mkdir(parents=True)
            doc = root / "INDEXED.md"
            doc.write_text("# Indexed\nEvidence.\n", encoding="utf-8")
            db_path = workspace / "research.db"
            refresh_index(db_path, workspace, roots=["docs/research"])
            doc.write_text("# Indexed\nChanged evidence.\n", encoding="utf-8")

            base_command = [
                sys.executable,
                str(TOOL_ROOT / "health.py"),
                "--db",
                str(db_path),
                "--workspace",
                str(workspace),
                "--json",
            ]
            stale = subprocess.run(
                base_command,
                capture_output=True,
                text=True,
                encoding="utf-8",
                check=False,
            )
            self.assertEqual(stale.returncode, 1)
            self.assertFalse(json.loads(stale.stdout)["fresh"])

            repaired = subprocess.run(
                [*base_command, "--refresh"],
                capture_output=True,
                text=True,
                encoding="utf-8",
                check=False,
            )
            self.assertEqual(repaired.returncode, 0, repaired.stderr)
            self.assertTrue(json.loads(repaired.stdout)["fresh"])

    def test_concurrent_health_refreshes_publish_one_valid_generation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            root = workspace / "docs/research"
            root.mkdir(parents=True)
            (root / "INDEXED.md").write_text(
                "# Indexed\nEvidence.\n",
                encoding="utf-8",
            )
            db_path = workspace / "research.db"
            command = [
                sys.executable,
                str(TOOL_ROOT / "health.py"),
                "--db",
                str(db_path),
                "--workspace",
                str(workspace),
                "--root",
                "docs/research",
                "--refresh",
                "--json",
            ]

            processes = [
                subprocess.Popen(
                    command,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    encoding="utf-8",
                )
                for _ in range(2)
            ]
            completed = [
                (process.communicate(timeout=30), process.returncode)
                for process in processes
            ]

            payloads = []
            for ((stdout, stderr), returncode) in completed:
                self.assertEqual(returncode, 0, stderr)
                payloads.append(json.loads(stdout))
            self.assertTrue(all(payload["fresh"] for payload in payloads))
            self.assertEqual(
                {payload["generation"] for payload in payloads},
                {inspect_index(db_path, workspace)["generation"]},
            )

    def test_health_formatter_names_state_and_changes(self) -> None:
        result = {
            "fresh": False,
            "ready": True,
            "workspace": "workspace",
            "db_path": "research.db",
            "document_count": 1,
            "chunk_count": 2,
            "current_file_count": 2,
            "format_version": 1,
            "tool_version": "2.0.0",
            "generation": "abc",
            "roots": ["docs/research"],
            "changes": {
                "added": ["docs/research/NEW.md"],
                "changed": [],
                "removed": [],
                "counts": {"added": 1, "changed": 0, "removed": 0},
            },
            "reasons": ["1 unindexed file(s) added"],
        }
        text = format_index_health(result)
        self.assertIn("Research index: stale (ready)", text)
        self.assertIn("Added files:", text)


def _documents(workspace: Path, paths: list[Path]) -> list[tuple[str, object, object]]:
    return [
        (path.relative_to(workspace).as_posix(), document_metadata(path, workspace), chunk_file(path))
        for path in sorted(paths)
    ]


def _row(sql: str) -> sqlite3.Row:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    try:
        return conn.execute(sql).fetchone()
    finally:
        conn.close()


class MCPWrapperContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        try:
            import mcp_server  # noqa: E402
        except ImportError as exc:
            raise unittest.SkipTest(f"mcp_server not importable: {exc}") from exc
        cls.mcp_server = mcp_server

    def test_zero_scope_and_blank_query_are_explicit_through_mcp_wrappers(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp)
            doc = workspace / "docs/research/bridges/BRIDGE_GHIDRA_REPORT.md"
            doc.parent.mkdir(parents=True)
            doc.write_text("# Bridge\nBridge evidence.\n", encoding="utf-8")
            db_path = workspace / "research.db"
            refresh_index(
                db_path,
                workspace,
                roots=["docs/research"],
            )

            original_db = self.mcp_server.DEFAULT_DB
            original_workspace = self.mcp_server.WORKSPACE
            self.mcp_server.DEFAULT_DB = db_path
            self.mcp_server.WORKSPACE = workspace
            try:
                validation = json.loads(
                    self.mcp_server.research_validate(
                        topic="absenttopic",
                        format="json",
                    )
                )
                handoff = json.loads(
                    self.mcp_server.research_handoff(
                        query="absenttopic",
                        format="json",
                    )
                )
                blank_search = self.mcp_server.research_search(query="")
                health = json.loads(
                    self.mcp_server.research_health(format="json")
                )
            finally:
                self.mcp_server.DEFAULT_DB = original_db
                self.mcp_server.WORKSPACE = original_workspace

            self.assertFalse(validation["scope_matched"])
            self.assertFalse(validation["valid"])
            self.assertFalse(handoff["matched"])
            self.assertEqual(handoff["evidence"], [])
            self.assertTrue(blank_search.startswith("No results for"))
            self.assertTrue(health["fresh"])
            self.assertEqual(health["roots"], ["docs/research"])


class MCPServerSmokeTests(unittest.TestCase):
    """Smoke tests for the FastMCP tool wrappers in mcp_server.

    Use the live .cache/research.db rather than an inline tempdir build,
    because the design's testing strategy specifies it (production-shaped
    coverage) and several tools (research_brief, research_validate)
    exercise the workspace argument against the real repo. Skipped when
    the live DB is missing.
    """

    mcp_server = None  # populated by setUpClass when imports succeed

    @classmethod
    def setUpClass(cls) -> None:
        from research_index.database import DEFAULT_DB

        if not DEFAULT_DB.exists():
            raise unittest.SkipTest(
                f"Live research index not built at {DEFAULT_DB}; "
                f"run `python tools/research_index/index.py` first."
            )

        try:
            import mcp_server  # noqa: E402  (TOOL_ROOT is already on sys.path)
        except ImportError as exc:
            raise unittest.SkipTest(f"mcp_server not importable: {exc}") from exc

        cls.mcp_server = mcp_server

    def test_research_search_returns_text(self) -> None:
        out = self.mcp_server.research_search(query="CellClass::BlowUpBridge", system="bridges", limit=3)
        self.assertIn("docs/research/bridges", out)

    def test_research_search_json_round_trips(self) -> None:
        out = self.mcp_server.research_search(query="CellClass::BlowUpBridge", system="bridges", limit=3, format="json")
        rows = json.loads(out)
        self.assertIsInstance(rows, list)

    def test_research_search_empty_result_returns_hint(self) -> None:
        # Runtime-generated query guarantees no index entry. A hard-coded
        # miss string could appear in a retained report or an opt-in plan,
        # turning this assertion into a self-reference hit (Phase 1
        # finding).
        miss_query = secrets.token_hex(16)
        out = self.mcp_server.research_search(query=miss_query)
        self.assertTrue(out.startswith("No results for"))

    def test_research_related_returns_text(self) -> None:
        out = self.mcp_server.research_related(target="CellClass::BlowUpBridge", by="term", limit=3)
        self.assertNotEqual(out.strip(), "")

    def test_research_related_empty_result_returns_hint(self) -> None:
        miss_term = secrets.token_hex(16)
        out = self.mcp_server.research_related(target=miss_term, by="term")
        self.assertTrue(out.startswith("No related docs for"))

    def test_research_graph_doc_mode_returns_text(self) -> None:
        out = self.mcp_server.research_graph(
            mode="evidence",
            target="CellClass::BlowUpBridge",
            limit=3,
        )
        self.assertNotEqual(out.strip(), "")

    def test_research_graph_json_round_trips(self) -> None:
        out = self.mcp_server.research_graph(
            mode="evidence",
            target="CellClass::BlowUpBridge",
            limit=3,
            format="json",
        )
        result = json.loads(out)
        self.assertIsInstance(result, dict)

    def test_research_map_returns_text(self) -> None:
        out = self.mcp_server.research_map(system="bridges", limit=5)
        self.assertNotEqual(out.strip(), "")

    def test_research_handoff_returns_text(self) -> None:
        out = self.mcp_server.research_handoff(query="CellClass::BlowUpBridge", limit=3)
        self.assertNotEqual(out.strip(), "")

    def test_research_validate_returns_text(self) -> None:
        out = self.mcp_server.research_validate(system="bridges", limit=5)
        self.assertNotEqual(out.strip(), "")

    def test_research_brief_returns_text(self) -> None:
        out = self.mcp_server.research_brief(query="CellClass::BlowUpBridge", limit=3)
        self.assertNotEqual(out.strip(), "")

    def test_research_brief_with_anchors_normalizes_none(self) -> None:
        # anchors=None should normalize to [] without exception.
        out = self.mcp_server.research_brief(query="CellClass::BlowUpBridge", anchors=None, limit=3)
        self.assertNotEqual(out.strip(), "")

    def test_research_health_reports_live_generation(self) -> None:
        out = self.mcp_server.research_health(format="json")
        result = json.loads(out)
        self.assertTrue(result["fresh"])
        self.assertGreater(result["document_count"], 0)


if __name__ == "__main__":
    unittest.main()
