# Implementation acceptance

These are checks for the implementing Codex session, not assertions that an application has already been built.

- App launches directly into the Loadbot menu. No room scene, roaming character, Back to room, or Rot navigation appears.
- Compare the running GUI with both references: black/beige palette, pixel borders, project sidebar, shortcuts/details, bottom toolbar, and optional beige terminal drawer.
- Use supplied asset files unchanged; borders keep their corner geometry when resized. All labels are real text.
- Add a catalog entry and confirm it uses existing components without new artwork or custom per-entry layout.
- Confirm project and shortcut selection, required input validation, picker behavior, refresh, empty/error states, and valid Run transitions using the actual backend.
- Verify real output and real exit/failure status; do not report success for simulated operations. Check a long-running shortcut for UI responsiveness.
- Verify keyboard navigation, activation, visible focus, disabled controls, and readable long labels/paths.
- Check initial desktop size and a smaller viewport, including a long project/shortcut list and an open drawer. No critical control becomes unreachable.
- Verify Open project folder uses the selected project. Confirm unsupported actions are explained instead of silently doing nothing.
- If a terminal is connected, verify shell input, output, resizing, close/reopen behavior, and cleanup. Otherwise state that it remains unconnected.
- Document launch instructions, chosen stack, backend adapter points, and remaining limitations.

Handoff package validation is recorded separately in package-validation.json. It covers asset integrity and paths, not application behavior or browser screenshots.
