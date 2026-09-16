# Clean-Slate System Implementation Order

This reference defines phase scope through 336 numbered GSI entries and the
additional mechanisms below. The order is suggested; establish actual dependencies
from current code and active-retail native evidence. Entries are scope hypotheses,
not proof of complete coverage or instructions to rebuild existing implementations.
Current progress and findings belong in [phase briefs](phase-briefs/README.md).
Phase 0 describes standing contracts that apply throughout the work.

## Legend

- `†` — establish the contract at this point, then extend it as new state
  appears. Every later phase that adds serializable state (managers, charge
  timers, team state) extends contracts 15–19; no phase is exempt.
- `LATE` — deliberately outside the first ordinary stock-skirmish milestone.
- `SKIP/PROVE` — do not implement without evidence that the path is active in
  retail Yuri's Revenge.
- **Milestone grouping in this plan:** ordinary skirmish vs AI on retail maps is Phases 0–12
  plus items 303–304 (crates, which are `Crates=yes` by stock lobby default).
  Phase 13 (save dialogs, multiplayer) and Phase 14 sit outside that bar.

## Phase 0 — Determinism, authority, and evidence contracts

1. **GSI-01.11** — Coordinates, cells/leptons/pixels, facing and fixed math
2. **GSI-01.07** — Scenario-owned deterministic RNG
3. **GSI-01.08** — Main deterministic gameplay RNG
4. **GSI-01.10** — Timers, delays and cadence conversion
5. **GSI-01.06** — Simulation clocks, game speed, pause and pacing
6. **GSI-01.12** — Storage, vectors, reference tracking and cleanup
7. **GSI-05.01** — Type-instance registration and stable identity
8. **GSI-05.02** — Active-object membership and deterministic iteration
9. **GSI-05.03** — Reveal, limbo, uninit and deletion lifecycle
10. **GSI-05.04** — Target/reference detach and expiration
11. **GSI-01.05** — Deterministic scheduler and tick-rung order
12. **GSI-03.17** — Session policy, houses, starts and match handoff
13. **GSI-16.01** — Deterministic command-envelope encoding and admission
14. **GSI-16.02** — Synchronized command queue, frame stamps and dispatch
15. **GSI-17.02†** — Save serialization and version contracts
16. **GSI-17.03†** — Object identity, swizzling and fixup tables
17. **GSI-17.04†** — Restoration, re-registration and resume contract
18. **GSI-16.11†** — State checksums and desync diagnostics
19. **GSI-17.07†** — Replay recording and deterministic playback
20. **GSI-17.08** — Logs, assertions, crashes and diagnostic dumps
21. **GSI-17.09** — Screenshot and visual-evidence capture

## Phase 1 — Retail-file and configuration substrate

22. **GSI-01.13** — Retail asset-root and path discovery
23. **GSI-02.01** — VFS and MIX archive precedence
24. **GSI-02.02** — Loose files, language, theater and map-pack resolution
25. **GSI-02.16** — Compression codecs and packed-data helpers
26. **GSI-02.03** — INI lexical parsing, typed defaults, exact lookup and sequential rules-layer application
27. **GSI-01.14** — Localization, code pages and platform strings

## Phase 2 — Rules, types and first-frame assets

28. **GSI-02.04** — Rules globals and type registries
29. **GSI-02.05** — Houses, sides, countries, colors and ownership data
30. **GSI-02.07** — Art metadata and animation declarations
31. **GSI-02.08** — Theater, tileset, LAT, ramp and morph metadata
32. **GSI-02.09** — Scenario/map INI and packed-section decoding
33. **GSI-02.13** — Palettes, remaps, colors and translucency tables
34. **GSI-02.11** — TMP tiles and subtile geometry
35. **GSI-02.10** — SHP sprites and frame metadata
36. **GSI-02.12** — VXL/HVA/VPL voxel data and transforms

## Phase 3 — Map and spatial world

