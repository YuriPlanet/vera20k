# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "mcp>=1.2.0,<2",
# ]
# ///
"""
research_index MCP Server — exposes the tools/research_index CLIs as MCP tools.

Library entry points are imported directly from research_index/. The CLI
scripts in this directory continue to work for shell invocations and CI.
"""

from __future__ import annotations

import json
import logging
import sys
import threading
from pathlib import Path
from typing import Literal

# Make the research_index package importable when launched by `python mcp_server.py`.
_SERVER_DIR = Path(__file__).resolve().parent
# Repo root is two parents up from this file's directory.
WORKSPACE = _SERVER_DIR.parents[1]
sys.path.insert(0, str(_SERVER_DIR))
sys.path.insert(0, str(WORKSPACE))

# Reconfigure stdout to UTF-8 (matches the CLI shims; research docs contain non-ASCII).
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

from mcp.server.fastmcp import FastMCP
from research_index.database import (
    DEFAULT_DB,
    related_by_document,
    related_by_term,
    search,
)
from research_index.brief import research_brief as _research_brief_lib
from research_index.formatting import (
    format_backlinks,
    format_document_graph,
    format_graph_view,
    format_index_health,
    format_parity_handoff,
    format_related_results,
    format_research_brief,
    format_search_results,
    format_system_map,
    format_validation,
)
from research_index.graph import (
    backlinks,
    document_graph,
    evidence_view,
    implementation_view,
)
from research_index.handoff import parity_handoff
from research_index.indexing import DEFAULT_ROOTS
from research_index.lifecycle import (
    ensure_fresh,
    inspect_index,
    refresh_index,
)
from research_index.system_map import system_map
from research_index.validation import validate_index

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger("research-index-mcp")

mcp = FastMCP("research-index")
_LIFECYCLE_LOCK = threading.RLock()


def _ensure_fresh_index() -> dict:
    """Certify one current generation before serving indexed evidence."""
    with _LIFECYCLE_LOCK:
        return ensure_fresh(DEFAULT_DB, WORKSPACE)


@mcp.tool()
def research_search(
    query: str,
    limit: int = 20,
    system: str | None = None,
    source: str | None = None,
    format: Literal["text", "json"] = "text",
) -> str:
    """Search retained research and retail INI data via SQLite FTS5.

    Default roots are docs/research/ and ini/. Plans require an explicit
    research_reindex root override. Historical research lives outside this
    workspace and is searched separately; see tools/research_index/README.md.
    Inferred status labels are retrieval hints, not proof of native parity.

    Use when looking up where a concept (function name, INI key, address,
    mechanism) is documented. For exploring docgraph adjacency or related
    docs, use research_related or research_graph instead (Phase 2).

    Args:
        query: Natural-language phrase, exact symbol/address, or path fragment.
        limit: Max results (default 20, matches search.py CLI).
        system: Filter by inferred system (e.g. "bridges", "miner", "chrono").
        source: Filter by source kind (e.g. "ghidra", "trace", "synthesis").
        format: "text" for formatted output, "json" for structured.
    """
    _ensure_fresh_index()
    rows = search(DEFAULT_DB, query, limit=limit, system=system, source_kind=source)

    if not rows:
        filter_parts = []
        if system:
            filter_parts.append(f"system={system!r}")
        if source:
            filter_parts.append(f"source={source!r}")
        filter_suffix = f" (filters: {', '.join(filter_parts)})" if filter_parts else ""
        return (
            f"No results for {query!r}{filter_suffix}. "
            f"Try a broader query or drop the filters."
        )

    if format == "json":
        return json.dumps(rows, indent=2)
    return format_search_results(rows)


@mcp.tool()
def research_related(
    target: str,
    by: Literal["doc", "term"] = "doc",
    limit: int = 20,
    format: Literal["text", "json"] = "text",
) -> str:
    """Docs sharing extracted evidence (symbols, addresses, INI keys, Rust paths) with a source.

    Use when you have one doc and want others that cite the same symbols,
    or when you want every doc that mentions an exact term. For free-text
    search use research_search; for graph adjacency by edge kind use
    research_graph.

    Args:
        target: Document path (when by="doc") or exact extracted term
            (when by="term"). Document paths must be repo-relative
            (e.g. "docs/research/bridges/BRIDGE_C4_GHIDRA_REPORT.md").
        by: "doc" to find docs related to a document; "term" to find docs
            referencing an exact extracted term.
        limit: Max results (default 20, matches related.py CLI).
        format: "text" for formatted output, "json" for structured.
    """
    _ensure_fresh_index()
    if by == "term":
        rows = related_by_term(DEFAULT_DB, target, limit)
    else:
        rows = related_by_document(DEFAULT_DB, target, limit)

    if not rows:
        return (
            f"No related docs for {target!r} (by={by!r}). "
            f"Confirm the path or term spelling; try research_search for "
            f"free-text lookup or research_graph for adjacency."
        )

    if format == "json":
        return json.dumps(rows, indent=2)
    return format_related_results(rows)


