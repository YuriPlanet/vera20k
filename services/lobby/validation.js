// The HTTPS boundary accepts only the canonical serde shapes emitted by
// skirmish_launch.rs and live_service/wire.rs. Gameplay/rules decisions remain
// with those owners; this file bounds data before it is persisted or fanned out.
const utf8 = new TextEncoder();

// JSON.parse permits escaped unpaired UTF-16 surrogates; Rust String does not.
// Check keys too, before any stringify/persistence. Workers implements this
// ECMAScript 2024 String method; Workers supports standard V8 built-ins:
// https://tc39.es/ecma262/2024/multipage/text-processing.html#sec-string.prototype.iswellformed
// https://developers.cloudflare.com/workers/runtime-apis/web-standards/
// The wire's 64KiB
// body limit also bounds visits; root counts as the first of 64 containers.
export function validJsonTree(root) {
  const pending = [[root, 0]];
  let visits = 65536;
  while (pending.length) {
    if (--visits < 0) return false;
    const [value, depth] = pending.pop();
    if (typeof value === 'string') {
      if (!value.isWellFormed()) return false;
    } else if (value !== null && typeof value === 'object') {
      if (depth >= 64) return false;
      for (const key of Object.keys(value)) {
        if (!key.isWellFormed()) return false;
        pending.push([value[key], depth + 1]);
      }
    }
  }
  return true;
}

export const byteLength = (value) => utf8.encode(typeof value === 'string' ? value : JSON.stringify(value)).length;
export const record = (value, required, optional = []) => value !== null && typeof value === 'object' && !Array.isArray(value)
  && required.every((key) => Object.hasOwn(value, key))
  && Object.keys(value).every((key) => required.includes(key) || optional.includes(key));
const integer = (value, min, max) => Number.isSafeInteger(value) && value >= min && value <= max;
const i32 = (value) => integer(value, -2147483648, 2147483647);
const bool = (value) => typeof value === 'boolean';
const text = (value, max, empty = false) => typeof value === 'string' && (empty || value.length > 0)
  && byteLength(value) <= max && !/[\u0000-\u001f\u007f-\u009f]/u.test(value);
export const playerName = (value) => typeof value === 'string' && /^[A-Za-z0-9_-]{1,24}$/.test(value);
export const digest = (value) => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
export const uuid = (value) => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
const nullable = (value, check) => value === null || check(value);
const distinct = (items) => new Set(items).size === items.length;

// Matches map::source::validate_network_map_name, independent of host OS.
const logicalPath = (value, empty = false) => text(value, 1024, empty) && (empty && value === ''
  || !value.includes(':') && value.split(/[\\/]/).every((part) => part !== '' && part !== '.' && part !== '..'));
const variant = (value, unit, named) => value === unit || record(value, [named]) && integer(value[named], 0, 255);
export const validSlot = (value) => record(value, ['country', 'country_random', 'color_index', 'color_random', 'start_position', 'team'])
  && integer(value.country, 0, 65535) && bool(value.country_random)
  && integer(value.color_index, 0, 255) && bool(value.color_random)
  && variant(value.start_position, 'Auto', 'Position') && variant(value.team, 'None', 'Team');

const validHuman = (value) => record(value, ['raw_slot_index', 'name', 'slot', 'role'])
  && nullable(value.raw_slot_index, (row) => integer(row, 0, 29)) && playerName(value.name) && validSlot(value.slot)
  && (value.role === 'Observer' || record(value.role, ['Player']) && record(value.role.Player, ['group'])
    && i32(value.role.Player.group) && value.role.Player.group !== -1);
const validAi = (value) => record(value, ['raw_slot_index', 'country', 'country_random', 'color_index', 'color_random', 'start_position', 'team', 'difficulty'])
  && integer(value.raw_slot_index, 0, 29) && ['Easy', 'Normal', 'Hard'].includes(value.difficulty)
  && validSlot(Object.fromEntries(Object.entries(value).filter(([key]) => key !== 'raw_slot_index' && key !== 'difficulty')));
const validMode = (value) => record(value, ['class', 'id', 'ui_name_key', 'tooltip_key', 'override_file', 'map_filter', 'random_maps_allowed', 'allies_allowed', 'must_ally'])
  && ['Battle', 'ManBattle', 'Siege', 'Unholy', 'FreeForAll', 'Cooperative'].includes(value.class) && i32(value.id)
  && text(value.ui_name_key, 256) && text(value.tooltip_key, 256, true) && logicalPath(value.override_file, true)
  && text(value.map_filter, 1024, true) && ['random_maps_allowed', 'allies_allowed', 'must_ally'].every((key) => bool(value[key]));
