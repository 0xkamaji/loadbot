# Standalone Loadbot GUI handoff

Build the Loadbot menu as the full application window, using the supplied black-and-beige assets. The optional terminal opens along the bottom. Loadbot may appear as a small static header mascot.

This package contains artwork, reference screenshots, reusable CSS, and implementation instructions. It is not the Loadbot application or backend.

## Give this to Codex

The handoff is at `src/gui/loadbot-gui-assets/` in the Loadbot repository. Preserve this directory and its structure. Paths in this document are relative to that directory. Open `CODEX-PROMPT.md` and give Codex that prompt. Start with `references/01-main-window.png` for the intended composition.

## What is where

| Folder / file | Purpose |
|---|---|
| `assets/ui/buttons/` | Blank default, hover, pressed, selected, disabled button PNGs |
| `assets/ui/frames/` | Window, title bar, panel skins |
| `assets/ui/inputs/` | Input skins and transparent focus ring |
| `assets/ui/menus/` | Hover and selected menu row skins |
| `assets/ui/icons/`, `icons-inverse/` | Separate icons for contrasting surfaces |
| `assets/ui/checkboxes/` | Fixed-size checkbox art |
| `assets/ui/slices/` | Nine separate pieces per resizable UI skin |
| `assets/terminal/` | Beige terminal panel and its slices |
| `assets/branding/` | Full transparent Loadbot mascot and compact header image |
| `fonts/` | Bundled monospace font and license |
| `references/` | Your two supplied images and interpretation notes |
| `spec/asset-manifest.json` | Exact sizes, paths, slice margins, colors, and hashes |
| `spec/` | Layout, reusable components, backend integration, acceptance checks |
| `examples/` | Loadbot-only CSS and a static HTML skin example |

Open `examples/skin-example.html` in a browser to inspect the actual PNGs on real HTML controls. It needs no install or server. Its controls are illustrative and do not execute commands.

New buttons and menu items should be instances of reusable components with real text, not newly generated images. All existing assets are copied into this package; no files in the earlier combined packs are required.
