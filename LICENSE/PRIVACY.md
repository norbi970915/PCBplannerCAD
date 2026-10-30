# Local data and privacy

PCBPlanner Studio is a personal project operated by Buchmüller Norbert, Romania. Contact: pcbplanner.info@gmail.com.

The desktop application has no accounts, advertising, payments or application analytics. Its editing, calculations and supported previews run locally. Opening a design does not upload that design to the publisher.

The app stores preferences, panel layouts, view presets and recent project/working-folder paths in its local WebView storage. Unsaved-document recovery is enabled by default and periodically creates local recovery copies in the app's local-data recovery folder. Recovery copies may contain private design content. The Recovery copies view restores them into a new folder; Preferences can disable future automatic copies. Disabling recovery does not erase existing copies.

For this release the app identity is `com.pcbplanner.studio`. On Windows its local data normally resides under `%LOCALAPPDATA%\com.pcbplanner.studio`. Existing data from the previous identity are preserved during migration; previous folders are never deleted automatically. Close all running copies before migrating or manually clearing app data. About identifies the local data and legal-files folders.

Projects, saved revisions, exports and imported library/model files remain in the folders you choose. Clearing app preferences or uninstalling the program does not necessarily delete these files or recovery copies. Back up your work before manually removing local data. Settings, source files and recovery copies do not automatically synchronize between computers.

File contents, paths and error details may appear in the app's local Messages and optional timing reports. Review those reports before sharing them. Clipboard/export/share actions copy the information you explicitly select. Support emails and any designs you voluntarily send to pcbplanner.info@gmail.com are received through the email provider; send only information necessary for support. You can ask about or request deletion of support correspondence using that address, subject to applicable retention obligations.

The Windows WebView runtime, operating system and installer may make their own update, security or diagnostic requests under their providers' settings. Download-website hosting has its own request processing. The desktop app's local-processing description is not a promise that the entire operating system makes no network requests.

This release previews VRML models locally. STEP model previews and thumbnails are unavailable; imported STEP references are retained, and the separate STEP export feature remains available. Check exported geometry and external assets before sharing a project.

Updated 30 October 2026. If online or diagnostic features are added, this information must be reviewed before release.
