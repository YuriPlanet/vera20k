"""Evidence-grounded coverage catalog for the exact shell UI matrix."""

from __future__ import annotations

from collections.abc import Iterable


DESIGN = "docs/plans/2026-07-25-exact-stock-skirmish-shell-ui-design.md"
MAIN_MENU = "docs/research/MAIN_MENU_DIALOG_0XE2_FULL_VISIBLE_COMPOSITION_GHIDRA_REPORT.md"
SINGLE_PLAYER = "docs/research/SINGLE_PLAYER_0X100_SKIRMISH_0X579_ROUTE_GHIDRA_REPORT.md"
SKIRMISH_MODEL = "docs/research/SKIRMISH_UI_CURRENT_SYSTEM_MODEL_SYNTHESIS.md"
SKIRMISH_INPUT = (
    "docs/research/skirmish-ui/"
    "SKIRMISH_SHELL_INPUT_FOCUS_MESSAGE_BROAD_RECHECK_GHIDRA_REPORT.md"
)
CHOOSE_MAP = (
    "docs/research/skirmish-ui/SKIRMISH_CHOOSE_MAP_MODAL_SHELL_COMPOSITION_GHIDRA_REPORT.md"
)
RANDOM_MAP = (
    "docs/research/skirmish-ui/"
    "SKIRMISH_RANDOM_MAP_SETUP_DIALOG_CONTROLS_OPTIONS_GHIDRA_REPORT.md"
)
SAVED_SEED = "docs/research/skirmish-ui/RANDOM_MAP_SAVED_SEED_SLOTS_GHIDRA_REPORT.md"
VALIDATION_MODAL = (
    "docs/research/skirmish-ui/"
    "SKIRMISH_START_VALIDATION_MODAL_ACTIVATION_RECHECK_GHIDRA_REPORT.md"
)
START_LOADING = "docs/research/SKIRMISH_START_TO_LOADING_SCREEN_ACTIVATION_GHIDRA_REPORT.md"
LOADING_FIRST = "docs/research/LOADING_FIRST_RENDERER_00552D60_GHIDRA_REPORT.md"
LOADING_SETUP = "docs/research/LOAD_PROGRESS_MANAGER_SETUP_GHIDRA_REPORT.md"
LOADING_SEQUENCE = (
    "docs/research/LOADING_FUN_0069AE90_SKIRMISH_CALLERS_AFTER_FIRST_RENDERER_GHIDRA_REPORT.md"
)
LOADING_REPAINT = "docs/research/PROGRESSCLASS_REPAINT_CADENCE_HWND_GHIDRA_REPORT.md"
LOADING_BAR = "docs/research/PROGBARM_PROGRESSCLASS_DRAW_GEOMETRY_GHIDRA_REPORT.md"
FIRST_TACTICAL = "docs/research/GSCREEN_RTACTICAL_GHIDRA_REPORT.md"
COLOR_TABLE = (
    "docs/research/skirmish-ui/SKIRMISH_STATUS_COLOR_ITEM_STT_TABLE_GHIDRA_REPORT.md"
)
OBSERVER_SCOPE = "docs/research/OBSERVER_SPECTATOR_FOG_GHIDRA_REPORT.md"
RULESMD = "ini/rulesmd.ini"

SOURCES = tuple(
    sorted(
        {
            DESIGN,
            MAIN_MENU,
            SINGLE_PLAYER,
            SKIRMISH_MODEL,
            SKIRMISH_INPUT,
            CHOOSE_MAP,
            RANDOM_MAP,
            SAVED_SEED,
            VALIDATION_MODAL,
            START_LOADING,
            LOADING_FIRST,
            LOADING_SETUP,
            LOADING_SEQUENCE,
            LOADING_REPAINT,
            LOADING_BAR,
            FIRST_TACTICAL,
            COLOR_TABLE,
            OBSERVER_SCOPE,
            RULESMD,
        }
    )
)

RESOLUTIONS = (
    {"height": 480, "width": 640},
    {"height": 600, "width": 800},
    {"height": 768, "width": 1024},
)

# Stock YR [Countries] order and [Sides] membership from rulesmd.ini. These
# identify the test inputs only; they deliberately do not guess LS art names.
COUNTRIES = (
    {"id": 0, "key": "Americans", "side": "allied"},
    {"id": 1, "key": "Alliance", "side": "allied"},
    {"id": 2, "key": "French", "side": "allied"},
    {"id": 3, "key": "Germans", "side": "allied"},
    {"id": 4, "key": "British", "side": "allied"},
    {"id": 5, "key": "Africans", "side": "soviet"},
    {"id": 6, "key": "Arabs", "side": "soviet"},
    {"id": 7, "key": "Confederation", "side": "soviet"},
    {"id": 8, "key": "Russians", "side": "soviet"},
    {"id": 9, "key": "YuriCountry", "side": "yuri"},
)