@mcp.tool()
def research_graph(
    mode: Literal["doc", "backlinks", "evidence", "implementation"],
    target: str,
    limit: int = 12,
    format: Literal["text", "json"] = "text",
) -> str:
    """Navigate the research docgraph.

    Four modes that resolve different edge kinds:
    - "doc": outgoing edges from a document (what it cites).
    - "backlinks": incoming edges to a document (who cites it).
    - "evidence": resolve an exact term to docs that cite it as evidence.
    - "implementation": resolve an exact term to Rust paths cited near it.

    Args:
        mode: One of "doc", "backlinks", "evidence", "implementation".
        target: Repo-relative document path for "doc"/"backlinks"; exact
            term for "evidence"/"implementation".
        limit: Max rows per section (default 12, matches graph.py CLI;
            note: library default for backlinks is 20, MCP overrides to 12
            for parity with the CLI).
        format: "text" for formatted output, "json" for structured.
    """
    _ensure_fresh_index()
    if mode == "doc":
        result = document_graph(DEFAULT_DB, target, limit)
        text = format_document_graph(result)
    elif mode == "backlinks":
        result = backlinks(DEFAULT_DB, target, limit)
        text = format_backlinks(result)
    elif mode == "evidence":
        result = evidence_view(DEFAULT_DB, target, limit)
        text = format_graph_view(result)
    else:  # implementation
        result = implementation_view(
            DEFAULT_DB,
            target,
            limit,
            workspace=WORKSPACE,
        )
        text = format_graph_view(result)

    if format == "json":
        return json.dumps(result, indent=2)
    return text


@mcp.tool()
def research_map(
    topic: str | None = None,
    system: str | None = None,
    source: str | None = None,
    status: str | None = None,
    limit: int = 80,
    format: Literal["text", "json"] = "text",
) -> str:
    """Inventory of docs for a system or topic, grouped by subsystem/source/status.

    Use to survey what is already researched before starting an investigation;
    returns groups, document counts, handoff sections, and uncertainty
    signals. For free-text lookup use research_search; for the
    implementation-handoff bundle use research_handoff.

    Args:
        topic: Optional topic phrase matched in paths, titles, headings, or
            chunks. Omit to list every doc in the (optionally filtered)
            scope.
        system: Filter by inferred system (e.g. "bridges", "miner", "chrono").
        source: Filter by source kind (e.g. "ghidra", "trace", "synthesis").
        status: Filter by status (e.g. "verified", "stale", "unknown").
        limit: Max rows per section (default 80, matches map.py CLI).
        format: "text" for formatted output, "json" for structured.
    """
    _ensure_fresh_index()
    result = system_map(
        DEFAULT_DB,
        system=system,
        topic=topic,
        source_kind=source,
        status=status,
        limit=limit,
    )
    if format == "json":
        return json.dumps(result, indent=2)
    return format_system_map(result)


@mcp.tool()
def research_handoff(
    query: str,
    system: str | None = None,
    source: str | None = None,
    limit: int = 8,
    format: Literal["text", "json"] = "text",
) -> str:
    """Implementation-handoff bundle for a mechanism or symbol.

    Returns the handoff sections, top evidence rows, and Rust touchpoints
    for a query in one call. Use when planning an implementation and you
    want the "what does gamemd do, what Rust does it map to" view. For a
    bigger pre-implementation aggregate (validation + map + handoff +
    anchors) use research_brief.

    Args:
        query: Mechanism, symbol, doc topic, or implementation question.
        system: Filter evidence and handoff sections by inferred system.
        source: Filter evidence and handoff sections by source kind.
        limit: Max rows per section (default 8, matches handoff.py CLI).
        format: "text" for formatted output, "json" for structured.
    """
    _ensure_fresh_index()
    result = parity_handoff(
        DEFAULT_DB,
        query,
        limit=limit,
        system=system,
        source_kind=source,
        workspace=WORKSPACE,
    )
    if format == "json":
        return json.dumps(result, indent=2)
    return format_parity_handoff(result)


