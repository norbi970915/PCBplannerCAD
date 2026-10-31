# PCBPlanner Studio

PCBPlanner Studio is a desktop PCB design suite for Windows: schematic capture, component libraries, PCB layout, 3D visualization and manufacturing outputs.

**Status:** v0.6.6 — Phases 0–5 implemented: libraries, schematic capture and ERC, PCB layout (routing, pours, rules, DRC), manufacturing outputs (Gerber X2, Excellon, BOM, pick-and-place, IPC-2581, PDF, panels), 3D view with STEP export, and import of schematics, boards and libraries. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Stack

| Part | Tech |
|---|---|
| Desktop shell | Tauri 2 (WebView2) |
| UI | React 19 + TypeScript, dockview panels, zustand state, shadcn/ui + Tailwind v4 |
| 2D canvas | Custom WebGL2 renderer with a built-in stroke font (`app/src/editor`) |
| Core engine | Rust (`crates/core`): model, commands, undo/redo, file I/O, schematic compiler, footprint wizard |
| PCB engine | Rust (`crates/core/src/pcb`): connectivity, DRC (R-tree), pours (i_overlay), router, ECO, panelization |
| Outputs | Rust (`crates/output`): Gerber X2 / Excellon writers and reader, assembly files, IPC-2581, STEP |
| 3D | three.js for local VRML models; STEP previews omitted in this release |
| Importers | Rust (`crates/import`): S-expression and XML design files |

All geometry is integer nanometers (`i64`), world Y up. The UI never mutates documents directly: it sends `Command`s to the Rust core, which returns deltas; every change is one undo step.

## Layout

```
crates/core/src/
  item.rs, library.rs, document.rs   data model (items, symbols, footprints, components)
  command.rs, session.rs             commands → edits, undo/redo
  io.rs, project.rs                  .arproj / .arsch / .arlib / .arpcb formats
  schematic/                         connectivity, netlist, ERC, annotation, BOM
  libgen/                            footprint wizard, symbol wizard, starter library
  pcb/                               board items, stack & rules, ratsnest, DRC, pours, router, ECO, panels
crates/import/src/                   external project, schematic, board and library parsers
crates/output/src/                   gerber, excellon, assembly, fab package, gerber reader, ipc2581, step
app/src-tauri/src/                   workspace (open docs, libraries, project) + IPC commands
app/src/
  core/                              types, API bindings, store, actions, clipboard
  editor/                            canvas, renderer, font, drawing, tools, editors
  library/                           library browser, component editor, wizards
  pcb/                               board dialogs (ECO, stack, rules, outputs, tuning, panelize…), PDF print
  view3d/                            3D viewer, model loading, Gerber viewer
  output/                            PDF writer
  panels/, shell/                    dock panels, menus, shortcuts, dialogs
```

## Prerequisites (Windows)

- Rust (stable, MSVC toolchain): `winget install Rustlang.Rustup`
- Visual Studio 2022 Build Tools with the "Desktop development with C++" workload
- Node.js 22+ and pnpm (`npm i -g pnpm`)
- WebView2 runtime (ships with Windows 11)

## Commands

```sh
pnpm install          # once, from the repo root
pnpm dev              # run the app with hot reload
pnpm build            # build installers -> target/release/bundle/{msi,nsis}
pnpm test             # all Rust tests (core, importers, end-to-end on installed sample designs)
pnpm typecheck        # TypeScript checks
```

In PowerShell use `pnpm.cmd dev`, or double-click `Start-PCBPlanner Studio.cmd` in the project folder. Keep the terminal open during development.

For a standalone app, run `pnpm.cmd build`. Share `target/release/bundle/nsis/PCBPlanner Studio_0.6.6_x64-setup.exe` (or the MSI in the sibling `msi/` directory). The installed app starts from the Windows Start menu without a development server. Native `.arpcb`, `.arsch`, `.arlib` and `.arproj` extensions remain supported; existing files also load with their earlier format tags, and newly saved files use `pcbplannercad.*` tags.

## View controls

With nothing selected in a board or footprint editor, switch the Properties panel to **View**. Control visible layers, component references and values, pad numbers and net names, graphics, tracks, vias and markers. **Snap to grid** controls placement and drag snapping; the idle selection cursor always moves freely. **Show grid** controls the grid display independently. **Saved views** stores named layer/label/object visibility presets locally per editor type; applying one preserves the current grid, snapping and active layer. View settings do not modify the design. Large copper fills prepare in the background: the canvas shows outlines first and a preparation status until their display is ready. Pours remain selectable around their holes during preparation. This changes display preparation only; saved geometry and design-rule checks are unchanged.

For continuation on another PC, start with the [2026-10-31 handoff](docs/HANDOFF-2026-10-31.md). It records the completed routing fixes, validation, setup and build commands, private fixture requirements and prioritized next steps.

For editor regressions run `pnpm.cmd -C app test:editor`. See [Latest workflow and editing update](docs/PROGRESS-2026-10-27-WORKFLOW.md) for continuation on another computer. The [current development and validation plan](docs/DEVELOPMENT-PLAN-2026-10-28.md) records implementation status, use cases, priorities and acceptance gates. The [earlier feature proposal](docs/NEXT-FEATURES-2026-10-27.md) remains historical context.

