"""Original House constructor sight flag and first local Update sight visitor.

Run python -m tools.spatial_oracle.house_initial_sight --check / --write.
The shared source fixture supplies a discovered Sight=0 aircraft's earlier
camera cache. Map and Techno methods execute; area/border calls are recorded.
"""
from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.shroud_current_sight import alliance_human_control


def cases():
    controls = ({}, {"pending": False}, {"current": False},
                {"discovered": False}, {"limbo": True},
                {"in_playfield": False}, {"passive": True},
                {"sight": 6, "latched": False},
                {"sight": 6, "latched": False, "startup": True},
                {"sight": 6, "latched": False, "startup": True, "building": True})
    return {"cases": [alliance_human_control(initial_update=True, **control)
                      for control in controls]}


if __name__ == "__main__":
    finish_vectors(cases, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="House constructor +24B store and first-local-Update sight visitor; two visits per control",
        assumptions=[
            "Original 4F57A7 constructor store executes alone; other House fields and preceding Update state are supplied.",
            "Original 4F9338..4F935F executes before its epilogue, including complete Map4ADCD0 and source70AF50/70B1D0 callees.",
            "Shared shroud_current_sight fixture supplies one Sight0 aircraft, prior camera radius6/position40,40, current position41,40 and original House source gates.",
            "One additional Sight6/unlatched control supplies LeptonsPerSightIncrease2000, zero elevation/veterancy and executes the real sight-radius calculation.",
            "Current-player and pending=false controls override the explicit supplied prior; repeat visits retain the first visit's state.",
            "Two startup controls first execute Map4ADCD0(1,0) with supplied Aircraft/Building RTTI; that earlier pass skips the Building and retains House24B.",
            "Initial controls supply Aircraft RTTI2 (41C180) or Building RTTI6; complete constructors do not run. Original executable sections and Main/Scenario RNG buffers are checked unchanged.",
            "No complete native scenario startup, actual cell discovery, renderer output or whole-match parity is claimed."
        ],
        substitutions=[
            "Source virtual RTTI/type lookup returns supplied fixture identities.",
            "Map5678E0 and border567DA0 record arguments and return without traversing cells.",
            "Inherited Emu allocation/CRT hooks are installed; this bounded path does not enter them."
        ],
        entry_points={"constructor_store": 0x4F57A7, "update_tail": 0x4F9338,
                      "map_visitor": 0x4ADCD0, "source_update": 0x70AF50}))