# Normal offline Skirmish resolves Random before loading; observer color is not
# in the ordinary playable branch. Names follow the verified color item table.
PLAYER_COLORS = (
    {"id": 0, "key": "Gold"},
    {"id": 1, "key": "Red"},
    {"id": 2, "key": "Blue"},
    {"id": 3, "key": "Green"},
    {"id": 4, "key": "Orange"},
    {"id": 5, "key": "SkyBlue"},
    {"id": 6, "key": "Purple"},
    {"id": 7, "key": "Pink"},
)

# Accepted offline Skirmish resolves only these ten playable launch countries.
# The shared Rust enum's Observer member is explicitly excluded below.
LOADING_ART_VARIANTS = (
    {"country_id": 0, "country_key": "Americans", "key": "Americans", "side": "allied"},
    {"country_id": 1, "country_key": "Alliance", "key": "Alliance", "side": "allied"},
    {"country_id": 2, "country_key": "French", "key": "French", "side": "allied"},
    {"country_id": 3, "country_key": "Germans", "key": "Germans", "side": "allied"},
    {"country_id": 4, "country_key": "British", "key": "British", "side": "allied"},
    {"country_id": 5, "country_key": "Africans", "key": "Africans", "side": "soviet"},
    {"country_id": 6, "country_key": "Arabs", "key": "Arabs", "side": "soviet"},
    {
        "country_id": 7,
        "country_key": "Confederation",
        "key": "Confederation",
        "side": "soviet",
    },
    {"country_id": 8, "country_key": "Russians", "key": "Russians", "side": "soviet"},
    {"country_id": 9, "country_key": "YuriCountry", "key": "Yuri", "side": "yuri"},
)

LOADING_PATHS = ("generated-random-map", "selected-stock-map")

STANDARD_POLICY = "standard"
RA2TS_CURRENT_POLICY = "ra2ts-lifecycle-proof-required"

SCOPE_EXCLUSIONS = (
    {
        "description": (
            "Observer loading art exists in shared Rust/native multiplayer surfaces, "
            "but Observer is gated to multiplayer game modes 3/4 and is not reachable "
            "from the owned offline Skirmish game mode 5 route."
        ),
        "id": "scope:observer-loading-art-offline-skirmish",
        "source_refs": [OBSERVER_SCOPE],
    },
    {
        "description": (
            "Random country is a setup sentinel resolved to a playable country before "
            "loading-art selection; it is tested as a transition and is not invented "
            "as an eleventh offline LS-art variant."
        ),
        "id": "scope:random-country-not-loading-art-variant",
        "source_refs": sorted([SKIRMISH_MODEL, LOADING_SETUP]),
    },
)

BLOCKERS = (
    {
        "description": (
            "Exact active mapping from the ten offline-stock LoadingArtVariant "
            "branches to every ls640/ls800 country SHP and MPLS/MPYLS palette "
            "is not exhaustively enumerated by the current cited native reports."
        ),
        "evidence_needed": (
            "An active-YR exhaustive switch/table proof with retail asset hashes, "
            "or comparable native captures covering all ten countries."
        ),
        "id": "catalog:loading-country-art-palette-map",
        "source_refs": [LOADING_FIRST, RULESMD],
    },
    {
        "description": (
            "Exact rendered player-color backing/ramp pixels for all eight "
            "playable colors are not cataloged as hashed native artifacts."
        ),
        "evidence_needed": (
            "An exhaustive ColorScheme-to-loading-pixel proof or comparable "
            "native loading captures for colors 0..7."
        ),
        "id": "catalog:loading-player-color-pixel-map",
        "source_refs": [LOADING_BAR, SKIRMISH_MODEL],
    },
    {
        "description": (
            "The first-renderer post-marker localized text content is statically "
            "ordered but not identified for a concrete stock locale/session."
        ),
        "evidence_needed": (
            "A native runtime text/capture trace with locale and session-node "
            "artifact identity, or an exhaustive text-source proof."
        ),
        "id": "catalog:loading-first-renderer-localized-text",
        "source_refs": [LOADING_FIRST, LOADING_SETUP],
    },
    {
        "description": (
            "The current process-isolated RA2TS panel restart fix lacks a comparable "
            "native lifecycle/timing sequence tied to executable and capture artifacts."
        ),
        "evidence_needed": (
            "Proof-grade process-isolated native differential evidence, or exhaustive "
            "lifecycle proof, covering startup, restart, shutdown, frames, and timing."
        ),
        "id": "catalog:ra2ts-process-lifecycle-comparability",
        "source_refs": [MAIN_MENU],
    },
)


