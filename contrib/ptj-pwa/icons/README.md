# PWA icons

The manifest references three PNG icons (not committed in this authoring
skeleton; the build/design components supply the actual raster assets):

- `icon-192.png`   — 192x192, `purpose: any`. Home-screen / launcher.
- `icon-512.png`   — 512x512, `purpose: any`. Splash / high-DPI.
- `maskable-512.png` — 512x512, `purpose: maskable`. Safe-zone padded so Android
  adaptive-icon masks (circle/squircle) don't clip the mark.

Also referenced from `index.html` for iOS: `apple-touch-icon` → `icon-192.png`.

Design notes: dark theme (`#0f1115` background per manifest), a simple mark that
reads at small sizes. These are assets, not code; produce them in the design
pipeline and drop them here before the PWA build packages the shell.
