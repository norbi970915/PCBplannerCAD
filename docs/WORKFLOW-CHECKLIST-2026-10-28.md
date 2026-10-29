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
- [ ] Route differential pairs through unequal bends, escape paired pads, and change layers with paired via fanout. Inspect both necks/annuli and verify atomic rejection, cancellation and one-step Undo/Redo.
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

- [x] Pull the latest source and rebuild before launching `target/release/pCAD.exe`; use saved disposable design copies.
- [x] Drag a via attached to straight tracks on top/inner/bottom layers; confirm every attached endpoint follows and its far fixed connection remains intact.
- [x] Repeat using M/click placement; cancel with Esc and confirm the saved board is unchanged.
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

## Focused native pass — 2026-10-28

See [the native results](NATIVE-WORKFLOW-RESULTS-2026-10-28.md) for the build, fixtures, saved-data evidence and limits. Broader unchecked cases above remain open.

- [x] Via drag on Top/Inner 1/Bottom, fixed endpoints, exact one-step Undo/Redo, unlocked foreign-via Push, and M/Esc cancellation.
- [x] Locked attached straight track rejects a connected via move; retry state and Esc preserve source items.
- [x] Consecutive free-angle polygon cuts starting on the newest edge; dependent refill and exact outline/fill Undo/Redo.
- [x] Concave Slice preserves total outline area and net/layer; outward Reshape extends the boundary and refills.
- [x] Connected Align Top at distinct offsets preserves fixed vias and pad connections, including a zero-length join; exact Undo/Redo and native close/reopen/resave.
- [x] Native dense-board preparation status, later zoom/layer dimming, fill visibility off/on and unchanged saved data.
- [x] Prepared holed-pour copper selection, exclusion of the hole and crossing-box selection from the hole into copper.
- [ ] Repeat native timing with sufficient active samples; the first recording is insufficient for a performance conclusion.
- [ ] Complete pending-job cancellation/refill/Undo races and independently inspect printed copper and holes.

## Import parity and review safety — pending native acceptance, 2026-10-29

See [the latest handoff](HANDOFF-2026-10-29.md) for automated evidence and source-format limits. Existing checked native cases above do not cover this new batch.

- [ ] Rebuild before opening version-2 native documents on either PC; verify an older version-1 project still opens and saves correctly.
- [ ] Import a disposable hierarchical project; inspect member nets, cross-sheet highlighting and ERC. Verify group prefixes, vector ranges, unrelated branches and bus crossings/junctions.
- [ ] Confirm intentional board-only graphics survive Update PCB; toggle Keep if absent from schematic, Undo/Redo and reopen.
- [ ] Verify excluded source parts stay visible in the schematic but do not create placement proposals. Review independent BOM/placement exclusions and not-fitted output.
- [ ] Review removals, replacements and net changes starting unchecked; Cancel preserves board data. Apply only intended metadata changes and verify exact Undo/Redo.
- [ ] Change board geometry, schematic connectivity and a resolved footprint library after opening the review. Apply must reject the stale plan without changes/history.
- [ ] Review the original unmatched mechanical-pad mapping explicitly; do not auto-map it by similar naming.
- [ ] Independently inspect BOM, placement and printed copper/holes before manufacturing acceptance.

## Routing and length tuning — pending native acceptance, 2026-10-29

Background results and remaining routing limits are in [the updated handoff](HANDOFF-2026-10-29.md). Automated checks pass; these native cases remain unchecked.

