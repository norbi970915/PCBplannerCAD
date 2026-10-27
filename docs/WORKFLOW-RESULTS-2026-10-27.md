# Workflow acceptance results — 2026-10-27

WF-001 through WF-004 now have fixes. The original via-short reproduction is rejected atomically by the native guard, with tests covering every crossed layer in through, blind, buried and micro spans. Historical findings and recovery evidence remain below. This is not a full workflow sign-off; remaining layout and manufacturing checks are listed in the handoff.

## Session and evidence

- Tested executable: `target/release/PCBPlannerCAD.exe`, version 0.6.0, built 2026-10-27 at 20:24:19 local time; repository baseline `15659c2`.
- Windows native application, with both a large window and a 1602 × 992 captured window. Display scaling was not independently recorded.
- All interactive edits used disposable copies under the Git-ignored `local-test-boards/workflow-2026-10-28/` directory. Original designs were preserved. The directory name comes from the existing checklist; this test session occurred on October 27.
- No product implementation was changed, and nothing was committed or pushed in this pass.
- Native evidence: `local-test-boards/workflow-2026-10-28/acceptance-output/results.json`; command log: `target/workflow-acceptance.log`.
- Local screenshot: `local-test-boards/workflow-2026-10-28/acceptance-output/stale-properties-and-messages.jpg`. It shows old PCB component editors beside the active schematic and the compressed Messages area.

## Checks performed

| Area | Evidence and result | Limit |
| --- | --- | --- |
| Open and navigate | Latest executable opened native board copies, a three-sheet project and a clean schematic project. Find located R1. | Full library creation and import fidelity were not audited. |
| Pad escape and different nets | Live routing started at clean pad centres on SIGNAL and GND, with visible tracks. Saved coordinates and net assignments were inspected. | Dense obstacle cases and every board side remain pending. |
| Via transition | Live Space picker opened, Enter inserted a via and continued on Bottom Copper. Backspace removed the transition and restored Top Copper. Esc dismissed the picker while preserving routing. | Inner-layer spans, blind/buried vias and blocked endpoints remain pending. |
| Native route round trip | Clean pad escape plus a top-to-bottom via produced no short/clearance violations; Undo, Redo and serialization round trips matched exactly. | Does not establish every online DRC state or rule. |
| Connected move/rotate | Native connected-footprint drag planning/execution reported zero blocked connections; Undo restored the exact prior document. | Final electrical topology after the transformed move was not independently asserted. Full live connected placement is pending. |
| Schematic wiring | Drew a wire between two resistor pins in the live editor. Saved endpoints were exactly `(0, -6.35 mm)` and `(25.4 mm, -6.35 mm)`. Compile returned 2 parts, 2 nets, zero errors and warnings. | New part placement, pin-type failures and hierarchy editing remain pending. |
| Schematic selection and Undo | A containing box selected all four schematic objects. Undo/Redo changed the new wire count correctly. Space rotated the selection; Undo plus save restored the exact original saved file. | PCB filter presets, Ctrl-click and crossing-box variants remain pending. |
| Three-sheet compile/ERC/BOM | Live and native compilation agreed: 160 parts, 248 nets, no errors, one multiple-net-names warning. BOM generated; save/reload preserved the compiled result. Latest native compile: about 0.003 s. | The fixture contains a net named +3.3V, VCC and VDD; the compiler chose +3.3V. That is a design warning, not a clean ERC sign-off. |
| Schematic-to-board changes | Native change plan contained 12 changes, no warning entries. Apply then Undo restored the prior board exactly, after loading required cached footprints. | Reannotation identity, user review/selection of changes and full live synchronization remain pending. |
| Medium-board DRC | 8,969 objects; full native DRC about 0.11 s, 67 violations, no rule parsing errors. | Existing violations were not individually adjudicated. |
| Larger-board DRC | 34,143 objects; native load about 1.06 s and full DRC about 1.05 s, 24,904 violations, no rule parsing errors. | A single warm-machine measurement, not a frame-rate or responsiveness benchmark. Imported geometry/rule compatibility needs review before interpreting the violation count. |
| Manufacturing package | 17 output files generated, including 11 Gerbers and 2 drill files plus assembly data/archive. Gerber/drill files parsed back with zero parser warnings. About 0.37 s for export and parsing. | Used the application's own reader. Independent CAM review, plotted geometry comparison and manufacturer acceptance are pending. |
| Schematic regression suite | 34 core schematic tests passed, covering connectivity, crossings/junctions, ERC, annotation, buses, power and hierarchy. | Automated coverage supplements live acceptance. |
| Editor regression suite | All 141 editor tests passed, including the public-presentation checks. | These do not detect the native window layout/reconciliation failures observed above. |