## Bringing designs in

Use the import actions in the File menu or Home tab to bring in supported project, schematic, board and library files. Choose an empty destination folder for an imported project. PCBPlanner Studio stores its sheets, board and embedded libraries together. Group/vector bus members and project bus aliases are retained across schematic sheets. Explicit board/BOM exclusions are preserved. Independent board graphics remain on the board during schematic updates; removals and connectivity changes require explicit selection in the review dialog.

The Components panel can import installed symbol and footprint libraries. Save imported libraries as `.arlib` files to keep them with your project. Native documents carrying bus aliases or independent-footprint retention use version 2; documents/libraries with layer-specific pad lands use version 3. Rebuild PCBPlanner Studio on both computers before opening these files. Earlier files remain readable, and older executables refuse these newer documents to protect their metadata.

Routing from an existing straight or curved track adopts its exact width for that session. A pad or via with one unambiguous connected width inherits it as well. W / Shift+W or the width field changes it; 0 restores the rule width. Finishing an inherited-width session restores the earlier width setting unless it was changed explicitly.

## Keyboard shortcuts

| Action | Keys |
|---|---|
| Place part / wire / bus / net label / global label / port | `P P` / `P W` / `P B` / `P N` / `P K` / `P H` |
| Power port VCC / GND, junction, no-connect, sheet symbol | `P V` / `P G`, `P J`, `P X`, `P S` |
| Symbol editor: pin · Footprint editor: pad | `P P` |
| Line, rectangle, circle, arc, polyline, text | `P L`, `P R`, `P C`, `P A`, `P Y`, `P T` |
| Schematic rotate / mirror (placing, dragging or selection) | `R` / `X` |
| Exit a tool | `Esc` |
| Pan during placement, routing or dragging | hold and drag the right or middle mouse button |
| Copy, cut, paste, duplicate | `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Ctrl+D` |
| Compile & ERC, annotate | `C C`, `T A` |
| Fit all, fit selection, zoom | `V F`, `V S`, wheel / `PgUp` / `PgDn` |
| Units, snap grid | `Q`, `G` / `Shift+G` |
| PCB/footprint layers, single layer mode | `+` / `-`, `Shift+S` |
| Highlight a net | `Alt`+click a wire or pin |

**Editing polygon pours:** `E C` cuts away the side you click, `E S` slices into two independent polygons, and `E R` reshapes or extends the boundary. Click the boundary, click any corners, then finish on the same boundary. Cut shows the two sides in green/red; click the red side to remove it. The tool stays active for repeated edits. Shift constrains corners to 45°, Backspace removes the last corner, and Esc cancels the current cut (Esc again exits). Outline changes and copper refill share one undo step. A mode strip and the selected polygon's context menu expose these actions too.

Cut, Slice and Reshape ignore the document grid and use free angles by default; hold Shift only when you want a 45° constraint. Subsequent cuts work on the new edges without restarting the tool.

Try it on [examples/polygon-playground.arpcb](examples/polygon-playground.arpcb), which contains a rectangle and a concave pour. `pnpm -C app test:polygons` runs the frontend geometry and interaction tests.

**Board**

| Action | Keys |
|---|---|
| Interactive route / differential pair / via | `X` at a pad or track, or `P T` / `P I` / `P V` (routing: `Space` via layer picker, `V` or `*` quick via, `Shift+Space` corner, `/` posture, `0` class width, `Shift+R` mode) |
| Polygon pour / keep-out / board outline / component | `P G` / `P K` / `P O` / `P C` |
| Move / rotate footprint | `M` / `R` (`Shift+R` reverses rotation; click places, `Esc` cancels a move and its preview rotation) |
| Flip footprint to the other side | `F` or `L` |
| Update PCB from schematic, layer stack, rules | `D U`, `D K`, `D R` |
| DRC, repour all, length tuning, clear markers | `T D`, `T G`, `T R`, `T M` |
| 3D view, fabrication outputs, print to PDF | `3`, `F F`, `Ctrl+P` |

Board selection filters are in the right **View** tab and stay available with an active selection. Drag empty space left to right to select enclosed objects, or right to left to select touching objects. Ctrl+click toggles individual objects; Shift adds, Ctrl toggles, and Ctrl+Shift subtracts box selections.

**Approving findings:** Each current DRC finding has an **Approve** action. Approved findings move to the collapsible **Approved** list; **Restore** returns a matching finding to review. Approvals are undoable and saved with the board. They match the exact finding, related geometry and rules, so changed findings need review again. The full report filters approval status and its CSV labels that status. Approval records do not disable routing checks. Boards containing approvals use native format version 5; older executables refuse these files rather than discard their review records.

