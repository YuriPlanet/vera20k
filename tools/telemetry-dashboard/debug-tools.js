/**
 * Optional offline developer controls, adapted from PR #2083 (@1372704087).
 * The read-only dashboard never advertises this capability. Every command is
 * gated on the latest authenticated snapshot and its current match session;
 * the engine independently validates both before applying it.
 */
import { onLocaleChange, t } from "./i18n.js";
const CHEAT_SWITCHES = [
    "infinite_money", "infinite_power", "instant_build", "build_anywhere",
    "invincible", "reveal_map", "tech_all", "tech_boost",
];
const TOGGLE_SWITCHES = [
    { name: "terrain_costs", key: "sw.terrainCosts", flag: "terrain_costs" },
    { name: "heightmap", key: "sw.heightmap", flag: "heightmap" },
    { name: "cell_grid", key: "sw.cellGrid", flag: "cell_grid" },
    { name: "foot_path", key: "sw.footPath", flag: "foot_path" },
    { name: "unit_inspector", key: "sw.inspector", flag: "unit_inspector" },
    { name: "debug_pause", key: "sw.pause", flag: "paused" },
];
let latest = null;
let connected = false;
let transmit = null;
let pendingCheats = null;
function dialog() {
    return document.getElementById("debug-tools-dialog");
}
function input(selector) {
    return document.querySelector(selector);
}
/** A compiled flag alone is insufficient: replay and multiplayer stay disabled. */
function canCommand() {
    const capability = latest?.debug_tools;
    return connected && capability?.enabled === true &&
        Number.isSafeInteger(capability.session) && (capability.session ?? 0) > 0;
}
function send(command) {
    const session = latest?.debug_tools?.session;
    if (!canCommand() || session == null)
        return;
    transmit?.({ session, command });
}
function buildSwitches() {
    const container = document.getElementById("switches");
    const toggles = document.createElement("div");
    toggles.className = "switches";
    for (const spec of TOGGLE_SWITCHES) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "switch";
        button.dataset.switch = spec.name;
        button.addEventListener("click", () => send({ cmd: "toggle", switch: spec.name }));
        toggles.append(button);
    }
    const steps = document.createElement("div");
    steps.className = "switches";
    for (const forward of [false, true]) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "switch step";
        button.id = forward ? "speed-next" : "speed-prev";
        button.textContent = forward ? "›" : "‹";
        button.addEventListener("click", () => send({ cmd: "cycle_speed_type", forward }));
        steps.append(button);
        if (!forward) {
            const value = document.createElement("span");
            value.className = "switch-value";
            value.id = "speed-value";
            steps.append(value);
        }
    }
    const frame = document.createElement("button");
    frame.type = "button";
    frame.className = "switch";
    frame.id = "frame-step";
    frame.addEventListener("click", () => send({ cmd: "frame_step" }));
    steps.append(frame);
    container.replaceChildren(toggles, steps);
}
function renderSwitches(debug) {
    const overlays = debug?.overlays;
    const active = canCommand() && !!overlays;
    document.getElementById("debug-switch-fields").disabled = !active;
    for (const spec of TOGGLE_SWITCHES) {
        const button = document.querySelector(`[data-switch="${spec.name}"]`);
        const on = overlays?.[spec.flag] === true;
        button.textContent = t(spec.key);
        button.classList.toggle("on", on);
        button.setAttribute("aria-pressed", String(on));
        button.disabled = !active || overlays?.[spec.flag] === undefined;
    }
    const speedEnabled = active && overlays?.terrain_costs === true;
    const previous = document.getElementById("speed-prev");
    const next = document.getElementById("speed-next");
    previous.disabled = !speedEnabled;
    previous.setAttribute("aria-label", t("tools.previousSpeed"));
    next.disabled = !speedEnabled;
    next.setAttribute("aria-label", t("tools.nextSpeed"));
    document.getElementById("speed-value").textContent = debug?.active_speed_type ?? "-";
    const frame = document.getElementById("frame-step");
    frame.textContent = t("sw.frameStep");
    frame.disabled = !active || overlays?.paused !== true;
}
function currentCheatFlags() {
    const flags = {};
    for (const name of CHEAT_SWITCHES)
        flags[name] = input(`input[data-cheat="${name}"]`)?.checked ?? false;
    const bounded = (selector, minimum, maximum) => {
        const value = Number(input(selector)?.value ?? minimum);
        return Number.isFinite(value) ? Math.max(minimum, Math.min(maximum, Math.round(value))) : minimum;
    };
    flags.money_target = bounded("#cheat-money", 0, 1_000_000_000);
    flags.damage_scale = bounded("#cheat-damage", 1, 100);
    return flags;
}
function markCheatChip() {
    const value = Number(input("#cheat-damage")?.value ?? 1);
    document.querySelectorAll(".cheat-chip").forEach((chip) => {
        const on = Number(chip.dataset.scale) === value;
        chip.classList.toggle("on", on);
        chip.setAttribute("aria-pressed", String(on));
    });
}
function sendCheats() {
    if (pendingCheats || !canCommand() || !transmit || !latest?.debug_tools?.cheats)
        return;
    const session = latest.debug_tools.session;
    const flags = currentCheatFlags();
    pendingCheats = { session, flags };
    send({ cmd: "set_cheats", flags });
    document.getElementById("debug-trainer-fields").disabled = true;
    markCheatChip();
}
/** Echo the owner's state; editing a numeric input is not interrupted by snapshots. */
function syncCheats(report) {
    // A snapshot already in flight can predate our command. Keep its desired
    // values and serialize edits until the owner echoes them, so a second full
    // replacement cannot silently undo the first selection.
    if (pendingCheats && report &&
        [...CHEAT_SWITCHES, "money_target", "damage_scale"].every((key) => report[key] === pendingCheats.flags[key]))
        pendingCheats = null;
    const displayed = pendingCheats?.flags ?? report;
    document.getElementById("debug-trainer-fields").disabled =
        !canCommand() || !report || pendingCheats !== null;
    for (const name of CHEAT_SWITCHES) {
        const control = input(`input[data-cheat="${name}"]`);
        if (control)
            control.checked = displayed?.[name] ?? false;
    }
    const money = input("#cheat-money");
    if (money && document.activeElement !== money) {
        // No owner means the trainer has never been configured. Offer the original
        // useful floor, while preserving an explicitly chosen zero after use.
        money.value = String(!pendingCheats && report && report.owner === null && report.money_target === 0
            ? 999_999 : displayed?.money_target ?? 0);
    }
    const damage = input("#cheat-damage");
    if (damage && document.activeElement !== damage)
        damage.value = String(Math.max(1, displayed?.damage_scale ?? 1));
    const owner = document.getElementById("cheat-owner");
    const house = latest?.houses.find((row) => row.name === report?.owner);
    owner.textContent = report?.owner
        ? `${t("cheat.owner")} ${report.owner}${house ? ` · ${t("cheat.wallet")} ${house.credits.toLocaleString()}` : ""}`
        : t(report && canCommand() ? "tools.localTarget" : "tools.noTarget");
    const power = document.getElementById("cheat-power");
    power.textContent = house ?
        `${t("cheat.power")} ${house.power_output} / ${house.power_drain} · ${t(report?.power_forced ? "cheat.powerForced" : house.low_power ? "cheat.powerLow" : "cheat.powerOk")}` : "";
    markCheatChip();
}
function bindCheats() {
    for (const name of CHEAT_SWITCHES)
        input(`input[data-cheat="${name}"]`)?.addEventListener("change", sendCheats);
    input("#cheat-money")?.addEventListener("change", sendCheats);
    input("#cheat-damage")?.addEventListener("change", sendCheats);
    document.querySelectorAll(".cheat-chip").forEach((chip) => {
        chip.addEventListener("click", () => {
            const damage = input("#cheat-damage");
            if (damage)
                damage.value = chip.dataset.scale ?? "1";
            sendCheats();
        });
    });
    const actions = { "cheat-heal": "heal_all", "cheat-veteran": "veteran_all", "cheat-supers": "superweapons_ready" };
    for (const [id, action] of Object.entries(actions)) {
        document.getElementById(id)?.addEventListener("click", () => {
            if (latest?.debug_tools?.cheats)
                send({ cmd: "cheat_action", action });
        });
    }
    document.getElementById("cheat-reset")?.addEventListener("click", () => {
        for (const name of CHEAT_SWITCHES) {
            const control = input(`input[data-cheat="${name}"]`);
            if (control)
                control.checked = false;
        }
        const damage = input("#cheat-damage");
        if (damage)
            damage.value = "1";
        sendCheats();
    });
}
/** The engine caps captures; independent UI limits keep malformed snapshots bounded. */
function renderEvents(debug) {
    const panel = document.getElementById("debug-events");
    panel.replaceChildren();
    const selected = debug?.selected.slice(0, 32) ?? [];
    if (selected.length === 0) {
        panel.textContent = t("ev.pick");
        return;
    }
    for (const entity of selected) {
        const heading = document.createElement("div");
        heading.className = "sub";
        heading.textContent = `${entity.type_name.slice(0, 64)} (id=${entity.stable_id})`;
        const list = document.createElement("div");
        list.className = "event-list";
        const events = entity.events?.slice(-64) ?? [];
        if (!entity.inspector_active || events.length === 0) {
            list.textContent = t(entity.inspector_active ? "ev.noneRecorded" : "ev.noInspector");
        }
        else {
            for (const event of events) {
                const line = document.createElement("div");
                line.className = "event";
                line.textContent = `[${event.tick}] ${event.text.slice(0, 512)}`;
                list.append(line);
            }
        }
        panel.append(heading, list);
    }
}
function render() {
    const capability = latest?.debug_tools;
    const opener = document.getElementById("debug-tools-open");
    opener.hidden = !capability;
    if (!capability && dialog().open)
        dialog().close();
    const status = document.getElementById("debug-tools-status");
    status.textContent = canCommand() ? t("tools.active") : capability?.unavailable_reason
        ? t("tools.unavailableReason", { reason: capability.unavailable_reason.slice(0, 512) })
        : t("tools.unavailable");
    renderSwitches(latest?.debug ?? null);
    syncCheats(capability?.cheats ?? null);
    if (dialog().open)
        renderEvents(latest?.debug ?? null);
}
export function updateDebugTools(snapshot, isConnected) {
    latest = snapshot;
    connected = isConnected;
    if (!canCommand() || pendingCheats?.session !== latest?.debug_tools?.session)
        pendingCheats = null;
    render();
}
export function setupDebugTools(sender) {
    transmit = sender;
    buildSwitches();
    bindCheats();
    document.getElementById("debug-tools-open").addEventListener("click", () => {
        if (!latest?.debug_tools || !connected)
            return;
        if (!dialog().open)
            dialog().showModal();
        render();
    });
    document.getElementById("debug-tools-close").addEventListener("click", () => dialog().close());
    dialog().addEventListener("click", (event) => {
        if (event.target !== dialog())
            return;
        const rect = dialog().getBoundingClientRect();
        if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom)
            dialog().close();
    });
    onLocaleChange(render);
    render();
}