The earlier routing regression fixture placed R1 at X=42.2 mm outside a board outline ending at X=40 mm. Its blocked route was not treated as a router defect; pad escape was repeated on a clean in-outline fixture instead.

Timings above are the latest native release-mode run on this laptop. They exclude UI interaction, rendering and output review.

## Issues

### WF-001 — P1: Properties retains stale component editors

**Steps:** select R1 in one PCB, route or change the tool, open another PCB and select its R1, then switch to a schematic. The problem appeared repeatedly during the same application session.

**Expected:** the right panel displays only the active document's current selection, tool or document options.

**Actual:** multiple COMPONENT sections accumulate. A clean schematic still displays old PCB footprint fields, including X=42.200 mm, Y=-1.010 mm and unrelated pad nets. Selecting the clean PCB's R1 adds its correct editor farther down instead of clearing old editors.

**Impact:** the displayed properties cannot be trusted to describe the current selection. Editing stale controls risks changing an unintended object/document; no such data corruption was deliberately attempted.

**Code evidence:** `app/src/panels/PropertiesPanel.tsx` gives the single `ItemEditor` the item ID key and its sibling `BulkProperties` the same string key for a one-item PCB selection. The editor key also lacks document identity. This is a strong reconciliation/key-collision hypothesis, not a verified fix. Undo/save/reopen was not established as a remedy.

**Follow-up:** give sibling editors distinct keys scoped to document and role, then repeat single/multiple selection, placement mode and document switching before trusting property edits.

### WF-002 — P2: Messages controls crowd out readable messages

**Steps:** use the default short Messages area, then compile a project or route to generate messages.

**Expected:** severity controls, search/source filters and at least several message rows remain usable at the default panel height.

**Actual:** search, source selector and export/follow controls occupy separate rows, leaving the message table unreadable at the observed height. Accessibility data still contains the messages.

**Code evidence:** `MessagesPanel.tsx` uses a wrapping toolbar; `NativeSelect` defaults its wrapper to full width. This explains part of the excessive toolbar height. Dock sizing/follow scrolling should also be checked rather than assuming one CSS change fixes every symptom.

**Follow-up:** constrain filter widths, preserve a minimum log viewport and verify default, enlarged and narrow layouts. Until then, resize Messages before reviewing diagnostics.

### WF-003 — P2: Schematic selection hint advertises unsupported idle shortcuts

**Steps:** box-select the two parts and two wires in `clean-schematic.arsch`; press M. Read the selection hint and keyboard implementation.

**Expected:** advertised M move / R rotate / X route controls apply to the active editor, or the hint presents the schematic's actual controls.

**Actual:** M did not enter move mode. The hint advertises M, R and X, while the idle M/R handler in `app/src/editor/tools/select.ts` is restricted to PCB/footprint documents. Schematic Space rotates and X mirrors. Space rotation and Undo were verified live, including an exact saved-file comparison.

**Follow-up:** make selection hints document-specific and decide whether to provide matching idle M/R schematic actions. Retest dragging, attached wires, rotation, mirroring and cancellation.

## Priorities for a dependable editor

1. Fix selection-to-properties identity and diagnostic readability before expanding features. The user must know which object and document an edit affects.
2. Validate connectivity through every operation: snapped wire/pad endpoints, junctions versus crossings, connected moves, layer changes, Undo/Redo and reopen. Make blocking rules and their effective values visible during routing.
3. Strengthen the schematic workflow: pin electrical types, clear ERC navigation, net inspection across sheets, reviewed annotation and stable source-to-board identity when designators change. Audit symbol-pin to footprint-pad mapping and unresolved library references.
4. Complete practical layout acceptance: predictable box/additive selection, layer/locked filters, constrained widths and via spans, pours and clearances, differential-pair limitations, grid persistence and usable net/component navigation.
5. Treat manufacturing output as a release check: matching origins/units, correct layer stack and drill spans, BOM/placement consistency, independent visual CAM review and a recorded DRC/connectivity state.
6. Measure large-layout interaction separately from native computation: pan/zoom, hit testing, box selection, route preview, undo and DRC refresh while actively editing.

These priorities are engineering requirements inferred from the observed workflow and capabilities review, not claims that all are absent or already implemented.

## Remaining live acceptance

Keep the full checklist open for connected PCB placement/flip/cancel, additive selection and filter presets, Select Similar, component browser ranges, dense routing/width/corner modes, inner-layer and blocked vias, differential pairs, grid persistence, arrays, pours/teardrops, net browser races, DRC CSV/navigation, measurement/log exports and independent fabrication review. Sustained interaction on the large fixture is also pending.

