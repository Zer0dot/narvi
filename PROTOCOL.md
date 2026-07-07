# Narvi — Protocol, CLI & Config Contracts

The contracts that bind `narvid`, `narvi` (CLI), the GUI, and the tray. Pin these;
do not improvise divergent shapes.

---

## Param model

The single source of truth for color state is `ColorParams`:

| Field | Type | Default | Valid range | Notes |
| --- | --- | --- | --- | --- |
| `vibrance` | f32 | 1.0 | 0.0 – 2.0 | protective saturation |
| `saturation` | f32 | 1.0 | 0.0 – 2.0 | flat saturation |
| `temperature` | u32 | 6500 | 1000 – 10000 | Kelvin; daemon converts to white-balance vec3 |
| `brightness` | f32 | 1.0 | 0.5 – 1.5 | multiplicative |
| `contrast` | f32 | 1.0 | 0.5 – 2.0 | pivot 0.5 |
| `gamma` | f32 | 1.0 | 0.5 – 2.0 | applied as `1/gamma` |
| `rgb` | [f32; 3] | [1.0,1.0,1.0] | 0.0 – 2.0 each | per-channel gain |

```rust
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ColorParams {
    pub vibrance: f32,
    pub saturation: f32,
    pub temperature: u32,
    pub brightness: f32,
    pub contrast: f32,
    pub gamma: f32,
    pub rgb: [f32; 3],
}
```
`Default` = all neutral (table above). All setters clamp to range.

### Shader generation (ColorParams → narvi.frag)

`narvid` renders the PARAMS block of `shaders/narvi.frag` from `ColorParams`:
- `BRIGHTNESS`, `CONTRAST`, `GAMMA`, `SATURATION`, `VIBRANCE` ← direct.
- `RGB_GAIN` ← `vec3(rgb[0], rgb[1], rgb[2])`.
- `WHITEBALANCE` ← `kelvin_to_rgb(temperature)`, normalized so 6500 K → `vec3(1.0)`.

`kelvin_to_rgb` uses the Tanner Helland approximation (compute in Rust):
```
t = kelvin / 100
red   = t <= 66 ? 255 : 329.698727446 * (t-60)^-0.1332047592
green = t <= 66 ? 99.4708025861*ln(t) - 161.1195681661
                : 288.1221695283 * (t-60)^-0.0755148492
blue  = t >= 66 ? 255 : t <= 19 ? 0 : 138.5177312231*ln(t-10) - 305.0447927307
```
Clamp each to [0,255], divide by the value at 6500 K so neutral maps to 1.0.

Write atomically: write to `narvi.frag.tmp`, `fsync`, `rename` over `narvi.frag`,
then issue the hyprctl keyword (see HYPRLAND.md). Never let Hyprland read a half-written file.

---

## Daemon socket protocol

- **Path:** `$XDG_RUNTIME_DIR/narvi/narvid.sock` (Unix domain, `SOCK_STREAM`).
- **Framing:** newline-delimited JSON, one object per line, request→response.
- Clients that want live updates send `subscribe` and then read server-pushed
  `event` lines on the same connection.

### Request
```json
{ "id": 1, "cmd": "set", "args": { "param": "vibrance", "value": 1.3 } }
```
`id` is a client-chosen correlation integer. `args` is command-specific (may be omitted).

### Response
```json
{ "id": 1, "ok": true,  "data": { ... } }
{ "id": 1, "ok": false, "error": "unknown profile: foo" }
```

### Server-pushed event (to subscribers only)
```json
{ "event": "state", "data": <Status> }
```
Emitted after ANY state change — manual, schedule, or auto-switch — so GUI sliders and
the tray always reflect reality.

### Commands

| `cmd` | `args` | `data` returned | Effect |
| --- | --- | --- | --- |
| `status` | — | `Status` | current full state |
| `get` | — | `ColorParams` | current effective params |
| `set` | `{param, value}` | `ColorParams` | set one param live |
| `nudge` | `{param, delta}` | `ColorParams` | relative change, clamped |
| `apply` | `{params}` | `ColorParams` | set the whole set at once |
| `on` / `off` / `toggle` | — | `Status` | enable/disable the shader |
| `profile.list` | — | `[string]` | profile names |
| `profile.load` | `{name}` | `Status` | activate profile |
| `profile.save` | `{name, params?}` | `[string]` | save current (or given) params as name |
| `profile.delete` | `{name}` | `[string]` | remove profile |
| `profile.next` / `profile.prev` | — | `Status` | cycle profiles |
| `subscribe` | — | `Status` | begin receiving `event` lines |
| `reload` | — | `Status` | reload config.toml from disk |

`param` is one of: `vibrance saturation temperature brightness contrast gamma r g b`
(`r`/`g`/`b` index `rgb`).

```rust
pub struct Status {
    pub enabled: bool,
    pub active_profile: Option<String>,   // None = transient/unsaved edits
    pub params: ColorParams,
    pub scheduling: ScheduleStatus,        // mode, next transition, current blend
    pub auto_switch: Option<String>,       // class that triggered current profile, if any
}
```

---

## CLI reference (`narvi`)

Thin client over the socket. Exit non-zero on daemon error.

```
narvi status                      # human summary (or --json)
narvi get [param]                 # all params, or one value
narvi set <param> <value>         # e.g. narvi set vibrance 1.3
narvi nudge <param> <±delta>      # e.g. narvi nudge vibrance +0.05
narvi apply --vibrance 1.3 --temperature 4000 ...   # multiple at once
narvi on | off | toggle
narvi profile <name>              # load
narvi profile list
narvi profile save <name>
narvi profile delete <name>
narvi profile next | prev
narvi reload
narvi gui                         # spawn the GUI (or run `narvi-gui`)
```
Global flags: `--json` (machine output), `--socket <path>` (override default).
If the daemon isn't running, CLI prints a clear error and hints `systemctl --user start narvi`.

---

## Config & profiles (TOML)

`~/.config/narvi/config.toml`. Generated shader → `~/.config/hypr/shaders/narvi.frag`.
See SPEC.md for the full annotated example. Schema:

- `[general]` `active_profile: string`, `restore_on_start: bool`
- `[hyprland]` `shader_path: string` (default `~/.config/hypr/shaders/narvi.frag`)
- `[scheduling]` `enabled: bool`, `mode: "sun"|"fixed"|"off"`, `latitude: f64`,
  `longitude: f64`, `day_profile: string`, `night_profile: string`,
  `transition_minutes: u32`. (`fixed` mode adds `day_time`/`night_time` `"HH:MM"`.)
- `[ui]` `theme: string`, `accent: string`, `preview: "sample"|"gradient"|"desktop"`,
  `preview_image: string` (optional)
- `[[profiles]]` array: `name: string`, the seven `ColorParams` fields, and optional
  `match: [string]` (window classes for auto-switch).

Validation: unknown keys → warn and ignore (forward-compat). Out-of-range values →
clamp + warn. Missing `active_profile` → fall back to the first profile or neutral.
