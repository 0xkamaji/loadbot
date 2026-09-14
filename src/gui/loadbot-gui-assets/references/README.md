# Reference priority

1. `01-main-window.png`: approved menu composition. This is the primary layout reference.
2. `02-interface-kit.png`: approved visual language and control states. This sheet is not the application layout.
3. `../spec/asset-manifest.json`: exact runtime files, metrics, slice margins, and implemented text colors.

Standalone overrides: remove “Back to room” and the shared room/Rot navigation bars. The menu fills the app window. Loadbot is an optional static header icon. Native window controls may replace drawn close/minimize controls.

The screenshots are flattened reference images. Do not cut text or controls from them for runtime use; individual controls are already in assets/. Tool names, paths, statuses, and descriptions shown here are examples. The backend catalog supplies real entries.

The reference's printed swatch values (#0E0E0C and #DCC7A3) describe the art direction. Existing normalized PNG skins vary slightly within that black/beige palette; use the manifest tokens for matching runtime text. Avoid bright white, green, and additional accent colors.
