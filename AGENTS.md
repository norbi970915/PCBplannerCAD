# Project instructions

## Public presentation

Describe pCAD directly. Never name or compare competing CAD applications in public-facing app text, onboarding, menus, dialogs, status/error messages, About text, build/release copy, README files, screenshots prepared for publication, or published progress notes.

Keep reference research and comparisons in Git-ignored local notes only. Do not add those notes or copied external designs to commits. Explain features using pCAD's own controls and behaviour.

Preserve file-format interoperability, internal API identifiers, original user-supplied filenames and required license notices. Those technical details do not authorize competitor branding in product copy. Use neutral descriptions such as structured project, XML schematic, symbol library and footprint folder for public import labels.

Before publishing, run the public-presentation regression check together with the editor tests and inspect the staged file list. Public release screenshots must show only pCAD, with private reference applications and notes out of view.

## Efficient validation

Prefer targeted source reads and command-line checks. Keep visual automation brief, avoid repeated successful builds without a new change or concrete failure, and summarize test output rather than printing full logs. Do not start sub-agents unless the user explicitly requests delegation.