def resolution_token(resolution: dict[str, int]) -> str:
    return f"{resolution['width']}x{resolution['height']}"


def _slug_value(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value).replace("_", "-").replace(" ", "-").lower()


def _row(
    *,
    family: str,
    checkpoint: str,
    resolution: dict[str, int],
    state: str,
    requirements: Iterable[str],
    source_refs: Iterable[str],
    variant: dict[str, object] | None = None,
    blocker_ids: Iterable[str] = (),
    verification_policy: str = STANDARD_POLICY,
) -> dict[str, object]:
    variant = dict(sorted((variant or {}).items()))
    suffix = "".join(f":{key}-{_slug_value(value)}" for key, value in variant.items())
    row_id = (
        f"{family}:{resolution_token(resolution)}:{checkpoint}:{state}{suffix}"
    )
    return {
        "blocker_ids": sorted(set(blocker_ids)),
        "checkpoint": checkpoint,
        "comparison_result": "NOT_RUN",
        "evidence": {
            "comparison_id": None,
            "native_ids": [],
            "rust_ids": [],
        },
        "family": family,
        "id": row_id,
        "owner": None,
        "requirements": sorted(set(requirements)),
        "residuals": [],
        "resolution": dict(resolution),
        "source_refs": sorted(set(source_refs)),
        "state": state,
        "status": "UNVERIFIED",
        "variant": variant,
        "verification_policy": verification_policy,
    }


PAINT_STATES = (
    ("first-visible-main-menu-frame", "initial-paint", MAIN_MENU),
    ("main-menu-0xe2", "initial-paint", MAIN_MENU),
    ("main-menu-0xe2", "steady-paint", MAIN_MENU),
    ("single-player-0x100", "initial-paint", SINGLE_PLAYER),
    ("single-player-0x100", "steady-paint", SINGLE_PLAYER),
    ("skirmish-0x102", "initial-paint", SKIRMISH_MODEL),
    ("skirmish-0x102", "steady-paint", SKIRMISH_MODEL),
    ("choose-map-0x6b", "initial-paint", CHOOSE_MAP),
    ("choose-map-0x6b", "steady-paint", CHOOSE_MAP),
    ("random-map-0x105", "initial-paint", RANDOM_MAP),
    ("random-map-0x105", "steady-paint", RANDOM_MAP),
    ("saved-seed-load-0xb7", "initial-paint", SAVED_SEED),
    ("saved-seed-load-0xb7", "steady-paint", SAVED_SEED),
    ("saved-seed-save-0x2b4", "initial-paint", SAVED_SEED),
    ("saved-seed-save-0x2b4", "steady-paint", SAVED_SEED),
    ("saved-seed-delete-0x2b5", "initial-paint", SAVED_SEED),
    ("saved-seed-delete-0x2b5", "steady-paint", SAVED_SEED),
    ("validation-modal-0xce", "initial-paint", VALIDATION_MODAL),
    ("validation-modal-0xce", "steady-paint", VALIDATION_MODAL),
)

POINTER_OWNERS = (
    ("main-menu-0xe2-buttons", MAIN_MENU),
    ("single-player-0x100-buttons", SINGLE_PLAYER),
    ("skirmish-0x102-right-panel-buttons", SKIRMISH_MODEL),
    ("choose-map-0x6b-buttons", CHOOSE_MAP),
    ("random-map-0x105-buttons", RANDOM_MAP),
    ("saved-seed-dialog-buttons", SAVED_SEED),
    ("validation-modal-0xce-ok", VALIDATION_MODAL),
)

POINTER_STATES = ("hover", "press", "release-inside", "release-outside")

INPUT_STATES = (
    "keyboard-tab-order-main-menu",
    "keyboard-tab-order-single-player",
    "keyboard-tab-order-skirmish",
    "player-name-first-focus-select-all",
    "player-name-caret-navigation",
    "player-name-printable-backspace-delete",
    "player-name-19-character-limit",
    "player-name-focus-survives-repaint",
    "modal-parent-input-blocked",
    "modal-default-routing",
    "modal-cancel-routing",
    "dropdown-keyboard-routing",
)

