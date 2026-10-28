# PCBPlannerCAD Roadmap

> Current priorities and verified status: [development and validation plan, 2026-10-28](DEVELOPMENT-PLAN-2026-10-28.md). This document retains historical phase/proposal details; some unchecked items and known gaps have since been implemented. See the current plan and handoff before starting work.

Target platform: **Windows desktop** (installable `.msi` / `.exe`). macOS/Linux are possible later with the same stack but are out of scope for now.

## Phase 0: Foundations ✅

- [x] Cargo + pnpm workspace, Tauri 2 shell, Windows installers
- [x] Rust core: nanometer units, document model, commands → edits, undo/redo, dirty tracking
- [x] Native file formats, sorted pretty JSON (diff-friendly)
- [x] Docking UI (Projects, Components, Properties, Messages), menu bar, status bar
- [x] WebGL2 canvas: adaptive grid, anti-aliased lines and fills, stroke font, pan/zoom, snapping
- [x] Sequential keyboard shortcuts, layer tabs and floating tool strip
- [ ] Spatial index (R-tree) for hit testing on very large designs
- [ ] Context menus, user preferences dialog

## Phase 1: Library + Schematic ✅ (v0.2.0)

**Model** — components = symbol (multi-unit) + footprint options + pin→pad map + parameters + 3D model link;
schematic sheets keep a snapshot of every placed part so designs open without their libraries.
Files: `.arproj` project, `.arsch` sheet, `.arlib` library, `.arpcb` board.

**Libraries**
- [x] Library tab: components / symbols / footprints with search, preview, create, duplicate, delete, save
- [x] Component editor: symbol link, footprint options (default + alternates, cross-library `Lib:Name`), parameters, pin→pad map with mismatch check
- [x] Symbol editor: units A/B/C…, pins (type, shape, length, hidden), lines, rectangles, circles, arcs, polylines, text; symbol options (prefix, pin number/name display, field placement, power symbol)
- [x] Footprint editor: SMD / THT / NPTH pads (rect, rounded, circle, oval, slots), graphics on all layers, single-layer mode
- [x] Footprint wizard (IPC-7351B nominal): chip, SOD/SMx, SOT, SOIC/SSOP/TSSOP, QFN, QFP, DIP, pin headers, axial, radial, crystals — live preview
- [x] Symbol wizard: paste a pin table → IC box symbol on the 1.27 mm grid
- [x] Built-in "PCBPlannerCAD Basic" library (30 parts, 56 footprints), generated in code — no licensing strings attached
- [x] Components panel: search across all open libraries, symbol + footprint preview, Place; one-click import of installed part libraries

**Schematic**
- [x] Parts (multi-unit cycling, rotate/mirror with Space/X), wires with L-routing and auto-junctions, buses, bus entries
- [x] Net labels (local), global labels, hierarchical ports, power ports (bar/arrow/ground/earth…), no-connect, text, graphics
- [x] Hierarchical sheet symbols with entries; double-click a sheet to open its child
- [x] Snapping to pins/wire ends, drag parts with wires following, copy/cut/paste/duplicate (also across sheets)
- [x] Properties panel for every object, placement options while a tool is active
- [x] Compile: netlist across the hierarchy, ERC (unconnected pins, output conflicts, undriven power, single-pin nets, dangling wires, duplicates, unannotated, sheet entry mismatches) — click a message to jump to it
- [x] Annotate (incremental / all, packs multi-unit parts), BOM CSV export, Alt+click net highlight

**Importers**
- [x] S-expression projects, symbol libraries, footprint libraries and hierarchical schematics; reused sheets become one copy per instance. Verified against 222 symbol libraries / 22,776 symbols and 15,435 footprints.
- [x] XML libraries and multi-sheet schematics; supply symbols become power ports and named net connections are preserved. Verified against 39 example libraries.
- Verified: three schematic fixtures compile with zero ERC errors; the reference board fixture preserves all pin connections.