- [ ] Rebuild, then check below/within/above length ranges, pair mismatch and a rule change/document switch while previews are pending. Pending feedback must stay neutral; stale rules/results must not flash as valid.
- [ ] Tune Accordion and Trombone around pads, vias, a fixed branch and foreign copper. Use a maximum amplitude that does not fit but permits smaller meanders. Inspect actual final clearances with full DRC.
- [ ] Tune through a filled foreign-net pour; confirm dependent refill, exact one-step Undo/Redo and saved-data recovery after close/reopen. Existing via/branch contacts must remain joined.
- [ ] Change tuning values rapidly, Cancel and reopen, change board/footprint data or reload while planning. Apply must use the displayed current shape or reject without a partial edit.
- [ ] Tune an already-in-range chain; confirm no IDs, dirty state or undo step change. Reject locked, duplicate and insufficient-room selections without mutation.
- [ ] Continue a pair through successive turns and Backspace; inspect both endpoints and gap. A rejected inner turn or constrained pad start must leave both members and shoved copper untouched.
- [ ] Review the remaining two constrained start corridors and the separate minimum-gap rejection before improving pair fanout; do not weaken clearance to pass a probe.

## Local folder hub, reviewed import and revisions — pending native acceptance, 2026-10-29

See [the workflow guide](LOCAL-PROJECT-WORKFLOW-2026-10-29.md). Background native filesystem/import tests and isolated browser workflows passed; the following real native-app cases remain open.

- [ ] Choose a working root, browse several subfolder levels, use breadcrumbs and restart to verify the root preference. Create a complete electronics project and open schematic, board and project library.
- [ ] Import a disposable XML component library; review symbols, footprint geometry, multigate/multi-pad mappings and parameters. Attach local STEP/VRML assets, adjust placement/rotation/scale and inspect them in 3D.
- [ ] Reopen a moved/copied project on the second PC; verify libraries and copied model assets resolve without work-PC paths. Inspect cached board definitions explicitly.
- [ ] Cancel a source picker, an in-flight analysis, a reviewed import and an unnamed Save All. Each must preserve current work and avoid false completion or a new checkpoint.
- [ ] Create two checkpoints with different schematic/board/library/model content. Restore the older state into a new folder and compare all files independently; the current project must remain intact.
- [ ] Confirm existing destinations, missing/external dependencies, linked folders and damaged checkpoint data produce an actionable rejection without partial outputs.
- [ ] Exercise read-only/locked-save failures, metadata changes and failed Close with real native dialogs. Confirm saved content, dirty flags and tabs remain coherent.
- [ ] Review remaining native binary/archive importer and automatic recovery limits before treating the workflow as a complete migration.

## Linked source pair and saved-version changelog — pending native acceptance, 2026-10-29

- [ ] Export a matching XML schematic/board pair. Import using same-name auto-detection, then repeat with explicit separately named files. Review linked-project status and source mismatch warnings; never auto-apply a mismatch.
- [ ] Edit/save native schematic and board files; verify originals remain unchanged, native paths stay in the project and Update PCB requires explicit review.
- [ ] Create before/after checkpoints containing a moved component, changed value/net/track width, changed rule order and a model/library edit. Compare the displayed field changes with independent inspection.
- [ ] Compare a version with itself and reverse the comparison. Verify a damaged payload is rejected and a comparison adds no dirty state, history or file writes.
- [ ] Confirm Ctrl+S updates working files while Save & checkpoint explicitly creates the historical versions. Test close/reopen and restore-to-copy on the other PC.

## Saved file thumbnails — pending native acceptance, 2026-10-29

- [ ] In the local Files panel browse saved schematic/board/project/library files and STEP/VRML assets. Confirm the content matches independently opened files; note first-sheet/first-library-entry and outline-pour limits.
- [ ] Save a board or schematic while its file/project card is visible; verify both thumbnails refresh after Save while an unsaved edit alone leaves the saved thumbnail unchanged.
- [ ] Scroll a large folder, change folders/themes, Save All and refresh while previews are pending. Stale replies must not appear on newer cards, and cancelled jobs must leave document/undo state unchanged.
- [ ] Check missing/corrupt/oversized files and references outside the working root. Keep a useful icon/error tooltip without blocking access to valid files.
- [ ] Inspect STEP model geometry and the board's 3D view natively, including transforms. Headless policy/geometry checks are separate from native interaction acceptance.

## Library output filename collisions — pending native retry, 2026-10-29

