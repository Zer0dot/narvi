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

## Configuration (TOML)

`~/.config/narvi/config.toml` — seeded with presets on first run (Default, Gaming,
Movie, Photo, Night) and **hot-reloaded on save**. The generated shader lands at
`~/.config/hypr/shaders/narvi.frag`. Full schema in `PROTOCOL.md`.

```toml
[general]
active_profile = "default"   # fallback when no saved state exists
restore_on_start = true      # restore last state (params/profile/on-off) at login

[hyprland]
shader_path = "~/.config/hypr/shaders/narvi.frag"

[scheduling]
enabled = true
mode = "sun"                 # "sun" | "fixed" | "off"
latitude = 40.71             # sun mode
longitude = -74.01
day_time = "07:30"           # fixed mode only, HH:MM
night_time = "21:00"
day_profile = "Default"
night_profile = "Night"
transition_minutes = 30      # smooth blend window around each boundary

[ui]
theme = "saturn"
accent = "#E0A23C"           # GUI accent override
preview = "sample"           # "sample" | "gradient"
preview_image = ""           # optional path replacing the preview image

[[profiles]]                 # ranges: PROTOCOL.md; out-of-range values clamp
name = "gaming"
vibrance = 1.5               # 0.0-2.0, protective (spares skin tones)
saturation = 1.1             # 0.0-2.0, flat
temperature = 6500           # 1000-10000 K
brightness = 1.0             # 0.5-1.5
contrast = 1.05              # 0.5-2.0
gamma = 1.0                  # 0.5-2.0
rgb = [1.0, 1.0, 1.0]        # per-channel gain, 0.0-2.0
match = ["steam_app_*", "gamescope"]   # auto-switch: window-class globs
```

Per-app auto-switch: focusing a window whose class matches a profile's `match`
globs applies that profile; focusing away reverts. Scheduling blends day↔night
profiles over `transition_minutes`. Unknown keys warn and are ignored.

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
