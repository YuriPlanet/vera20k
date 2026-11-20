# Skirmish cheat codes

During an offline skirmish, press **Enter** or **Backspace** to open chat.
Type a message and press **Enter** to display it in your match. Long messages
appear a chunk at a time as you type. **Esc** cancels the text still on the line.
Local chat works even with cheats off.

To use commands, tick **Allow cheat codes** in **Skirmish → Game Options**
before the match. Open chat, type the full command (for example, `/help`),
and press **Enter**. With cheats on, **/** also opens chat with the slash
already entered.

| Command | Effect |
| --- | --- |
| `/money 10000` | Adds 10,000 credits to your current balance. |
| `/spysat` | Reveals the map for your house once. A later shroud reset can hide terrain again. |
| `/rhino 200` | Places up to 200 Rhino Tanks where you click. |
| `/spawn ZEP 100` | Places up to 100 Kirov Airships where you click. |
| `/help` | Lists the commands. |

`/spawn` accepts infantry, vehicle, ship and aircraft IDs from the active rules,
including units added by a mod. Both `/spawn` and `/rhino` accept 1 to 1,000
units per order.

To place a group, type the command and press **Enter**. You can move the camera
to find your spot, then **left-click the battlefield** to spawn the group.
Click water for ships. **Right-click** or press **Esc** to cancel before placing.
Opening chat again also cancels the pending group.
The game starts at the clicked cell, fills suitable cells around it and reports
how many units fit. The units belong to your house, whichever side you play.
`/money`, `/spysat` and `/help` take effect as soon as you press **Enter**.

Cheat codes default off. The checkbox saves to `[VERA] SkirmishCheats=` in
`VERA.INI` and applies only to offline skirmishes. Replays play back the cheats
you used.