37. **GSI-04.01** — Map grid, dimensions, bounds and cell lookup
38. **GSI-04.02** — Theater tiles and isometric ground geometry
39. **GSI-04.03** — Elevation, ramps, cliffs, slopes and height
40. **GSI-04.04** — Land type, movement zones and passability
41. **GSI-04.06** — Zone/subzone connectivity
42. **GSI-04.05** — Occupancy, cell object lists, layers and reservations
43. **GSI-04.07** — Overlay placement, ownership, damage and removal
44. **GSI-04.09** — Ore/gem identity and per-cell quantity
45. **GSI-04.10** — Terrain objects, crushing, fire and destruction
46. **GSI-04.12** — High-bridge topology and traversal
47. **GSI-04.13** — Low/water bridges and traversal
48. **GSI-04.15** — Active low-bridge tubes and endpoints
49. **GSI-04.16** — Waypoints, starts, regions and navigation anchors
50. **GSI-04.18** — Unexplored shroud and persisted map knowledge
51. **GSI-04.11** — Smudges, craters and scorch marks
52. **GSI-04.20** — Ambient lighting, tint and dynamic-light state

## Phase 4 — Entity runtime, scenario launch and first frame

53. **GSI-05.05** — Abstract/Object base state and spatial identity
54. **GSI-05.16** — House authority, ownership, diplomacy and outcome state
55. **GSI-05.06** — Mission/Radio/Techno/Foot behavioral spine
56. **GSI-05.15** — Terrain, overlay and smudge instance lifecycle
57. **GSI-17.01** — Scenario loading and deterministic construction order
58. **GSI-01.04** — Scenario initialization, start, exit and teardown
59. **GSI-05.07** — Infantry instances, stances and sequences
60. **GSI-05.08** — Vehicle/naval instances and unit state
61. **GSI-05.10** — Buildings, foundations, occupants and animation state
62. **GSI-02.14** — CSF strings, fonts and UI text lookup
63. **GSI-01.01** — Executable bootstrap and startup checks
64. **GSI-01.02** — Window, message pump, activation and focus
65. **GSI-01.03** — Top-level shell/game state machine
66. **GSI-03.11** — Map browser, metadata, preview and starts
67. **GSI-03.10** — Skirmish slots, factions, teams, colors and options
68. **GSI-03.09** — Loading screen, progress and transition
69. **GSI-13.02** — Isometric projection, camera, clipping and scrolling
70. **GSI-13.03** — Object layers, Y/Z order, occlusion and traversal
71. **GSI-13.08** — Palettes, remapping, blending and color conversion
72. **GSI-13.09** — Depth, alpha surfaces, masks and overlap handling
73. **GSI-13.04** — TMP ground, ramps, shores, LAT and transitions
74. **GSI-13.05** — Overlays, walls, ore, bridges and terrain drawing
75. **GSI-13.06** — SHP frames, facings, sequences and blitting
76. **GSI-13.07** — Voxel transforms, turrets and rasterization
77. **GSI-13.10** — Lighting, object lights and palette effects
78. **GSI-13.01** — Tactical redraw and final frame composition
79. **GSI-13.26** — Minimal loading/shell presentation path

## Phase 5 — Input, orders, ground movement and shroud

80. **GSI-14.01** — Keyboard input, hotkeys and bindings
81. **GSI-14.02** — Mouse input, dragging, wheel and double-click
82. **GSI-14.06** — Tactical scrolling, camera clamps and bookmarks
83. **GSI-14.04** — Single, bandbox and type selection
84. **GSI-14.03** — Cursor action and target-context resolution
85. **GSI-14.07** — Semantic player-order construction (includes YR's
    waypoint planning mode — `WaypointPathClass`, the House path arrays — a
    whole input mode, not a detail)
86. **GSI-14.05** — Control groups and selection cycling
87. **GSI-13.22** — Selection markers, bars, pips and action lines
88. **GSI-13.25** — Cursor, tooltip, range and target feedback
89. **GSI-07.01** — Command admission, ownership checks and replacement
90. **GSI-07.02** — Mission-control metadata
91. **GSI-07.03** — Assign, queue, override, suspend and restore missions
92. **GSI-07.04** — Mission dispatcher, substates and timers
93. **GSI-07.05** — Sleep mission
94. **GSI-07.18** — Stop mission
95. **GSI-07.10** — Guard mission
96. **GSI-06.04** — Terrain cost, speed types and modifiers
97. **GSI-06.02** — Reachability and zone queries
98. **GSI-06.01** — Movement admission and cell-entry gates
99. **GSI-06.03** — Path search, tie-breaking and reconstruction
100. **GSI-06.06** — Path queues, reservations and traffic commits
101. **GSI-06.07** — Occupancy and bridge/layer transitions
102. **GSI-06.12** — Locomotor dispatch, ownership and authority handoff
103. **GSI-06.11** — Facing, turning, acceleration and braking
104. **GSI-06.13** — Drive locomotion
105. **GSI-06.14** — Walk locomotion
106. **GSI-06.05** — Path smoothing, retries and blocked recovery
107. **GSI-06.08** — Collision, scatter, pushing and crushing
108. **GSI-07.07** — Move mission
109. **GSI-07.08** — QMove mission
110. **GSI-07.11** — Sticky mission selector
111. **GSI-12.01** — Sight range and reveal footprints
112. **GSI-12.03** — Visibility reference frames and elevation
113. **GSI-12.02** — Reveal/conceal mutation protocol
114. **GSI-13.21** — Shroud overlay composition
115. **GSI-13.11** — Ground, object and voxel shadows

