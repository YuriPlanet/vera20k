// Kill stars, simulation side: every kill adds to the killer's `Kills`,
// and each two kills raise its `Level`, up to 10. Both are public object
// variables, so every player's `../ui` draws the same stars.
import { Event, KilledEvent, NO_HANDLE, Rules, Trigger, log, onScenarioStart } from "@vera20k/sdk";
import { Kills, Level } from "./generated/vars";

const MAX_LEVEL: i64 = 10;
const KILLS_PER_LEVEL: i64 = 2;

function onKill(e: KilledEvent): void {
  const killer = e.killer;
  if (killer == NO_HANDLE) return;
  const kills = Kills.get(killer) + 1;
  Kills.set(killer, kills);
  const level = kills / KILLS_PER_LEVEL;
  Level.set(killer, level < MAX_LEVEL ? level : MAX_LEVEL);
}

onScenarioStart((): void => {
  const trigger = Trigger.on(Event.Killed);
  const victims: string[] = ["E1", "E2", "GGI", "MTNK", "HTNK", "FV", "HTK", "DOG", "ADOG"];
  for (let i = 0; i < victims.length; i++) {
    const kind = Rules.technoType(victims[i]);
    if (kind != NO_HANDLE) trigger.bind(kind);
  }
  if (trigger.enqueue(onKill).done() == NO_HANDLE) log("kill-stars: the trigger was refused");
});
