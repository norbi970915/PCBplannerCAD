# Home-PC continuation — 2026-10-31

Prepared: 2026-10-31 14:08 EET (Europe/Bucharest).

Start here, then read [today's detailed handoff](HANDOFF-2026-10-31.md). This guide covers the routing and cleanup changes prepared for the user's requested source delivery.

## Delivery and working state

Repository: [PCBplannerCAD](https://github.com/norbi970915/PCBplannerCAD), branch main. The work-PC checkout is A:\Workspaces\PCBPlannerCAD. Keep the existing folder name on the home PC; use its actual path in your terminal.

The source baseline before this delivery is cbd83bfcb7113985ccc727b509313e9cc40f2216. That is a reference for reviewing the changes, not a reset target. After the requested commit/push succeeds, fetch the latest main and record its commit with git log -1 --oneline. A local handoff file or commit alone does not transfer work to another PC.

The user explicitly requested this delivery. Future work must remain local until another explicit push request. Do not rewrite shared history, publish releases, or push backup branches as part of continuation.

The existing executable on this PC was not rebuilt for today's fixes. Current app configuration is PCBPlanner Studio 0.6.6, executable target/release/PCBPlannerStudio.exe. Rebuild from the delivered source before desktop testing. Older guides can mention an earlier executable name.

## What changed

- Bounded shove backtracking reconsiders blocked connected moves, including independent corridor choices and larger detours guided by failed neighbors. Fast attempts reserve work for fallbacks instead of exhausting the enclosing allowance.
- Connected deformation preserves fixed interior branch/via contacts and exact source headings.
- Via displacement recognizes physical interior and off-center contacts, retains necessary connection tethers, checks defined layer spans and validates the complete subsequent route. Repeated displacement avoids accumulating old tethers.
- Reviewed Cleanup Tracks & Vias preserves locks and physical contacts, removes eligible duplicates, trims only useless free tails, and validates the reviewed plan atomically at Apply. Stale previews cannot modify a changed document.
- Checked interactive routing automatically cleans eligible old straight-track bends in the affected area. Protected new tracks remain exact, and routing, cleanup and dependent refill share one Undo/Redo step. Ignore routing and ordinary edits do not opt in.
- Cleanup handles exact interior landings, partial collinear overlap, covered duplicate segments, long staircases and shorter two-segment bends. Speculative splits are discarded when unused; repeated overlapping routes preserve fixed vias without accumulating duplicate tracks.
- Obstacle cleanup can rebuild both shoulders together around an existing safe vertex or an exact point on an old edge. It preserves corner mode, clearance, keepouts and unrelated same-net islands. Edges without an interior lattice point are never rounded.
- Pair intent, configured pairs, length-rule scopes, locks and fixed contacts remain protected. The cleanup guard checks both whole-net length context and actual track/arc layers conservatively; layer-specific length totals are not a new measurement feature.

The detailed dated handoff records the reproduced failures, work caps, earlier stages and controlled-test evidence.

## Verified background checks

Latest source checks on this PC:

| Check | Result |
|---|---|
| Core native tests | 439 passed, 3 existing ignored |
| Native application tests | 32 passed, 1 existing ignored |
| Editor and public-presentation tests | 295 passed |
| TypeScript | Passed |
| Public naming check after handoff edits | Passed, including this guide |

Accepted native cases check actual copper, changed-copper DRC, fixed contacts, exact Undo/Redo and native save/reopen. Controls include rotated/reversed/free-angle geometry, custom clearances, keepouts, same-net islands, length/pair protection, off-center contacts and repeated routing.

These are command-line checks. No desktop automation, app restart or executable replacement was performed for today's changes. The private congested-board fixture is absent here, so historical dense acceptance counts and timings have not been recertified.

Ignored evidence on the work PC is under target/shove-backtracking-2026-10-31/, target/routing-limits-2026-10-31/, target/routing-dependencies-2026-10-31/, target/routing-cleanup-2026-10-31/, target/automatic-route-cleanup-2026-10-31/, target/route-cleanup-expansion-2026-10-31/ and target/route-obstacle-cleanup-2026-10-31/. These logs are not part of Git delivery.

## Sync safely at home

Save and close the app and stop an existing development terminal with Ctrl+C. Preserve home edits, local commits, private designs, libraries, models and ignored files before updating. A copy of the existing checkout, including hidden .git, plus any external design folders is the simplest complete backup.

Inspect the existing checkout first:

~~~powershell
git status --short --branch
git remote -v
~~~

Confirm origin is the repository above. Only if local edits/commits have been reviewed and the checkout is ready to update, run:

~~~powershell
git fetch origin
if ($LASTEXITCODE -ne 0) { throw 'Fetch failed; stop here.' }

git switch main
if ($LASTEXITCODE -ne 0) { throw 'Could not switch branches; preserve local work.' }

git pull --ff-only origin main
if ($LASTEXITCODE -ne 0) { throw 'Fast-forward refused; keep the existing checkout and review divergence.' }

git status --short --branch
git log -1 --oneline
~~~

If there are unreconciled local changes or history divergence, preserve the existing checkout and use a fresh clone in a new, unused folder. Selectively recover actual local edits afterward. Do not automatically reset, clean, force-push, merge obsolete history or pop an old stash. The [older sync guide](HOME-PC-SYNC-2026-10-28.md) explains the earlier one-time history rewrite; normal continuation does not require another rewrite.

Git supplies source and documentation. It does not supply dependencies, built executables, private designs, models, local project checkpoints or app-data recovery copies. Transfer needed design assets separately and privately.

## Install, validate and rebuild

Use Windows x64 with WebView2 and a working WebGL2 driver. Development requires Node.js, pnpm, stable Rust/MSVC, C++ Build Tools and a Windows SDK. Use pnpm.cmd in PowerShell to avoid script-execution-policy issues.

From the home checkout root, run each step successfully before continuing:

~~~powershell
pnpm.cmd install --frozen-lockfile
if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed.' }

cargo test -p pcbplannercad-core -p pcbplannercad --lib --locked
if ($LASTEXITCODE -ne 0) { throw 'Native checks failed.' }

pnpm.cmd -C app typecheck
if ($LASTEXITCODE -ne 0) { throw 'TypeScript checks failed.' }

pnpm.cmd -C app test:editor
if ($LASTEXITCODE -ne 0) { throw 'Editor checks failed.' }

pnpm.cmd -C app tauri build --no-bundle
if ($LASTEXITCODE -ne 0) { throw 'Build failed; do not launch a stale executable.' }

.\target\release\PCBPlannerStudio.exe
~~~

Alternatively, use pnpm.cmd dev for a development session. Keep its terminal open. If port 1420 is occupied, stop the previous development session before starting another. Do not terminate an unidentified process.

Do not restore omitted STEP preview dependencies as an incidental build repair. This release keeps its documented VRML preview and STEP export behavior.

## First desktop acceptance on a disposable design copy

1. Open a saved linked project and confirm schematic, board and libraries load.
2. Route around existing copper, reconnect partway along an old track, and overlap old copper. Confirm the new route remains exact, old eligible bends disappear and useful continuations survive.
3. Repeat overlapping routes around a fixed via. Check connections, track count, one-step Undo/Redo, save and reopen.
4. Try locked copper, a length-constrained net and temporary/configured pairs. Confirm their protected geometry is retained.
5. Exercise a blocked shortcut near foreign copper, an isolated same-net island and a keepout. Confirm a safe alternative or safe rejection; optional cleanup must not discard valid requested routing.
6. Preview explicit cleanup, alter the document, then check that the old review expires. Re-preview and apply, then Undo.
7. Check connected via moves on every spanned layer, repeated pair moves, layer changes, pours and polygon cut/slice. Preserve original designs separately.

## Continue implementation in this order

1. Transfer a disposable private fixture to target/f1-filled-final.arpcb, if available. Repeat the documented congested-board probes and measure routing plus cleanup commit latency. Use the commands and fixture-specific IDs in today's detailed handoff; do not assume its historical counts or IDs apply to another board.
2. Investigate remaining rejected grouped targets and reverse dependencies using reproduced native cases and the shared work allowance.
3. Extend joint via/fanout displacement while preserving fixed pads, curves, locks, stacked vias, all physical contacts and every crossed layer.
4. Extend cleanup across multiple obstacle waypoints and investigate safe filled-pour-aware shortening. Current cleanup keeps one waypoint, operates on straight tracks at checked route commits, and remains conservative around existing fill contacts. It does not optimize general dragging or reshape circular arcs.
5. Repeat focused background checks, update the dated handoff, then rebuild once the final changes pass and perform brief desktop acceptance.

Key implementation locations: crates/core/src/pcb/router_backtrack.rs, router_shove.rs, router_chain.rs and router_via.rs; cleanup.rs and cleanup_contacts.rs; route_cleanup.rs, route_cleanup_geom.rs, route_cleanup_checks.rs and route_cleanup_paths.rs. Regression cases sit alongside those modules. Native request handling is in app/src-tauri/src/commands/docs.rs and pcb.rs; editor routing integration is in app/src/editor/tools/route.ts; reviewed cleanup UI is in app/src/pcb/BoardToolDialogs.tsx and cleanupReview.ts.

Keep public source, docs, comments, filenames and UI focused on the app itself. Reference research belongs only in ignored local notes; required exact file-compatibility markers and license notices remain intact. Do not add private designs, local reports, recovery data, generated binaries or reference notes to Git.
