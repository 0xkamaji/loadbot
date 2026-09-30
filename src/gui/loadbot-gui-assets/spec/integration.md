# Loadbot integration boundary

The handoff has no repository backend or executable Loadbot code. Inspect the actual repository before implementing these connections. These are responsibilities, not claims that methods with particular names already exist.

| GUI action | Existing backend responsibility to locate |
|---|---|
| Project menu / Refresh catalog | Discover and read the current catalog |
| Shortcut menu / details | Read shortcut metadata and supported input schema |
| Add project | Use the existing registration flow and persistence |
| Choose folder/file | Host picker producing a validated path for the backend |
| Output destination | Read/set it only when that shortcut supports it |
| Run shortcut | Invoke the same operation as CLI with structured inputs |
| Status/output | Stream task output and expose real completion or failure |
| Open project folder | Use the desktop host's folder-opening mechanism |
| Terminal drawer | Host an actual supported shell/PTY session when available |

Prefer a thin adapter over existing operations. Keep CLI-specific prompting out of the shared operation layer. Do not use the sample screenshot labels as a catalog or assume every shortcut takes a folder. Render supported field types from metadata. If necessary, document the smallest backend change needed to expose currently CLI-only behavior.

Pass executable arguments structurally where the existing execution layer allows it. Avoid assembling shell strings from UI values. Preserve repository execution/approval conventions. Long-running work must not block the UI. Disable duplicate runs if concurrency is unsupported. Offer cancel only when the backend can actually cancel the task; report real cancellation status.

A read-only task output panel is useful independently of the optional terminal. A terminal requires a shell/PTY, input forwarding, resize handling, output rendering, and session cleanup. Do not label simulated commands or a textarea a working terminal. The example HTML contains only a static skin preview. Reuse an existing terminal integration if the repository has one; otherwise explicitly identify the integration work still needed.

No new external dependency is required merely to view this pack. Choose application dependencies based on the repository's existing stack and packaging targets. Keep saved preferences/catalog changes within Loadbot's existing configuration conventions.
