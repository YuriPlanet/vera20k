# Building GetFLH identity matrix

`python -m tools.spatial_oracle.building_flh_direct --check` executes the original
Building `453840` override, Techno `6F3AD0` GetFLH and its complete scalar matrix,
trigonometric-table and conversion callees. The existing `building_prism.Scene`
provides supplied Building/type storage with the original Building vtable;
Building/type construction and native INI reads are outside this fixture.
Unrelated Prism callbacks remain installed but are not reached. No coordinate
or math result is supplied.

The 16 selected coordinate controls use two authored input pairs: FLH
`50,0,300` / TurretOffset `0`, and asymmetric FLH
`-250,37,-23` / TurretOffset `95`. Six stock poses retain cardinal, diagonal
and nearby headings; five asymmetric/pivot poses retain both burst parities
in several quadrants. These are focused regressions, not a heading sweep.
The location is `3200,3200,0`, weapon index zero and target null. Original FacingClass methods install each heading.
Occupancy, primary pixel offset, barrel voxel and turret voxel arms are disabled.
Building's nonzero incoming base `7,11,13` is dropped by the actual override.

Every control must enter the original identity arm at `6F3C1A`; all actor/type
bytes and full Main, Scenario and mapgen RNG storage must remain unchanged.
The complete mapped text must equal the configured source PE before and after
execution. The fixture reuses its Building across these read-only queries.

Native starts with identity, translates by world-X TurretOffset, rotates once
by the absolute quantized firing direction, translates by FLH with the burst
mirror, then converts the result. The separate locomotor arm uses its own basis
and relative aim/body rotation. Composing that second rotation for a Building
can change a shot's origin by one lepton because the sine table is asymmetric.

The Rust consumer is `building_identity_matrix_matches_original_get_flh`, through
the actual Rules/ART reader and shared fire-coordinate owner. Existing Tesla
birth and ordinary delayed-fire tests compare the corrected stock source
exactly, and neighboring pixel/dual-offset/garrison/voxel controls keep their
existing native corpora. The native fixture does not run source AI, complete
FireAt, collision, damage, save/load or a whole match. It does not certify every
authored integer, interpolated FacingClass timing or null-locomotor Foot objects.