CONTROL_STATES = (
    "skirmish-combo-collapsed",
    "skirmish-combo-open",
    "skirmish-combo-row-hover",
    "skirmish-combo-row-select",
    "skirmish-combo-scroll-arrow",
    "skirmish-combo-scroll-track",
    "skirmish-combo-scroll-thumb-drag",
    "skirmish-combo-release-outside",
    "skirmish-checkbox-unchecked",
    "skirmish-checkbox-hover",
    "skirmish-checkbox-pressed",
    "skirmish-checkbox-checked",
    "skirmish-checkbox-label-click",
    "skirmish-trackbar-idle",
    "skirmish-trackbar-hover",
    "skirmish-trackbar-pressed",
    "skirmish-trackbar-drag",
    "skirmish-trackbar-release",
    "skirmish-trackbar-release-outside",
    "choose-map-mode-list-scroll",
    "choose-map-map-list-scroll",
    "choose-map-list-row-hover",
    "choose-map-list-row-highlight-no-preview-refresh",
    "random-map-combo-open",
    "random-map-combo-row-select",
    "random-map-player-trackbar-drag",
    "random-map-randomize-toggle-state",
    "random-map-generate-progress-state",
    "skirmish-country-random-selection",
    "skirmish-country-random-resolves-before-loading-art-selection",
    "saved-seed-list-scroll",
    "saved-seed-list-row-select",
    "saved-seed-save-name-edit",
)

TRANSITIONS = (
    "main-menu-to-single-player",
    "single-player-back-to-main-menu",
    "single-player-to-skirmish",
    "skirmish-back-to-single-player",
    "choose-map-open",
    "choose-map-cancel-to-skirmish",
    "choose-map-use-map-to-skirmish",
    "random-map-open",
    "random-map-cancel-return",
    "random-map-accept-return",
    "saved-seed-load-cancel-return",
    "saved-seed-save-cancel-return",
    "saved-seed-delete-cancel-return",
    "validation-error-no-opponent",
    "validation-error-map-capacity",
    "validation-error-same-team",
    "validation-error-mode-rejection",
    "validation-ok-dismissal",
    "accepted-start-to-loading",
    "loading-to-first-tactical-frame",
    "complete-owned-route",
)

AUDIO_STATES = (
    "main-menu-music-entry-and-loop",
    "main-menu-button-sound-order",
    "single-player-button-sound-order",
    "skirmish-button-sound-order",
    "checkbox-click-and-label-silence",
    "combo-open-close-and-scrollbar-silence",
    "trackbar-change-and-no-change-silence",
    "modal-button-sound-order",
    "start-loading-music-transition",
)

LOADING_CADENCE_STATES = (
    ("selected-stock-map", "effective-visible-milestone-sequence"),
    ("selected-stock-map", "duplicate-and-lower-milestone-suppression"),
    ("generated-random-map", "signed-halving-milestone-sequence"),
    ("all-stock-loads", "synchronous-direct-draw-repaint-cadence"),
)