- [ ] Retry the previously blocked linked import in a rebuilt app. Verify the library paths are distinct and both case-sensitive source identities remain visible.
- [ ] Review the two remaining placement/connectivity differences and one warning deliberately; do not auto-apply them to make the import look clean.
- [ ] Save/reopen the imported project and inspect representative components, pad mappings and footprint geometry from each affected library. Original exports must remain unchanged.


## Editing guards and saved views — pending native acceptance, 2026-10-29

The 13:09 executable does not include this later source batch. See [the current handoff](HANDOFF-2026-10-29.md) for background evidence. Preserve open work before rebuilding.

- [ ] Try a mixed bulk edit containing locked and unlocked copper. A blocked edit must leave every item, dirty state, IDs and pending Undo/Redo unchanged. Unlock deliberately, then edit and verify one-step recovery.
- [ ] Verify pure unlock works for a track, curved track, via, footprint and pour. A replacement combining unlock with geometry/net/property changes must require a separate unlock.
- [ ] In the View tab, expand Saved views, save a routing view, change layers/labels/markers, apply it and compare the visible result. Check active-layer-only and filled-pour modes.
- [ ] Confirm grid, snapping, active layer and selection filters retain their current settings when applying a preset. Check that design dirty/history state is unchanged.
- [ ] Replace and delete a preset, restart and verify persistence; verify board and footprint editor presets remain separate. Presets stay on this PC rather than travelling with project checkpoints.

The user reported the previously blocked linked import now works. Detailed component/geometry/connectivity inspection in the earlier import checklist is still pending.


## Save-target and revision consistency — pending native acceptance, 2026-10-29

This is a later source batch; rebuild after preserving open work. The current 13:09 executable does not contain it.

- [ ] Open two disposable libraries, modify both, then try Save As from the first to the second library's path. The operation must reject without changing either file, path or dirty state. A new distinct destination must still work.
- [ ] Attempt document/library Save As to another open design or the open project metadata file. Verify rejection, then normal saving to the correct original path.
- [ ] Compare two checkpoints and export the changelog. Inspect checkpoint identities, file/object changes and omitted counts independently. Cancel an export and test a blocked output path; verify no false success or damaged saved file.
- [ ] Switch projects while a slow history operation is pending. The new dialog must clear the old list/results and remain usable; a queued request must not checkpoint or restore the replacement project. Already accepted filesystem work may finish for its original source.
- [ ] Export a PDF/text log to a separate path and inspect the file. The shared byte-export service must reject paths belonging to open designs, libraries or the project metadata.

Native output writers outside the shared byte-export service still need separate target-ownership review. Automatic recovery discovery and external-file-change conflict handling remain pending.


## Fabrication package and variant safety — pending native acceptance, 2026-10-29

Rebuild after preserving open work; the 13:09 executable predates this source batch.

- [ ] Generate a package into a new named subfolder, with ZIP enabled and disabled. Verify every expected file and ZIP member independently, and inspect the displayed destination.
- [ ] Attempt generation into an existing populated and empty folder. Verify rejection without changes to any prior file. Cancel the parent-folder picker and check that no output is published.
- [ ] Use View in Gerber viewer for both modes. Unpacked output must open the package folder and display copper/drills; inspect geometry independently as well.
- [ ] Compare CSV BOM, placement and XML BOM for a deliberately chosen assembly variant, not-fitted parts and independent BOM/placement exclusions. Physical component geometry and nets must remain in XML output.
- [ ] Export another open standalone board while a project is active. Its title must be used and unrelated project variants must not apply.
- [ ] Try BOM/placement/XML/STEP/OBJ exports to paths of open design/library/project files. Verify rejection, unchanged saved bytes and successful export to a separate path.
- [ ] Confirm busy output controls and Escape behavior, error visibility, cancellation and project switching. A late completed package may exist for its original request, but must not become the replacement project's result.

Background checks do not certify manufacturing correctness. Legacy project creation and automatic recovery remain separate pending work.


