# Infantry default-action motion query

The Infantry sequencer520AE0 default arm asks the active ILocomotion+10 at
520D38. It chooses forced Walk3/Crawl6 only when that query is true and the
Foot speed fraction exceeds0.1; otherwise it chooses Deployed28, Prone2 or
Ready0. The common movement::motion_query owner answers this slot for every
represented family. The Infantry consumer formerly copied only the Walk and
Jumpjet branches, so ordinary Teleport requests incorrectly selected idle
sequences. The fixed consumer calls the common owner.

## Reproduce original controls

Configure the existing native_oracle executable selector with RA2_DIR or
VERA20K_GAMEMD_EXE. Supply physical retail ini/rulesmd.ini and ini/artmd.ini
(the normal extract-ini outputs), or VERA20K_SHRAPNEL_INPUTS with the same
original files. No original INI bytes or executable are committed.

```sh
PYTHONPATH=. python -m tools.spatial_oracle.jumpjet_infantry_actions --default-motion --check
PYTHONPATH=. python -m tools.spatial_oracle.jumpjet_infantry_actions --check
PYTHONPATH=. python -m tools.spatial_oracle.cmin_dock --check
```

The separate infantry_default_motion.json/meta corpus preserves54 original
controls. Default mode retains its existing527 Jumpjet rows. --check writes
no references; --write deliberately regenerates original executable results.
Metadata pins Unicorn identity, supplied inputs and callback
boundaries.

## Executed evidence

Existing Actions/States creates the initial fixture, then original Teleport
constructor718000 and Link55A710 replace Jumpjet behind Infantry+674. Original
interface vtable7F5000 reads class+34/interface+30; original MoveTo718100 sets
that request and Stop718230 clears it. Eight clone Infantry slots are restored
from original7EB058. Original executable code remains unchanged.

Physical ClegSequence values execute the existing sequence constructor loop
and complete ReadSequenceData523D00/ReadString/sscanf. JumpJet=false,
MovementZone=Infantry7 and Crawls=true are supplied physical CLEG facts;
full type/ART loading is excluded from the native fixture. Rust regression
retail_teleport_default_action_matches_the_native_sequencer reads retail
RULESMD/ARTMD through RuleSet, ArtRegistry and the production sequence binder,
checks all42 frame/count/stride records and compares Doing, signed Stage,
timer start/duration/rate and unchanged complete Rust RNG states for54 rows.

48 rows cross constructor-only, MoveTo and MoveTo/Stop with standing/prone,
fractions0/.05/.1/.5, and Doing-1 or completed Cheer32. Four same-action rows
preserve the old clock; two unfinished Cheer rows skip the query and action.
At frame1000, armed .5 selects Walk3 with Stage0 and timer1000/3/rate3, or
Crawl6 with Stage0 and1000/1/rate1. Stopped or <=.1 selects Ready0/1000/0/0
or Prone2/1000/6/6. Original DoAction passes force1 and final argument0.
All three complete1012-byte RNG states remain unchanged through every
sequencer. The preceding original Infantry MoveTo subcell selection spends
one separate Scenario draw and chooses3264/2752/0 from3200/2688/0; that
resolution and RNG draw are recorded, not compared through the Rust adapter.
The armed destination is class+1C/interface+18 and Stop clears it; the
resolved-coordinate scratch is class+28/interface+24 and remains retained.

## Coverage and remaining prerequisites

This proves the ordinary active query/sequence boundary, not a complete
Teleport lifetime or Infantry AI. Existing RTTI/type/cell getters, CanEnter
result0, marked-layer/corridor/ground/raw/subcell backing and unrelated
Mark/visibility/OS sinks remain declared native fixture boundaries. Supplied
FPCW0E7F is not evidence for a live-retail process control word.

Teleport's existing is_moving owner remains a Relocate-phase adapter for
native request+30. Ordinary move/stop controls agree; Teleport Process,
Chronosphere producers and the complete request lifecycle still need their
own mechanism. Rocket's exact null-coordinate destination producer/lifecycle
is unrepresented; the common query returns None, retaining the consumer's
previous false fallback. No new request state, phase copy, Rocket destination
or wrapper owner is introduced. Existing Jumpjet SimFixed near0.8 truncation
limits and its497 exact/30 documented-difference comparison remain unchanged.
