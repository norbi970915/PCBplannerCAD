# Layout workflow acceptance — 2026-10-28

Use a saved copy of a real project and the newly built executable. These are pending live checks, not completed acceptance claims.

## Session

- Board/file:
- Executable build timestamp:
- Board size, component and net counts:
- Display scaling and window size:
- Online DRC and rule enforcement settings:

## Workflow

- [ ] Open the project; inspect components, nets, layer names and original routing.
- [ ] Place and move parts with M, rotate with R/right click and flip with F/middle click; verify connected tracks, Cancel and Undo/Redo.
- [ ] Box-select left-to-right and right-to-left; Ctrl-click and Shift-click; verify Components/Copper/Vias presets and hidden/locked exclusions.
- [ ] Use Select Similar with each relevant criterion; compare the match count and actual selection.
- [ ] Browse and filter components, select ranges, double-click to zoom and Move Parts; confirm locked parts remain fixed.
- [ ] Route out of small pads on different nets, both board sides and near obstacles. Confirm pad-center starts, visible previews, snapping and clearance behaviour.
- [ ] Change width/mode/corners without moving the cursor; use 0 and Shift+Space; inspect blocked-route feedback.
- [ ] Insert vias to top, bottom and inner targets; test Shift+V, blocked endpoints, Backspace, Esc and Undo/Redo.
- [ ] Route differential pairs through unequal bends. Paired via fanout is not supported; verify the explanatory message.
- [ ] Change custom grids in mm and mil, step with G/Shift+G, toggle snapping and restart to verify remembered steps.
- [ ] Create rectangular and circular arrays with and without retained nets/rotation; inspect designators, dimensions and pours, then Undo/Redo and save/reopen.
- [ ] Edit/refill pours and use locking/bulk properties. Confirm teardrops and online DRC update after connected edits.
- [ ] Sort/filter/page the net list; select only routing; switch quickly between net highlights and confirm the final click wins.
- [ ] Run a full DRC; filter and navigate issues; export CSV. Watch checking/stale/failure feedback while editing.
- [ ] Measure, reverse/edit endpoints, copy a readout and record it; search/copy/save relevant Messages.
- [ ] Save/reopen and review final DRC, connectivity, Gerbers, drills and placement outputs.
- [ ] Note responsiveness during the full workflow on the representative large board; record the action and approximate delay, not just an overall impression.

## Issue record

Repeat this block for each defect or improvement:

- ID / priority (blocks work, wrong result, inconvenience, enhancement):
- Board or small reproducible copy:
- Exact starting tool/layer/selection filters/rule settings:
- Steps and shortcuts used:
- Expected result:
- Actual result:
- Does Undo recover correctly? Does save/reopen change it?
- Screenshot, message-log excerpt or exported DRC row:
- Frequency and approximate delay, if relevant:

## First acceptance pass — 2026-10-27

See [the results and issue log](WORKFLOW-RESULTS-2026-10-27.md) for evidence and limits. The three recorded UI issues have local fixes. The checklist remains open: the full set of placement, arrays, differential pairs, pours, output review and sustained large-layout interactions has not yet been signed off.

## Fix follow-up — 2026-10-27

- [x] Fix stale Properties across PCB/schematic document switches, including matching item IDs; verify the displayed editor belongs to the active document.
- [x] Correct schematic selection hints and check the advertised rotation/mirror actions.
- [x] Verify PCB move/rotate cancellation preserves the saved board exactly.
- [x] Verify a connected schematic drag moves both attached endpoints, fixes the other ends and undoes exactly.
- [x] Verify native routed copper remains physically joined to its source pad after move/rotation, with exact Undo recovery.
- [x] Verify Messages headers, filters and log rows remain readable at 1602 × 992 after window restore and compilation.
- [x] Verify G / Shift+G move to the next grid and restore the original grid; recompile the saved schematic in the final build.

## Schematic workflow

- [x] Open a three-sheet project and compile/ERC; record part/net counts and each warning.
- [x] Connect pins in a clean schematic, verify saved endpoints and compiled nets; box-select, rotate, Undo/Redo and verify saved data recovery.
- [ ] Place/edit symbols, verify pin types and footprint-pad mapping; annotate and review changes.
- [ ] Edit junctions, crossings, net labels, power ports and hierarchical connections; verify expected ERC failures and navigation live.
- [ ] Move/rotate/mirror connected parts, including attached-wire behaviour, cancellation and reopen.
- [ ] Review and apply source-to-board changes in the UI; confirm reannotation does not silently duplicate or replace components.
- [ ] Export/review the final BOM, schematic sheets and board consistency as one project.

## Routing / vias / arrays / pours follow-up — 2026-10-27

- [x] Route through existing other-net copper, check exact pad-centre endpoints and saved physical connectivity.
- [x] Check four-layer via types natively, plus Space picker, V, Shift+V and Backspace live; verify saved spans and drill sizes.
- [x] Change width, corners and obstacle mode in place; restore width from rules.
- [x] Verify rectangular/circular live array geometry, references, cleared nets and exact one-step Undo; verify retained nets/orientation options natively.
- [x] Apply array plans through native commands, refill pours, check clearances and recover with Undo/Redo/save/reopen.
- [x] Edit overlapping pour priority and refill selected plus overlaps live; verify properties and both fills recover in one Undo.
- [x] **WF-004 / P1:** add an atomic native via guard and regress the original GND–SIGNAL short. Six-layer matrix verifies every crossed layer, including through fallback, and permits obstacles outside the span. Frontend checks preserve routing layer/anchor and retry history.
- [x] Verify WF-004 in the rebuilt Windows app: rejected SIGNAL via keeps Inner 2, reports R1.2/Top Layer, adds no copper or history step, and preserves the saved board exactly. Verify a legal same-net via still places and undoes correctly.
- [ ] Complete differential-pair UI, keep-out/blocked endpoint, mixed-array placement DRC and live thermal/island/locking cases.

See [the morning handoff](HANDOFF-2026-10-28.md) for changes, verification and the remaining priorities.