WF-001, WF-002 and WF-003 were addressed in the follow-up below. No unchecked category is counted as passed by association with automated tests.

## Local fix follow-up and continued workflow

The requested three fixes are implemented locally, on top of the original session baseline. The first-pass observations above describe the old executable. Nothing has been committed or pushed.

- **WF-001:** the Properties body now remounts when document identity changes. Single-item and bulk editors have distinct keys; bulk controls are shown only in selection mode. Live testing selected PCB R1 (item 1), switched to the schematic and selected schematic R1 (also item 1): no stale COMPONENT or pad-net sections remained, and the correct PART editor appeared.
- **WF-002:** message filters stay on one horizontally scrollable row, with a bounded source selector. Follow scrolls only the message viewport, avoiding ancestor/dock scrolling. The Messages dock group also has a 170 px minimum height on initial creation, layout restore and panel reopening. The first panel-level constraint attempt had no effect in the installed docking library; investigation found the group-level setter is required. The final build was verified at 1602 × 992: headers, filter controls and actual message rows remain visible after window restore and compilation. Local evidence: `local-test-boards/workflow-2026-10-28/acceptance-output/messages-minimum-height-fixed.jpg`.
- **WF-003:** idle hints now distinguish PCB routing, footprint mirroring and schematic drag/Space/X controls. The schematic drag hint also describes its implemented rotation and mirror keys. This preserves existing schematic shortcuts. Two added regression cases check hints against the actual key actions; the live schematic status bar shows the corrected text.

Continued workflow evidence:

| Check | Result |
| --- | --- |
| PCB move pickup, rotation and cancellation | M picked up the selected connected footprint; R previewed a 90-degree turn. Esc cancelled both. Saving afterward produced an exact byte-for-byte match to the original disposable board. |
| Connected PCB move/rotate topology | The native acceptance runner now additionally checks physical connectivity: every SIGNAL track/via remains connected to source pad 1 after the transformed move. Three copper objects were checked, with zero blocked connections; Undo restored the exact prior document. This closes the earlier topology-assertion gap for this fixture. |
| Connected schematic drag | Dragged R1 by +1.27 mm X / -1.27 mm Y. Both attached wires' R1 endpoints followed by exactly that delta, while their R2 endpoints stayed fixed. Undo and save restored the original file exactly. |
| Grid stepping and final schematic compile | In the final executable, G changed 1.27 mm to 2.5 mm; Shift+G restored 1.27 mm. Reopening the saved schematic and compiling returned 2 parts, 2 nets, zero errors and warnings, visible in Messages. Custom grid editing/persistence remains pending. |
| Native acceptance rerun | All eight checks completed without assertion failures: pad/via round trip, connected move, saved schematic wiring, three-sheet compile/BOM, board synchronization Undo, two board-size DRC measurements and fabrication export/reimport. DRC fixture violations remain design-review work, as above. |
| Verification | TypeScript passed; all 143 editor tests passed, including public-presentation checks. Windows executable and installers rebuilt successfully. Logs: `target/workflow-fixes-typecheck.log`, `target/workflow-fixes-editor-tests.log`, `target/workflow-fixes-native.log`, `target/workflow-fixes-build.log`. |

Final executable timestamp: 2026-10-27 21:18:48 local time. The rebuilt application was left running on the saved clean schematic project.

The broad checklist remains deliberately open for arrays, differential pairs, inner-layer/blocked via cases, pour editing, browser/filter combinations, independent output review and sustained large-layout interaction. This follow-up concentrated on the reported defects and connected-edit recovery to keep validation cost bounded.

## Reproduce native checks

With the local fixture directory present:

```powershell
cargo run -p pcbplannercad-output --example workflow_acceptance --release --locked -- C:/Workspaces/PCBplannerCAD/local-test-boards/workflow-2026-10-28
cargo test -p pcbplannercad-core schematic --locked
pnpm -C app test:editor
```

`workflow_acceptance` strips pre-existing tracks/vias from its in-memory clean board before routing so repeated live saves do not contaminate the baseline. It writes only acceptance outputs, not the source fixtures. The clean schematic assertion expects the completed two-wire circuit. The optional three-sheet and larger-board checks run when those local fixtures exist; the demo designs are not committed with the runner.

## Routing, vias, arrays and pours — focused pass

This pass used the existing Windows build and a disposable four-layer board with two resistors, defined top-to-inner and inner-to-inner via types, and overlapping Ground/Power pours. Only acceptance helpers and notes were added; no product fixes, commit or push were made in this pass. Original designs were preserved.

