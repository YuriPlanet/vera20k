# Generated House names and authored Building admission

`campaign_start --scenario-names` extends the existing `CampaignReader`, its
INI/CRT transport and original PE runner. It preserves the default campaign
and `--houses` profiles. `--scenario-names` and `--houses` are mutually exclusive.

```sh
VERA20K_GAMEMD_EXE=/path/to/original/gamemd.exe \
VERA20K_CAMPAIGN_START_ASSETS=/existing/empty-directory \
python -m tools.input_oracle.campaign_start --scenario-names --check
```

`--write` deliberately regenerates `campaign_house_names.json` and its
`.meta.json`. The sidecar records the command, scope and
substitutions; it pins no repository-source hashes. Source freezing guards
generation only.
No physical asset is requested; the supplied directory is preferably empty.

Three original copy regions produce `<human player>` and two `Computer`
names: human `68804A..688092`, computers `688210..688252`. Prepared House
lookup storage and stored indexes0/1/2 are explicit priors. The human's
separate UTF16 display handle is supplied as `Computer`. These rows do not
execute whole House constructors or `Create_Houses`.

Eleven whole `House::FromName50C170` controls retain the native array ordinal
and resolved fixture identity/stored index. They cover duplicate names,
reversed AI array order, wrong case, leading/trailing spaces, internal
`Computer1`, country/player-slot-looking tokens, display handle and empty
array. Exact byte comparison and first match come from original execution.

Eleven `BuildingReadINI44F820` controls consume real ordered-cache
Structures records, `ReadString128`, native `strtok`/`atoi` and actual House/type
lookup. `Basic.NewINIFormat=4` passes through original `68A13D..68A15B` before
each row. Supplied type ID `NAHAND` goes through whole BuildingType
constructor45DD90 in a bounded one-entry registry; its index0 is not a full
retail ordinal. Physical type scalar/ART reading is excluded.

Mode5 current-House and unknown-name/type rows execute the reader's real
loop, epilogue and `RET44FE51`. Admitted rows stop **before CALL44FAFD** after
`PUSH0x720`; neither allocation nor a Building constructor return is supplied.
The corpus retains reached lookup/current-gate outputs and parsed boundary
values. Null means that an output was not reached. Cell20,20 is a parse input,
not proof of successful terrain placement. A duplicate whose first match is
current is rejected without retrying its later same-named House.

The producer compares all three complete0x3F4-byte RNG states and every
original executable byte after each selected execution, retaining compact
checks instead of raw repeated state/code payloads. Copy rows write only the
native name; lookup/reader rows write no House field. The reader reaches no
Building allocation/constructor or gameplay detach. The separate Type setup
owns its native registry/ID writes. Full constructor RNG/timers remain with
the existing `--houses` and constructor comparisons.

`houses`, `lookup_rows` and `building_reader_rows` supply native observations
for the production authored-owner regression. Full scenario registration,
placement/Unlimbo, ENGINEER capture, paid production and their RNG/timer/detach
effects require their existing production owners. Prefix zero-draw checks do
not certify those later mechanisms.
