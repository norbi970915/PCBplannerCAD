# PCBPlannerCAD workflow update — 2026-10-27

Continues [the editing update](PROGRESS-2026-10-27-EDITING.md). This batch uses source inspection and quick command-line checks. Live interaction, a complete layout workflow and large-board frame-rate measurements are deferred to the next session. The work was prepared locally; publication was subsequently explicitly requested by the user.

## New controls

| Area | What is available | Where to use it |
| --- | --- | --- |
| Routing controls | Obstacle mode, corner style, direct width choice, rule width and via target layer | Floating Interactive routing bar |
| Routing keys | Shift+Space cycles corners; 0 restores rule width; W / Shift+W steps width; V adds a via; Shift+V reverses automatic copper-stack stepping | While routing |
| Route backtracking | Backspace undoes the last committed step of the active route, including its via, and restores its anchor/layer | While routing; stops at the session boundary and refuses to undo unrelated edits |
| Selection presets | Components, Copper, Vias, Everything, plus optional exclusion of locked objects | View panel or selection-filter popup |
| Selection actions | Selected-object count with Zoom to selection and Clear selection | Select toolbar |
| Select similar | Same object type with optional layer/span, net, width/size, value or footprint criteria | Edit → Select Similar, or a single selected object's context menu |
| Arrays | Rectangular rows/columns and circular patterns; optional rotation and explicit net retention | Edit → Create Array or the selection context menu |
| Component browser | Natural reference sorting, value/footprint search, side/lock filters, Ctrl/Shift selection, zoom and Move Parts | PCB panel → Components |
| Net browser | Click column headings to sort; bounded 100-row pages; Select routing and Zoom net | PCB panel → Nets |
| DRC browser | Severity/type/layer/text filtering, related net/designator search, keyboard navigation, Show selected and filtered CSV export | Tools → Browse DRC Results or status bar → Issues |
| Grid settings | Exact custom mm/mil steps, presets, grid visibility and snapping; step remembered by editor type | View → Grid & Snapping, or click Grid in the status bar |
| Find | Filter by object type and scroll the active keyboard result into view | Ctrl+F |
| Measurement | C copies readout; Enter records it in Messages; Tab reverses endpoints; Backspace edits the second point | Measure Distance |
| Diagnostic logs | Text/source filters, Follow/pause, Copy shown and Save log | Messages panel |

## Behaviour and safeguards

- Identical routing previews are reused at unchanged snapped points. Width, corner, mode and board-geometry changes invalidate reuse. Transient failures can retry. Obsolete queued endpoints are dropped when the cursor returns to the in-flight point.
- Settings changes refresh routing previews immediately. Global Undo/Redo leaves routing to avoid keeping an anchor on removed copper; Backspace preserves its guarded routing session.
- Pair continuation now uses the active net's last endpoint even when the two tracks have different segment counts. Both sides must report success before the preview is considered reached. Paired via fanout remains unavailable and now explains that limit instead of silently trying a single via.
- Arrays leave the source cell in place, allocate case-insensitively unique component designators and add copies in one undo command. Copies are unlocked. Nets are cleared by default; the explicit keep-nets option shares the original nets. This does not create or remap schematic channels. Copied pours have empty fills and need repouring. Rotated rectangles become four-corner polylines. Plans reject repeated circular positions, unsafe coordinates, unsupported generated objects and more than 10,000 added objects. An open array dialog refuses to apply to a changed board.
- Similar-object selection and the browser's ordinary selection respect selection filters and hidden objects/layers. Everything resets type, layer and locked-object exclusions.
- DRC shows unchecked, checking, stale, current and failed states, with rule errors exposed. Stale asynchronous results are discarded, pending edits are requeued together and full checks are requested after rules or footprint geometry change. Manual full checks retain ownership while online work waits. Turning online checking off does not discard an explicitly requested manual check. Failed checks do not enter an automatic retry loop.
- Incremental online DRC omits batch-only unrouted markers; use a full DRC before final review. Ratsnest analysis and net highlights discard outdated replies. These changes do not claim measured frame-rate gains.

## Validation

138 editor checks passed before packaging, including array geometry/reference/net behaviour, circular orientation, selection/visibility rules, custom-grid stepping, measurement records, net sorting, DRC filtering/CSV and asynchronous DRC ownership/recovery. TypeScript checking passed. No native interaction tests were performed. No Rust model or file-format change was introduced in this batch.

Use [the next workflow checklist](WORKFLOW-CHECKLIST-2026-10-28.md) for acceptance. Persistent groups, schematic channel replication, custom pads/per-pad overrides, recursive via shoving and pair tuning remain separate projects.

## Packaging result

Final TypeScript checking, all 138 editor checks and the whitespace/diff check passed. The release executable and Windows MSI/NSIS installers built successfully. The existing dynamic-import chunking notice remains; it did not fail the build. Launch `C:\Workspaces\PCBplannerCAD\target\release\PCBPlannerCAD.exe` for the next acceptance session. Build outputs stay Git-ignored. No live board interaction, commit or push was performed.

## Space via-layer picker

During an active route, Space opens a small copper-layer picker at the cursor. Selecting a different layer commits the route and via at the endpoint captured when Space was pressed, then continues on that layer. Arrow keys/Tab choose, Enter confirms, and Esc or clicking outside cancels only the picker. The current layer is excluded. / flips corner posture; Shift+Space continues to cycle corner styles. V / Shift+V remains the quick via action. Ending the route or switching documents closes the picker. Paired via fanout remains unavailable.

## Publication check

After the Space via-layer picker, all 141 editor checks, TypeScript checking and the final Windows executable/MSI/NSIS build passed. Live board testing remains deferred to the checklist. Source and dated notes are included in the requested Git publication; personal boards, private comparison notes and build outputs remain excluded.
