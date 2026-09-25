# Research evidence and historical archive

This directory retains evidence cited directly by source, tools, tests and project
guidance and the active movement task, plus numeric reference data and the Ghidra
working notes. A retained report is a lead to verify, not a certification that its
claims or Rust references are current. Keep new evidence beside its implementation or native comparison
where practical; avoid another general research backlog.

On 2026-09-16, the first archival pass removed 2,403 research files after
preservation and checksum verification in a separate versioned archive. A second
pass archived the retired system map and checker (11 files), another 50 research
reports retained only for that map, and 20 completed or superseded planning and
audit records. Seven reports still cited by active acceptance documents or phase
briefs stayed in place. The research collection now has 180 retained files plus
this guide. Historical cross-references use immutable links instead of keeping
the entire old library in ordinary searches.

Current task plans and phase briefs remain in `docs/plans/` for continuation and
are opt-in search material. Archiving a completion record does not close its
remaining gaps; the [phase briefs](../plans/phase-briefs/README.md) retain the
relevant handoff and residual pointers.

## Read or recover historical evidence

The complete pre-cleanup research and plan snapshot is preserved at source commit
`108924bc237d14342b68b8bb78f2bba0400d2443`:

- [Research library](https://github.com/YuriPlanet/vera20k/tree/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research)
- [Plan snapshot](https://github.com/YuriPlanet/vera20k/tree/108924bc237d14342b68b8bb78f2bba0400d2443/docs/plans)

The second pass is preserved at source commit
`1dbaf80c89c9348df493ab618dbefed8663aef96`:

- [Retired system map](https://github.com/YuriPlanet/vera20k/tree/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/system-map) and [its optional historical checker](https://github.com/YuriPlanet/vera20k/tree/1dbaf80c89c9348df493ab618dbefed8663aef96/tools/system_map)
- [Research](https://github.com/YuriPlanet/vera20k/tree/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research), [plans](https://github.com/YuriPlanet/vera20k/tree/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/plans), [gap scans](https://github.com/YuriPlanet/vera20k/tree/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/gap-scans) and [contracts](https://github.com/YuriPlanet/vera20k/tree/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/contracts)

These are historical records, including useful evidence, old assumptions and
superseded status claims. Archiving does not establish that a report is wrong or
that a documented gap has been fixed. Recheck native bodies, callers, data and
current production code before using a claim.

Read a specific old path without restoring it to the active tree:

```powershell
git show 108924bc237d14342b68b8bb78f2bba0400d2443:docs/research/AAHEATSEEKER2_HOMINGTRACK_EXACT_MATH_GHIDRA_REPORT.md
```

To recover either snapshot outside this checkout:

```powershell
git -c core.autocrlf=false archive --format=zip --output ../vera20k-research-archive.zip 108924bc237d14342b68b8bb78f2bba0400d2443 docs/research docs/plans
git -c core.autocrlf=false archive --format=zip --output ../vera20k-retired-docs-archive.zip 1dbaf80c89c9348df493ab618dbefed8663aef96 docs/research docs/plans docs/system-map docs/gap-scans docs/contracts tools/system_map
```

Extract the ZIP into a separate directory, retaining its original paths.
Both revisions are in this repository's history; a
shallow clone may need to fetch that revision first. There is no need to copy
the historical corpus back into the active checkout to investigate one finding.

Older plans and reports cite commands and results from the retired research
index tool. Its last version is preserved at source commit
`89d68023b6cea0a6cde9ee53dab52fa080f5529a`:
[tools/research_index](https://github.com/YuriPlanet/vera20k/tree/89d68023b6cea0a6cde9ee53dab52fa080f5529a/tools/research_index).
A checkout that built its index keeps an untracked `tools/research_index/.cache/`
after updating; delete it.
