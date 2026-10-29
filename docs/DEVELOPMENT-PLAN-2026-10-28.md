# pCAD development and workflow validation plan

Date: 2026-10-28, 15:02 (Europe/Bucharest).
Source baseline: `a0fc787`.

This plan records the current findings, representative use cases, implementation order and acceptance gates. It is proposed work, not a claim that the pending features or desktop checks are complete. Use this document for current priorities; older roadmap tables remain historical and contain stale gaps.

## Goal and development order

Make pCAD dependable for complete projects, then extend advanced layout capabilities. Feature availability, automated verification and native desktop acceptance are separate states.

1. Reconcile the roadmap and accept the delivered native workflows.
2. Close editing-validation, file-saving and recovery gaps.
3. Add persistent groups and improve everyday selection and view workflows.
4. Extend pad geometry and library reliability.
5. Add paired via transitions and coordinated differential-pair tuning.
6. Complete independent manufacturing-output review and a physical prototype.
7. Extend reusable circuit blocks, bus routing, mechanical checks and multi-board work only after the earlier gates.

These are dependency-based milestones, not calendar delivery promises. Fixes for data loss, wrong connectivity or incorrect output take precedence over feature additions.

## Current evidence and unresolved work

| Area | Current finding | Next action |
| --- | --- | --- |
| Polygon editing | Repeated free-angle Cut, Slice and Reshape exist, including edits on new edges. | Native stress acceptance: concavity, holes, narrow slivers, cancellation, refill and recovery. |
| Connected placement | Via and footprint movement, rotation/flip, connected Align/Distribute and Pack are implemented. Failed multi-part plans return no partial edits; rejected single drops can retry. | Rebuild and exercise real pointer input, locked/blocked cases, stale responses and exact Undo/Redo. |
| Large-board display | Spatial picking, retained drawing batches and background large-pour display preparation are implemented. | Measure native latency, memory and presentation on both PCs; verify visibility and cancellation. |
| Rules and editing | Router/via checks exist. Failed constrained rerouting no longer silently falls back to Ignore. | Audit all direct and indirect editing paths for consistent enforcement; full DRC remains necessary. |
| Arrays and locking | Rectangular/circular arrays, bulk copper properties and locks exist. | Complete mixed-selection, placement-DRC and persistence acceptance. |
| Groups | Persistent editor groups remain future work. | Define membership, transforms, lock policy and connections crossing group boundaries. |
| Pads | Basic pad shapes exist; richer custom geometry and overrides remain gaps. | Introduce compatible model extensions in stages, using one consistent geometry interpretation. |
| Differential pairs | Pair routing and individual length tuning exist. Paired via fanout and coordinated pair tuning remain gaps. | Add pair-aware transitions, coordinated tuning and target checks. |
| Schematic and project flow | Capture, ERC, annotation and schematic-to-board updates exist. Several full-project interactions remain unchecked. | Complete hierarchy, mapping, identity and change-review acceptance. |
| Manufacturing and 3D | Fabrication/assembly outputs and mechanical export exist; STEP solids are faceted. | Independently inspect outputs and manufacture a small prototype. |
| Saving and recovery | Safe writes, project checkpoints and separate automatic recovery copies are implemented; see the 2026-10-29 handoff for later evidence. | Complete native restart/recovery and large-board profiling; complete native external-save conflict/Save Copy acceptance and audit legacy creation paths. |

Automated baseline recorded for the delivered source: 185 editor tests, 216 core tests, 44 import tests, 45 output tests and 24 isolated headless WebGL checks passed. TypeScript, frontend production build and native app integration passed. Three optional core performance tests and one output performance test were ignored. Six fixture-dependent integration tests reported success but may skip their bodies without configured inputs. A bundled fill-worker smoke check passed under the app's content-security policy.

These results do not sign off native interaction or manufacturing correctness. The running desktop executable was preserved and predates the latest source. Rebuild before acceptance. See [the handoff](HANDOFF-2026-10-28.md) and [the workflow checklist](WORKFLOW-CHECKLIST-2026-10-28.md).

## Workflow assessment method

Assess complete tasks rather than counting menu entries. Record a distinct state for each capability:

- Missing: no usable implementation.
- Partial: only part of the task works or important limitations remain.
- Implemented: code supports the intended task.
- Automated-tested: specified invariants have repeatable checks.
- Desktop-accepted: the complete task passed in a recorded native build.

A desktop-accepted state can still carry explicitly documented scope limits. Reopen acceptance after changes to the relevant geometry, transaction or renderer path.

For each task record the exact build/version, board, hardware, display scaling, layer, tool and rule settings; steps and shortcuts; click count and completion time; geometry/connectivity/DRC result; cancellation and Undo/Redo; save/reopen; and performance evidence. Repeat important timing tasks after warm-up and retain distributions and outliers, not only a best result.

