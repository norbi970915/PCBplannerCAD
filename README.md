# pCAD

pCAD is a desktop PCB design suite for Windows: schematic capture, component libraries, PCB layout, 3D visualization and manufacturing outputs.

**Status:** v0.6.0 — Phases 0–5 implemented: libraries, schematic capture and ERC, PCB layout (routing, pours, rules, DRC), manufacturing outputs (Gerber X2, Excellon, BOM, pick-and-place, IPC-2581, PDF, panels), 3D view with STEP export, and import of schematics, boards and libraries. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Stack

| Part | Tech |
|---|---|
| Desktop shell | Tauri 2 (WebView2) |
| UI | React 19 + TypeScript, dockview panels, zustand state, shadcn/ui + Tailwind v4 |
| 2D canvas | Custom WebGL2 renderer with a built-in stroke font (`app/src/editor`) |
| Core engine | Rust (`crates/core`): model, commands, undo/redo, file I/O, schematic compiler, footprint wizard |
| PCB engine | Rust (`crates/core/src/pcb`): connectivity, DRC (R-tree), pours (i_overlay), router, ECO, panelization |
| Outputs | Rust (`crates/output`): Gerber X2 / Excellon writers and reader, assembly files, IPC-2581, STEP |
| 3D | three.js, OpenCascade (occt-import-js, WASM) for STEP models |
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

In PowerShell use `pnpm.cmd dev`, or double-click `Start-pCAD.cmd` in the project folder. Keep the terminal open during development.

For a standalone app, run `pnpm.cmd build`. Share `target/release/bundle/nsis/pCAD_0.6.0_x64-setup.exe` (or the MSI in the sibling `msi/` directory). The installed app starts from the Windows Start menu without a development server. Native `.arpcb`, `.arsch`, `.arlib` and `.arproj` extensions remain supported; existing files also load with their earlier format tags, and newly saved files use `pcbplannercad.*` tags.

## View controls

With nothing selected in a board or footprint editor, switch the Properties panel to **View**. Control visible layers, component references and values, pad numbers and net names, graphics, tracks, vias and markers. **Snap to grid** controls placement and drag snapping; the idle selection cursor always moves freely. **Show grid** controls the grid display independently. View settings do not modify the design. Large copper fills prepare in the background: the canvas shows outlines first and a preparation status until their display is ready. Pours remain selectable around their holes during preparation. This changes display preparation only; saved geometry and design-rule checks are unchanged.

For editor regressions run `pnpm.cmd -C app test:editor`. See [Latest workflow and editing update](docs/PROGRESS-2026-10-27-WORKFLOW.md) for continuation on another computer. The [current development and validation plan](docs/DEVELOPMENT-PLAN-2026-10-28.md) records implementation status, use cases, priorities and acceptance gates. The [earlier feature proposal](docs/NEXT-FEATURES-2026-10-27.md) remains historical context.

## Bringing designs in

Use the import actions in the File menu or Home tab to bring in supported project, schematic, board and library files. Choose an empty destination folder for an imported project. pCAD stores its sheets, board and embedded libraries together.

The Components panel can import installed symbol and footprint libraries. Save imported libraries as `.arlib` files to keep them with your project.

## Keyboard shortcuts

| Action | Keys |
|---|---|
| Place part / wire / bus / net label / global label / port | `P P` / `P W` / `P B` / `P N` / `P K` / `P H` |
| Power port VCC / GND, junction, no-connect, sheet symbol | `P V` / `P G`, `P J`, `P X`, `P S` |
| Symbol editor: pin · Footprint editor: pad | `P P` |
| Line, rectangle, circle, arc, polyline, text | `P L`, `P R`, `P C`, `P A`, `P Y`, `P T` |
| Schematic rotate / mirror (placing, dragging or selection) | `Space` / `X` |
| Finish a wire or exit a tool | right-click / `Esc` |
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
| Interactive route / differential pair / via | `X` at a pad or track, or `P T` / `P I` / `P V` (routing: `V` or `*` via + layer, `/` or `Space` corner, `Shift+R` mode) |
| Polygon pour / keep-out / board outline / component | `P G` / `P K` / `P O` / `P C` |
| Move / rotate footprint | `M` / `R` (`Shift+R` reverses rotation; click places, `Esc` cancels a move and its preview rotation) |
| Flip footprint to the other side | `F` or `L` |
| Update PCB from schematic, layer stack, rules | `D U`, `D K`, `D R` |
| DRC, repour all, length tuning, clear markers | `T D`, `T G`, `T R`, `T M` |
| 3D view, fabrication outputs, print to PDF | `3`, `F F`, `Ctrl+P` |

Board selection filters are in the right **View** tab and stay available with an active selection. Drag empty space left to right to select enclosed objects, or right to left to select touching objects. Ctrl+click toggles individual objects; Shift adds, Ctrl toggles, and Ctrl+Shift subtracts box selections.

**PCB editing:** Tools → Pour Manager edits priorities, clearances and thermal settings with undoable dependent refill. The Properties panel supports bulk track/via editing and explicit copper Lock/Unlock. Align, Distribute and Pack & Move use the current connected-move behavior; Tab changes that behavior during an M move. Blocked connected moves can be placed again or cancelled with Esc. An incomplete multi-part plan applies no changes. Tools → Teardrops can persist automatic updates after routing and connected moves.

## Local diagnostics

Use Help → **Record/Stop Canvas Timings** for a 30-second local CPU timing report in Messages. For background checks, `pnpm.cmd -C app test:editor` runs editor regressions; `pnpm.cmd -C app test:renderer` runs the optional isolated headless rendering check when Playwright is available. Read-only board profiling is available through `pnpm.cmd -C app benchmark:navigation <absolute-native-board-path>`. See [the latest handoff](docs/HANDOFF-2026-10-28.md) for setup, measured limits and home-laptop acceptance steps.

Library folders are discovered by their layout. Set `PCAD_LIBRARY_ROOT` to a data folder containing `symbols`, `footprints` or `3dmodels` to select one explicitly. The naming check scans source paths, identifiers, messages, metadata and documentation; only exact file-compatibility markers and required notices are exempt.
