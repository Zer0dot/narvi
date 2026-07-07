# Narvi — Hyprland Integration Cookbook

Concrete commands, socket formats, and the landmines. Verified against Hyprland 0.55.x
/ aquamarine. When in doubt, shell out to the `hyprctl` binary rather than re-implementing
the IPC — it's stable and version-tracking.

---

## Applying / clearing the shader

```sh
# apply (after regenerating the .frag)
hyprctl keyword decoration:screen_shader ~/.config/hypr/shaders/narvi.frag

# disable (note the literal token — empty string is unreliable)
hyprctl keyword decoration:screen_shader "[[EMPTY]]"
```
Hyprland compiles the shader at the moment the keyword is **set**, not when the file
changes. So the daemon must always re-issue the keyword after rewriting the file. There
is no file watch.

The shader file MUST live under `~/.config` (mutable). It cannot live in the Nix store
(read-only) because the daemon rewrites it constantly.

## Reading state

```sh
hyprctl -j monitors        # JSON array of outputs
hyprctl -j activewindow    # JSON: .class, .title, .address
hyprctl version            # confirm >= 0.55 if gating features
```

## Per-app auto-switch — the event socket (socket2)

Connect and stream events (do NOT poll `activewindow`):

```
path: $XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock
```
Read newline-delimited lines of the form `EVENT>>DATA`. Relevant events:

```
activewindow>>CLASS,TITLE        # focus changed; CLASS is the app-id to match on
activewindowv2>>ADDRESS          # same change, by window address
closewindow>>ADDRESS
```
Parse `activewindow`: split once on `>>`, then split DATA on the FIRST `,` (title may
contain commas). Match `CLASS` against each profile's `match` globs; first match wins.
When focus moves to a window matching nothing, revert to the manually-selected profile
(remember it). Glob match (`steam_app_*`) — use the `glob` or `globset` crate, not regex.

The command socket (for sending commands without the binary) is the sibling
`.socket.sock`; prefer the `hyprctl` binary instead.

## Environment / discovery

- `$HYPRLAND_INSTANCE_SIGNATURE` (a.k.a. HIS) names the running instance; both sockets
  live under `$XDG_RUNTIME_DIR/hypr/$HIS/`.
- If `$HYPRLAND_INSTANCE_SIGNATURE` is unset (e.g. systemd user service started before
  the var is imported), read the newest dir under `$XDG_RUNTIME_DIR/hypr/`, or have the
  HM module pass it in. Fail loud with a clear message if no instance is found.

---

## Landmines (read before writing shader/render code)

1. **GLSL version:** `#version 300 es` only. `320 es` is rejected
   ("Supported versions are: 1.00 ES, and 3.0 ES"). Use `in`/`out`/`fragColor`/`texture(...)`,
   never `varying`/`gl_FragColor`/`texture2D`.
2. **Global, not per-monitor:** `decoration:screen_shader` applies to ALL outputs. There
   is no per-monitor shader. Per-output color would need DRM CTM, which the NVIDIA
   proprietary driver ignores — out of scope.
3. **Read-only Nix store:** the generated `.frag` must be in `~/.config`, never `/nix/store`.
4. **Set, don't reload:** editing the file or `hyprctl reload` does not recompile the
   shader reliably; re-issue the `keyword` command.
5. **Atomic writes:** write `.tmp` + `rename` so Hyprland never reads a partial file.
6. **`hyprsunset` / `gammastep` conflict:** Narvi owns temperature via the shader. Running
   hyprsunset/gammastep simultaneously double-applies. Document; optionally detect and warn.
7. **Cosmetic artifact:** screen shaders can briefly glitch on window close/manipulation
   (upstream issue #8561). Not a Narvi bug; don't chase it.
8. **Preview double-application:** a screencopy of the desktop already has the active
   shader baked in. The sample-image preview avoids this (runs the math on a clean image).
   Any desktop-capture preview must capture with the shader temporarily disabled.
