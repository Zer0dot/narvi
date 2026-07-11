# Narvi

Real-time color-management suite for [Hyprland](https://hyprland.org) — digital vibrance,
temperature, brightness/contrast, gamma, and per-channel RGB, driven by a single generated
`decoration:screen_shader`. The Wayland answer to nvidia-settings' Digital Vibrance, but a
whole color pipeline.

> Narvi is a moon of Saturn; part of a Saturn-themed family.

## Components

- **`narvid`** — daemon: owns state, generates/hot-reloads the shader, day/night
  scheduling, per-app auto-switch (window-class globs), config hot-reload.
- **`narvi`** — CLI client (drives Hyprland keybinds and scripting).
- **`narvi-gui`** — egui dashboard, Saturn theme, live preview through the real pipeline.
- **`narvi-tray`** — StatusNotifierItem; click toggles the GUI, menu applies presets.

## Usage

```sh
narvid &                          # or the systemd user service (see below)
narvi status
narvi set vibrance 1.3
narvi nudge temperature -- -300
narvi profile gaming              # load; also: list / save / delete / next / prev
narvi toggle                      # shader on/off
narvi-gui                         # dashboard
```

`narvi-gui` and `narvi-tray` auto-start `narvid` if the daemon stays unreachable
for a few seconds (rate-limited; gives up after repeated failures); the `narvi`
CLI never does. When the `narvi.service` systemd user unit exists they start it
via `systemctl --user start` so the daemon stays supervised; otherwise they exec
`narvid` detached. Set `NARVI_AUTOSPAWN=0` to opt out (e.g. so
`systemctl --user stop narvi` sticks); home-manager users can set
`programs.narvi.autospawn = false` instead. A file lock next to the socket
guarantees a single daemon instance either way.

Config lives at `~/.config/narvi/config.toml` (seeded with presets on first run:
Default, Gaming, Movie, Photo, Night) and hot-reloads on save. The generated shader
lands at `~/.config/hypr/shaders/narvi.frag`. See `PROTOCOL.md` for the full schema
and socket protocol.

Per-app auto-switch: give a profile `match = ["steam_app_*", "mpv"]` and focusing a
matching window applies it; focusing away reverts. Scheduling: `mode = "sun"` with
`latitude`/`longitude` (or `mode = "fixed"` with `day_time`/`night_time`) blends
between `day_profile` and `night_profile` over `transition_minutes`.

## Install

### Nix flake + home-manager

```nix
# flake input
inputs.narvi.url = "github:zer0dot/narvi";

# home-manager
imports = [ inputs.narvi.homeManagerModules.narvi ];
programs.narvi.enable = true;   # daemon service, tray, default keybinds
```

One-off: `nix run github:zer0dot/narvi#narvi -- status` · `nix build .#narvi`.

Default keybinds (disable with `programs.narvi.hyprlandKeybinds = false`):
SUPER+SHIFT N toggle · V/B vibrance ± · M next profile · G GUI.

### Cargo

```sh
cargo install --path crates/narvi-core --locked   # lib (dep of the rest)
cargo install --path crates/narvi --locked
cargo install --path crates/narvid --locked
cargo install --path crates/narvi-gui --locked
cargo install --path crates/narvi-tray --locked
```

### AUR

`PKGBUILD` in-repo; `makepkg -si` from a release tarball checkout.

## Development

```sh
cargo build && cargo test
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p narvid              # needs a live Hyprland session
nix develop                      # NixOS: provides the GUI's runtime libs
```

On NixOS, run the GUI from `nix develop` (it sets `LD_LIBRARY_PATH` for
wayland/libxkbcommon/vulkan); the packaged build is rpath-patched instead.

## Notes

- `decoration:screen_shader` is global — no per-monitor color (DRM CTM is not honored
  by the NVIDIA proprietary driver).
- Narvi owns temperature: don't run `hyprsunset`/`gammastep` simultaneously, they
  double-apply.
- Brief artifacts on window close are a known Hyprland issue (#8561), not Narvi.

## License

MIT
