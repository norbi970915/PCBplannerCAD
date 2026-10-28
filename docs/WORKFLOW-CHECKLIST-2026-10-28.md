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

## pCAD connected-via and navigation follow-up — pending home UI checks

- [ ] Pull the latest source and rebuild before launching `target/release/pCAD.exe`; use saved disposable design copies.
- [ ] Drag a via attached to straight tracks on top/inner/bottom layers; confirm every attached endpoint follows and its far fixed connection remains intact.
- [ ] Repeat using M/click placement; cancel with Esc and confirm the saved board is unchanged.
- [ ] Test a via in a straight segment interior and a via on a fixed pad; verify both original connections remain joined.
- [ ] Check curved-track endpoints and interior via contacts, including clockwise/wrapped sweeps: the curve stays and a joining route reaches the moved via. Repeat with a locked curve, and with two selected endpoint vias.
- [ ] Verify blind/buried spans and reversed layer endpoints; same-net copper outside the span and other nets stay fixed.
- [ ] Check Preserve angles / Free / explicit Disconnect, locked attached tracks, blocked destinations and Rules off. Run full DRC after testing.
- [ ] Verify one Undo restores via, routing, teardrops and fills together; Redo and save/reopen retain the result.
- [ ] Paste/array vias and change diameter/drill/span/net in single/bulk properties. Invalid moves should not partly apply or consume IDs/history; legal edits should refill dependent copper.
- [ ] Start a new operation or edit the board while a connected move plan is pending; its late reply must not overwrite newer geometry or hide the newer preview.
- [ ] Use Help → Record/Stop Canvas Timings while panning/zooming/selecting/dragging. Save the Performance summary locally with display size, GPU and board size; CPU timings do not measure presented FPS.
- [ ] Check net highlighting, hover, clearing highlights and high-DPI rendering; verify overlays stay readable.

Background verification updated **2026-10-28 12:38 (Europe/Bucharest)**: 164 editor tests, 214 native tests and 18 headless renderer checks passed. These do not check off the pending native UI acceptance above.

- [ ] Verify a locked neighboring straight track stays exact while an attached via/track moves; confirm the retained junction and undo result.
- [ ] Select multiple vias with both endpoint and interior contacts on one segment; compare preview and final connectivity.
- [ ] After a small track edit, change active layer, hide/show layers, toggle pour fills and Undo/Redo; confirm no old tracks/fills remain on screen.
- [ ] Record timings after warm edits and separately record initial opening time; cold filled-pour triangulation is still a known source of delay.

## Deferred filled-pour display — pending home acceptance

- [ ] Rebuild, then open a large saved board. Confirm other geometry responds while “Preparing copper display…” is shown and fills arrive without changing the document's saved/dirty state.
- [ ] Pan/zoom, pick copper inside pours and avoid holes, and use crossing boxes at hole boundaries while preparation is pending and after it finishes.
- [ ] Hide/show layers, toggle filled pours, change active-layer-only mode, and switch documents during preparation. Cancelled/hidden jobs must not reappear or overwrite the current view.
- [ ] Refill, edit a pour, Undo/Redo and close/reopen a document while work is pending; verify geometry, selection and history remain correct.
- [ ] Move a large pour and inspect its outline preview; after placement, wait for its final fill display. Esc must leave the saved geometry unchanged.
- [ ] Print a board after deferred preparation and inspect copper/holes independently. Confirm normal printing still works before a display mesh has been cached.

Background checks for this batch: 174 editor tests, 24 headless WebGL checks, a bundled-worker check under the app's security policy, TypeScript and the frontend build passed. These do not check off the pending native interaction tests above.

## Connected alignment and rejected-move retry — pending home acceptance

- [ ] Align and Distribute routed footprints using different offsets; confirm routing stays physically attached, other components stay fixed, and Undo/Redo/save/reopen recover exactly.
- [ ] Check Preserve angles, Free and explicit Disconnect. Tab changes the connection behavior during an M move.
- [ ] Pack parts where a later part has locked or blocked attached copper; confirm no earlier part moves and no cursor pickup starts on failure.
- [ ] Attempt a blocked M/drop in Avoid/Push mode, choose a legal location and place again. Esc must cancel without adding a history step.
- [ ] Hold/release the pointer across a fast rejected floating placement; verify there is no duplicate plan or commit.
- [ ] Change rules/move settings, selections, footprint geometry or the active document while planning; stale results must not overwrite current work.
- [ ] Run full DRC after testing Free/Ignore and direct component edits; clean-route rejection does not replace whole-board review.