@mcp.tool()
def research_validate(
    topic: str | None = None,
    system: str | None = None,
    source: str | None = None,
    status: str | None = None,
    limit: int = 40,
    format: Literal["text", "json"] = "text",
) -> str:
    """Refresh the generation, then validate a scope and live local links.

    Returns missing files, checksum mismatches, missing markdown link
    targets, and stale/unknown-status docs. Use research_health without
    refresh when you need to inspect pending corpus changes before mutation.

    Args:
        topic: Optional topic phrase to validate within.
        system: Filter by inferred system.
        source: Filter by source kind.
        status: Filter by status.
        limit: Max issue rows per section (default 40, matches validate.py
            CLI).
        format: "text" for formatted output, "json" for structured.
    """
    _ensure_fresh_index()
    result = validate_index(
        DEFAULT_DB,
        WORKSPACE,
        system=system,
        topic=topic,
        source_kind=source,
        status=status,
        limit=limit,
    )
    if format == "json":
        return json.dumps(result, indent=2)
    return format_validation(result)


@mcp.tool()
def research_brief(
    query: str,
    anchors: list[str] | None = None,
    system: str | None = None,
    source: str | None = None,
    limit: int = 8,
    format: Literal["text", "json"] = "text",
) -> str:
    """Compact pre-implementation planning bundle. HEAVY — prefer cheaper tools first.

    Aggregates validation + system map + parity handoff + per-anchor
    evidence/implementation views in one call. Issues 5+ SQLite
    connections per invocation. Use only when assembling a full
    pre-implementation context block. For a quick lookup use
    research_search; for an implementation-only handoff use
    research_handoff; for a system inventory use research_map.

    Args:
        query: System topic, mechanism, function, or implementation
            question.
        anchors: Optional exact symbols or addresses to pin (each gets a
            per-anchor evidence + implementation view appended). Defaults
            to an empty list when omitted.
        system: Filter by inferred system.
        source: Filter by source kind.
        limit: Max rows per section (default 8, matches brief.py CLI).
        format: "text" for formatted output, "json" for structured.
    """
    _ensure_fresh_index()
    anchors_list = anchors if anchors else []
    result = _research_brief_lib(
        DEFAULT_DB,
        WORKSPACE,
        query,
        system=system,
        source_kind=source,
        anchors=anchors_list,
        limit=limit,
    )
    if format == "json":
        return json.dumps(result, indent=2)
    return format_research_brief(result)


@mcp.tool()
def research_reindex(
    roots: list[str] | None = None,
) -> str:
    """Force a safe FTS rebuild and publish its generation manifest.

    Walks the given roots (markdown + ini files), chunks them, and writes a
    fresh SQLite DB with a unique temporary file plus atomic replacement.
    A cross-process lock serializes publication. Unsafe, missing, or empty
    roots fail before replacing the current generation.

    Args:
        roots: Optional repo-relative paths to walk. Defaults to
            ("docs/research", "ini") when omitted. Add "docs/plans" only
            for an intentional plan search; overrides persist until reindexed.
            External archives require a separate CLI workspace and database.

    Returns:
        One-line summary: ``indexed documents=N chunks=M db=<path>``.
    """
    root_strs = roots if roots else list(DEFAULT_ROOTS)
    with _LIFECYCLE_LOCK:
        result = refresh_index(
            DEFAULT_DB,
            WORKSPACE,
            roots=root_strs,
        )
    return result["summary"]


@mcp.tool()
def research_health(
    refresh: bool = False,
    limit: int = 40,
    format: Literal["text", "json"] = "text",
) -> str:
    """Inspect index generation freshness, optionally rebuilding if stale.

    Unlike every evidence-reading tool, inspection is non-mutating by
    default. Use this to see pending corpus changes. Set ``refresh=True`` to
    synchronously publish and certify a current generation.

    Args:
        refresh: Rebuild when stale. Defaults to read-only inspection.
        limit: Max changed-file rows per category.
        format: "text" for compact health, "json" for structured details.
    """
    if refresh:
        with _LIFECYCLE_LOCK:
            result = ensure_fresh(
                DEFAULT_DB,
                WORKSPACE,
                limit=limit,
            )
    else:
        result = inspect_index(
            DEFAULT_DB,
            WORKSPACE,
            limit=limit,
        )
    if format == "json":
        return json.dumps(result, indent=2)
    return format_index_health(result)


if __name__ == "__main__":
    logger.info(f"research-index MCP server starting (workspace={WORKSPACE})")
    mcp.run()
