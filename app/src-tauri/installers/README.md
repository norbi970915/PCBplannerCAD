# Windows installer templates

These templates are pinned to the locked Tauri CLI 2.12.4. Both installer formats explicitly assign the application icon to launch shortcuts. The setup installer also repairs icon metadata while migrating existing shortcuts; the shared desktop creation function covers silent setup and the wizard finish checkbox. Existing shortcut choices and application IDs remain intact.

The build verifies template checksums and the CLI version before packaging. When updating the CLI, review upstream template changes and refresh provenance.json deliberately. Required upstream notice is retained in LICENSE/upstream/tauri-installer-templates@2.12.4.txt and generated third-party notices.
