# AGENTS.md — Narvi

Read this first, then SPEC.md (what), PROTOCOL.md (contracts), HYPRLAND.md (integration +
gotchas), BUILD_PLAN.md (order + acceptance), THEME.md (visual style). Build strictly in
the BUILD_PLAN milestone order; each milestone has acceptance checks that are your
definition of done.

## What this is
A real-time color-management suite for Hyprland (digital vibrance + temperature +
brightness/contrast + gamma + RGB), via a generated `decoration:screen_shader`. Four
binaries over one Unix socket: daemon (`narvid`), CLI (`narvi`), GUI (`narvi-gui`), tray.

## Repo layout
```
narvi/
├── Cargo.toml                # workspace
├── crates/
│   ├── narvi-core/           # ColorParams, shader gen, kelvin_to_rgb, config, profiles
│   ├── narvid/               # daemon: socket, apply, scheduler, socket2 auto-switch, tray
│   ├── narvi/                # CLI
│   └── narvi-gui/            # egui app
├── shaders/narvi.frag        # reference shader (PARAMS block regenerated at runtime)
├── flake.nix                 # package + home-manager module
├── SPEC.md PROTOCOL.md HYPRLAND.md BUILD_PLAN.md THEME.md
└── LICENSE                   # MIT
```
Keep contracts in `narvi-core`; the three front-ends depend on it and never duplicate
param logic.

## Stack & dependencies (use latest compatible)
- `eframe` + `egui` (GUI), `egui_extras` for the image preview
- `ksni` (tray / StatusNotifierItem)
- `tokio` (daemon: socket + timers + socket2 stream)
- `serde` + `serde_json` (socket) + `toml` (config)
- `clap` (CLI)
- `sunrise` (sun times), `chrono` (scheduling)
- `globset` (class match — NOT regex), `notify` (config watch)
- `directories` (XDG paths), `anyhow` (bins) + `thiserror` (lib)

## Conventions
- Comments state **what + why**, briefly. No essays, no history, no rejected-alternative
  narration. Prefer no comment when the code is self-evident.
- `narvi-core` returns typed errors (`thiserror`); binaries use `anyhow` at the edges.
- All param mutation clamps to PROTOCOL.md ranges — clamp in `narvi-core`, not per-caller.
- Shader writes are atomic (`.tmp` + `rename`); always re-issue `hyprctl keyword` after.
- No `unwrap()`/`expect()` on runtime paths (sockets, IO, hyprctl); handle and log.

## Build / test / run
```sh
cargo build
cargo clippy -- -D warnings
cargo fmt --check
cargo test                       # core unit tests (shader gen, kelvin, clamping)
cargo run -p narvid              # run the daemon (needs a live Hyprland session)
cargo run -p narvi -- set vibrance 1.3
cargo run -p narvi-gui
nix build .#narvi                # packaged build
```
The shader-applies-and-compiles check (M1) and the live slider check (M5) require a real
Hyprland session — they cannot be verified headless. Note that in any handoff.

## Non-negotiables
- GLSL `#version 300 es`, `in`/`out`/`fragColor`/`texture()` (see HYPRLAND.md landmines).
- Generated shader lives in `~/.config`, never the Nix store.
- `decoration:screen_shader` is global; do not promise per-monitor color.
- Daemon is the single source of truth; clients are thin and subscribe for updates.
