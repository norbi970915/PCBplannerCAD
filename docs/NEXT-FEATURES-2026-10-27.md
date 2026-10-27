# PCBPlannerCAD next feature priorities — 2026-10-27

## Where to continue

This is a proposed development plan based on inspection of source commit `2436510`. The features below have not been implemented by this assessment. The latest completed-work handoff is [PROGRESS-2026-10-27-1544.md](PROGRESS-2026-10-27-1544.md).

On the other computer, open the PCBPlannerCAD checkout and run:

```powershell
git status --short
git pull --ff-only origin main
pnpm.cmd install --frozen-lockfile
pnpm.cmd dev
```

Preserve any local changes before pulling. For a new checkout, clone `https://github.com/norbi970915/PCBplannerCAD.git` and follow the prerequisites in the root README. Personal PCB projects and local test boards are separate from the application source and are not included in Git.

## Recommended order

Start with **the polygon/pour manager, bulk property editing and locking**, then automatic teardrop updates. These address repeated clicking and accidental changes in everyday polygon and layout work. Router and pad-model changes are larger follow-on projects.

Sizes are relative engineering estimates, not delivery promises.

| Priority | Feature | Current implementation | Proposed addition | Size |
| --- | --- | --- | --- | --- |
| 1 | Polygon/pour manager | Pours have individual properties; no central manager. | List pours with name, net, layer, priority, clearance and thermal settings. Highlight/zoom from a row, reorder priorities, and refill selected pours. | Medium |
| 2 | Bulk property editing | Multiple selection mainly shows counts and rotate, mirror and delete actions. | Edit common properties of several items together, starting with track width and via diameter/drill/span. Support mixed values and one undo step. | Medium |
| 3 | Lock tracks, vias and polygons | Footprints have locking; these copper items do not. | Protect finished routing and pours from movement, deletion and geometry edits, with a clear explicit unlock action. | Small–medium |
| 4 | Automatic teardrop updates | Stale teardrops are removed automatically; generation is invoked through a dialog. | Persist generation settings and create/regenerate affected teardrops after routing and connected moves, in the same undo transaction. | Medium |
| 5 | Via shoving and connected transformations | Shoving moves straight tracks; via obstacles stop the shove. Connected translation exists, with rotation/flip gaps. | Move obstructing vias with their attached tracks across the affected layers. Preserve connections when rotating/flipping components during movement. | Large |
| 6 | Custom pads and per-pad settings | Four basic pad shapes, one copper shape/size definition, and limited overrides. | Arbitrary copper shapes, drill offsets, individual mask/paste margins and thermal settings; later, pad shapes by layer. | Large |
| 7 | Groups and component arrays | Whole-board panelization exists; persistent editor groups and selected-object array tools are missing. | Move a group as a unit; duplicate selected objects in rows, grids or circles, with clear numbering and net behavior. | Medium |
| 8 | Differential-pair tuning | Length tuning accepts a single same-net track chain. | Tune pairs together, constrain skew and coupling, and display live progress toward the target. | Large |

## First implementation: polygon/pour manager

Suggested entry: **Tools → Pour Manager**. Ordinary pours should be clearly separated from generated teardrops and keep-outs.

- Show a searchable table with name, net, layer, priority and fill state.
- Selecting a row highlights its outline and offers zoom-to-pour.
- Edit clearance, connection/thermal settings and priority using the existing model.
- Reorder priorities with explicit up/down controls; handle existing equal priorities deterministically.
- Apply edits and dependent refill as one undoable change; Cancel leaves the board unchanged.
- Offer refill for selected pours and all pours. Account for overlapping pours affected by priority changes.

Useful starting points:

- `app/src/panels/BoardProperties.tsx`: existing zone property editing.
- `app/src/pcb/dialogs.tsx` and `app/src/pcb/BoardToolDialogs.tsx`: board dialog registration and patterns.
- `app/src/shell/commands.ts`: command registration.
- `crates/core/src/pcb/items.rs`: zone model.
- `crates/core/src/pcb/fill.rs`: fill computation.
- `crates/core/src/command.rs` and `crates/core/src/session.rs`: edits, refill and undo transactions.

Acceptance checks: overlapping pours on the same and different nets; holes and concave outlines; equal priorities; selected refill with overlapping neighbors; undo/redo; save/reopen; and a representative large board. Keep the board responsive while previews/refills are pending.

## Other implementation entry points

| Area | Relevant source |
| --- | --- |
| Bulk editing | `app/src/panels/PropertiesPanel.tsx` (multiple-selection branch), `app/src/panels/BoardProperties.tsx`, core command transactions |
| Locking | `crates/core/src/pcb/items.rs`, `app/src/core/types.ts`, selection/edit tools, cleanup and routing commands |
| Automatic teardrops | `crates/core/src/pcb/teardrop.rs`, `crates/core/src/session.rs`, `crates/core/src/command.rs`, `app/src/pcb/BoardToolDialogs.tsx` |
| Connected movement / shoving | `crates/core/src/pcb/drag.rs`, `crates/core/src/pcb/router_shove.rs`, `app/src/editor/trackDrag.ts`, `app/src/editor/tools/select.ts` |
| Pad model | `crates/core/src/item.rs`, frontend type mirror, draw/geometry, DRC, importers and manufacturing outputs |
| Pair tuning | `crates/core/src/pcb/router_tune.rs`, `crates/core/src/pcb/router.rs`, tuning dialog in `app/src/pcb/dialogs.tsx` |

For model changes, retain backward-compatible loading of existing designs. Locking needs consistent behavior in backend commands and automatic operations, as well as the UI. Pad geometry must agree across display, connectivity, DRC and exported copper/drills.

## Larger follow-on features

### Repeat layout and reusable circuit blocks

Place and route one channel, then transfer its layout to equivalent channels with correct footprint and net mapping. Build on groups and schematic-to-board identifiers. Validate topology and show mismatches before applying. Distinguish reusable circuit layout from existing whole-board panelization.

### Delay-based tuning

Add propagation-delay targets across layers and vias, then use these in tuning and DRC. Existing stackup data and physical-length tuning provide starting points. This requires explicit propagation parameters and consistent path accounting; physical length alone does not establish matched delay.

## Smaller additions

- Freeform lasso selection using the existing selection filters and add/subtract behavior.
- Radial and leader dimensions, plus dimensions in the footprint editor.
- Customizable shortcuts and toolbars with conflict detection and reset-to-default controls.

## Verification and outstanding work

The source assessment did not add features or repeat native interaction testing. Before the preceding source push, 96 editor tests, TypeScript checking and the Rust workspace tests passed. Some performance tests are ignored; fixture-dependent tests may return early when optional files are unavailable.

Carry forward the pending native checks from the completed-work handoff: the revised DRC markers, cursor placement after Update PCB, Find on a board, differential pairs with rule enforcement disabled, and online-DRC/teardrop performance on larger boards. That handoff also records that rule enforcement was left off during testing; verify the current setting before normal editing.

Validation commands from the repository root:

```powershell
pnpm.cmd -C app test:editor
pnpm.cmd typecheck
cargo test --workspace --locked
```

Use disposable design copies for native checks. Test undo/redo and save/reopen for each new editing feature. Rebuild release installers after acceptance; source updates do not update an already-running executable.

## Publishing preferences

Public documentation should describe PCBPlannerCAD directly, without comparisons to other CAD products. Keep private reference notes, local test boards, personal projects and build outputs out of commits. Stage explicit paths and inspect the staged file list before pushing.
