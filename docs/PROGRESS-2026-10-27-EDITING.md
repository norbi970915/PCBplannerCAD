# PCBPlannerCAD editing update — 2026-10-27

Supersedes the implementation status in the 15:44 handoff. Source and validation were handled with targeted file reads and command-line tests; long visual comparisons and sustained frame-rate benchmarks were not repeated.

## Implemented

- **Tools → Pour Manager:** searchable ordinary-pour table, selection highlighting and zoom, name/net/layer/priority/clearance/minimum-width/thermal editing, deterministic priority reordering, explicit locking and refill. Keep-outs and generated teardrops are excluded. Draft changes are discarded on Cancel; applying properties and dependent refill is one undo transaction. Selected refill follows overlapping pours on the same layer, including old/new outline bounds and rule clearances; unrelated fills remain untouched. Stale dialogs refuse to overwrite a changed board.
- **Bulk properties and locking:** the Properties panel edits widths of selected straight/arc tracks and diameter/drill/span of selected vias. Blank fields preserve mixed values. Validation completes before applying replacements. Explicit Lock/Unlock covers footprints, tracks, arc tracks, vias and zones. The Rust command boundary rejects edits/deletions of locked items, while allowing refill and explicit unlock. Cleanup and shove routing preserve locked copper. Older native files default new copper locks to false.
- **Automatic teardrops:** optional settings in the Teardrops dialog persist with the board. Routing and connected edits regenerate affected joints, retain unchanged teardrop IDs and include generated copper/refill in the same undo transaction. Manual generation/removal remains available. Locked teardrops are preserved.
- **Connected movement:** rotation and side flip during footprint movement now use the backend connected-movement planner. SMD side changes add a via transition where needed. Track drags use same-ID replacements rather than delete/recreate.
- **Via shoving:** bounded candidate displacement moves a conflicting unlocked via and its attached straight tracks across its layer span, checks the changed copper with DRC, and retries routing. Fixed pads, locked connections and arc-attached vias remain fixed. Board edges and keep-outs reject illegal candidates. This is deliberately bounded, not a general recursive multi-via shove solver.
- **Responsiveness:** a bounding-volume spatial index narrows hover and rectangular-selection candidates; its results were checked against a linear scan on 34,000 entries. Heavy route, pour, drag and document-edit commands are dispatched away from the native window thread. Removed the temporary TIMING lock print.

## Validation

- 106 editor tests passed, including bulk-edit validation/mixed values, locking, priority ties, ID-preserving drag and spatial-index correctness.
- TypeScript checking passed.
- Rust workspace: 190 core tests passed (3 performance tests ignored), 43 import tests, 44 output tests (1 performance test ignored), and 6 reported integration tests passed. Fixture-dependent tests may return early when external designs are unavailable.
- Added regressions for backend lock enforcement and unlock/undo, automatic generation/deletion/undo of teardrops, overlapping pour edits and refill/undo, selected-refill isolation, via shoving with attached tracks on both layers and clean DRC, and connected rotation/flip with undo.
- Production executable, MSI and NSIS installers built successfully. Existing optional-module externalization/chunking warnings remain.

## Routing via safety and public presentation

Layer-change routing now commits its tracks, pushed copper, via and refill in one undo transaction. A partial/blocked route or failed document commit does not insert a misplaced via or switch the active layer. Cancellation during via lookup leaves the document untouched. Regression coverage includes exact endpoint placement, blocked routing and rejected commits.

Public import, library, export and About wording uses neutral format descriptions. The repository instructions record the public-presentation policy, and regression checks cover UI wording and published Markdown. Reference comparisons and copied designs remain local and Git-ignored.

## Usage

Open a board, choose Tools → Pour Manager, or select copper and use Copper properties & locking in the Properties panel. In Tools → Teardrops, enable Update automatically after routing and connected moves to persist automatic generation settings. Existing M/R and right/middle-click movement controls continue to work.

## Remaining verification and follow-on work

The older native checklist (DRC marker appearance, Update PCB cursor placement, board Find, rules-off differential pairs, and sustained online-DRC/teardrop performance on the largest boards) is not claimed complete by command-line tests. Rule enforcement defaults to on; verify the visible rules switch before normal editing because saved local preferences can disable it.

Future projects from the broader roadmap remain: fully recursive via shoving, automatic pad/track conflict relocation, custom pads and per-pad overrides, persistent groups/arrays, pair tuning, radial/leader dimensions, ODB++ and exact B-rep STEP. The installed executable must be rebuilt when source changes; this update includes fresh local build outputs, which remain Git-ignored.

## Small routing usability update

- The on-canvas routing bar now offers obstacle mode, corner style and rule-width reset controls. Settings changes refresh an active routing preview immediately.
- Shift+Space cycles 45°, 90° and free-angle corners; plain Space still flips the corner posture. Press 0 to restore width from board rules. W / Shift+W continues to change width.
- Identical routing requests at the same snapped position reuse the preview. Changes to routing settings or board geometry invalidate that reuse; failed requests can retry. Returning to an in-flight endpoint cancels an obsolete queued endpoint.
- Focused validation: 110 editor tests passed and TypeScript checking passed. This update does not claim measured frame-rate gains on large boards.

## Additional workflow controls for the next layout session

- Routing width dropdown: choose standard widths, widths from board net classes, or From rules. Existing custom width values remain available in the list.
- Selection presets: Components, Copper (tracks/arcs and vias), and Vias preserve current layer exclusions. Everything resets both types and layers. Presets appear in the View panel and selection-filter popup.
- Selection count: while using Select, click the selected-object count in the toolbar for Zoom to selection or Clear selection.

Live validation is deferred to the next full layout workflow. Check preset selection with click/Ctrl-click and both box directions; verify that layer exclusions remain active; try width changes mid-route and rule-width reset; check selection count, zoom and clear after moving or rotating parts. Record each issue with the action, expected result, actual result and a screenshot or saved board where useful.

Further routing, arrays, selection, browsing and diagnostics work is recorded in [the workflow update](PROGRESS-2026-10-27-WORKFLOW.md).