**DRC and rules:** Running DRC selects the right **DRC** tab beside Properties and View, including in detached board windows. Findings are searchable, filterable and paged without truncating the result set; click a finding to select and center its board objects. Online checks update results without taking focus from another tab. **Design rules…** edits this board's net classes, pair definitions and ordered rules; invalid or stale drafts stay open for correction. **Apply & run DRC** checks the saved settings immediately. Put specific scopes above generic rules: routing width and pair gap use the same first matching rule as the native check.

**Connected moves:** Track slides and corner drags adjust nearby bends first, retaining the distant route corridor and expanding the affected section only when needed. Component and via moves rebuild attached routing, removing obsolete bends. Native planning provides the copper preview and the placement result; the last completed valid route stays visible during continuous pointer motion while a newer position is checked. Placement always requires the exact current target. Repeated moves reject self-crossings and acute reversals. Fixed branches, locked tracks and existing connections are protected even with design rules disabled. Move one straight segment or a connected two-segment corner; unsupported selections stay unchanged with an explanation. Right-drag pans throughout, R rotates components and Escape cancels.

**PCB editing:** Tools → Pour Manager edits priorities, clearances and thermal settings with undoable dependent refill. The Properties panel supports bulk track/via editing and explicit copper Lock/Unlock. Align, Distribute and Pack & Move use the current connected-move behavior; Tab changes that behavior during an M move. Blocked connected moves can be placed again or cancelled with Esc. An incomplete multi-part plan applies no changes. Tools → Teardrops can persist automatic updates after routing and connected moves.

**Routing and length tuning:** Route feedback shows whole-net copper length and session pair mismatch. Length tuning previews a selected track chain, tries smaller meanders up to the chosen amplitude, preserves existing fixed copper contacts and rechecks the displayed plan before applying it. Tuned tracks and dependent pour refill share one undo step. Pair continuation preserves both accepted endpoints and rejects invalid gaps. Space opens the next-layer picker; paired transitions add both vias and their escape necks in one undo step. Fanout uses scoped native clearances, and the final transaction checks copper on every crossed layer before accepting either via. Coordinated pair tuning remains pending.

**Manufacturing:** Fabrication packages separate plated drill data by physical layer span, including blind and buried vias. The layer map and ZIP include each span; through pads and physical drill hits stay unique even when pad land shapes vary by layer.

**Local project hub:** Browse a working folder with nested subfolders and saved-content thumbnails, create complete schematic + board projects, review electronics-library imports and map local STEP/VRML assets. Project revisions save local checkpoints, compare and export readable changes between them and restore them into a fresh copy. See [the local workflow guide](docs/LOCAL-PROJECT-WORKFLOW-2026-10-29.md) for supported formats and current limits.

## Local diagnostics

Use Help → **Record/Stop Canvas Timings** for a 30-second local CPU timing report in Messages. For background checks, `pnpm.cmd -C app test:editor` runs editor regressions; `pnpm.cmd -C app test:renderer` runs the optional isolated headless rendering check when Playwright is available. Read-only board profiling is available through `pnpm.cmd -C app benchmark:navigation <absolute-native-board-path>`. See [the latest handoff](docs/HANDOFF-2026-10-29.md) for setup, measured limits and home-laptop acceptance steps.

Library folders are discovered by their layout. Set `PCAD_LIBRARY_ROOT` to a data folder containing `symbols`, `footprints` or `3dmodels` to select one explicitly. The naming check scans source paths, identifiers, messages, metadata and documentation; only exact file-compatibility markers and required notices are exempt.

**Fabrication packages:** Each export publishes a complete new named folder, preserving previous packages. The dialog opens either its ZIP or unpacked folder in the viewer. Assembly variants apply to the current project board, with consistent CSV/XML BOM filtering.

**Recovery copies:** Periodically preserve unsaved native documents and editable libraries in local app data. File → Recovery copies restores into a new folder, with project links when available. External model assets and Undo history are excluded. File cards and icons use consistent colors in both themes.

## Continuing on another PC

See [the October 29 home-PC guide](docs/HOME-PC-CONTINUATION-2026-10-29.md) for safe sync, dependencies, rebuilding and prioritized native tests. Private designs and generated executables are not transferred by Git.

## Personal project and release terms

PCBPlanner Studio is the personal project of Buchmüller Norbert. See LICENSE.md for free-use terms and LICENSE/PRIVACY.md for local storage and recovery information. About provides third-party notices and source availability. STEP model previews and thumbnails are omitted from this release; VRML previews and STEP export remain available.

The legal preparation and verification scripts run before packaging. They include complete dependency notices and versioned covered-source archives. Do not distribute an executable separately from its matching installer resources. Public release still requires an explicit owner decision and current name/rights review.

The application is now named **PCBPlanner Studio**, with the **PCBPlanner Basic** starter library. Existing native file tags, extensions, internal package names and preference keys are retained for compatibility. The identifier is `com.pcbplanner.studio`; migration copies missing local data from earlier identities without deleting originals. Installers using the former publisher may appear as a separate installed app.

On a fresh development machine, install the locked JavaScript dependencies and run `cargo fetch --locked` before the offline legal preparation/build pipeline. Generated legal files and exact covered-source archives are release inputs, not optional documentation.
