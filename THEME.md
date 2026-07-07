# Narvi — Visual Theme ("Saturn")

A mission-control / observatory HUD: deep navy with amber-gold accents, monospace,
uppercase tracked labels, bracket-corner panel frames. Derived from the reference mockup.

## Palette

| Token | Hex | Use |
| --- | --- | --- |
| `bg` | `#0B0E14` | window background |
| `panel` | `#121722` | panels |
| `card` | `#1A2130` | cards / inset groups |
| `border` | `#2A3447` | thin 1px borders, low opacity |
| `accent` | `#E0A23C` | gold: sliders, active tab, primary buttons |
| `accent_glow` | `#F0B860` | hover/active highlight, glow |
| `text` | `#EDE8DC` | primary (cream) |
| `text_muted` | `#8A93A6` | secondary (gray) |
| `text_dim` | `#5C6678` | labels |

Accent is user-overridable via `[ui].accent`.

## Typography
- Monospace throughout (ship/document a Nerd Font fallback; respect system mono).
- Headers/labels: UPPERCASE, letter-spacing ~0.12–0.15em.
- Body / values: normal case.

## Components
- **Sliders:** thin track (`border`), small round handle (`accent`, `accent_glow` on
  hover/drag), editable numeric field to the right. No value bubble (that was the other
  option). Drag = live apply.
- **Buttons:** primary = filled `accent`, text `bg` (e.g. Save). Secondary = transparent
  with `border` outline, `accent` text (e.g. Reset).
- **Tabs:** inactive `text_muted`; active `text` with a 2px `accent` underline.
- **Panel frame:** bracket corners `⌜ ⌝ ⌞ ⌟` drawn at the four corners (a small painter
  helper), not a full border.
- **Background flourish:** faint radial glow orb(s) + thin orbital-ring linework, very low
  opacity, behind content.
- **Status dots:** small filled circles in `accent`/`text`.

## egui setup (sketch)
```rust
fn saturn_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill        = hex("#121722");
    v.window_fill       = hex("#0B0E14");
    v.extreme_bg_color  = hex("#0B0E14");
    v.faint_bg_color    = hex("#1A2130");
    v.override_text_color = Some(hex("#EDE8DC"));
    v.widgets.noninteractive.bg_stroke = stroke(1.0, "#2A3447");
    v.selection.bg_fill = hex("#E0A23C").linear_multiply(0.35);
    v.selection.stroke  = stroke(1.0, "#E0A23C");
    v.hyperlink_color   = hex("#E0A23C");
    v
}
```
Custom bits (bracket corners, glow orbs, orbital rings, the thin slider, the animated
sprite easter-egg) are immediate-mode `egui::Painter` draws — the reason egui was chosen
over iced. Keep them in a `theme`/`widgets` module so the look is centralized.

## Live preview swatch
A bundled reference image (skin tones, sky, foliage) drawn as an `egui` texture with the
**exact same pipeline math** applied on the CPU (mirror `narvi-core`'s ops), so the
preview matches the real screen output. `[ui].preview_image` overrides the image.