| Check | Evidence and outcome |
| --- | --- |
| Existing native coverage | 19 routing/via, 26 pour and 5 copper-edit tests passed. Two ignored tests were not run. This covers walkaround/shove, unreachable endpoints, board boundaries, differential-pair spacing, via types, thermal reliefs, keep-outs, islands, priorities and transactional edits. |
| Four-layer route transaction | A native pad-centred SIGNAL route traversed Top, Inner 1, Inner 2 and Bottom using defined 0.1 mm and 0.2 mm drill spans and a 0.3 mm drill through-via fallback. All copper remained connected to R1.1; no short/clearance violations. Seven Undo/Redo steps recovered exact snapshots, including dependent pour fills. |
| Bottom pad escape | Native routing escaped R2's mirrored bottom-side pad centre and passed short/clearance checks. This was a native check, not a live bottom-pad placement sign-off. |
| Live routing through a foreign pour | Started R1.1 at exactly (9.2, 10) mm and committed to (9.2, 16) mm through Ground copper. Space offered Inner 1, Inner 2 and Bottom while excluding the current Top layer. Enter inserted the configured top-to-inner via. |
| Live routing controls | W changed 0.25 mm to 0.30 mm; 0 restored rule width. Shift+Space selected 90-degree corners; Shift+R selected Push. Shift+V returned from Inner 1 to Top; Backspace removed that transition and restored Inner 1; V continued to Inner 2. The saved vias had exactly the expected 0→33 / 0.1 mm drill and 33→34 / 0.2 mm drill spans. The saved live route passed native connectivity and short/clearance checks. |
| Rectangular arrays | Live 2×2 array produced R3–R5 at (15,10), (10,15), (15,15) mm with cleared copy nets and untouched originals. One Undo plus save restored the routed board byte-for-byte. Native/frontend cases separately verified both kept and cleared nets. |
| Circular arrays | Live six-position pattern around (15,10) mm produced five copies with 60/120/180/240/300-degree rotations, correct positions and cleared nets. An independent saved-file calculation verified each position. One Undo restored the original board exactly. Native cases also verified four-position patterns with rotated and fixed orientations. |
| Array/pour integration | All four frontend array plans were applied through native commands, explicitly repoured and checked for shorts/clearance, then undone/redone and serialized/reopened. Each stage matched exactly. These checks used the clean native baseline. Live array checks establish geometry and recovery; they do not certify placement clearances on arbitrary routed layouts. |
| Pour properties and overlapping refill | Live Pour Manager lowered Power priority from 20 to -10 and used “Refill selected + overlaps.” Power became empty and Ground's fill changed over its former area. One Undo restored both properties and fills byte-for-byte. Native priority/clearance/refill cases also passed. |
| Foreign-pad via rejection | **Failed: WF-004 below.** Rules were on; the editor accepted a through-via on a GND pad while routing SIGNAL from an inner layer. Native DRC confirmed the short. |

Evidence is under the ignored `local-test-boards/workflow-2026-10-28/acceptance-output/` directory:

- `layout-results.json`: eight passing native acceptance cases; `live-layout-results.json`: saved-file verifications and the failed rejection case.
- `live-routed.arpcb`, `live-rectangular-array.arpcb`, `live-circular-array.arpcb`, `live-pour-priority.arpcb`: archived live results.
- `live-via-crossnet.arpcb`, `inspection-drc.json`: failing via placement and native DRC report.
- Logs: `target/workflow-routing-tests.log`, `target/workflow-pour-tests.log`, `target/workflow-copper-edit-tests.log`, `target/workflow-layout-native.log`, `target/workflow-via-negative.log`.

### WF-004 — P1: routing via ignores a foreign pad on a spanned layer

**Reproduce:** open `live-routed.arpcb`; enable design rules, select Inner 2 and use Push mode with 90-degree corners. Start SIGNAL routing from the existing via at (15,16) mm. Commit an inner-layer track to (10.8,10) mm, the centre of top-side GND pad R1.2. Press V to continue on Bottom. The configured fallback is a through-via spanning Top through Bottom.

**Expected:** reject the via at that position, retain the current layer and avoid adding conflicting copper. The track's current-layer clearance alone is insufficient to validate a multi-layer via.

**Actual:** the editor inserted a SIGNAL via with 0.6 mm diameter / 0.3 mm drill at (10.8,10) mm and switched to Bottom. The current-layer track preview was clear, but the via touched GND on Top. Rules-on state was verified in the live accessibility tree.

