# Paste this into Codex in the Loadbot repository

Implement a standalone Loadbot GUI using this `loadbot-gui` handoff folder as the visual source of truth.

First read START-HERE.md, spec/layout.md, spec/components.md, spec/integration.md, and spec/acceptance.md. Inspect references/01-main-window.png and references/02-interface-kit.png, then consult spec/asset-manifest.json for the actual runtime assets. Inspect the repository's existing instructions, UI stack, Loadbot catalog, and execution APIs before choosing an implementation approach.

The menu is the entire application window. Use near-black panels, warm beige text and outlines, beige selected rows with dark text, the provided pixel frame artwork, and the optional beige terminal drawer at the bottom. Preserve the reference's project sidebar, shortcut list, selected-shortcut details, input/output selectors, Run action, and bottom toolbar. Omit the reference's Back to room button, room scene, roaming characters, Rot navigation, and theme switcher. A small static Loadbot header mascot is appropriate. Use the native window controls when the host provides them.

Use the supplied PNGs; preserve their identity. Build reusable WindowFrame, Panel, Button, IconButton, MenuRow, InputControl, Checkbox, StatusRow, and TerminalDrawer components. Render labels and all catalog content as real text. Use nine-slice scaling from the manifest so corners stay crisp; keep color and font choices centralized. Do not turn screenshots into a full-window image or generate new artwork to approximate the supplied assets.

Reuse the repository's existing UI stack if one exists. Keep backend operations separate from rendering. Populate projects, shortcuts, descriptions, and input controls from Loadbot's real catalog and reuse the same operations as its CLI. The names in the screenshots are illustrative, not a hard-coded feature list. Use native file/folder pickers where supported, keep execution responsive, and display real output and exit status. Distinguish a real PTY terminal from a read-only output log; report any unimplemented terminal capability explicitly.

Implement and validate the GUI in the repository. Follow spec/acceptance.md and compare the running interface with the supplied references. Report what is connected to real Loadbot operations, what remains unavailable, and how to launch it. Do not claim the handoff's static HTML example is a working backend or a terminal.
