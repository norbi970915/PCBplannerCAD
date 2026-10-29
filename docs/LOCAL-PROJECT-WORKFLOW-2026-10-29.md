# Local project folders, imports and revisions

Date: 2026-10-29. These changes require a rebuilt native executable. The running desktop app was not restarted during background development.

## Working folder and electronics projects

Open **File → Local project hub**, the Home hub card, or **Projects → Files**. Choose a working folder on this PC. Its location is remembered locally. Click folder cards and breadcrumbs to browse nested customer/product folders. **Folder** creates a subfolder; **Electronics design** creates a new project directory with both schematic and board, a project library and a model-assets folder.

Example layout:

```text
Working folder/
  Customer/
    Sensor/
      Sensor.arproj
      Schematic.arsch
      Board.arpcb
      libraries/Project.arlib
      models/
      .pcad-history/       saved checkpoints, created when needed
```

Project, schematic, board and library cards open the corresponding native file. Other assets remain visible in the folder browser. Cards now include saved-content thumbnails and file dates. Schematics and boards show their saved geometry; project cards combine the first sheet and board; libraries show the first symbol/unit and footprint. Local STEP/VRML assets have model thumbnails. Unsupported, oversized, damaged or empty files retain an icon. Folder rename/move and multi-select remain future work. It is local storage without accounts, uploads or background synchronization.

## Saved-content thumbnails

Thumbnails load only near the visible file list. PNGs use a bounded cache, with one serialized preparation job and at most two shared GPU contexts for all cards. Save/Save All/library-save acknowledgements invalidate saved previews, including project cards whose manifest itself did not change. Manual Refresh also clears the cache; use it after external file edits. Unsaved canvas edits do not appear in a saved-file thumbnail.

Previews do not open documents, change history or write files. Linked preview sources must remain inside the selected working root. Current limits are 32 MiB per native input, 8 MiB per model asset and 30,000 document/definition items. Large/missing inputs fall back to an icon while the file remains available to open. Board copper pours are drawn as outlines in thumbnails, not as full filled-pour meshes. Models are previewed as assets; a standalone model editing/viewing panel remains future work.

The bundled STEP engine uses a version-checked static binding adapter so it can run without string-to-JavaScript evaluation. The policy permits same-origin assets and WebAssembly compilation. Updating the pinned engine requires reviewing the adapter. The existing interactive model loader shares this fix; full native 3D acceptance is still required.

## Import wizard

Open **File → Import → Import wizard** or **Import** in the local folder browser.

1. Choose a design/library file or a footprint folder. When selecting a schematic, auto-detection looks for a same-name companion board. **Choose companion board** can pair a separately named file in the same source format. Analyze parses a read-only source snapshot and displays counts, planned output files, linked-project status and import notes.
2. For a library, optionally attach local STEP/VRML models to footprint names. Reanalyze after changing mappings. The wizard copies selected assets; it does not download cloud package/model links.
3. Review warnings, pin/footprint mappings and output names. Select a parent directory and a new folder name. The destination must not already exist, including an empty folder.
4. **Import reviewed copy** publishes the prepared files and opens the new project or library. No source files are modified. Later source-file changes require a new analysis. Cancel during analysis or review applies no import and discards late responses.

Supported inputs:

| Input | Behavior |
| --- | --- |
| .kicad_pro | Complete structured project with hierarchical sheets, embedded libraries and board when present. |
| .kicad_sch / .kicad_pcb | Schematic with companion board when found, or standalone board in a new native project. |
| XML .sch / .brd | Schematic with companion board when found, or standalone board. Legacy binary files sharing these extensions are not supported. |
| XML .lbr | Components, symbols, packages/footprints, parameters and pin mappings. |
| .kicad_sym / .pretty folder | Symbol library or footprint library. |
| .arlib | Native library copy, with optional replacement model mappings. |
| .step / .stp / .wrl | Local model attachments to a library footprint; file headers are checked, and rendering still needs inspection. |

Direct .PcbDoc, .SchDoc, .PcbLib, .SchLib, .IntLib and .PrjPcb conversion remains unimplemented. Native .f3d, .f3z, .flbr, .fsch and .fbrd archives also remain unsupported. Recognition of an extension is not a claim that its contents can be translated. No support is claimed for direct mechanical archive editing, cloud access, arbitrary 3D assembly libraries, or complete feature parity of imported electronics files.

A linked import puts schematic sheets and the board in one .arproj project, with native working files kept together in the new folder. Normal Save/Save All updates those native files. Reviewed **Update PCB** transfers schematic changes to the board. Import does not continuously update the original export files, connect to a cloud account, or provide automatic bidirectional editing. A placement/connectivity review warns when the selected sources may not match; nothing is auto-repaired during import.

When source library names collide as filenames through case or character sanitization, the importer chooses distinct disk filenames. Logical library names and component/footprint references remain intact; the review lists each disambiguated filename. Reserved disk names are handled separately from source identities.

