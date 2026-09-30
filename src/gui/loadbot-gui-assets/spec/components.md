# Reusable component and asset contract

Create one reusable implementation per component, then supply labels, icons, values, callbacks, and state as data. Keep styling and backend operations separate. Adding a shortcut to the catalog should create a menu row without new PNGs.

| Component | Assets | Runtime behavior |
|---|---|---|
| WindowFrame | ui/frames/window.png | Resizable client-area decoration |
| Panel / Titlebar | ui/frames/panel.png, titlebar.png | Layout containers with real headings |
| Button | ui/buttons/*.png | Real label; default, hover, pressed, selected, disabled |
| IconButton | Button plus ui/icons/*.png | Accessible name; use native control behavior |
| MenuRow | ui/menus/*.png plus icons | Transparent default; persistent selection; keyboard access |
| InputControl | ui/inputs/*.png | Real text/select/path widget and validation |
| Checkbox | ui/checkboxes/*.png | Native checked/disabled semantics with supplied skin |
| StatusRow | Real text, optional existing icon | Input required, Ready, Running, Complete, Failed as actual state |
| TerminalDrawer | terminal/panel.png | Dark text on beige; toggle and scrolling |

All table paths are relative to assets/. Not every screenshot symbol has a matching exported icon: use a text label or an existing appropriate symbol; do not imply an asset is supplied when it is absent.

## Scaling and geometry

All manifest paths are relative to `src/gui/loadbot-gui-assets/` in the repository. `nine_slice` order is top, right, bottom, left. Use either the master PNG with a nine-slice facility or its nine exported pieces, never both. Keep corners fixed; stretch horizontal edges horizontally and vertical edges vertically, with nearest-neighbor sampling. Fill the center in both directions. Do not stretch the entire master bitmap to the target rectangle.

The window uses 12px slices, panel/titlebar/input 8px, buttons/menu rows 6px, and the transparent input focus ring 4px. Check the manifest for every asset instead of assuming one border width. The beige terminal uses 8px slices. The focus ring is an overlay over the input skin; it does not replace its background.

Manifest content padding is measured from the outside edge, including borders. Subtract border thickness when mapping to CSS padding. `minimum_size` is a mathematical slice limit, not a usable control size. Practical base sizes: buttons at least 96 x 40, inputs 128 x 40, icons 16 x 16, checkboxes 24 x 24. Give touch controls at least 44px interaction height. Prefer integer zoom and preserve the mascot's aspect ratio.

Textures are stretched by the supplied nine-slice contract, not guaranteed to tile seamlessly. Inspect large panels at the final size.

## Colors and type

Centralize text #dfc6a0, selected text #171512, disabled text #897962, surface #171512, terminal text #211c16. Let the PNG skins supply their beige fills and outlines. Default icons are light for dark surfaces; inverse icons are dark for beige surfaces.

Bundled DejaVu Sans Mono is a reproducible fallback chosen for the previous UI kit, not the exact generated pixel lettering. Start with 14px body/control text, 18px titles, line height 1.4. Preserve fonts/LICENSE-DejaVu.txt. If a dedicated pixel font is approved later, replace it centrally and recheck measurements.

Use native buttons, labels, input semantics, disabled behavior, keyboard activation, and visible focus. A selected state is distinct from a momentary pressed state. Color changes alone must not convey success/failure or required input. Escape should follow the host's normal dialog behavior; it should not unexpectedly close the application.

See examples/assets.css for a framework-neutral browser example of PNG border-image usage. Other toolkits should implement equivalent nine-patch behavior.