**Known gaps (carried forward)**
- Per-pin name/number visibility, De Morgan bodies, alternate pin functions, bus vector expansion (`D[0..7]`)
- Older legacy source files, source design blocks (`<modules>`) and curved net wires
- Pads: custom shapes, chamfers, drill offsets, paste/mask margins

## Phase 1.5 (next, small)

- [ ] Schematic → PCB netlist handoff checks (every part has a footprint, pins match pads)
- [ ] Hidden power pins of unplaced multi-unit parts
- [ ] Preferences: mouse wheel behavior (zoom or scroll), grid presets, colors

## Phase 2: PCB core ✅

- [x] Board document with a singleton board-settings item: layer stack (1–16 copper layers, dielectrics, mask/silk colors), rules, net classes, differential pairs
- [x] Layer stack manager (D K), board outline tool (P O), footprint placement (P C; Components ▸ Place on a board), flip to bottom (L)
- [x] Update PCB from schematic (D U): ECO dialog with add / remove / change footprint / value / nets / parameters / net renames, each selectable
- [x] Cross-probing: selecting parts in a schematic marks their footprints on the board and vice versa
- [x] Ratsnest (live, after every edit), interactive routing (P T) with walk-around / push / ignore modes, 45° / 90° / any-angle corners, vias with layer switching (`*`)
- [x] Query-based design rules (`InNet('GND')`, `InNetClass('Power')`, `OnLayer(…)`, `IsPad`, `HasFootprint('SOIC*')`, and/or/not; first match wins) — Rules dialog (D R)
- [x] Online + batch DRC (T D): clearance, shorts, track width, via style, hole size, board-edge clearance, courtyard overlap, silk over pads, keep-outs, unrouted nets — markers on the board, messages jump to them
- [x] **Board import:** layer stack, tracks, arcs, vias, zones, net classes and embedded footprints; imported together with the project
- Verified: imported reference designs show no unexpected schematic-to-board update changes in the exercised end-to-end tests. See fixture-dependent verification limits in the test notes.

## Phase 3: Manufacturing ✅

- [x] Polygon pours with priorities, clearances, thermal reliefs or direct connect, island removal; keep-out areas (P G, P K, T G repour)
- [x] Gerber X2 (copper, mask, paste, silk, outline), Excellon PTH/NPTH, pick-and-place in supported assembly formats, BOM, IPC-2581, one-click fab package with .zip (F F)
- [x] Built-in Gerber viewer to check the output (File ▸ Gerber Viewer)
- [x] Print to PDF (Ctrl+P) for sheets and boards
- [x] Panelization: N × M arrays with rails, fiducials, routed gaps or V-score lines (Tools ▸ Panelize)
- Milestone still open: order a real board from the generated files and confirm it works

## Phase 4: 3D ✅

- [x] 3D view (key `3`): board solid with holes and cutouts, top/bottom copper-mask-silk textures, component models
- [x] STEP / STP and VRML component models, placeholder boxes otherwise; model paths resolve from the source library configuration
- [x] STEP export (meshed solids) and OBJ export for MCAD

## Phase 5: Pro features ✅

- [x] Push-and-shove routing, differential-pair routing (P I), length tuning with accordion / trombone meanders (T R)
- [x] Impedance calculator (microstrip / stripline, IPC-2141A)
- [x] Assembly variants (Project ▸ Variants) applied to BOM, pick-and-place and fab outputs; per-part "Not fitted"
- [x] Scripting console (Tools ▸ Script Console): JavaScript API over the open document and the backend commands

**Known gaps**
- STEP export writes faceted (meshed) solids, not exact B-rep
- Shove router moves tracks only (no via shoving); diff-pair tuning tunes each line separately
- No blind/buried via spans in the stack manager UI (the model supports `fromLayer`/`toLayer`)
- Large boards: first open is a few seconds in debug builds; release builds are much faster

## Import architecture notes

The data model preserves component units, explicit pin-to-pad mappings, footprint options, parameters, hierarchical sheets and layer mappings. Importers report any source data they cannot represent.

Importers live in their own crate (`crates/import`), produce core `Command`s / documents, and report anything they can't map to the Messages panel.
