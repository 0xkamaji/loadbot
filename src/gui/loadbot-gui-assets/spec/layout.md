# Standalone window layout

Use the same menu hierarchy as references/01-main-window.png. The composition fills the available client area; there is no background room or launcher overlay.

- Header: small static Loadbot mascot, LOADBOT title, and host-appropriate window controls. No Back to room action.
- Main area: a project sidebar at roughly 30% width and a shortcut/detail panel taking the remainder.
- Sidebar: PROJECTS heading, scrollable project menu, Add project and Refresh catalog at the bottom.
- Main panel: SHORTCUTS / selected project heading, scrollable shortcut list, divider, selected shortcut name and description, generated input controls, output destination when supported, and Run shortcut.
- Bottom toolbar: Terminal toggle on the left, Open project folder on the right.
- Terminal: collapsed initially; opening it reveals a beige drawer above the bottom toolbar. Allow its content to scroll independently while retaining access to menus.

Initial desktop target: approximately 1000 x 680 logical pixels; this is a starting layout, not a bitmap scale requirement. Use flexible layout. At small widths, stack the project and shortcut areas or offer a project selector; allow vertical scrolling and preserve Run/Terminal access. Long labels and paths must wrap, ellipsize with a full-value affordance, or scroll within their control. A large catalog must not enlarge the OS window.

Selection should remain visible independently from hover and keyboard focus. Selecting a project refreshes its shortcuts; selecting a shortcut refreshes its description and inputs. Prevent inputs from one shortcut leaking into another unless the backend explicitly defines a shared setting.

Empty catalog: provide Add project. No shortcut selected: show a clear selection prompt. Invalid/missing input: explain what is required beside the disabled Run button. Running: show active status and output while retaining responsiveness; prevent unsupported duplicate runs. Completion and failure use text/icons, with real backend results. Preserve selection during refresh when the selected item still exists.

The header mascot is decorative and static. It does not open another GUI or need sprites, paths, collisions, furniture layers, or animation code.
