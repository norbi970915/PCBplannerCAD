# Third-party source availability

Third-party components retain their own licenses. Original PCBPlanner Studio code remains the personal project of Buchmüller Norbert and is not automatically licensed under those components' licenses.

The accompanying `generated/third-party-notices.txt` identifies dependency versions, declared license expressions, preserved license/copyright notices and upstream repositories. The build records both frontend dependency and Windows native dependency graphs, including some build-only components; inclusion in that inventory does not mean each component executes in the installed app.

For the Mozilla Public License 2.0 components recorded in the native graph, the complete original versioned crate source archives are supplied alongside the application under `generated/sources/`. These archives retain their original source and license notices. The generated manifest records their versions and hashes. Corresponding covered source is made available under the MPL, independently of PCBPlanner Studio's free-use terms. The build refuses to prepare a release if a required source archive or notice is missing.

These source archives can be opened with tools that read gzip-compressed tar archives. The original upstream repository and package coordinates are also recorded in the inventory. Contact pcbplanner.info@gmail.com if a supplied source archive cannot be read or an included notice appears incomplete.

This release does not include the previous STEP-preview engine or its JavaScript/WebAssembly bindings. Its LGPL dependency is excluded from the release dependency graph and frontend artifact. VRML rendering uses separately licensed components described in the notices.

About / Legal displays these documents and can open the local legal-files folder. Preserve this folder, the source archives and their notices when sharing a complete release package.