## Phase 6 — Combat, death and immediate feedback

116. **GSI-05.11** — Projectile instances and target references
117. **GSI-05.12** — Animation instances and attached ownership
118. **GSI-05.13** — Particle and particle-system instances
119. **GSI-05.14** — Voxel debris and falling objects
120. **GSI-07.06** — Attack mission
121. **GSI-08.01** — Target legality, acquisition and threat scoring
122. **GSI-08.02** — Primary/secondary/elite weapon selection
123. **GSI-08.03** — Fire, reload, ammo, power and mission gates
124. **GSI-08.04** — Range, line of fire, FLH and fire error
125. **GSI-08.05** — ROF, burst, radial fire and rearm
126. **GSI-08.14** — Body/turret/barrel facing and recoil
127. **GSI-08.06** — Projectile creation and launch effects
128. **GSI-08.07** — Projectile flight models
129. **GSI-08.08** — Collision, proximity, fuse and detonation
130. **GSI-08.09** — Area damage, spread and falloff
131. **GSI-08.10** — Damage, armor, Verses, healing and immunities
132. **GSI-08.11** — Death, destruction, kill credit and debris
133. **GSI-08.33** — Warhead effects on targets, terrain, bridges and ore
134. **GSI-08.12** — Veterancy, experience and elite effects
135. **GSI-08.13** — Infantry fear, prone/crawl and death sequences
136. **GSI-08.17** — Crush-death consequences
137. **GSI-12.05** — Cloak state and decloak triggers
138. **GSI-12.06** — Sensors, detection and stealth legality
139. **GSI-07.09** — Retreat mission
140. **GSI-07.16** — Area Guard mission
141. **GSI-07.20** — Hunt mission
142. **GSI-07.34** — Attack Move selector
143. **GSI-04.21** — Radiation, hazards, fire and environmental damage
144. **GSI-04.14** — Bridge damage, collapse, debris and repair
145. **GSI-13.12** — Damage, fire, smoke, debris and animation drawing
146. **GSI-02.15** — Audio indexes and AUD/VOC/WAV decoding
147. **GSI-15.01** — Sound registry, variants, priority and volume
148. **GSI-15.04** — Audio device, mixer and update cadence
149. **GSI-15.03** — Channels, interruption, looping and fades
150. **GSI-15.02** — Positional attenuation and stereo pan
151. **GSI-15.05** — Gameplay-to-audio trigger routing
152. **GSI-15.08** — Unit voices and acknowledgements

## Phase 7 — Harvesting and economy

Prior work and findings: [Phase 7 record](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/gap-scans/2026-09-06-phase7-closure/README.md).

