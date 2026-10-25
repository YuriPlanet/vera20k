"""Pure Rust projection of the joined original Shrapnel occupant execution.

House active-unit totals are native+5574 reads, not EntityStore storage counts.
Ordered memberships replace aggregate actor-only Logic/Display counts; original
global/per-type counts and absolute native IDs remain in the full native packet.
"""
from pathlib import Path
import json

from tools.native_oracle import _canonical, finish_vectors, provenance
from tools.spatial_oracle.shrapnel_repair import packet_io
from .occupants_test_vectors import project_cases

HERE = Path(__file__).resolve().parent


def project_joined_cases(source, additional_event_kinds=()):
    """Shared pure projection for damage and hut callers of Joined."""
    result = project_cases(source, ('anim_constructor', 'anim_constructor_return',
                                  'native_anim_callee', 'named_sound_request',
                                  *additional_event_kinds))
    for case in result['cases']:
        cursor = case['stages'][0]['before']['native_id_cursor']
        case['native_id_cursor_before'] = cursor
        for stage in case['stages']:
            for boundary in ('before', 'after'):
                state = stage[boundary]
                for key in ('owner_type_count', 'logic_count', 'display_count'):
                    state.pop(key)
                stage[f'memberships_{boundary}'] = state.pop('memberships')
                stage[f'anims_{boundary}'] = state.pop('anims')
                stage[f'native_id_cursor_offset_{boundary}'] = state.pop('native_id_cursor') - cursor
                for anim in stage[f'anims_{boundary}']:
                    anim['native_id_offset'] = anim.pop('native_id') - cursor
    return result


def generate():
    source = packet_io.read_result(HERE / 'occupants_joined.json.gz')
    promotion = json.loads((HERE / 'occupants_joined_promotion.json').read_bytes())
    identity = packet_io.digest(_canonical(source))
    assert identity == promotion['results']['occupants_joined.json']['published_payload_sha256']
    return dict(schema=1,
                joined_native_payload_sha256=identity, scene=source['scene'],
                native_actor_inputs=source['cases'][0]['native_inputs'], **project_joined_cases(source))


def metadata():
    return provenance(scope=__doc__, assumptions=[
        'Shared project_cases extracts every receiver/timer/RNG result without calculating a gameplay outcome. Complete words/indices for Main/Scenario/MapGen come directly from the native packet.',
        'House+5574 is compared to HouseTracking active-unit total after subtracting unrelated production-scene units. Raw native per-type inventory+5564 remains full-packet evidence; it is not compared to EntityStore insert/remove storage counts.',
        'Native ordered Logic, five Display layers and Anim registry use canonical actor/constructor identities. No membership is inferred from Alive/Limbo. Actor-only aggregate counts are omitted because joined constructors admit live Anims.',
        'Absolute native constructor IDs/cursor remain in the full packet. The Rust projection subtracts the explicitly supplied pre-controller cursor to compare same-call allocation order without claiming whole ScenarioLoad ID parity.',
        'Constructor requests/results and raw Bouncer fields are retained. No later AnimAI, Bouncer contact, audio playback, complete Logic tick or whole-world initialization parity is claimed.',
    ], substitutions=[], entry_points={'occupants':0x487A10,'anim_constructor':0x421EA0,
        'bounce_init':0x4397E0,'anim_start':0x424CE0}) | dict(
        source_files={name:packet_io.digest((HERE/name).read_bytes()) for name in (
            'occupants_joined.json.gz', 'occupants_joined_promotion.json')})


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'occupants_joined_test_vectors.json', provenance=metadata)
