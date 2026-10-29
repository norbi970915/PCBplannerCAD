# pCAD home-PC continuation — 2026-10-29

Read this guide before updating the home checkout. The October 29 delivery includes import/routing fixes, the local project hub, revisions, recovery, interface colors, linked-component navigation, detachable views, external-save checks and File → Save Copy.

The work-PC release was rebuilt at **2026-10-29 16:24:26 (Europe/Bucharest)**. Git transfers source and documentation; it does not transfer that executable, dependencies, private designs, external models or local recovery copies. Rebuild on the home PC before testing.

## 1. Preserve home work and sync safely

Save and close pCAD, then stop its development terminal with Ctrl+C. Back up the existing checkout, including its hidden .git directory and any ignored design files, to a separate folder. Back up design/library/model folders outside the checkout too. Local recovery copies live in the app-data location shown by File → Recovery copies and need their own backup when required.

Inside the existing home checkout, inspect:

~~~powershell
git status --short --branch
git remote -v
~~~

The expected origin is https://github.com/norbi970915/PCBplannerCAD.git. Keep the existing local folder name; no rename is required.

If the working tree is clean, run the following in order. Each failure stops the block:

~~~powershell
git fetch origin
if ($LASTEXITCODE -ne 0) { throw 'Fetch failed; stop here.' }

git switch main
if ($LASTEXITCODE -ne 0) { throw 'Could not switch to main; preserve and review local work.' }

git pull --ff-only
if ($LASTEXITCODE -ne 0) { throw 'Fast-forward refused; use the backup/fresh-clone guidance below.' }

git status --short --branch
git log -1 --oneline
~~~

If there are local edits/commits, or the pull refuses because the checkout still has the older rewritten history, keep that checkout untouched. The safest alternative is a fresh clone into a **new, unused folder**, then selectively bring over reviewed local changes and private designs. Do not copy the old .git directory or overwrite the new source tree with the old checkout.

~~~powershell
# Run from the parent directory; PCBPlannerCAD-home must not already exist.
git clone https://github.com/norbi970915/PCBplannerCAD.git PCBPlannerCAD-home
if ($LASTEXITCODE -ne 0) { throw 'Clone failed; preserve the original checkout.' }
~~~

The [earlier sync guide](HOME-PC-SYNC-2026-10-28.md) also describes a deliberate one-time history realignment, with backup/branch/stash safeguards. Do not run a hard reset, git clean, force push or automatic stash pop merely because a normal pull fails.

## 2. Install and rebuild on the home PC

Use Windows x64 with WebView2 and a working WebGL2 graphics driver. Building needs Node.js, pnpm, stable Rust/MSVC, C++ Build Tools and a Windows SDK. The work-PC versions checked for this delivery are Node **24.19.0**, pnpm **12.15.0**, Rust/Cargo **1.99.1**. Use pnpm.cmd in PowerShell to avoid script-execution-policy issues.

From the updated repository root, run each command and stop on any failure:

~~~powershell
pnpm.cmd install --frozen-lockfile
pnpm.cmd typecheck
pnpm.cmd -C app test:editor
cargo test --workspace --locked
pnpm.cmd -C app tauri build --no-bundle
~~~

After the build succeeds, open **target/release/pCAD.exe**. Do not test an old executable left from before the pull. The no-bundle command builds the standalone app without generating installers.

For development, use **pnpm.cmd dev** instead. Keep its terminal open and avoid starting a second server on port 1420. Investigate the existing session before stopping any process.

Optional headless browser checks are **pnpm.cmd -C app test:docking** and **pnpm.cmd -C app test:projects**. They need an installed Playwright runtime/browser; see the scripts for PCAD_PLAYWRIGHT_PATH and PCAD_BROWSER_EXECUTABLE overrides. The absolute runtime path on the work PC should not be copied as a home-PC requirement. These scripts launch isolated headless browsers and do not control the desktop.

Fixture-dependent import/output tests can return early when their optional external input paths are not configured. Passing the default suite does not establish a complete private-design migration or manufacturing acceptance.

## 3. Test the latest changes first

Use disposable copies of saved designs, kept outside the repository.

1. **Save Copy:** open a linked project, edit its board, then choose File → Save Copy with a new filename. Open the copy separately and verify its current geometry. The working board must keep its original path, project link, dirty marker and Undo history. A symbol/footprint editor copies its complete containing library.
2. **Conflicting Save:** with a dirty document open, change or remove its saved file using a separate editor/process. Save must explain the conflict and preserve the external file and live edits. Preserve your version with Save Copy before deliberately reopening the external version.
3. **Save All:** edit two documents and a library; externally change a later member or the project file. A known content conflict must be caught before any member is saved. Save All is not a multi-file transaction if later I/O fails or another writer races it.
4. **Destinations:** cancel a copy, choose an existing file, and try an open design/library/project path. Nothing should be overwritten or falsely reported as saved. Save As to a different filename also requires an unused destination.
5. **Linked views:** double-click a schematic component body in Select mode. Verify the correct linked footprint is selected and fitted, including an already detached board; unrelated boards with the same reference must remain separate.
6. **Multiple monitors:** detach a board/tool tab, move its window across monitors, and drag its tab back onto a docking target. Check mixed display scales, keyboard menus, text-input Escape, native close/return, main-window minimization and shared Undo.
7. **Recovery:** make known unsaved edits, create a recovery copy, restart safely, restore to a new folder and verify the recovered linked design. External model assets and Undo history are outside the recovery payload.

The [workflow checklist](WORKFLOW-CHECKLIST-2026-10-28.md) has the detailed cases. Background tests are recorded in [the current handoff](HANDOFF-2026-10-29.md); physical-monitor, crash and manufacturing checks remain open until actually exercised.

## 4. Continue implementation after recording native results

Read AGENTS.md, the current handoff and Git status before editing. Update the handoff as results arrive.

- Fix any reproducible save, close, recovery or native window failure first.
- Audit the legacy project-creation path for safe companion-file creation and rollback.
- Add deliberate external-version review/reload controls and investigate portable whole-project copies with external assets.
- Profile large-file saving/recovery and complete pending display-job cancellation/refill/Undo races.
- Continue import metadata/pin-map review, connected Distribute/blocked Pack, routing/tuning and independent output acceptance as listed in the handoff.
- Use background checks while the user is busy. Ask before controlling their desktop.
- Keep private designs, model assets, screenshots, recovery data, logs and local backup refs out of Git. A future push needs a fresh explicit request.