## Recovery and interface color — pending native acceptance, 2026-10-29

Preserve open work before rebuilding; the current 13:09 executable predates this batch.

- [ ] Modify a saved schematic, board and library, and create an untitled document. Wait for automatic capture, then inspect the timestamp and native copied contents without saving originals.
- [ ] Restart the rebuilt app and discover the copies. Restore to a new folder, open its linked project and compare known unsaved edits and embedded library definitions. Original files must remain unchanged.
- [ ] Verify explicit copy creation, disabled automatic capture, copy failure feedback and three-copy retention. A failed copy must keep earlier complete records.
- [ ] Test a damaged record, missing dependency and existing restore destination. Valid records must remain discoverable and failures must not publish partial recovery folders.
- [ ] Reconnect external model assets using the restored source-location notes. Check library/model transforms; assets and Undo history are outside the recovery payload.
- [ ] Exercise Cancel, repeated window-close requests and failure to query unsaved work. Inspect explicit Quit behavior and final-copy failure handling with disposable data.
- [ ] Profile capture time, navigation responsiveness, memory and file sizes on a large board. Background native commands still hold the workspace during serialization.
- [ ] Inspect colored project/file cards, tool/document tabs and editing icons in light/dark themes at the normal display scale. Check active/disabled/primary buttons and confirm canvas layer colors are unchanged.

These native checks remain open. Automated persistence tests and headless screenshots do not complete crash, power-loss or performance acceptance.

## Linked component navigation and native detached views — pending acceptance, 2026-10-29

Use the latest linked-view rebuild recorded in the handoff. Preserve private source designs and use disposable saved copies for edits.

- [ ] Open a linked project. Double-click schematic component bodies and verify the correct board footprint is selected and fitted. Repeat with an unrelated open board using the same reference.
- [ ] Check unplaced parts, unannotated references, duplicates and a missing project board. Diagnostics must explain the problem without choosing another design's footprint.
- [ ] Use Detach to window for the board, schematic, Properties, library and 3D panels. Move windows across actual monitors, including different display scales and negative screen coordinates.
- [ ] Drag a tab outside the main app to create its window; drag that tab back onto center and edge docking targets. Repeat several times, test Escape cancellation, and check that a cancelled/internal drop does not open an extra window.
- [ ] Double-click a schematic part while the board is detached. The existing secondary board must focus and fit the footprint without opening a duplicate.
- [ ] Test keyboard sequences, routing/Escape, context menus, selection popovers, tooltips and via-layer selection in the secondary window. Type shortcut letters into Properties inputs; they must stay text. Escape must cancel the typed value without saving it.
- [ ] Minimize the main window while navigating/editing the detached board and 3D views. Verify responsiveness and DPI after monitor moves, without claiming a frame-rate target.
- [ ] Modify an unsaved document, close only its secondary window and verify the tab returns with geometry, dirty state and Undo intact. Test multiple detached groups and close orders; main-app close must retain its normal unsaved-work confirmation.
- [ ] Save, exit, reopen and verify the documented default-layout fallback for detached sessions. Open the linked project normally and inspect both files and recovery copies. Monitor positions are not restored.
- [ ] Exercise a denied popup and verify the original tab remains visible with a warning. Native creation/close/focus are separate acceptance from mocked IPC.

The 19 headless docking workflows use browser windows, synthetic HTML5 drag events and mocked native IPC. They do not complete physical-monitor, native WebView or crash acceptance.

## External-save conflicts and Save Copy — pending native acceptance, 2026-10-29

The 15:57 linked-view executable predates this source batch. Rebuild only after preserving open work. Use disposable project folders and a separate editor/process to create the external changes.

