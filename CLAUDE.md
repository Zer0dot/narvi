# CLAUDE.md — Narvi

> **Status: v1 implemented (M0–M8).** All four binaries work against a live Hyprland
> session. `BUILD_PLAN.md` milestones are the map of what exists; keep changes within
> the contracts in `PROTOCOL.md`.

## Read order

`AGENTS.md` is the canonical entry guide — **read it first.** Then, as needed:

- `SPEC.md` — what Narvi is (goals/non-goals, architecture, features, full config example).
- `PROTOCOL.md` — the binding contracts: `ColorParams`, socket protocol, CLI surface, TOML schema.
- `HYPRLAND.md` — integration cookbook + the GLSL/Hyprland landmines. Read before any shader/render code.
- `BUILD_PLAN.md` — milestone order (M0–M8) with per-milestone acceptance checks = definition of done.
- `THEME.md` — the "Saturn" visual style for the egui GUI.

## What this is

A real-time color-management suite for **Hyprland** — digital vibrance + temperature +
brightness/contrast + gamma + per-channel RGB — driven by a single generated
`decoration:screen_shader`. Four binaries over one Unix domain socket:

- **`narvid`** — daemon: owns state, generates/hot-reloads the shader, scheduling,
  socket2 per-app auto-switch, tray.
- **`narvi`** — CLI (thin socket client; drives Hyprland keybinds).
- **`narvi-gui`** — egui dashboard with live preview.
- **tray** — `ksni` StatusNotifierItem (lives in `narvid` or a `narvi-tray` bin).

## Layout

```
narvi/
├── Cargo.toml                # workspace
├── crates/
│   ├── narvi-core/           # ColorParams, shader gen, kelvin, pipeline, config, client (lib)
│   ├── narvid/               # daemon binary (socket, apply, sched, socket2, watch)
│   ├── narvi/                # CLI binary
│   ├── narvi-gui/            # egui app binary (Saturn theme, wgpu backend)
│   └── narvi-tray/           # ksni tray binary
├── shaders/narvi.frag        # reference shader (copy kept in narvi-core/src, test-synced)
├── flake.nix + nix/          # package + home-manager module
├── PKGBUILD                  # AUR
└── LICENSE                   # MIT
```

Keep all param/contract logic in **`narvi-core`**; the three front-ends depend on it and
never duplicate param logic. The daemon is the single source of truth; clients are thin
and subscribe for updates.

## Stack

Rust + egui. Key crates: `eframe`/`egui` (wgpu backend — glow/EGL is unreliable on
NVIDIA + Wayland) + `image`, `ksni` (tray), `tokio` (daemon), `serde`/`serde_json`/`toml`,
`clap`, `sunrise` + `chrono`, `globset` (class match — NOT regex), `notify`,
`directories`, `anyhow` (bins) + `thiserror` (lib).

## Build / test / run (once scaffolded)

```sh
cargo build
cargo clippy -- -D warnings
cargo fmt --check
cargo test                       # core unit tests (shader gen, kelvin, clamping)
cargo run -p narvid              # daemon — needs a live Hyprland session
cargo run -p narvi -- set vibrance 1.3
cargo run -p narvi-gui
nix build .#narvi
```

The shader-applies-and-compiles check (M1) and the live slider check (M5) require a real
Hyprland session — they cannot be verified headless. Note that in any handoff.

## Conventions & non-negotiables

(See `AGENTS.md` for the full list — these are the load-bearing ones.)

- Build strictly in `BUILD_PLAN.md` milestone order; each milestone's acceptance checks are done-criteria.
- Comments state **what + why**, briefly. No essays. Prefer no comment when self-evident.
- `narvi-core` returns typed errors (`thiserror`); binaries use `anyhow` at the edges.
- All param mutation **clamps to PROTOCOL.md ranges in `narvi-core`**, not per-caller.
- Shader writes are **atomic** (`.tmp` + fsync + `rename`); always re-issue
  `hyprctl keyword decoration:screen_shader` after — Hyprland compiles on *set*, not on file change.
- No `unwrap()`/`expect()` on runtime paths (sockets, IO, hyprctl); handle and log.
- GLSL `#version 300 es` only; `in`/`out`/`fragColor`/`texture()` — never `varying`/`gl_FragColor`/`texture2D`.
- Generated shader lives in `~/.config/hypr/shaders/`, never the Nix store. `decoration:screen_shader` is global (no per-monitor color).

## Communication

- **Always be concise and information-dense.** No preamble, no filler, no restating the
  question. Lead with the answer; cut anything that doesn't add information.

## Git workflow

- **Frequent, short commits.** Commit small, self-contained units of work — don't batch a
  whole milestone into one commit.
- **Conventional-commit style, terse.** `feat: reduce top bar width`, `fix: clamp gamma at edges`,
  `refactor: extract kelvin table`, `chore: add CI`. Lowercase, imperative, one line.

## Rules

1. **Plan before changing**: Always propose a plan before making any code change larger than a trivial fix. Wait for explicit user approval before implementing.
2. **Clarify before planning**: Always eliminate ambiguity and raise any concerns using the AskUserQuestion tool before making a plan. Do not assume — ask.
