# Narvi — Build Plan & Acceptance Criteria

Build in this order. Each milestone is independently verifiable — do not start the next
until the current one's checks pass. "Done" = every box in that milestone is true.

---

## M0 — Scaffold
- Cargo workspace with crates: `narvi-core` (lib), `narvid` (daemon), `narvi` (CLI),
  `narvi-gui` (egui app). Tray lives in `narvid` (or a `narvi-tray` bin).
- `flake.nix` (package + HM module stub), `LICENSE` (MIT), `README.md`, `.gitignore`,
  `AGENTS.md`, `rustfmt.toml`, CI workflow (fmt + clippy + test).
- ✅ `cargo build` and `cargo clippy` clean; `nix build` produces all binaries.

## M1 — Core: params + shader generation (`narvi-core`)
- `ColorParams` (PROTOCOL.md) with clamping setters and `Default`.
- `kelvin_to_rgb` (Tanner Helland), normalized to neutral at 6500 K.
- `render_shader(&ColorParams) -> String` that fills the PARAMS block of `shaders/narvi.frag`.
- ✅ Unit tests: neutral params reproduce `vec3(1.0)` white balance; 4000 K is warm
  (R>B); generated shader contains expected constants; out-of-range inputs clamp.
- ✅ Manual: write a 1.4-vibrance shader, apply with `hyprctl keyword`, confirm it
  **compiles** and the screen visibly changes. (This is the single most important check.)

## M2 — Daemon + socket (`narvid`)
- Listen on `$XDG_RUNTIME_DIR/narvi/narvid.sock`; implement every command in PROTOCOL.md.
- Atomic shader write + `hyprctl keyword` apply on change. `off` → `[[EMPTY]]`.
- Load/save `config.toml`; `restore_on_start`.
- Subscriber push of `state` events.
- ✅ `socat`/`nc` a `set` request → response matches schema and the screen changes.
- ✅ Two clients: one subscribes, the other sets — subscriber receives the `state` event.

## M3 — CLI (`narvi`)
- All subcommands in PROTOCOL.md over the socket; `--json`, `--socket`; clear error when
  daemon down.
- ✅ `narvi set vibrance 1.3` then `narvi get vibrance` → `1.3`; `narvi off`/`on` toggle
  the shader; `narvi profile list` works.

## M4 — Profiles + presets
- TOML profile array; built-in presets seeded on first run: Default, Gaming, Movie,
  Photo, Night.
- `profile.load/save/delete/next/prev`.
- ✅ Save a tweak as "test", load Default, reload "test" → params return; presets present
  on a fresh config.

## M5 — GUI (`narvi-gui`)
- egui dashboard: live-preview swatch (sample image, real pipeline math), line-sliders +
  numeric fields per control, profile picker, Reset/Save. Saturn theme (THEME.md).
- Talks to daemon; subscribes so external changes move the sliders.
- ✅ Dragging a slider changes the screen live; loading a profile updates all controls;
  a CLI `set` from another terminal moves the GUI slider.

## M6 — Tray
- `ksni` StatusNotifierItem; **click toggles the panel**; menu = presets + Quit.
- ✅ Tray icon appears in waybar; click shows/hides the GUI; preset menu items apply.

## M7 — Scheduling + auto-switch
- Sun-based scheduler (sunrise/sunset from lat/long or geoclue); smooth interpolation
  between day/night profiles over `transition_minutes`.
- Hyprland socket2 listener; class-glob auto-switch with revert-to-manual.
- ✅ Force a near-term transition → params blend over time; focusing a `match`ing window
  switches profile and unfocusing reverts; both emit `state` events.

## M8 — Packaging
- `flake.nix`: package + HM module (installs `narvid` user service, default Hyprland
  keybinds, tray autostart, seeds config). `cargo install` works. AUR `PKGBUILD`.
- ✅ `nix run`, `cargo install --path .`, and a clean HM activation all yield a working
  setup; `systemctl --user status narvi` is active.

---

## Stretch (post-v1)
- Desktop freeze-frame preview (capture once with shader off).
- Animated orbital sprite easter-egg.
- Click-to-bind window picker; title-regex matching.

## Definition of done (v1)
All of M0–M8 green, `cargo clippy -- -D warnings` clean, `cargo fmt --check` clean,
README documents install for all three channels, and a fresh Hyprland login restores the
last profile automatically.
