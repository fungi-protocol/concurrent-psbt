# Arcade Design System

Arcade is Cash App's internal design system — the component library, design tokens, iconography, and illustration set that power the Cash App mobile product and surrounding surfaces.

This project is a distilled, web-renderable version of Arcade, built from the attached Figma files and Cash Sans font package. It contains brand tokens, typography, core icons + illustrations, and a mobile UI kit of high-fidelity components.

## Sources

- Figma: **01 Arcade - Illustrations** (3 pages, 10 frames)
- Figma: **01 Arcade - Tokens** (11 pages — Color, Typography, Border-radius, Layout, Motion, etc.)
- Figma: **01 Arcade - Assets** (5 pages — 1074 icons, 218 country flags, 6 avatars)
- Figma: **02 Arcade - Mobile** (56 pages — Badge, Button-cta, Cell, Card, Bottom-navigation, Sheet, Toast, Modal, Title-bar-core, etc.)
- Fonts: Cash Sans family (9 weights + italics), Cash Sans Mono
- GitHub: `squareup/cash-design-system` (available if connected)

## Index

| Path | What it is |
|---|---|
| `README.md` | This file — overview, brand fundamentals, iconography |
| `SKILL.md` | Agent Skill manifest |
| `colors_and_type.css` | All CSS variables — colors, type, spacing, radii, motion, dark mode |
| `fonts/` | Cash Sans + Cash Sans Mono OTFs |
| `assets/icons/` | Core 24px icon set (SVG) |
| `assets/illustrations/` | Cash App illustration set (SVG, light + dark variants) |
| `preview/` | Design-system card files rendered in the Design System tab |
| `ui_kits/mobile/` | Mobile UI kit — high-fi iOS/Android click-thru prototype |

---

## Content Fundamentals

**Voice.** Cash App copy is short, direct, and plainspoken. Sentences fit on a line. No marketing adjectives, no "unlock," no "seamless," no "powerful." Labels are nouns ("Savings," "Bitcoin," "Paychecks"); actions are verbs ("Send," "Request," "Deposit," "Cash Out").

**Person.** Usually **you** — never "we" in product surfaces. System messages use the third person ("Deposit pending," not "Your deposit is pending").

**Casing.** Sentence case for headers, titles, buttons, and most UI. Proper nouns stay capitalized ("Bitcoin," "Afterpay," "Cash Card"). Uppercase is reserved for `.t-label` micro-labels and legal/metadata strips.

**Numbers.** Money is the hero. Large display numerics set in Cash Sans Regular — the zero and 1 have distinctive shapes ("$1,234.56"). Currency code follows the amount for non-USD ("£20 GBP").

**Punctuation.** Periods are dropped at the end of short UI labels and buttons. Kept in sentences inside modals and cells.

**Tone.** Friendly, a little dry, occasionally playful — but never goofy. The illustrations carry the weirdness; the copy stays level. Examples: "Got it," "That's it!", "Something went wrong. Try again." Error states never blame the user.

**Emoji.** Not used in product UI. The illustration set carries personality instead.

---

## Visual Foundations

### Palette

- **Primary brand green** `#00D64F` (also `#00E013` for emphatic accents). Used for: primary CTAs, brand mark, status success, progress fills.
- **Signal lime** `#CCFF14`. A pop color found throughout illustrations and marketing surfaces — never body UI.
- **Sky blue** `#59CBFF`, **Purple** `#9747FF`, **Red** `#D7040E`, **Orange** `#CC4B03`. Limited use — categorical accents and destructive states.
- **Neutrals**: pure black `#000` and white `#FFF` are the foundation. Grays step 50 → 1000 in consistent pairs (`#F2F2F2`, `#E8E8E8`, `#666`, `#232323`).
- **Light/dark parity**: every surface has a dark-mode counterpart. Dark mode is true black (`#000` app background), not gray.

### Type

Cash Sans at all sizes. Three uses:

1. **Display numerics** — Cash Sans Regular (400) at 44–72px, tight tracking. The app's hero moment on balance/amount screens.
2. **Titles** — Cash Sans Medium (500) at 20–32px, slightly tight tracking.
3. **Body** — Cash Sans Regular (400) at 14–17px.

**CashMarket** (a rounded sibling family) is used specifically on button labels per component spec. We ship Cash Sans Medium as a fallback — flag to the user if CashMarket OTFs are provided.

### Backgrounds

Pure white or pure black — no gradients, no textures, no patterns. Depth comes from stacked surfaces (`bg-surface` on `bg-subtle`), not shadows or borders. Full-bleed illustrations and photography are the exception: marketing surfaces hand the whole frame to an illustration on a solid color block.

