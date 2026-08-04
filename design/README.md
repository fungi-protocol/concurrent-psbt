# Design — Tab

Derived from the claude.ai design project [Concurrent PSBT Construction](https://claude.ai/design/p/a23b3a37-4d9d-4fcb-bc8d-30d94179325d?file=Tab.dc.html), restyled to use only open assets: **Inter** and **IBM Plex Mono** via Google Fonts (OFL) and inline [Lucide](https://lucide.dev) icons (ISC). No proprietary fonts, icons, or design-system code — both files can be shared or hosted anywhere.

`tab.dc.html` is a five-step product walkthrough of **Tab** — group bill splitting where
everyone chips into one shared PSBT and a single payment goes out. Each phone mock is
paired with a plain-language explanation plus an "under the hood" note mapping the UX to
the concurrent-PSBT mechanics (unordered drafts, CRDT-style merging, unique-ID conflict
surfacing, canonical sort before broadcast).

`tab-prototype.html` is the screens-only, clickable version — a mobile-first app you
tap through as if it were real. On a phone it fills the screen (safe-area aware, no
device chrome); on desktop it presents as a centered phone-sized shell. The flow:
Join → Invite (the table fills in live) → Live tab → Chip in — your only tap — then
the chips land and the tab settles itself… interrupted once by the scripted mismatch
moment (Dev's share changes → "Take a fresh look" → re-confirm) before it settles to
Paid. Plain HTML/JS (no React), fully self-contained; the QR and fingerprint generators
are ported from `tab.dc.html` so the visuals match.

## Viewing

Open either file directly in a browser — no server needed. Both load fonts from Google
Fonts, and the walkthrough additionally pulls React 18 + Babel from unpkg via
`support.js`, so an internet connection is required.

```sh
open design/tab-prototype.html   # clickable prototype
open design/tab.dc.html          # annotated walkthrough
```

## Layout

| Path | What it is |
|---|---|
| `tab.dc.html` | The design document (canvas mode, 6 phone screens with annotations) |
| `tab-prototype.html` | Mobile-first clickable prototype (self-contained) |
| `support.js` | dc-runtime — parses/renders `.dc.html` documents |
