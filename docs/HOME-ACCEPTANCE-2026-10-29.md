# Native acceptance and fixes — 2026-10-29

Source baseline: e4f4d84. Changes remain local and uncommitted; no push was requested. The standalone target/release/pCAD.exe was rebuilt with the fixes below and exercised on disposable files. Personal working designs were not edited.

## Defects fixed

### Connected idle rotation

A native test reproduced R rotating a selected footprint while leaving its attached track at the old pad position. Idle R, selected Space rotation and menu rotation now share the connected-move planner when copper connections are present and Disconnect is not selected. The complete footprint/copper plan commits as one routing transaction. Stale board geometry, footprint definitions, reload epochs, selection, settings, cancelled operations and blocked plans do not commit.

Native verification: the signal pad rotated to (10, 12) mm, replacement copper reached its exact center, and the far via stayed at (32, 10) mm. One Undo restored the original state; Redo reproduced the exact saved data. Explicit Disconnect keeps its existing direct-rotation behavior. Property-based transforms and mirror workflows still need their own acceptance; this does not establish every transform path.

### Legacy project creation

The legacy create command could silently adopt an unrelated existing Sheet1.arsch and used direct writes. It now preflights both destinations and uses guarded staged writes. If project publication fails, it removes only its own unchanged companion file. An external project or a companion changed by another writer is preserved. Four filesystem regressions cover collisions, publication failure, external edits during rollback and successful reopen. This is not a crash-atomic two-file transaction; the newer project hub already publishes a complete new folder.

## Native cases passed

- Linked schematic component double-click selected and fitted the correct board footprint.
- An externally edited working board caused Save to reject while preserving external bytes, live rotation, dirty state and working path.
- Save Copy wrote the current 90-degree rotated design under a new filename. The original linked board path and project link stayed unchanged; Undo/Redo still worked.
- A native detached board retained selection and unsaved work. Undo from its window updated the main view; closing the secondary window returned its board tab without losing state.
- Automatic and explicit recovery copies contained the unsaved board, schematic and editable library. A native restart discovered them. Restore created a new linked project; all four recovered files exactly matched the captured payload and the original external board was preserved. The restored project opened successfully.
- Routing started at the rotated signal pad center and measured 22.828428 mm of existing copper. Manual targets produced Below target, In range and Over target states without changing copper.
- Space opened the via layer picker showing the current top layer and bottom continuation. Escape cancelled it without changing layer, copper or saved state.
- Native tuning preview showed a 20 mm selected chain becoming 22 mm, in range. Apply added approximately 2 mm, retained the fixed via, and saved correctly. One Undo restored the exact pre-tuning saved design.
- The actual six-layer project opened in the rebuilt app. Its native Engineering change order showed 69 parameter updates and the existing unmatched mechanical-pad warning, with no destructive connectivity proposals. The review was cancelled without applying changes; the board is left open for continued testing.

Synthetic fixture unit indexing was corrected before these acceptance cases. Its initial invalid-fixture diagnostic is not counted as a product defect. The recovery restart was orderly; forced crash and power-loss behavior were not tested.

## Background and file-level evidence

- 234 editor tests passed, including public-presentation checks and four new connected-rotation safety regressions. TypeScript passed.
- Native app/workspace tests: 25 passed, including four new project-creation filesystem regressions. The unchanged full-suite baseline passed 266 core, 53 import and 49 output tests; three core and one output optional performance tests were ignored.
- Configured import tests also ran against installed sample data, including actual symbol/footprint libraries, demo schematics and boards: 53 unit tests passed. Optional integration entry counts alone are not proof of every external fixture family.
- Isolated headless checks passed 23 docking/save workflows and 26 project/recovery workflows. Browser IPC is mocked; native filesystem checks and desktop observations are separate evidence.
- The six-layer import audited 8 sheets, 89 parts, 158 compiled/board nets, 32 pairs and zero ERC findings. Review contains 69 parameter updates and one unmatched mechanical-pad warning, with no removals or pad-net updates. No ECO was applied. Existing 107 board DRC findings remain.
- All eight sampled single-track routes and all eight single-chain tuning probes succeeded. Five paired probes succeeded; two starts are blocked by ground pads and one path fails the required pair gap. No clearance was relaxed.
- A separate disposable workflow passed pad routing, via insertion, exact Undo/Redo/save/reopen and connected rotation/movement. Full DRC on its 8,969-object board completed in about 0.91 seconds in an unoptimized CLI run and reported 67 existing findings; this is not a navigation/frame-rate benchmark.
- Fabrication generation produced 17 files: 11 Gerbers, two drill files, two CSVs, a layer map and ZIP. Built-in reimport reported no parser warnings. A separate Python standard-library check verified all 16 ZIP members byte-for-byte, archive CRCs/path safety, 189 placement rows against saved board positions/rotations/sides, and Gerber/drill headers and terminators. This is not independent graphical manufacturing approval.

Evidence remains ignored under target/acceptance-* and a disposable temporary design folder. Recovery records remain in local app data. Private copies, logs and external designs must not be committed.

## Remaining acceptance

- Actual multiple-monitor/mixed-DPI behavior, repeated cross-window dragging, native popup denial, main-window minimization and input focus across all panel types.
- Save All/dialog variants beyond the targeted desktop cases, large-file save/recovery latency, actual crash restoration and external model asset recovery.
- Pair starts/termination in constrained pad geometry, repeated turns/Backspace, native pair gauge, coordinated pair meanders, persistent skew rules and paired layer transitions.
- Native editing of the imported bus hierarchy and deliberate review of its unmatched mechanical pad and metadata changes. Never infer a pin-map repair from the warning alone.
- Pending refill/display cancellation and Undo races, connected Distribute/blocked Pack, mixed-layer transforms, and independent graphical/physical manufacturing checks.

The broader checklist remains open where its full case was not exercised. Rebuild again after future source changes; this report does not authorize a push.
