// The shared line builder, and how the simulation prints a line.

import { log } from "@vera20k/sdk";
import { Line } from "../../data/line";

export { Line } from "../../data/line";

/** `line` in the players' message list. */
export function say(line: Line): void {
  log(line.str);
}