**Native evidence:** error code `short`, rule `Clearance`, Top Layer, items 1 and 24: “Short circuit between GND and SIGNAL: Pad R1.2 (GND) touches Via (SIGNAL) on Top Layer”.

**Recovery:** the failing board was archived, then one Undo removed the via and another removed the approach track. Saving restored the exact pre-test board. The working copy was left saved with the valid earlier route.

**Code lead:** `app/src/editor/tools/route.ts` commits transitions after checking the track preview endpoint. That check does not establish via clearance across every layer in its actual span. The next fix should validate the chosen via geometry/span before committing or changing layers, while preserving safe failure, cancellation and route history. This is a reproducible defect, not a claim that the native DRC failed: native DRC correctly detected it.

### Remaining work after this pass

The WF-004 fix and native span matrix are recorded below. Remaining live work includes differential-pair UI and unsupported paired-via feedback, mixed-object arrays and their placement DRC, pour locking/thermal/island controls, and sustained dense-layout interactions. Custom grids, browser combinations, measurements/exports and independent manufacturing review also remain open.

The local acceptance helper can be rerun with:

```powershell
cargo run -p pcbplannercad-output --example layout_workflow --release --locked -- C:/Workspaces/PCBplannerCAD/local-test-boards/workflow-2026-10-28
```

It preserves an existing `layout-start.arpcb` instead of overwriting a board open in the editor. Optional second argument: a saved board path to inspect with native DRC; this writes `inspection-drc.json`. The local `generate-array-plans.mjs` and `verify-live-layout.mjs` scripts reproduce the frontend-plan and archived live-file checks when the ignored fixtures are present.

## WF-004 fix and dependable-editor follow-up

The native document transaction now plans routed vias on a copy, including shoved geometry and copper-pour refills, and runs focused DRC on each added or geometrically/electrically changed via. Validation uses the actual via span from the physical layer stack, including layers beyond the requested transition when the selected type falls back to a through-via. The transaction publishes nothing on rejection: document contents, item allocation, dirty state, redo history and undo history are preserved.

The native check is enabled by default through the document command endpoint. Interactive transitions and standalone via placement share the guarded route transaction. The main design-rule switch is forwarded explicitly; turning rules off remains an intentional bypass, with DRC still able to report the resulting violations. This is not universal rejection of every generic paste/array/property edit; the remaining workflow must still include DRC review.

On a rejected interactive transition, the router keeps its source layer and anchor, adds no route-history entry and displays a blocked endpoint plus a hint to read the rule/layer details in Messages. Moving to a clear position permits retry. Lock-only edits are allowed on existing invalid vias so they can be unlocked and repaired; unsafe geometry changes are still checked.

Validation:

- **197 native core tests passed**, including seven via-guard tests. The span matrix covers 36 combinations across six physical copper layers: through, micro, blind, buried, reversed endpoints and a through-via fallback. It rejects crossed-layer obstacles and allows obstacles outside the actual span.
- Additional guard cases cover top/bottom pads, keep-outs on all six layers, clearance without physical touch, same-net copper, refillable foreign pours, final shove geometry, explicit rules-off, lock-only repairs, original R1.2 reproduction, preserved redo/dirty state and unchanged ID allocation on failure.
- **146 editor tests passed**, including native-enforcement argument forwarding, source-layer/anchor preservation after rejection, retry without a spurious undo step and standalone via command routing. TypeScript passed.
- **93 import/output tests passed**: 43 importer unit tests, six importer integration tests and 44 output tests. Four ignored tests across the core/output suites were not run.
- **Nine local layout acceptance cases passed** through the guarded route path, including replaying the original saved failing transaction and verifying exact non-mutation on rejection.

The preceding failed rejection result is historical evidence from the old executable. Its archived invalid board is intentionally retained for reproduction and native DRC diagnosis; the fix does not silently modify loaded designs.

Final Windows executable/installers built successfully on 2026-10-27 (executable timestamp 22:22:04 local time). The live rebuilt editor rejected the SIGNAL through-via over GND pad R1.2, stayed on Inner 2 and displayed the native Top Layer conflict. Saving immediately afterward matched the pre-attempt board byte-for-byte, with the same 14 objects and no dirty flag. Backspace then removed only the legitimate approach track transaction; after finishing and saving, the original 12-object repro matched exactly. A legal same-net via was accepted and undone exactly as a positive smoke check. Local evidence: `via-guard-ui.json` and `via-guard-rejected.jpg` in the ignored acceptance-output directory.

For the next laptop, see [the dated handoff](HANDOFF-2026-10-28.md). Portable regression fixtures live in the source tests. Ignored demo/reproduction boards and private research are not included in Git.
