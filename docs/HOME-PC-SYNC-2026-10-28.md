# pCAD — home-PC sync and safe continuation

Date: 2026-10-28 (Europe/Bucharest).

The published `main` history was rewritten across 11 commits, including messages and historical source snapshots. The source tree at the rewritten tip stayed unchanged. The rewritten baseline is `7efea94`; later workday commits may follow it. Always sync to the latest fetched `origin/main`, not to that baseline hash.

An existing checkout on another computer needs the one-time procedure below. Afterward, ordinary fast-forward pulls work again. No local project-folder rename is needed.

## Before leaving the work PC

1. Save design work and update [the handoff](HANDOFF-2026-10-28.md) with completed work, checks, known limits and the next tasks.
2. Review the changed/staged files. Keep personal designs, test boards, local reports, credentials and generated binaries out of commits.
3. Commit and push the intended source/documentation changes only after an explicit user request covering that work. A local commit alone does not transfer anything to the home PC.
4. Verify the push succeeded and note the final commit. Do not publish local recovery refs or backup branches.

This guide was prepared locally for that later handoff. Preparing it does not schedule or authorize an automatic commit or push.

## What the home PC needs

- Git and access to the existing repository: `https://github.com/norbi970915/PCBplannerCAD.git`.
- Windows x64, WebView2 and a graphics driver supporting WebGL2.
- Node.js and pnpm. This checkout was checked with Node `24.19.0` and pnpm `12.15.0`.
- Stable Rust using the MSVC toolchain. The work PC has Rust/Cargo `1.99.1`.
- Visual Studio 2022 Build Tools, the Desktop development with C++ workload and a Windows SDK.
- Space for dependency downloads, native compilation and installers. Generated builds are not transferred by Git.
- Separate copies of the saved designs, native libraries and model files needed for testing. These are not supplied by the source repository.

Use `pnpm.cmd` in PowerShell to avoid the script-execution-policy problem encountered earlier. Existing development prerequisites are also in [README](../README.md).

## Back up the home PC first

Save and close pCAD and stop its development terminal with Ctrl+C.

Before any reset, copy the entire existing project folder, including its hidden `.git` directory, to a backup outside the checkout. Back up any design/library/model folders stored elsewhere too. Verify the copies exist and open a saved design copy if needed. Keep these backups local.

The Git backup branch below preserves local commits. The stash preserves tracked changes and untracked files. **Neither replaces the folder backup: `--include-untracked` does not capture ignored files**, which can include your designs and test data.

If backup space is unavailable or local changes are unclear, leave the existing folder untouched and use a fresh clone in a separate folder instead. Bring over only reviewed design data and actual local code changes afterward.

## One-time sync of an existing checkout

Read this guide in the repository website before attempting to pull into an old checkout. At the end of the workday it must first have been committed and pushed from the work PC.

Open PowerShell inside the existing home checkout. First inspect:

```powershell
git status --short --branch
git remote -v
```

Confirm that `origin` is the repository above and that the backup is complete. Then run this block. It stops if a Git command fails:

```powershell
$ErrorActionPreference = 'Stop'

function Invoke-PCADSyncGit {
    param([string[]]$GitArguments)
    & git @GitArguments
    if ($LASTEXITCODE -ne 0) {
        throw 'Sync stopped. Preserve the backup and investigate the failed command.'
    }
}

$syncStamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$syncBackupBranch = 'local-backup-before-sync-' + $syncStamp

Invoke-PCADSyncGit -GitArguments @('branch', $syncBackupBranch)
Invoke-PCADSyncGit -GitArguments @('stash', 'push', '--include-untracked', '-m', ('Before history sync ' + $syncStamp))
Invoke-PCADSyncGit -GitArguments @('fetch', 'origin')
Invoke-PCADSyncGit -GitArguments @('switch', 'main')
Invoke-PCADSyncGit -GitArguments @('reset', '--hard', 'origin/main')
Invoke-PCADSyncGit -GitArguments @('status', '--short', '--branch')
Invoke-PCADSyncGit -GitArguments @('log', '-1', '--oneline')
```

The reset deliberately replaces tracked working files with the published source. Run it only after the complete backup and successful branch/stash steps. A “No local changes to save” stash result is normal.

The final status should show `main` aligned with `origin/main`. Record any remaining changes before continuing. Keep the backup branch and stash local until all needed work has been recovered.

Do not merge the old branch into `main`, force-push it, run `git clean`, or automatically run `git stash pop`. Selectively recover actual local edits after reviewing them against the current source. This avoids restoring obsolete identifiers or reconnecting the old history. Restore personal data from the folder backup separately; do not copy the old `.git` or whole source tree back over the synced checkout.

## Install, verify and launch

From the synced repository root:

```powershell
pnpm.cmd install --frozen-lockfile
pnpm.cmd typecheck
pnpm.cmd -C app test:editor
cargo test -p pcbplannercad-core -p pcbplannercad-import -p pcbplannercad-output --locked
```

Optional external-fixture tests can return early when their input paths are not configured. See the handoff for neutral fixture settings and optional headless-renderer setup.

For development:

```powershell
pnpm.cmd dev
```

Keep that terminal open while developing. If port 1420 is occupied, find and stop the previous pCAD development session before starting another; do not terminate an unrelated process blindly.

For a rebuilt standalone executable and installers:

```powershell
pnpm.cmd build
```

Launch `target/release/pCAD.exe` after a successful build, or use the newly built installer. An executable already present on the home PC may predate the pulled source. Rebuild before testing the latest behavior.

## Safe continuation at home

Open the synced checkout as the active workspace. Read `AGENTS.md`, [the current handoff](HANDOFF-2026-10-28.md) and [the acceptance checklist](WORKFLOW-CHECKLIST-2026-10-28.md) before editing. Inspect Git status and any local recovery work first.

Use saved disposable design copies for native interaction tests. Remaining checks include connected via moves, cancellation, Undo/Redo, layer spans, locked copper, import/library browsing after the neutral API changes, and dense-board navigation. Large filled pours now prepare in the background on the canvas; full preparation still takes CPU time. Test startup responsiveness, cancellation, hole-aware selection and printing in the rebuilt native app. The current handoff separates automated evidence from pending native UI acceptance.

Ask before controlling the desktop if the user is working on it. Terminal and isolated headless checks can run in the background. Keep private designs out of Git and request fresh authorization before another push.

## Normal later updates

Once this one-time sync and any selective recovery are complete:

```powershell
git pull --ff-only
```

Save or commit local work first when necessary. If a fast-forward pull refuses to proceed, inspect the local commits/changes instead of merging old history automatically.