For portable projects, import libraries into a new subfolder of the project's libraries directory, then use **Add library to current project**. The saved library keeps copied models beside it, and model lookup includes open library folders after reopening a project. Existing missing/external model references are reported rather than fabricated.

The library footprint Properties panel exposes model X/Y/Z offset in millimeters, rotation in degrees and scale. Inspect the model before placing the footprint into a board. Already cached board footprints require a deliberate definition update; changing a library is not an automatic update of all placed footprints.

## Local revision checkpoints

Open **File → Project revisions**, the project Tools revision button or **Revisions** in the folder hub while a project is open.

**Save & checkpoint** saves the current work first. If a save fails or a file-name dialog is cancelled, no checkpoint is created. Give each checkpoint a note describing its design state.

A checkpoint captures the project folder's saved regular files, local libraries and assets. Referenced files, model dependencies and native document parsing are checked. Linked folders/files, missing dependencies and external project/model references are rejected, so an incomplete package is not presented as self-contained. The .git directory, checkpoint store and private staging directories are excluded. Current limits are 16,384 files and 2 GiB of source data per checkpoint; snapshots are complete copies and are held in memory during preparation.

The checkpoint store is .pcad-history inside the project folder. A manifest records file sizes and checksums for accidental corruption detection. Keep this folder when copying the project to another PC if you want its revision history there too. Checksums do not authenticate untrusted checkpoints.

**Restore copy** verifies the recorded files and creates a new project folder under the chosen parent. It rejects existing destinations and damaged data. It never overwrites the current working folder. Open the restored copy separately when ready.

**Compare saved versions** selects a From and To checkpoint and displays a changelog. It lists added/removed/modified files and, for native documents, added/removed/modified object IDs with readable component designators, position/value/net/geometry changes. Library definitions, pin mappings, linked project files and rules can be inspected; rule reordering is reported. Model/binary assets are listed as content changes with saved sizes. Formatting-only changes are distinguished from changed saved data.

After comparing, **Export changelog…** saves a UTF-8 text report with both checkpoint identities, file totals and the displayed details. Detail limits and omitted counts remain explicit in the exported report. Cancelling the picker writes nothing.

Comparison reads verified snapshots and never changes the working design. It can run in either direction or compare a checkpoint with itself. Damaged data is rejected. Object matching follows persisted IDs: reimporting a whole design can appear as many removals/additions, and entity identity is not guessed. Detail display is capped at 200 lines per file with an omitted count; native JSON files larger than 64 MiB get a file-level summary. Model checksums are streamed, without loading both large model files merely to summarize them. This is a text changelog rather than a graphical overlay or an event-by-event undo log.

Project checkpoints remain manual. Separate automatic recovery copies are described below; automatic checkpoints on each Save, branch/merge operations, checkpoint retention/deletion controls and remote synchronization are not implemented. A checkpoint on the same disk is not an independent backup. Concurrent external edits and power-loss durability across directory metadata are not guaranteed.

## Save and operation safety

Document/library Save As and shared byte exports reject another open document, library or the open project metadata path. History operations are bound to the reviewed project, and stale replies after a project switch are discarded. A restore/checkpoint already accepted for the original project can complete there; switching does not redirect it.

Individual native document/library/project saves flush a sibling staging file before replacing the destination. Failed writes preserve the prior saved file and dirty/origin state. Save As rejects a target already open in another document. Project metadata edits are published in memory only after saving succeeds.

New hub projects and reviewed imports are assembled in a private sibling directory before the complete new folder is published. Existing output names and case-insensitive collisions are rejected. Save All is a sequence of individual saves: a later failure does not undo earlier successful saves, and the UI refreshes state rather than claiming every file was saved.

A failed native Close retains the live tab. Changing projects removes obsolete project tabs and loads the paired board. Import publication and checkpoint creation use native workspace guards for unsaved project dependencies.

## Sidebar library lists

Click **Project libraries** or **Open libraries** in the Projects sidebar to expand or collapse that group. The groups start collapsed and remember their separate choices on this PC. Each header shows its library count and an indicator when a contained library has unsaved changes. Expand a group and click a library to open it.

## Named View presets

In the Properties panel's **View** tab, expand **Saved views**. Set your layer, label and object visibility, enter a name and choose **Save new view**. Select a preset and choose **Apply view** to restore it. **Replace selected** updates that preset from the current display; **Delete preset** removes only the saved display preference.

Presets remember hidden layers, component/pad/net labels, object and marker visibility, layer display mode and filled-pour visibility. Grid display, snapping, active layer and selection filters keep their current settings. Filled-pour visibility is the existing app-wide switch; the remaining display settings apply to the current document. Applying a preset does not edit geometry or add an Undo step.

