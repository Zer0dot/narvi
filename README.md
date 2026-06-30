# Narvi

Real-time color-management suite for [Hyprland](https://hyprland.org) — digital vibrance,
temperature, brightness/contrast, gamma, and per-channel RGB, driven by a single generated
`decoration:screen_shader`. The Wayland answer to nvidia-settings' Digital Vibrance, but a
whole color pipeline.

> Narvi is a moon of Saturn; part of a Saturn-themed family.

## Components

- **`narvid`** — daemon: owns state, generates/hot-reloads the shader, scheduling, per-app auto-switch.
- **`narvi`** — CLI client (drives Hyprland keybinds and scripting).
- **`narvi-gui`** — egui dashboard with live preview.
- **`narvi-tray`** — `ksni` tray; click toggles the panel.

## Status

Pre-1.0, under active construction. See `BUILD_PLAN.md` for milestones.

## Build

```sh
cargo build
cargo test
cargo run -p narvid          # needs a live Hyprland session
cargo run -p narvi -- set vibrance 1.3
```

## Install

`cargo install narvi` · AUR `narvi` · Nix flake (`nix run`, home-manager module) — see M8.

## License

MIT
