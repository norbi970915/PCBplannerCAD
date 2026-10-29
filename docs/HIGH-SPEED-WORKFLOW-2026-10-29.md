# Routing length feedback and project audit — 2026-10-29

Native executable rebuilt at target/release/pCAD.exe. Rebuild locally after pulling these source changes.

## Implemented

- Interactive routing now displays accepted-preview routed copper length, including existing copper and shove replacements. It shows red below/above the allowed range and green within it. Pending previews remain neutral.
- Targets resolve from the first enabled matching native Length rule, or a session manual target with tolerance. Manual bounds cannot widen a board rule. Whole-net rules use a track context without layer or component ownership.
- Pair routing displays both member totals and their mismatch against a session limit. These are copper totals, including branches; vias, pad/package transit, propagation delay and impedance are excluded. Session targets/mismatch limits are not saved design rules.
- Full DRC now evaluates native Length rules, including arc copper and zero-length pad nets. Incremental DRC drops stale length markers; run full DRC for final length acceptance.
- Length tuning has a debounced, read-only geometry preview and target meter. Apply recomputes on the latest board; locked chains are rejected. This remains selected-chain tuning, without coordinated pair meanders.
- Structured import registers complete matching _P/_N, _DP/_DN and +/- net pairs; orphan names never create a partner. Pair polarity and constraints must be reviewed. Explicit zero minimum track width remains zero. An external custom-rule file produces an unsupported-constraints warning.
- A project_audit command-line example imports a complete project into a new folder, preserves originals and generates board/length/ERC/ECO/routing probe reports.

## Validation

TypeScript check and release build passed. Editor tests: 188 passed, including public-presentation checks. Core tests: 219 passed, 3 performance tests ignored. Import tests: 46 passed.

Disposable six-layer carrier example: 8 schematic sheets, 89 compiled parts, 204 schematic nets, 2,866 board objects, 158 measured board nets and 32 inferred pairs. ERC: zero findings. Board connectivity: zero unrouted connections. Correct zero-width import removed 1,288 false width findings; 107 other findings remain (79 clearance, 7 outline, 6 keepout, 8 courtyard, 7 silk).

Eight read-only existing signal-track routing probes reached their endpoints (about 10–12 ms each in an unoptimized CLI run). Six paired probes reached both endpoints; two rejected the starting clearance. Two single-chain +5% tuning probes succeeded; six returned insufficient-room diagnostics with no mutations. These local samples do not establish general responsiveness or complete high-speed workflow acceptance.

Schematic-to-board audit proposes 68 parameter updates, 21 net updates, 28 footprint removals and 6 warnings. Nothing from that plan was applied. Bus-alias import warnings and parity differences require review before any schematic-to-board update.

## Open work

- Native smoke check: full project and all three editable libraries opened; 2,866-object board rendered in 0.28 s according to the app. Routing toolbar and red below-target bar appeared with a 25 mm manual target. Full native acceptance through green/over-target states, pair feedback and Undo/Redo remains open; scripted boundary arithmetic passed.
- Improve pair starts/fanout in constrained geometry; inspect the two rejected probes before changing clearance behavior.
- Coordinated differential meanders, persistent skew rules, signal-path/delay measurement, paired layer transitions and custom-rule translation remain unsupported.
- Review existing board findings and bus/hierarchy parity differences rather than treating an imported example as manufacturing approval.

Local reports and external example copies remain under the Git-ignored local-test-boards folder. Private reference research remains under Git-ignored docs/local-notes.