Each editor type has up to 24 named presets on this PC. Names are limited to 60 characters and duplicates require explicit replacement. These preferences are separate from project files/checkpoints and do not move automatically to another PC.

## Separate fabrication packages

Open **Fabrication outputs**, review the output/assembly settings and enter a **New package folder** name. **Generate fab package…** asks for a parent folder and creates the named child folder after all files are ready. Existing folders are rejected, so use a distinct folder name for each export. The completed destination is shown in the dialog.

**View in Gerber viewer** opens the ZIP when enabled, or the package folder for unpacked output. Review copper, mask, paste, outline, holes and assembly data independently before manufacturing.

Assembly variants apply to the current project's board. The selected variant reaches CSV and XML assembly output; XML BOMs omit not-fitted/BOM-excluded entries while retaining physical board components, pads and nets. Export does not edit the working design. Document/library/project paths currently open in pCAD are protected from conflicting output writes.

## Automatic recovery copies

Open **File → Recovery copies**, or the Recovery entry on Home. Automatic capture checks for unsaved work about once a minute, starting ten seconds after UI startup. It keeps three complete copies per app session in the local application data folder shown in the dialog. The checkbox disables automatic capture; **Create recovery copy now** works independently.

Copies contain open native schematic/board contents and editable libraries, including unsaved changes. A linked recovered project is included when its referenced files are available. Missing dependencies are explained, and individual files remain recoverable. Read-only built-in libraries are excluded.

Choose a **Recovered folder name**, press **Restore copy…** and select its parent. A complete new folder is created; existing folders and original working files are preserved. Open the recovered project or individual files when ready. Normal project-switch safeguards still protect any unsaved work currently open.

External model assets and Undo history are not copied. Keep the original project/library asset folders and relink relative model paths after restoration. RECOVERY-NOTES.txt lists original locations and limitations. Recovery copies stay on this PC unless copied separately and are also retained after normal exits.

Changed content is copied without changing dirty flags or Undo state. Damaged records are reported and cannot be restored. Limits are 512 files and 128 MiB payload per copy; the browser scans up to 256 records from newer sessions first. Older app sessions are retained; cross-session cleanup is currently manual. Capture timing/memory on large boards still needs native profiling.

These are periodic recovery copies, separate from normal Save and explicit project checkpoints. Edits since the most recent completed copy may not be recoverable after interruption. Native read/confirmation failures keep the window open; an explicit Quit attempts a final copy when automatic recovery is enabled.

## Linked components and multiple windows

Open the linked electronics design through its project file. With Select active in the schematic, double-click the component body. pCAD opens or focuses the linked board and zooms to the matching footprint. A board already on another screen is reused. Unplaced, unannotated or duplicate references are explained in Messages; opening unrelated schematic and board files separately does not create a project link.

Use the small **Detach to window** button beside a document/tool tab, or drag the tab outside the application. Move the detached window to another monitor with its OS title bar. To return it, drag the **tab** into the main layout until the docking overlay appears; drop in the center for tabs or at an edge for a split view. **Dock back** returns it directly. Closing a secondary window returns its tabs too.

All views share the same live documents, selection and Undo history. Save updates the same design files. Closing the main app retains the normal unsaved-work check. Shared project dialogs open in the main window, while canvas menus and popovers appear with their view.

Detached windows and their monitor positions are session-only. A restart with a saved detached layout starts with the default layout; reopen the project normally. Physical monitor/DPI behavior still needs native acceptance. These changes require the current executable described in the handoff.

## External changes and Save Copy

In the current source, Save checks whether a working design, library or project file was changed or removed on disk after pCAD loaded/saved it. A conflict leaves both the external file and your open edits intact, and explains the failure in Messages. A timestamp change with identical contents is allowed. Save All checks for known conflicts before starting its writes.

Choose **File → Save Copy…** and an unused filename to preserve the active document without changing its working path, project links, unsaved marker or Undo history. A symbol/footprint library-entry editor copies its complete containing library. Cancel writes nothing; an existing destination is rejected. The copy is not automatically opened. Save As changes the working path and also requires a new filename when changing destinations.

After preserving your version, deliberately close/reopen the external file when you want to load it; the usual unsaved-work safeguards apply. Do not discard your live edit merely to make Save succeed. Save Copy covers one document/library; project checkpoints and recovery copies have their own broader scope, and external model assets require separate care.

These checks are optimistic and cannot lock out every simultaneous external writer. Save All is not an all-or-nothing multi-file transaction if a later I/O failure or concurrent edit occurs. A safe rebuild is required before native testing of this source batch.

## Verification and follow-up

See [the handoff](HANDOFF-2026-10-29.md) for current automated counts and [the checklist](WORKFLOW-CHECKLIST-2026-10-28.md) for native acceptance. Headless browser tests mock IPC; native filesystem and importer tests exercise real disposable files separately. These tests do not certify manufacturing output or complete migration of a private library.
