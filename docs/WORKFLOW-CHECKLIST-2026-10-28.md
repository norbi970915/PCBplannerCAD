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
