# Design — Tab

Imported from the claude.ai design project [Concurrent PSBT Construction](https://claude.ai/design/p/a23b3a37-4d9d-4fcb-bc8d-30d94179325d?file=Tab.dc.html).

`Tab.dc.html` is a five-step product walkthrough of **Tab** — group bill splitting where
everyone chips into one shared PSBT and a single payment goes out. Each phone mock is
paired with a plain-language explanation plus an "under the hood" note mapping the UX to
the concurrent-PSBT mechanics (unordered drafts, CRDT-style merging, unique-ID conflict
surfacing, canonical sort before broadcast).

`Tab-prototype.html` is the screens-only, clickable version: one phone frame you tap
through — Join → Invite (the table fills in) → Live tab → Chip in → the tab settles
itself → Paid. The mismatch exception screen is reachable from the chips under the
phone, which also let you jump anywhere; ←/→ keys navigate too. It's plain
HTML/JS (no React) reusing the same Arcade tokens, fonts, and assets, with the QR and
fingerprint generators ported from `Tab.dc.html` so the visuals match.

## Viewing

The `.dc.html` format is self-contained: `support.js` bootstraps React 18 + Babel from
unpkg and renders the `<x-dc>` template with its `DCLogic` component script. Serve the
folder over HTTP (fonts and SVGs are fetched relative to the page):

```sh
python3 -m http.server 8931 --directory design
open http://127.0.0.1:8931/Tab.dc.html
```

## Layout

| Path | What it is |
|---|---|
| `Tab.dc.html` | The design document (canvas mode, 6 phone screens) |
| `support.js` | dc-runtime — parses/renders `.dc.html` documents |
| `_ds/arcade-design-system-…/` | Arcade (Cash App) design system: tokens CSS, component bundle, Cash Sans fonts |
| `assets/icons/` | 24px monoline icon set (SVG) |
| `assets/illustrations/` | Cash App spot illustrations (SVG) |