Use official capability descriptions to identify candidate requirements. Documentation establishes intended capability, not observed interaction quality. Any reference execution requires an accessible installation, its exact version and a recorded procedure. Reference research and comparisons remain in ignored local notes; public deliverables describe pCAD directly.

## Representative use cases and fixtures

Create original, small synthetic fixtures suitable for publication. Keep personal boards, copied external designs and diagnostic reports ignored. The examples below are proposed acceptance workloads, not already executed tests.

| ID | Project | Tasks and expected evidence |
| --- | --- | --- |
| UC-01 | Small two-layer board | Capture a schematic, map footprints, update the PCB, place and route, fill pours and generate a complete fabrication package. Reopen without unexpected changes. |
| UC-02 | Four-layer controller | Use inner planes, varied footprints, net rules and layer transitions. Move connected parts/vias and apply reviewed schematic changes while retaining identity and connectivity. |
| UC-03 | Repeated-channel board | Verify arrays, numbering, net-retention options and later groups. Future layout reuse must validate circuit/net mapping before copying routing. |
| UC-04 | Differential-pair board | Route unequal bends and obstacles, change layers and tune. Measure width, gap, coupled/uncoupled sections, length and skew against explicit targets. |
| UC-05 | Polygon stress board | Repeated free-angle cuts/slices/reshape, concavity, holes, overlaps, priorities, thermal reliefs and islands. Compare outline, fill, DRC and output. |
| UC-06 | Dense private board | Measure load, first usable canvas, full-fill completion, navigation, selection, highlighting, edits and memory over a sustained session. Never commit the fixture or reports. |
| UC-07 | Library and mapping project | Create/replace footprints, verify pin-to-pad mappings, embedded data, missing models and portable project reopening. Include richer pads as those stages land. |

Every fixture needs an expected result or an independent oracle. For example: moving one via retains three specified connections and its ID, leaves locked copper exact, cancels without changes and recovers in one Undo step.

## Milestone 1: native acceptance of delivered work

### Work

- Reconcile stale roadmap items against the current source and this status table.
- Rebuild and record the executable's source revision and build timestamp.
- Exercise via/footprint moves, rotation/flip, Align, Distribute and Pack with distinct offsets, mixed layers and locked attachments.
- Test blocked placement, retry, Esc, fast pointer release and rapid new operations while planning is pending.
- Exercise repeated polygon edits on both original and newly created edges; verify dependent refill.
- Toggle layers, filled pours and active-layer-only display; switch/close documents and Undo/refill while fill jobs are pending.
- Verify point/crossing-box selection around copper and holes before and after preparation.
- Save/reopen and independently inspect printed copper/holes.

### Exit gate

No unresolved data-loss, wrong-connectivity, partial-transaction or Undo defects in these workflows. Record actual desktop outcomes in the workflow checklist; headless evidence alone does not complete it.

## Milestone 2: editing safety and recovery

### Work

- Inventory validation and locks for dragging, single/bulk Properties, paste, arrays, rotation, flip, cleanup, scripting and source-to-board updates.
- Share geometry and rule decisions where operations should have equivalent behaviour.
- Extend native transaction guards where validation can be bypassed by direct edits. Reject a failed transaction completely with an actionable reason and affected object/rule.
- Preserve explicit Free/Ignore choices; never silently use them as a fallback for failed constrained work.
- Verify stale operations cannot commit against a changed document or library definition.
- Audit file replacement, backup/autosave policy and recovery discovery. Implement missing safe-write and recoverable autosave behaviour without overwriting newer saved data.
- Test interrupted writes, malformed recovery data and recovery of multiple documents using disposable projects.

### Use case and exit gate

A bulk via edit either applies the intended complete valid change or leaves the source untouched and explains why. Rejected/cancelled operations consume no history step or IDs. Recovery must be verified against known saved/unsaved content, with exact reopen behaviour and preserved original files.

## Milestone 3: efficient daily layout

### Work

- Persistent named groups with deliberate membership, nested-group policy, locking, external connections and Undo/save compatibility.
- Saved View presets for placement, routing, pours and assembly review.
- Clearer overlapping-object selection and selection feedback; evaluate lasso selection after the existing modes are reliable.
- Searchable commands and configurable shortcuts with conflict detection and reset controls.
- Fix polygon-workflow friction found in UC-05, retaining free angles and repeated edits.
- Provide bounded progress/cancellation for expensive tasks.

### Use case and exit gate

Group a connector circuit, repeatedly move/rotate it, simplify the view for routing and restore assembly visibility. Relative geometry, external connectivity, numbering and locks remain correct; Cancel and one-step Undo are predictable. View changes never dirty the design.

## Milestone 4: pad geometry and library reliability

### Stages

1. Individual mask/paste margins and drill offsets.
2. Additional standard shapes and chamfers.
3. Arbitrary copper shapes.
4. Per-layer pad definitions when supported end to end.

