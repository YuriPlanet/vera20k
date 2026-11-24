"""Select retained native observations only; no simulation answers."""
import collections

def compact_state(state):
    keys = ('pointer', 'flags', 'health', 'alive', 'limbo', 'location', 'mission', 'queued', 'status', 'mission_timer', 'idle_latch_6b3', 'legacy_index', 'archive', 'nav', 'target', 'slave_owner_2dc', 'team_5d4', 'infantry_doing', 'movement_timer', 'retries', 'blockage_timer', 'speed_fraction_bytes', 'tether', 'contacts', 'type_flags', 'planning_object_514', 'planning_object', 'planning_registry', 'warped_out_271', 'attack_move_saved_mission_5c4', 'attack_move_engaged_5d1', 'walk', 'producer_archive', 'producer_scatter_gate_state', 'rng')
    return {key: state[key] for key in keys if key in state}

def projection(receipt, prior):
    events = receipt['events']

    def event(row):
        result = {key: row[key] for key in ('sequence', 'return_sequence', 'kind', 'pc', 'caller', 'frame', 'phase', 'this', 'args', 'result', 'result_al', 'source_coordinate', 'fnpc_result_cell') if key in row}
        for key in ('before', 'after', 'state'):
            if key in row:
                result[key] = compact_state(row[key])
        return result
    selected = [event(row) for row in events if row['kind'] in ('InfantryCtor', 'FootCtor', 'PlanningCreate', 'PlanningSetter', 'FootIdle', 'InfantryIdle', 'TechnoIdle', 'TechnoIdleEffect') or (row['kind'] == 'InfantryScatter' and row['before']['nav'] == 0) or (row['kind'] == 'NativeBoundary' and row['pc'] in ('0x004DA87A', '0x0051BCA4', '0x00444CA3', '0x00444D11', '0x0051D422', '0x006385DC', '0x004D8523', '0x004D8530', '0x0051BD16', '0x0051D45B') and (row['pc'] not in ('0x0051BD16', '0x0051D45B') or row['frame'] in (269, 486)))]
    joined = prior['joined'] if receipt['control'] == 'rally' else prior
    queue = joined['admitted_queue']['queue']
    return dict(control=receipt['control'], primary_comparison=receipt['primary_comparison'], live_virtual_identities=receipt['live_virtual_identities'], events=selected, latch_writer_counts=dict(collections.Counter((row['pc'] for row in receipt['latch_writes']))), all_three_complete_rng_states=receipt['complete_rng_states'], repeated_real_calls=receipt['repeated_real_calls'], primary_boundary=[compact_state(row) for row in receipt['primary_boundary']], original_request_order=[{key: row[key] for key in ('entry', 'site', 'frame', 'this', 'args', 'result')} for row in queue['requests']], original_advances=queue['advances'], final_native_queue=queue['final'], native_code_unchanged=receipt['native_code_unchanged'])
