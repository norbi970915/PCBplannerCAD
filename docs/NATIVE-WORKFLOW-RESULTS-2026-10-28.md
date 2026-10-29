# pCAD native workflow results — 2026-10-28

## Session and scope

Source: `c7976f7`. Executable: `target/release/pCAD.exe`, built on 2026-10-28 at 18:30:17 (Europe/Bucharest). The release build succeeded without generating installers. TypeScript, 185 editor checks and 216 native core tests passed before this desktop pass; three optional core performance tests were ignored. One editor check initially could not locate Git in the test process PATH; it passed after restoring PATH, without source changes.

Hardware: Intel Core Ultra 7 258V, Intel Arc 140V. Native window capture: 2560 × 1392. The timing recorder reported a canvas viewport of 1980 × 963 and pixel ratio 1. Rules and online DRC were on. Connected movement used Preserve angles with Push.

All interaction used disposable copies under the ignored `local-test-boards/native-acceptance-2026-10-28/` directory. Original designs were preserved. This is focused native acceptance, not completion of every item in the full workflow checklist. No reference application was needed to resolve these cases.

## Connected via movement

An original four-layer fixture has one through via at (20, 15) mm, with straight tracks joining fixed vias on Top, Inner 1 and Bottom. A separate foreign-net via provides a Push obstacle.

- Dragging the central via to (23, 12) mm preserved its ID and connections on all three occupied layers. Saved tracks retained their fixed far endpoints and joined the new via position. Online DRC finished clean.
- One Undo restored every original item exactly. Redo restored the moved items exactly, including allocated routing IDs.
- M/click placement onto the unlocked foreign-net via pushed that obstacle aside. Inspection of the saved result distinguished successful Push from a short: the two via centers were separated and online DRC finished clean.
- M followed by Esc preserved the saved item data exactly.
- On a separate copy with an attached straight track locked, dragging rejected the move and retained a floating retry state. Esc and Save preserved all source items exactly.

These fixtures do not establish native acceptance of blind/buried moves, curved contacts, multi-via transforms, all movement modes or teardrop/pour recovery during a move. Their automated coverage remains recorded in the handoff.

## Polygon editing

Used a disposable copy of the original polygon playground.

- Made a free-angle cut between the left and top edges of the rectangular pour, selected the side to remove, then immediately started another cut on the new diagonal edge. Both edits committed without restarting Cut.
- Saved outlines retained non-45-degree vertices. Both operations refilled copper.
- Two Undo steps restored the original outlines and filled polygons exactly. Two Redo steps restored the edited items exactly.
- Sliced the concave pour into two independent objects. Saved outlines preserved the original total area exactly; both pieces retained the GND net and Top layer.
- Reshaped the other pour with an outward corner. Saved geometry extended beyond the former boundary, increased its area and contained refilled copper.

This pass did not complete thin-sliver stress, invalid-path recovery, every cancellation case or independent output review.

## Connected footprint alignment and persistence

Used three routed resistors at (10, 8), (20, 13) and (30, 18) mm with embedded footprint definitions. Each pad 1 connects to its own fixed through via.

- Selected the footprints and copper and invoked Edit → Align / Distribute → Align Top.
- All three footprint centers ended at Y = 18 mm; their X coordinates and all fixed via positions remained exact.
- Connections followed the first two footprints. The middle footprint landed directly on its fixed via, so its zero-length track was removed; the pad and via remained coincident. The third footprint and its track stayed fixed.
- One Undo restored the complete source item list. Redo restored the aligned result exactly.
- Closed the document, reopened it from disk, and saved again. Item data matched the aligned result exactly; embedded pads and routing displayed correctly.
- DRC reported zero errors and one warning. The fixture intentionally leaves its separate net groups unrouted; it is not a completed circuit.

An initial hand-authored fixture omitted embedded footprint data. It was corrected before the alignment check; that setup mistake is not recorded as a product defect.

Distribute, blocked multi-part Pack, Free/Disconnect, stale responses and mixed-layer component transforms still require the pending native cases.

## Filled-pour display and selection

- Opened a copied 34,143-object board containing 148 pours. The layout was visible while the native canvas displayed “Preparing copper display…”. Fills subsequently appeared and the status cleared.
- Wheel zoom and layer dimming changed the view. Toggled filled pours off and restored them through the keyboard command; the native controls reflected both states. No worker-fallback message was observed.
- Switching to a small document restored its own geometry. Viewing did not mark the dense document dirty; the copied file remained byte-for-byte identical to its source.
- The dense fixture already reports 24,720 DRC errors and 184 warnings. Those results are not attributed to this display check or presented as a clean-board result.
- A separate original fixture has a 1,200-vertex circular hole in a filled pour, exceeding the worker preparation threshold. Copper was selectable; clicking the hole cleared selection. A right-to-left crossing box starting inside the hole and reaching copper selected the pour correctly.

The 30-second timing recording captured only one frame sample and no scene samples, so it is insufficient for a performance conclusion. No presented-FPS, sustained pan/zoom or input-latency result is claimed. Native interaction specifically during pending preparation, visibility/document cancellation races, refill/Undo while jobs are pending, and independent PDF copper/hole review remain open.

## Outcome and continuation

No new confirmed product defect was found in the covered cases. No application source changes or additional rebuild were needed. Local fixture/results files remain ignored; this report and checklist updates contain only the observations and limits.

Continue with the unchecked native cases in [the workflow checklist](WORKFLOW-CHECKLIST-2026-10-28.md), especially Distribute/Pack rejection, mixed-layer transforms, pending display-job cancellation and independent printed/output geometry. Follow [the development plan](DEVELOPMENT-PLAN-2026-10-28.md) before starting the editing-safety and recovery audit. This pass does not close the native acceptance milestone as a whole.
