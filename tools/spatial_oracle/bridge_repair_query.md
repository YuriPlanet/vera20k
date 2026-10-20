# Engineer hut geometry query

[`bridge_repair_query.py`](bridge_repair_query.py), [188 native rows](bridge_repair_query.json)
and [provenance](bridge_repair_query.meta.json) execute complete retail Map587410,
GetCell5657A0, FindBridgeRecord56DA10 and packed coordinate addition42D510.
The original direction initializer49F2F0 executes. Run:

```sh
python -m tools.spatial_oracle.bridge_repair_query --check
```

Every row verifies unchanged mapped `.text`, supplied resident cells and ordered
record bytes. No query, lookup, search, Boolean or arithmetic result is supplied.
Metadata pins harness/shared sources and canonical payload identity.

The corpus covers both ordinary overlay families, all individual identities,
both walk axes/directions, terminal/intact states; every offset in both structural
tile windows; active/inactive and chained records, interior tolerance matches and
skipped non-high record kinds; same-cell tile priority and later-match precedence;
overlapping wood/concrete windows; retained fallback identity and fixed-stride
coordinate aliasing; and template widths1/2/3/4/255 with unsigned subtiles0/1/4/7/255.
The native lookup/search trace and final fallback coordinate are retained.

The result contract is AL. The scan retains the last match in Y-major order;
wood then concrete tile windows precede overlays on each cell. A structural match
uses registered template width and original82AA04/82AA24/82AA44 data before ordered
high-record searches at tolerance3. An inactive record or a matched record whose
endpoints differ from the retained endpoint admits repair. Ordinary overlays walk
both directions and admit terminals100/101 or231/232. Retained fallback coordinates
can change during subsequent lookups; a caller cannot replace them with scan XY.

The Rust comparison is
`sim::bridge_state::repair_query::tests::complete_native_query_corpus_matches_al_and_retained_dummy`.
It drives the production sparse-grid query, the existing registered TMP reader,
shared ordered-record search and packed coordinate owners; expected AL/Dummy
values come exclusively from this native corpus.

Supplied sparse allocation, resident cells, ordered records and template widths
are explicit inputs. This packet does not execute their native construction,
retail map loading, full Scenario initialization, UI or Engineer lifecycle.
The inherited native harness has callback sinks, but reaching any rejects a case.
There are no query RNG, timer or detach operations. Actual physical-map query
coverage remains in [Anytown hut cursor](anytown_damage/hut_cursor.md);
caller/action/display composition is covered separately by
[the Engineer caller packet](engineer_bridge_cursor_caller.md).

Records are oriented consistently with the original record builder's selected
walk axis. During fixture preparation a crossed-axis active pair caused the
original tolerance walk to cycle; that malformed supplied pair is excluded from
the valid record fixtures. No guard or substituted search result masks it.
Production record-builder geometry and whole-bridge reachability remain separate
required audit evidence.
