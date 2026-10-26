# Polygon cut, slice and reshape

PCBPlannerCAD supports repeated polygon cuts, slices and boundary reshaping using a click-to-click workflow.

## Use

- **Cut (`E C`)**: click the outline, add optional corners, click the outline again. Move over either piece to mark it red, then click that piece to remove it. Green is kept. Space swaps the proposed side; Enter accepts the highlighted choice.
- **Slice (`E S`)**: use the same boundary/corners/boundary sequence. Both pieces are retained as independent pour objects with the original net, layer, priority, clearances and thermal settings. They share a boundary; slicing alone does not create an electrical isolation gap.
- **Reshape (`E R`)**: redraw a stretch of the outline, including paths outside the original to extend it. Space switches between the larger and smaller result before finishing on the boundary.

These tools stay active for repeated edits. Shift constrains intermediate corners to 45 degrees. Backspace removes a corner or returns from side choice to path editing. Esc cancels the current path, and Esc again exits. Right-click cancels using the editor's existing convention. A canvas strip, the Place menu and a selected pour's context menu expose the modes. Boundary snapping takes precedence over unrelated pads/tracks. Other-layer outlines require explicit selection and are excluded in single-layer mode.

Corners and the displayed cursor now ignore the document grid: use any angle by default, with only Shift enabling a 45-degree constraint. Boundary endpoints still attach to the polygon. Repeated cuts on a newly created diagonal edge allow the sub-nanometer projection rounding inherent in integer-nanometer coordinates; that rounding previously caused valid subsequent cuts to be rejected.

Cuts and slices must stay inside the polygon and produce two valid sides. Crossing/retracing/touching paths are rejected without losing the in-progress edit. Cut-outs wholly inside a pour and arc-shaped cuts are outside this implementation.

Outline/handle edits and slices now use the core `editPolygon` command. It plans on a document copy, checks that the source outline is current, preserves winding/settings, recomputes pours with their dependencies, and records only changed objects in one undo step. A failed command leaves the document/history untouched. Whole-pour drag-moves still use the pre-existing move/repour path.

## Playground

Open `examples/polygon-playground.arpcb` for a rectangular and concave GND pour. Use the Fills button if the editor was previously set to outlines only. The sample disables island removal so its disconnected demonstration copper stays visible.

## Verification

- 14 frontend tests passed using Node's test runner: cut side choice/preview, immediate slice, rapid-click protection, corners/Backspace/cancel, failure recovery, hidden layers, Shift constraint, stale completion after reset, malformed geometry, winding/translated coordinates, same-edge notches and concave shapes. Regression coverage also verifies free-angle corners with a coarse board grid, three consecutive cuts on updated diagonal boundaries without tool reactivation, and the user's screenshot case: a small trim between an existing bevel and a vertical edge, followed by another trim. The interaction harness applies successful commands to its document between cuts.
- Four new core tests passed: dependent refill and one-step undo/redo; split ID allocation, metadata, fill and save/reload; malformed/stale command rejection; winding and keep-outs.
- Full Rust workspace: 246 reported passed, no failures, four ignored performance tests. Two fixture-dependent importer end-to-end tests returned early.
- TypeScript, frontend production bundle and release Rust binary build passed. Existing occt-import-js externalization and chunk-import warnings remain.
- Hands-on in the rebuilt desktop app: E-menu discovery, Cut activation, live red/green preview, choosing a side, dependent copper display and single Ctrl+Z restoration. Switched with the canvas strip to Slice; a multi-corner cut through the concave pour created a fifth object from the original four, and a single Ctrl+Z returned to four. No user design files were edited.
- Follow-up hands-on verification: the active window was still running the earlier frontend tool despite the dev server serving the fix. Refreshed only the frontend and reopened the same backend document, preserving its dirty state and undo history. Made two consecutive free-angle corner trims on the already-beveled concave pour, with each cut starting on the newest diagonal edge. Both committed without reactivating Cut; Ctrl+Z twice restored the user's starting shape. The corrected idle hint includes "free angle, no grid".

Run frontend tests from `app/` with `node --import ./tests/register.mjs --experimental-strip-types --test tests/polygon.test.ts` (or `pnpm test:polygons`). Run backend coverage with `cargo test -p pcbplannercad-core pcb::polygon`.
