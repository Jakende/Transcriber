# Transcription Rust interface

The app is a macOS desktop utility in Operate mode. Its primary flow is selecting
recordings, choosing recognition and export settings, running a local job, and
editing the result. Source files, offline use, and reuse of existing models remain
visible product facts. The UI is implemented in Rust/egui, without a web view.

Use the macOS system sans and monospace fonts when available, with egui fallbacks.
Keep one blue action color, neutral content surfaces, a separate toolbar/sidebar
surface, and complete light/dark palettes. Text hierarchy: 24 pt page title, 15–16
pt section/navigation titles, 14 pt body, 13 pt controls, 12 pt supplementary text.
Spacing follows 6/12/18/24/28 pt rhythms; controls use 6 pt corners and grouped
settings use 12 pt corners. Color is reserved for actions, selection, and status.

The queue belongs in the sidebar. Recognition and output are distinct groups.
Advanced settings are disclosed progressively; the primary execution action stays
visible below them. Empty states explain the next useful action. Model storage
paths belong in diagnostics, while activity messages summarize operational events.
The same controls, typography, focus behavior, and margins apply to the library,
media import, glossary, diagnostics, and editor windows.

Minimum window: 880×620 points; default: 1100×760 points. Sidebars are resizable,
forms and actions wrap, and long documents/settings scroll. Both themes must keep
text, focus, hover, selection, errors, and disabled states legible. No decorative
motion, gradients, stock imagery, emoji icons, or marketing sections.