Each stage requires backward-compatible loading and agreement between drawing, selection, transforms, connectivity, pours, DRC, import, PDF and exported copper/drills. Define unsupported-import diagnostics rather than silently changing geometry.

Complete pin-to-pad mapping, footprint replacement, embedded library portability and missing-model handling. Verify copied projects on a second PC without relying on work-PC paths.

### Use case and exit gate

Create an unusual connector footprint, place and connect it, refill and export. An independent output review confirms the intended copper, openings and drill position. Existing projects retain their prior geometry after migration and reopen.

## Milestone 5: coordinated differential-pair work

### Stages

1. Pair-aware layer transitions with two validated vias, including atomic rejection.
2. Consistent width/gap constraints and useful obstacle feedback.
3. Coordinated tuning of both members.
4. Independent skew correction and explicit coupled/uncoupled accounting.
5. Persistent targets/tolerances with rule-check results.
6. Later, propagation-delay accounting using explicit layer/via parameters.

### Use case and exit gate

Route a pair around obstacles, transition layers and tune to declared targets. Validate geometry, connectivity, gap/clearance, physical lengths and skew with known examples. Cancellation and Undo affect the whole pair transaction. Physical length matching and an impedance calculator do not establish signal-integrity acceptance.

## Milestone 6: complete-project and manufacturing acceptance

### Work

- Exercise junctions/crossings, labels, power connections, hierarchy and ERC navigation.
- Verify annotation, pin/pad mapping and source-to-board change review. Reannotation or footprint replacement must not silently duplicate parts or destroy unrelated routing.
- Ensure BOM, assembly variants and placement outputs agree.
- Inspect copper, mask, paste, drills, plated/nonplated holes, slots, outlines, origins, units and bottom-side component rotations using an independent viewer/parser.
- Check mechanical dimensions, holes, model placement and orientation; document faceted STEP limitations.
- Produce a small physical prototype and compare the assembled board with the intended connectivity and dimensions.

### Exit gate

A repeatable schematic-to-fabrication workflow has recorded results, independently reviewed outputs and a checked physical prototype. Existing writer/reader tests alone cannot close this gate.

## Performance acceptance across milestones

Measure on both the work PC and home laptop. Record CPU, GPU/driver, RAM, display resolution/scaling, build mode, board counts and rule settings.

Record separately:

- File decoding/import and native load/IPC time.
- First usable canvas and full visible-fill completion.
- Native input-to-visible response and frame presentation where measurable.
- Picking, highlight changes, connected planning, DRC, refill and Undo latency.
- Long pauses, peak memory and memory after repeated open/edit/close cycles.
- Warm small-edit buffer uploads versus full scene loading.

A proposed goal is navigation close to the display refresh rate on representative boards; establish measured hardware-specific targets before declaring a pass. Worker callbacks and CPU submission timing are not presented FPS. Background fills improve responsiveness without eliminating triangulation cost or memory use.

Profile the failing phase before optimizing. Native fill/DRC scheduling, caching or more extensive worker work should follow evidence rather than assumptions.

## Later features and dependencies

| Feature | Prerequisite and decision gate |
| --- | --- |
| Reusable circuit/layout blocks | Stable groups, component identities and validated net/topology mapping. Preview mismatches before applying. |
| Multi-net/bus routing | Reliable single/pair routing, explicit constraints and complete multi-object transaction handling. |
| Richer mechanical checks | Trustworthy board/model geometry and coordinate transforms; define clearance tolerances. |
| Delay-driven tuning | Explicit propagation parameters and consistent path accounting through tracks, layers and vias. |
| Multi-board workflows | Stable project identities, connectors, mechanical alignment and change review. |
| Advanced analysis integrations | Defined interfaces, units, reproducible models and independent verification; do not imply analysis from geometry checks. |

## Regression and delivery policy

For each implementation, record the problem, current state, acceptance fixture, changes, evidence, limits and next action. Add tests for meaningful failure invariants and cross-subsystem geometry rather than mirroring implementation details.

Use targeted native/editor checks during development. Run the required editor/public-presentation suite before publication, plus TypeScript/native/build/headless checks appropriate to actual code changes. Optional external-fixture tests must report whether their bodies ran. Keep desktop acceptance and manufacturing review explicitly separate.

Update [the handoff](HANDOFF-2026-10-28.md) as milestones progress and [the checklist](WORKFLOW-CHECKLIST-2026-10-28.md) only after observed acceptance. Rebuild before testing changed native behaviour. Follow [the home-PC sync guide](HOME-PC-SYNC-2026-10-28.md) when continuing on the other computer.

Keep private designs, reference notes, local reports, credentials and generated binaries out of commits. Inspect the staged list and publish only under an explicit user request covering that batch. This plan does not schedule future work or authorize later pushes.