def build_rows() -> list[dict[str, object]]:
    """Build the complete deterministic obligation set with no parity claims."""

    rows: list[dict[str, object]] = []
    for resolution in RESOLUTIONS:
        for checkpoint, state, source in PAINT_STATES:
            rows.append(
                _row(
                    family="paint",
                    checkpoint=checkpoint,
                    resolution=resolution,
                    state=state,
                    requirements=("cursor", "frames", "pixels", "text"),
                    source_refs=(DESIGN, source),
                )
            )

        rows.append(
            _row(
                family="paint",
                checkpoint="main-menu-0xe2",
                resolution=resolution,
                state="ra2ts-panel-process-lifecycle-sequence",
                requirements=("frames", "music", "pixels", "transition"),
                source_refs=(DESIGN, MAIN_MENU),
                blocker_ids=("catalog:ra2ts-process-lifecycle-comparability",),
                verification_policy=RA2TS_CURRENT_POLICY,
            )
        )

        for checkpoint, source in POINTER_OWNERS:
            for state in POINTER_STATES:
                rows.append(
                    _row(
                        family="pointer",
                        checkpoint=checkpoint,
                        resolution=resolution,
                        state=state,
                        requirements=("cursor", "frames", "input", "pixels", "ui-sound"),
                        source_refs=(DESIGN, source),
                    )
                )

        for state in INPUT_STATES:
            rows.append(
                _row(
                    family="input",
                    checkpoint="owned-shell-route",
                    resolution=resolution,
                    state=state,
                    requirements=("cursor", "focus", "frames", "input", "text"),
                    source_refs=(DESIGN, SKIRMISH_INPUT),
                )
            )

        for state in CONTROL_STATES:
            source = RANDOM_MAP if state.startswith("random-map") else SKIRMISH_MODEL
            if state.startswith("choose-map"):
                source = CHOOSE_MAP
            elif state.startswith("saved-seed"):
                source = SAVED_SEED
            rows.append(
                _row(
                    family="control",
                    checkpoint="owned-shell-controls",
                    resolution=resolution,
                    state=state,
                    requirements=("cursor", "frames", "input", "pixels", "text", "ui-sound"),
                    source_refs=(DESIGN, source),
                )
            )

        for state in TRANSITIONS:
            rows.append(
                _row(
                    family="transition",
                    checkpoint="owned-shell-route",
                    resolution=resolution,
                    state=state,
                    requirements=(
                        "cursor",
                        "frames",
                        "input",
                        "music",
                        "pixels",
                        "route",
                        "transition",
                        "ui-sound",
                    ),
                    source_refs=(DESIGN, START_LOADING, VALIDATION_MODAL),
                )
            )

        for state in AUDIO_STATES:
            rows.append(
                _row(
                    family="audio",
                    checkpoint="owned-shell-route",
                    resolution=resolution,
                    state=state,
                    requirements=("frames", "music", "transition", "ui-sound"),
                    source_refs=(DESIGN, SKIRMISH_INPUT),
                )
            )

        for loading_path, state in LOADING_CADENCE_STATES:
            rows.append(
                _row(
                    family="loading-cadence",
                    checkpoint="stock-loading",
                    resolution=resolution,
                    state=state,
                    variant={"loading_path": loading_path},
                    requirements=("frames", "loading", "pixels", "transition"),
                    source_refs=(DESIGN, LOADING_REPAINT, LOADING_SEQUENCE),
                )
            )

        loading_blockers = (
            "catalog:loading-country-art-palette-map",
            "catalog:loading-first-renderer-localized-text",
            "catalog:loading-player-color-pixel-map",
        )
        for art_variant in LOADING_ART_VARIANTS:
            for color in PLAYER_COLORS:
                rows.append(
                    _row(
                        family="loading-branch",
                        checkpoint="stock-loading",
                        resolution=resolution,
                        state="initial-and-steady-paints",
                        variant={
                            "color_id": color["id"],
                            "color_key": color["key"],
                            "country_id": art_variant["country_id"],
                            "country_key": art_variant["country_key"],
                            "loading_art_variant": art_variant["key"],
                            "side": art_variant["side"],
                        },
                        requirements=(
                            "cursor",
                            "frames",
                            "loading",
                            "pixels",
                            "text",
                            "transition",
                        ),
                        source_refs=(
                            DESIGN,
                            LOADING_BAR,
                            LOADING_FIRST,
                            LOADING_SETUP,
                            COLOR_TABLE,
                            RULESMD,
                        ),
                        blocker_ids=loading_blockers,
                    )
                )

        rows.append(
            _row(
                family="first-tactical",
                checkpoint="first-playable-tactical-frame",
                resolution=resolution,
                state="first-frame-after-loading-completes",
                requirements=("cursor", "frames", "pixels", "route", "transition"),
                source_refs=(DESIGN, FIRST_TACTICAL, START_LOADING),
            )
        )

    rows.sort(key=lambda row: row["id"])
    return rows


def build_blockers() -> list[dict[str, object]]:
    result = []
    for blocker in BLOCKERS:
        result.append(
            {
                **blocker,
                "evidence_id": None,
                "status": "UNKNOWN",
            }
        )
    result.sort(key=lambda item: item["id"])
    return result


def catalog_snapshot() -> dict[str, object]:
    """Return the evidence-independent catalog material used for hashing."""

    return {
        "blockers": build_blockers(),
        "countries": list(COUNTRIES),
        "loading_paths": list(LOADING_PATHS),
        "loading_art_variants": list(LOADING_ART_VARIANTS),
        "player_colors": list(PLAYER_COLORS),
        "resolutions": list(RESOLUTIONS),
        "rows": build_rows(),
        "scope_exclusions": list(SCOPE_EXCLUSIONS),
        "sources": list(SOURCES),
    }
