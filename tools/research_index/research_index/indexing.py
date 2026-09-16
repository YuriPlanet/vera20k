"""Build/rebuild the research-index SQLite FTS database.

Owns the orchestration loop that pairs file discovery + chunking + metadata
extraction with the SQL primitive in database.rebuild_database. Consumed by
the index.py CLI and the research_reindex MCP tool.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from .chunking import chunk_file
from .database import rebuild_database
from .metadata import document_metadata, iter_indexable_files


DEFAULT_ROOTS: tuple[str, ...] = ("docs/research", "ini")


@dataclass(frozen=True)
class RebuildResult:
    document_count: int
    chunk_count: int
    db_path: Path

    def summary(self) -> str:
        return (
            f"indexed documents={self.document_count} "
            f"chunks={self.chunk_count} db={self.db_path}"
        )


def rebuild_index_result(
    workspace: Path,
    roots: list[Path],
    db_path: Path,
) -> RebuildResult:
    """Rebuild the index and return structured publication counts."""
    documents = []
    chunk_total = 0

    for path in iter_indexable_files(roots):
        relpath = path.relative_to(workspace).as_posix()
        meta = document_metadata(path, workspace)
        chunks = list(chunk_file(path))
        if not chunks:
            continue
        documents.append((relpath, meta, chunks))
        chunk_total += len(chunks)

    if not documents:
        raise ValueError("refusing to replace the research index with zero documents")

    rebuild_database(db_path, workspace, documents)
    return RebuildResult(len(documents), chunk_total, db_path)


def rebuild_index(workspace: Path, roots: list[Path], db_path: Path) -> str:
    """Rebuild the FTS database from disk.

    Args:
        workspace: Repo root. Relpaths are computed against this; missing
            link validation reads files relative to it.
        roots: Absolute or workspace-relative paths to walk for indexable
            files (markdown, ini). Caller is responsible for joining with
            workspace if needed.
        db_path: Output SQLite database path. Parent directory will be
            created if missing. Rebuild uses a unique sibling temporary
            database followed by ``os.replace``.

    Returns:
        One-line summary string matching the legacy index.py output:
        ``indexed documents=N chunks=M db=<path>``
    """
    return rebuild_index_result(workspace, roots, db_path).summary()
