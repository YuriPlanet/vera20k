# Research evidence and historical archive

This directory retains evidence cited directly by source, tools, tests and project
guidance and the active movement task, plus numeric reference data and the Ghidra
working notes. A retained report is a lead to verify, not a certification that its
claims or Rust references
are current. Keep new evidence beside its implementation or native comparison
where practical; avoid another general research backlog.

On 2026-09-16, 2,403 other research files were removed from the active tree after
preservation and checksum verification in a separate versioned archive. The 230
retained files preserve direct dependencies; historical cross-references use
immutable links instead of keeping the entire old library in ordinary searches.
Task plans remain in `docs/plans/` for continuation and are opt-in search material.

## Read or recover historical evidence

The complete pre-cleanup research and plan snapshot is preserved at source commit
`108924bc237d14342b68b8bb78f2bba0400d2443`:

- [Research library](https://github.com/YuriPlanet/vera20k/tree/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research)
- [Plan snapshot](https://github.com/YuriPlanet/vera20k/tree/108924bc237d14342b68b8bb78f2bba0400d2443/docs/plans)

These are historical records, including useful evidence, old assumptions and
superseded status claims. Archiving does not establish that a report is wrong or
that a documented gap has been fixed. Recheck native bodies, callers, data and
current production code before using a claim.

Read a specific old path without restoring it to the active tree:

```powershell
git show 108924bc237d14342b68b8bb78f2bba0400d2443:docs/research/AAHEATSEEKER2_HOMINGTRACK_EXACT_MATH_GHIDRA_REPORT.md
```

To recover the complete snapshot outside this checkout:

```powershell
git -c core.autocrlf=false archive --format=zip --output ../vera20k-research-archive.zip 108924bc237d14342b68b8bb78f2bba0400d2443 docs/research docs/plans
```

Extract the ZIP into a separate directory, retaining the `docs/research/` and
`docs/plans/` paths. The original revision is in this repository's history; a
shallow clone may need to fetch that revision first. There is no need to copy
the historical corpus back into the active checkout to investigate one finding.

## Search scope

The [research index](../../tools/research_index/README.md) searches retained
research and local retail INIs by default. Its guide documents explicit plan
search and a separate workspace/database for archive searches. An existing index
must be rebuilt with the new default roots after updating this checkout:

```powershell
python tools/research_index/index.py
```

Do not treat an inferred `verified` filename status as native proof or parity.
Use current source, saved native evidence and actual validation results.