### Corners

Pills for buttons (`9999px`). Generous radii elsewhere: cards `16–24px`, modals `24–32px`, sheets `24px top-only`, chips `12–16px`. No sharp corners on interactive elements.

### Borders & Shadows

- Borders are 1px, low-contrast (`#E8E8E8` on light, `#333` on dark). Used sparingly — mostly on inputs.
- Shadows exist but are subtle — `0 4px 12px rgba(0,0,0,0.06)` is the standard card elevation. Sheets use an upward shadow (`0 -8px 32px rgba(0,0,0,0.12)`).
- Separation is more often achieved with background-color steps than shadows.

### Motion

- **Easing**: `cubic-bezier(0.22, 1, 0.36, 1)` (ease-out) for enters; `cubic-bezier(0.64, 0, 0.78, 0)` (ease-in) for exits. Standard `cubic-bezier(0.4, 0, 0.2, 1)` for moves.
- **Durations**: 120ms (micro), 200ms (standard), 320ms (sheet/modal).
- Bounces are reserved for success moments (e.g. the check mark after a send completes).
- Sheets swipe up from the bottom. Modals fade + scale from 96% → 100%.

### States

- **Hover** (web): 8% darker/lighter on the same surface. Never opacity-based.
- **Pressed** (mobile): background shifts to next inset step (`bg-hover` → `bg-pressed`) AND a subtle 98% scale. No color tint.
- **Focus** (keyboard): 2px outline in `--accent` with 2px offset.
- **Disabled**: 38% opacity on label, no interaction response.

### Layout

- Mobile canvas: 375×812 base, safe areas respected.
- Spacing scale: 2, 4, 8, 12, 16, 20, 24, 32, 40, 48, 64. No half-steps.
- Page horizontal padding: 16px mobile, 24px tablet, 32px+ desktop.
- Tab bar height 83px (iOS) / 64px (Android). Title bar 44–56px.

### Transparency & Blur

Used surgically — only for iOS-style backdrops: navigation bars over scrolling content (`backdrop-filter: blur(20px)` with 80% surface tint), and dimmers behind modals (`rgba(0,0,0,0.4)`).

### Cards

Rounded (`16–24px`), solid fill (`--bg-surface`), optional 1px subtle border, optional small shadow. Never both heavy border + heavy shadow. Interior padding is 16 or 20.

---

## Iconography

The Cash App icon set is extensive (~1000+ icons per the Figma metadata) and ships at **three sizes: 16 / 24 / 32 px**. Every icon has a matching size variant. Many come in both outline and `Fill` variants (e.g. `alert24.svg` vs `alertFill24.svg`). Navigation icons live under the `navigation*` prefix and are sized for tab bars / nav chrome specifically.

Style is **geometric, monoline, rounded corners**. Stroke weight is visually consistent across the set. Icons are monochromatic — color comes from CSS `currentColor` or an `overrideFill` prop at the component level, not baked into the SVG.

**What's copied into this project:** a curated ~60 icon starter set in `assets/icons/` at 24px (plus a handful at 16 for carets). The full 1000+ set exists in the Figma file and should be copied over on demand; the naming convention (`<name><size>.svg`) makes bulk grabs straightforward.

**Usage**:
- Nav tab bars → `navigation*.svg` (filled, 28–32px)
- Cells / list rows → `*24.svg` outline, `--fg-secondary`
- Inline in text → `*16.svg`, `currentColor`
- Hero / empty states → illustrations (`assets/illustrations/`), NOT scaled-up icons

**No emoji, no unicode symbols, no Material Icons**. If an icon is missing, request it — do not substitute or redraw.

**Illustrations** are a separate, more expressive system. Spot illustrations are ~64×64 to 120×120, full scene illustrations are 240×240+. Every illustration has a `-dark` variant (`cash-stack.svg` + `cash-stack-dark.svg`). They carry the brand's personality — the cash-smiley, the rainbow, the unicorn, the bitcoin-lock — and are used on empty states, onboarding hero, messaging cards, and success screens.

---

## Caveats

- **CashMarket font** is not in the provided upload. Button labels use Cash Sans Medium as a substitute — supply the real CashMarket OTFs to restore exact fidelity.
- Only a subset of the 1000+ icon set is copied into `assets/icons/`. The naming scheme is predictable (`<name><16|24|32>.svg`) — request more in bulk if needed.
- I did not have access to web dashboard / merchant-side surfaces, so the UI kit is **mobile only**. If Cash for Business or the web dashboard should be covered, attach that Figma and I'll add it.