- [ ] Open and edit a saved schematic/board/library, change its disk contents externally, then Save. Verify the external bytes, live edits, original path, dirty marker and Undo remain intact.
- [ ] Remove the current working file externally. Saving must report a conflict without silently recreating it. Repeat with an equal-length edit and with identical contents rewritten: only the actual content change should conflict.
- [ ] Edit two documents and a library. Cause a known conflict in the later document, then separately in the library and project file. Save All must reject before saving any member and must not report complete success.
- [ ] Change project metadata externally, then try a variant/link edit or adding a new sheet/board. A known project conflict must preserve existing links and avoid creating its new companion file.
- [ ] Use File → Save Copy into a new filename after a conflict. Open the copy separately and compare its current content. The original working path, linked project, dirty marker and Undo must stay unchanged.
- [ ] Repeat Save Copy from an open symbol/footprint entry; verify the complete containing library is copied and the original library remains open/dirty at its original path.
- [ ] Cancel the copy picker, choose an existing destination and choose a path owned by an open document/library/project. No copy may overwrite an existing file or report false success. Repeat Save As to an unused and an existing different filename.
- [ ] Preserve the live version, deliberately close/reopen the external file, edit and save it again. Verify the new disk baseline and usual discard safeguards.
- [ ] Check large-board Save/hash timing and UI responsiveness, read-only destinations and late I/O failures. Save All is not a multi-file transaction and does not lock out every concurrent writer.

Background evidence: 230 editor tests, 21 native workspace/app tests, 7 targeted storage tests and 23 headless menu/docking workflows passed. Browser IPC is mocked; physical filesystem cases run separately on disposable files. Whole-project copying, automatic external reload and legacy project creation remain separate work.

## Focused home acceptance — 2026-10-29

These specific cases passed; they do not close the broader multi-case sections above. Evidence and limits: [native acceptance and fixes](HOME-ACCEPTANCE-2026-10-29.md).

- [x] Native external-board Save conflict preserves external bytes and live edits.
- [x] Native Save Copy preserves working path/project link and Undo/Redo.
- [x] Linked schematic double-click selects the matching footprint.
- [x] Native detached board shares selection/Undo and returns on window close.
- [x] Automatic/manual recovery survives orderly restart and restores an exact linked copy.
- [x] Idle R keeps attached copper at the rotated pad center; one-step Undo/Redo is exact.
- [x] Pad-center routing shows the expected copper total and below/in/above target feedback.
- [x] Space via picker opens with the correct layer choices and cancels without edits.
- [x] Native 20-to-22 mm tuning previews/applies and one Undo restores exact saved data.
- [x] Legacy project creation rejects companion collisions and cleans up only its own unchanged file on failure (native filesystem regressions).
- [ ] Complete remaining physical-monitor/DPI, forced-crash, pair-routing and graphical manufacturing acceptance.


## Extended dense-board CLI and headless acceptance — 2026-10-29

These checks used disposable imported boards and do not close native pointer/monitor acceptance above. Details and known limits are in the current handoff.

- [x] Eight-layer source copper/footprint positions preserve net, layer, span, drill and width, within 1 nm coordinate quantization.
- [x] Correct inner pad lands and single-segment schematic names survive native/library save and pad renumber/copy/Undo.
- [x] Dense via arrays reject real conflicts on crossed inner layers without partial edits/history.
- [x] Refill is idempotent; accepted connected move/rotation probes have exact Undo/Redo.
- [x] Twelve of sixteen real filled-board length-tuning probes pass target tolerance, native DRC, Undo/Redo and reopen; four reject insufficient room.
- [x] Eight copper CAM layers, 770 physical drill hits and package ZIP reopen with no parser warnings; partial-span drill partition regressions pass.
- [x] Thirty headless WebGL checks cover layer-dependent pad pixels, nine deferred pour jobs and incremental track uploads.
- [x] Actual schematic ECO has no topology changes and preserves layer geometry through apply/Undo/Redo/reopen.
- [ ] Native mouse/keyboard acceptance of the final dense-board pair and via changes; Windows automation currently reports access denied.
- [ ] Review the original dense design findings with circuit intent; passing import parity does not close source ERC/DRC.
