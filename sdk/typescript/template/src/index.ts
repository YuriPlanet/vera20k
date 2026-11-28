import { log, onScenarioStart } from "@vera20k/sdk";

// Runs once when the match starts.
onScenarioStart((): void => {
  log("Hello from my first module!");
});