const launchIntegers = ['starting_credits', 'unit_count', 'tech_level', 'game_speed', 'default_ai_difficulty'];
const launchBooleans = ['short_game', 'bases', 'bridges_destroyable', 'super_weapons', 'build_off_ally', 'crates', 'mcv_redeploy',
  'fog_of_war', 'shroud', 'tiberium_grows', 'multi_engineer', 'harvester_truce', 'ally_change_allowed'];
const validPreFill = (value) => record(value, ['human_nodes', 'ai_slots', 'fixed_tail'])
  && Array.isArray(value.human_nodes) && value.human_nodes.length <= 30
  && value.human_nodes.every((row) => record(row, ['priority', 'source_order', 'observer']) && i32(row.priority)
    && integer(row.source_order, 0, 29) && bool(row.observer))
  && Array.isArray(value.ai_slots) && value.ai_slots.length <= 30
  && value.ai_slots.every((row) => record(row, ['slot_index', 'valid']) && integer(row.slot_index, 0, 29) && bool(row.valid))
  && distinct(value.ai_slots.map((row) => row.slot_index))
  && Array.isArray(value.fixed_tail) && value.fixed_tail.length === 2 && value.fixed_tail[0] === 'Neutral' && value.fixed_tail[1] === 'Special';

export function validOptions(value) {
  if (!record(value, ['launch', 'map_metadata', 'vacancies', 'observer_row', 'cooperative_difficulty', 'seed'], ['random_map'])
    || byteLength(value) > 24576) return false;
  const launch = value.launch, map = value.map_metadata;
  if (!record(launch, ['mode', 'selected_map_file', 'humans', 'local_player_index', 'ai_player_count', 'opponents', 'pre_fill_house_roster', 'options'])
    || !validMode(launch.mode) || !logicalPath(launch.selected_map_file)
    || !Array.isArray(launch.humans) || launch.humans.length < 1 || launch.humans.length > 30 || !launch.humans.every(validHuman)
    || !distinct(launch.humans.map((human) => human.name.toLowerCase()))
    || !distinct(launch.humans.map((human) => human.raw_slot_index).filter((row) => row !== null))
    || !integer(launch.local_player_index, 0, launch.humans.length - 1) || !integer(launch.ai_player_count, 0, 30)
    || !Array.isArray(launch.opponents) || launch.opponents.length > 30 || !launch.opponents.every(validAi)
    || !distinct(launch.opponents.map((ai) => ai.raw_slot_index)) || !validPreFill(launch.pre_fill_house_roster)
    || !record(launch.options, [...launchIntegers, ...launchBooleans])
    || !launchIntegers.every((key) => i32(launch.options[key])) || !launchBooleans.every((key) => bool(launch.options[key]))) return false;
  if (!record(map, ['display_name', 'player_capacity', 'official', 'byte_length', 'digest', 'source_sha256', 'effective_digest'])
    || !text(map.display_name, 256) || !integer(map.player_capacity, 1, 30) || !bool(map.official)
    || !integer(map.byte_length, 1, 8 * 1024 * 1024) || !text(map.digest, 128) || !digest(map.source_sha256) || !digest(map.effective_digest)
    || !Array.isArray(value.vacancies) || value.vacancies.length !== 30
    || !value.vacancies.every((row) => ['open', 'closed', 'observer', 'waiting'].includes(row))
    || !nullable(value.observer_row, (row) => integer(row, 0, 29)) || !i32(value.cooperative_difficulty)
    || !nullable(value.seed, (seed) => integer(seed, 0, 0xffffffff))) return false;
  if (value.random_map !== undefined && value.random_map !== null) {
    if (!record(value.random_map, ['start_staging', 'preview']) || !text(value.random_map.start_staging, 512)
      || !record(value.random_map.preview, ['source_sha256', 'byte_length']) || !digest(value.random_map.preview.source_sha256)
      || !integer(value.random_map.preview.byte_length, 1, 512 * 1024)) return false;
  }
  return launch.humans.filter((human) => human.role !== 'Observer').length + launch.opponents.length <= map.player_capacity;
}

export function validChannel(value) {
  return record(value, ['name', 'kind', 'max_users'], ['id', 'users', 'tournament', 'flags', 'game_flags', 'ping', 'ex_info'])
    && text(value.name, 80) && value.kind === 41 && integer(value.max_users, 2, 30)
    && (value.id === undefined || text(value.id, 64, true))
    && ['users', 'tournament', 'ping'].every((key) => value[key] === undefined || i32(value[key]))
    && ['flags', 'game_flags'].every((key) => value[key] === undefined || integer(value[key], 0, 0xffffffff))
    && (value.ex_info === undefined || text(value.ex_info, 512, true));
}
