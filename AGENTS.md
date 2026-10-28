# Project instructions

## Public presentation

Describe pCAD directly. Never name or compare competing CAD applications in public-facing app text, onboarding, menus, dialogs, status/error messages, About text, build/release copy, README files, screenshots prepared for publication, or published progress notes.

Keep reference research and comparisons in Git-ignored local notes only. Do not add those notes or copied external designs to commits. Explain features using pCAD's own controls and behaviour.

Preserve only the exact external file extensions, serialized format markers, original user-supplied filenames and required license notices needed for compatibility. Use neutral names for internal APIs, source folders, identifiers, descriptions, comments and messages. Technical compatibility does not authorize product names or comparisons in prose. Use neutral descriptions such as structured project, XML schematic, symbol library and footprint folder for public import labels.

Before publishing, run the public-presentation regression check together with the editor tests and inspect the staged file list. Public release screenshots must show only pCAD, with private reference applications and notes out of view.

## Efficient validation

Prefer targeted source reads and command-line checks. Keep visual automation brief, avoid repeated successful builds without a new change or concrete failure, and summarize test output rather than printing full logs. Do not start sub-agents unless the user explicitly requests delegation.

## Git delivery

Never push without an explicit human request covering the current changes. A previous push request is not standing authorization for later work. Do not rewrite shared history or publish releases without a separate explicit request.