153. **GSI-09.01** — Credits, transactions and displayed money
154. **GSI-09.02** — Storage, silos and resource loss
155. **GSI-09.03** — Ore/gem value, cargo and unload conversion
156. **GSI-09.05** — Miner work-site and return decisions
157. **GSI-07.15** — Harvest mission
158. **GSI-07.17** — Return mission — `PROVEN DEAD` (2026-09-05: vtable slot
    `+0x234` holds the base stub `0x005B2ED0` (`return 450`) in all eight
    vtables and no code path assigns mission 12; rulesmd.ini marks `[Return]`
    `; <unused>`. Harvester return-to-refinery runs *inside* the Harvest
    mission's states — see item 157. Keep the enum slot.)
159. **GSI-07.37** — Radio contact and link protocol
160. **GSI-07.38** — Docking reservations, queues and authority handoff
161. **GSI-07.39** — Refinery docking, transfer and release
162. **GSI-07.21** — Unload mission
163. **GSI-09.04** — Resource growth and spread scheduling
164. **GSI-15.06** — EVA queueing and house voice (build with the Phase 6
    audio block 146–152: EVA and unit voices share one VoxClass queue, and
    Phase 6 combat already fires `EVA_UnitLost` / base-under-attack)
165. **GSI-15.07** — Music catalog, selection and transitions

## Phase 8 — Production, construction, power and radar

Brief: [Phase 8](phase-briefs/phase-08-production-power-radar.md).

166. **GSI-04.08** — Walls, gates, fences, pavement and buildable overlays
167. **GSI-05.17** — Factory identity, registration and lifecycle
168. **GSI-09.07** — Power, low power and blackout
169. **GSI-09.08** — Tech tree, prerequisites and build limits
170. **GSI-09.09** — Factories, queues and parallel production
171. **GSI-09.10** — Build time, cost and production progress
172. **GSI-09.11** — Placement legality, foundations and adjacency
173. **GSI-07.23** — Construction mission
174. **GSI-09.12** — Buildup, completion and activation
175. **GSI-07.41** — Factory exit and blocked-exit recovery
176. **GSI-09.16** — MCV deployment and construction-yard authority
177. **GSI-09.14** — Selling, refunds and teardown
178. **GSI-07.24** — Selling mission
179. **GSI-12.10** — Radar availability, jams and blackout
180. **GSI-14.10** — Sidebar tabs, buttons and production queues
181. **GSI-14.08** — Placement, repair, sell, deploy and rally input
182. **GSI-13.24** — Sidebar, power strip, cameos and queue presentation
183. **GSI-13.23** — Radar/minimap presentation
184. **GSI-14.11** — Radar interaction and navigation

## Phase 9 — Capture, transport, service, naval and aircraft

185. **GSI-05.09** — Aircraft instances, airports and flight identity
186. **GSI-06.15** — Ship locomotion
187. **GSI-06.17** — Hover locomotion
188. **GSI-06.16** — Fly locomotion
189. **GSI-06.24** — Takeoff, landing, altitude and airport approach
190. **GSI-06.18** — Jumpjet locomotion (stock users include the Rocketeer,
    Kirov, Floating Disc, and Siege Chopper; the Magnetron's `LocomotorBeam`
    warhead attaches this locomotor to lift victims — item 241 depends on it)
191. **GSI-06.19** — Rocket locomotion
192. **GSI-05.21** — Attached-manager lifecycle infrastructure
193. **GSI-06.20** — Teleport locomotion
194. **GSI-07.12** — Enter mission
195. **GSI-07.13** — Capture mission
196. **GSI-09.15** — Ownership-transfer effects
197. **GSI-07.22** — Sabotage/Capture selector
198. **GSI-07.42** — Passenger load/unload and transporter destruction
199. **GSI-07.29** — Open mission
200. **GSI-07.45** — Gate protocol and linked traversal
201. **GSI-07.40** — Aircraft docking, rearm and release
202. **GSI-09.13** — Repair, rearm, hospital and armory services (note: YR's
    Tech Hospital / Machine Shop are NOT enter-to-heal docks — they grant
    house-wide passive self-heal via `InfantryGainSelfHeal` /
    `UnitsGainSelfHeal`; the old TS enter path is commented out)
203. **GSI-07.25** — Repair mission
204. **GSI-09.17** — Building upgrades and slots
205. **GSI-12.07** — Disguise, spies and mirage presentation

## Phase 10 — AI, teams, triggers and outcomes

206. **GSI-02.06** — TaskForce, Script, Team and AITrigger data
207. **GSI-04.17** — Tags, cell tags, variables and map flags
208. **GSI-05.18** — Team identity, membership and lifecycle
209. **GSI-05.19** — Trigger/Tag identity and persistence
210. **GSI-10.14** — Difficulty, IQ, handicap and aggression
211. **GSI-10.01** — House AI brain and strategic cadence
212. **GSI-10.03** — AI economy and spending priorities
213. **GSI-10.04** — AI production and factory assignment
214. **GSI-10.02** — Base planning, placement and rebuilding
215. **GSI-10.05** — Threat maps, defense and target selection
216. **GSI-10.07** — TaskForce composition and recruitment
217. **GSI-10.08** — Team formation, ownership and dissolution
218. **GSI-06.10** — Team formation movement and coordination
219. **GSI-07.33** — Wait/Deliberate mission
220. **GSI-10.09** — Script steps, arguments and branching
221. **GSI-10.06** — AITrigger selection, weights and cooldowns
222. **GSI-10.11** — Trigger event predicates
223. **GSI-10.12** — Trigger action dispatch and ordering
224. **GSI-10.10** — Scenario Trigger/Tag evaluation
225. **GSI-10.15** — Win/loss, surrender and scenario outcome
226. **GSI-10.16** — Scores, kills, losses and statistics
227. **GSI-03.06** — Victory/defeat transition and results

## Phase 11 — Stock faction and special-unit breadth

228. **GSI-08.15** — Air-to-air, anti-air, strafing and bombing
229. **GSI-07.43** — Open-topped, garrison and bunker coordination
230. **GSI-08.16** — Garrison/open-topped firing and damage routing
231. **GSI-07.44** — IFV/Gunner passenger-dependent weapon selection
232. **GSI-08.27** — Fire, chaos, berserk and persistent statuses
233. **GSI-08.18** — C4, Ivan bombs and bridge charges
234. **GSI-08.19** — Prism-support network
235. **GSI-08.20** — Gattling stages and reset
236. **GSI-08.21** — Sonic weapon and Wave handoff
237. **GSI-08.22** — Tesla strikes and EBolt handoff
238. **GSI-08.23** — Laser weapons and LaserDraw handoff
239. **GSI-08.26** — Radiation-site manager
240. **GSI-08.24** — Radiation beams and eruption damage
241. **GSI-08.25** — Magnetron lift, carry and drop
242. **GSI-08.28** — Mind control and capture manager
243. **GSI-08.29** — Temporal targeting and erasure
244. **GSI-08.30** — Parasite behavior
245. **GSI-08.31** — Spawn manager and spawned aircraft
246. **GSI-08.32** — Slave manager
247. **GSI-09.06** — Slave miner
248. **GSI-09.18** — Grinder
249. **GSI-09.19** — Cloning Vat
250. **GSI-09.20** — Bio Reactor
251. **GSI-09.21** — Ore Purifier
252. **GSI-12.08** — Gap Generator
253. **GSI-13.13** — Particle, trail, spark and smoke composition
254. **GSI-13.14** — LaserDraw lifetime and rendering
255. **GSI-13.15** — Transient laser-segment manager
256. **GSI-13.16** — DiskLaser manager
257. **GSI-13.17** — EBolt lifetime and drawing
258. **GSI-13.18** — Sonic/magnetic Wave drawing
259. **GSI-13.19** — AlphaShape lifetime and drawing

## Phase 12 — Superweapons and strategic actions

260. **GSI-05.20** — Superweapon identity and charge-state ownership
261. **GSI-11.01** — Generic charge, readiness, targeting and launch
262. **GSI-11.12** — Building links, availability and sidebar state
263. **GSI-14.09** — Superweapon targeting mode and feedback
264. **GSI-07.27** — Missile mission
265. **GSI-07.31** — Paradrop Approach mission
266. **GSI-07.32** — Paradrop Overfly mission
267. **GSI-07.35** — Spyplane Approach mission
268. **GSI-07.36** — Spyplane Overfly mission
269. **GSI-04.22** — Active Lightning-Storm weather subset
270. **GSI-11.02** — Nuclear missile (`Type=MultiMissile` — implement the
    MultiMissile machinery here, including the `[WEEDGUY]` hack data and the
    `EMPulseWarhead`/`EMPulseProjectile` keys the falling nuke consumes)
271. **GSI-11.03** — Lightning Storm
272. **GSI-11.04** — Iron Curtain
273. **GSI-11.05** — Force Shield
274. **GSI-11.06** — Chronosphere/ChronoWarp (two linked `[SuperWeaponTypes]`
    entries: ChronoWarp is `PostClick=yes` / `PreDependent=ChronoSphere` —
    implement both)
275. **GSI-11.07** — Psychic Dominator
276. **GSI-11.08** — Genetic Mutator
277. **GSI-11.09** — Paradrop superweapon (two `Type=` values: `ParaDrop` and
    `AmerParaDrop` — the American bonus drop and the Tech Airport drop both
    route here)
278. **GSI-11.10** — Spy-plane/recon
279. **GSI-12.09** — SpySat/Psychic Reveal map effects
280. **GSI-11.11** — Psychic Reveal
281. **GSI-13.20** — Remaining active weapon/superweapon visuals

## Phase 13 — Save/load completion and multiplayer

First finish the `†` persistence and replay contracts introduced in Phase 0.
Items 15–17 land in 282–283 below; item 19 (replay) has no numbered landing
row — treat "replay playback completion and divergence check" as an explicit
part of this phase so the `†` on item 19 is actually closed here.

282. **GSI-03.08** — Save/load dialogs and slot metadata
283. **GSI-14.12** — In-game save/load, surrender, restart and quit
284. **GSI-16.05** — Connections, packets, queues and retries
285. **GSI-16.06** — LAN discovery and transport
286. **GSI-16.07** — Lobby membership, readiness and launch
287. **GSI-16.04** — Seed, map, content and options handshake
288. **GSI-16.08** — Map negotiation and file transfer
289. **GSI-16.03** — Lockstep pacing, latency and stalls
290. **GSI-16.10** — Multiplayer chat, beacons and alliances
291. **GSI-14.13** — Multiplayer communication UI
292. **GSI-16.12** — Reconnect, timeout, drop and abort
293. **GSI-03.13** — Multiplayer mode catalog
294. **GSI-03.14** — LAN host/join and lobby shell
295. **GSI-16.13** — Observer/spectator data policy
296. **GSI-12.11** — Observer and allied vision sharing
297. **GSI-03.16** — Observer setup and multiplayer results

## Phase 14 — Shell, campaign and optional modes (`LATE`)

298. **GSI-03.01** — Main menu and shell transitions
299. **GSI-03.02** — Options, hotkeys, display and audio settings
300. **GSI-17.06** — Settings, hotkeys and profile persistence
301. **GSI-01.09** — Random-map RNG
302. **GSI-03.12** — Random-map generation and preview
303. **GSI-04.23** — Crates and powerups (milestone-visible despite Phase 14
    placement: stock lobby default is `Crates=yes`, one crate per human player
    at match start, ~3-minute regen — every ordinary skirmish exercises this;
    treat as Phase 6–7 work. Row number retained for reference stability.)
304. **GSI-08.34** — Crate combat modifiers (same milestone-visibility note as
    303)
305. **GSI-06.09** — Convoy chains and follower cohesion
306. **GSI-07.30** — Patrol mission — `SKIP/PROVE` (no assigner in stock YR,
    Ghidra-verified 2026-08-09: the handler body `0x004D4280` is called only
    from inside the Hunt override, and the "Patrol" string is referenced only
    by the mission-name table — no player command reaches mission 25. Patrol
    *behavior* belongs to item 141 (Hunt); keep the enum slot and the map-INI
    `Mission=Patrol` round-trip.)
307. **GSI-07.28** — Harmless mission
308. **GSI-07.26** — Rescue mission
309. **GSI-02.17** — Bink/VQA media interface
310. **GSI-15.10** — Movie playback and audio synchronization
311. **GSI-15.11** — Briefing and cinematic speech
312. **GSI-15.09** — Subtitles and speech-linked text
313. **GSI-03.03** — Campaign catalog and selection
314. **GSI-03.05** — Mission selection, briefing and objectives
315. **GSI-10.13** — Campaign scripting and cinematics
316. **GSI-03.04** — Campaign progression and carryover
317. **GSI-17.05** — Campaign persistence
318. **GSI-03.07** — Movies, previews and credits
319. **GSI-16.09** — Online service compatibility/replacement
320. **GSI-03.15** — Online account, chat and game shell

## Phase 15 — Do not implement without active-YR proof

321. **GSI-04.19** — Optional TS-style fog cells — `SKIP/PROVE`
322. **GSI-06.21** — Mech locomotion — `SKIP`
323. **GSI-06.22** — DropPod locomotion — `SKIP`
324. **GSI-06.23** — Subterranean locomotion — `SKIP`
325. **GSI-07.14** — Eaten mission — `SKIP/PROVE`
326. **GSI-07.19** — Ambush mission (dead slot) — `SKIP` (gamemd's name is
    "Ambush"; rulesmd.ini marks it `; <unused>`. A map-INI `Mission=Ambush`
    on a pre-placed object must still round-trip — keep the enum slot.)
327. **GSI-12.04** — Optional fog visibility policy — `SKIP/PROVE`
328. **GSI-16.14** — Modem, serial and null-modem transports — `SKIP`
329. **GSI-18.01** — Veins and veinhole mechanics — `SKIP`
330. **GSI-18.02** — Firestorm/laser-fence legacy path — `SKIP/PROVE`
331. **GSI-18.03** — EMP superweapon path — `SKIP` (but the *live nuke*
    consumes EMPulse-named data — `EMPulseWarhead=EMPuls`,
    `EMPulseProjectile=PulsPr`, warhead `1=EMPuls` — those keys belong to
    item 270 and must parse)
332. **GSI-18.04** — Ion Cannon path — `SKIP` (trap: `[IonCannonSpecial]` is
    a live, uncommented rulesmd.ini section, but it is absent from
    `[SuperWeaponTypes]` — the registration *list* is the activation
    authority, never section presence)
333. **GSI-18.05** — Hunter Seeker path — `SKIP/PROVE`
334. **GSI-18.06** — Chemical Missile path — `SKIP/PROVE` (the MultiMissile
    machinery it shares is live via `NukeSpecial` — item 270; only the
    Chemical variant stays skipped)
335. **GSI-18.07** — Editor-only behavior — `SKIP`
336. **GSI-18.08** — Native cheats/developer controls — `SKIP`

## Additional mechanisms

These seven mechanisms supplement the numbered rows; row numbering stays stable.

- **Spy infiltration effect family** — per-building-type effects
  (`SpyPowerBlackout`, `SpyMoneyStealPercent`, radar reshroud, superweapon
  reset, stolen-tech prerequisites `RequiresStolenAlliedTech`/`SovietTech`/
  `ThirdTech`, ~30 `Spyable=yes` buildings; `BuildingClass::OnSpyInfiltrate`).
  Item 205 is presentation only; 194/196 don't reach this. Phase 9. Fires
  whenever a player uses spies — a standard stock tactic.
- **Neutral tech-structure economy** — Oil Derrick passive income
  (`[CAOILD]` `ProduceCashStartup/Amount/Delay`) and Secret Lab random boon
  (`[CASLAB]` `SecretLab=yes` + `[General]` `SecretInfantry/Units/Buildings`).
  Derricks are contested in the opening minutes of most retail-map matches.
  Phase 9 (capture family) or 7 (income timer).
- **Robot Tank ↔ Robot Control Center** — powered-unit deactivation
  (`RobotControlCount`, offline/back-online handlers, EVA + frozen tanks on
  center loss; YR-active per research corpus). Item 168 is house power, not
  per-unit online state. Phase 11.
- **Attack-dog leap kill** — dogs' leap-and-instakill vs infantry plus
  dog-vs-disguise reveal targeting. No item; 107/120/135 don't lead there.
  Leap mechanism itself UNCHECKED in the corpus. Phase 6 or 11. Every dog
  engagement; dogs are early-game staples.
- **Boris airstrike designation (AirstrikeClass)** — flare designation,
  called-in MiG team (`AirstrikeTeam/TeamType/RechargeTime`), building tint,
  and the aircraft radio-deaf gate that null-checks the manager pointer.
  Distinct from the spawn manager (245). Phase 11, in the 234–259 manager
  cluster. Every Boris anti-building attack.
- **Tesla Coil overpower** — trooper charging (`Overpowerable=true`,
  `ChargeToDrainRatio`), stronger bolt, firing through low power. Items
  237/168 don't reach the charger-count state. Phase 11. Routine Soviet play.
- **Floating Disc drain (sim side)** — credit drain and power-plant/defense
  disable (`DrainMoneyFrameDelay/Amount`, `Drainable=yes`, `DiskDrain`
  `DrainWeapon=yes`). Item 256 covers only the beam visual; widen it or add a
  sim row. Phase 11. Standard Yuri harassment.

Additional scope notes: crew survivors (`CrewEscape=50%`, `Crewed=`) fold
under 132/177; suicide weapons (`Suicide=yes` — firer dies on shot: Terrorist,
Demo Truck weapons) fold under 123/127 with `Explodes=yes` under 132/133;
Industrial Plant cost bonus (`FactoryPlant=yes`, `UnitsCostBonus=0.75`) folds
under 171 beside its named siblings 249–251; Psychic Sensor attack-intent
warning (`PsychicDetectionRadius=15` on Yuri's standard radar building) folds
under 138; Siege Chopper / simple-deployer type-transforms
(`IsSimpleDeployer`, `UnloadingClass`, `DeployFire`) fold under 176/181.

Mission-liveness residue: Harmless (307) and Rescue (308) have populated
mission-control data but UNCHECKED liveness — run a liveness pass before
Phase 14 work; no SKIP flag is justified today.
