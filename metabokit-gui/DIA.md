# MetaboKit 0.3 — DDA / DIA

Click **DDA** or **DIA** immediately below the MetaboKit wordmark to switch
workflows. The last mode is saved locally and restored on launch. Settings and
results are separate for each mode. The switch is disabled during an analysis;
the native bridge also enforces one analysis at a time.

## DIA workflow

Choose the dataset folder. When it contains the original `param.txt`, the app
imports its sample order, library selections, isolation windows and numerical
parameters. Relative paths resolve against that file, while original library
labels remain unchanged in reports. JSON presets retain both.

Without `param.txt`, select the centroided mzML files' folder, add an RT-annotated
spectral library, and enter the acquisition's exact isolation windows. DIA uses
absolute mass tolerances, not the DDA engine's ppm settings. Built-in libraries
and optional annotation metadata require the corresponding external `libs`
folder. No scientific libraries or sample data are bundled with the application.

Run into a new or empty output folder. Outputs retain the original schemas:

- `Report.csv`, `Report_fill_all.csv`, `target_list.csv`
- `misc/analyte.bin`, `misc/param.bin`, `misc/file_order.txt`
- Original-format MS1, MS2 and MS1-feature caches
- Additional `dia-settings.json` and, on success only, `dia-run.json`

Open this folder in DIA Visualizer to inspect precursor and fragment traces,
areas, peak metrics and fragment correlations. Original DIA output folders are
also supported. The target list is paged, and scan data is read only for the
selected target/sample/RT interval. Chromatograms retain absolute intensity;
the existing DDA mirror-spectrum normalization to 1,000 is unchanged.

## Peak integration

The Visualizer can re-integrate any result folder with caches, using the
integration module from MRMhub INTEGRATOR (`get_auc.rs`): trapezoidal area above
a selectable baseline, apex RT, height and FWHM. The kernel is in
`crates/metabokit-dia/src/peak.rs`; windows, edits and export are in
`integration.rs`.

- **Windows.** Each target/sample starts from the engine's own window:
  `RT ± half-width` when identified, the gap-filling window otherwise. Scans
  are included when `start ≤ RT < end`, as in the engine. Drag either edge, or
  drag across a chromatogram to draw a new window; ← → move an edge by one MS1
  scan (Shift: five). Edited edges sit midway between scans. The precursor and
  its fragments share one window. Edits save immediately and can be restored
  per sample or per target.
- **Baselines.** *None* is MetaboKit's own area. *Valley to valley*, *vertical
  drop* and *5th percentile* follow MRMhub; the percentile looks one window
  width either side of the window. Set a project default and per-target
  overrides.
- **Acquisition sequence.** Mark reference samples, preview, then apply.
  Start and end interpolate piecewise-linearly along elapsed acquisition time
  (the mzML `startTimeStamp` stored in each MS1 cache), holding the nearest
  reference outside their range. One reference copies its window to every
  sample. Missing or duplicate timestamps fall back to run order. This proposes
  bounds; it does not detect peaks.
- **Files.** Edits live in `integration/windows.json` (keyed by target id and
  sample file name, guarded by target name). *Export integrated report*
  re-integrates every target in every sample and writes
  `integration/Report_integrated.csv` (Report_fill_all.csv with only the
  `AREA_` columns replaced) and `integration/peaks.csv` (one row per
  trace/sample: window, source, baseline, apex, area, height, FWHM, scans).
  Files are replaced only after a complete write; a report that does not match
  `misc/analyte.bin`, or edits from another folder, stop the export.
  `Report.csv`, `Report_fill_all.csv` and the caches are never modified.
- Export shares the single analysis slot and Stop button with DDA/DIA runs.

With no edits and the *None* baseline, integration reproduces the engine
exactly. On the TQC data below, all 25,260 area cells (24,044 identified,
compared with Report.csv; 1,216 gap-filled, compared with Report_fill_all.csv)
match byte-for-byte, and a viewer spot check of 429 identified samples and 717
fragment traces matches `misc/analyte.bin` bit-for-bit. Export took 0.23 s.
Gap-filled windows use the report's rounded RT and m/z, as gap filling does.
Two related viewer changes: the isolation window is chosen from the library
mass (the engine's rule) rather than the measured m/z, and gap-filled traces
use the same rounding.

MRMhub is dual-licensed (AGPLv3 for non-commercial use, or commercial). Confirm
the licensing of `peak.rs` and the sequence logic in `ui/dia-integration.js`
before distributing builds.

## Compatibility and resource decisions

The DIA engine is a separate native Rust crate adapted from the supplied
MetaboKit DIA source, with its BSD-3-Clause license preserved in source and in
the app's resources. It does not call an external CLI or depend on the Downloads
folder. Numerical kernels, f32 arithmetic, window selection, library fragment
selection, threshold comparisons, integration and report ordering are retained.

- One sample is resident by default. Feature-detection windows use a private
  worker pool and merge in the original order. CPU workers and resident-sample
  count can be adjusted independently.
- Gap filling has bounded sample concurrency. Target-list generation loads one
  sample's MS1 scans at a time, retaining compact ridge results rather than every
  sample's scans. Total memory still depends on the largest sample and on the
  result matrix; this is not a fixed-memory streaming engine.
- Disabling “Keep scan caches” removes only generated scan/feature caches after
  successful completion. Reports, settings and original mzML files are kept.
- Cancellation is cooperative at scan/window/library-entry/report checkpoints.
  Partial files and the settings are retained; there is no successful-run marker.
- The viewer consumes the MS2 cache's collision-energy field correctly. The
  original viewer omitted this field when decoding; that decoding defect is not
  reproduced. Analysis output bytes are unchanged.

## Verification (2026-10-11)

Compared with a separately built, unmodified copy of the supplied DIA CLI on
the three `TQC_*_DIA.mzML` samples, matching library, annotation metadata and
`param.txt`:

- 3,634 library entries; 9,677 sample/library matches; 3,409 reported targets.
- All 15 report and cache files match **byte-for-byte**, including all numeric
  values and names. No numerical tolerances or path-label normalization were
  used in the comparison.
- Identical results with one worker, five workers/one resident sample, and
  five workers/two resident samples. Cache-disabled reports also match, and
  generated caches are absent afterward. Processing-stage cancellation tested.
- 29 Rust tests pass, including existing DDA and mirror-spectrum tests plus DIA
  parameter/library, parser, output-protection, cancellation and viewer tests.
  Two JavaScript window-parser tests pass. Browser smoke test covers the label
  toggle, reload persistence, independent settings, dataset controls, actual
  sample chromatograms, minimum window size and run/cancel events.

One separately measured warm-cache run used about 517 MiB peak RSS for the
original and 442 MiB for the one-resident-sample port (roughly 15% less). Elapsed
time was 1.94 s versus 2.55 s: the conservative default trades some batch
throughput for lower scan residency. These are indicative single-run figures,
not general performance guarantees. Higher resident-sample counts are available
when memory permits.

This establishes parity for the supplied data and tested cases, not proof for
every acquisition or instrument format. Numerical runtime parity has been
checked on Apple Silicon; the release also builds for Intel.

The final signed DMG was mounted read-only: both Mach-O architectures, version
0.3.0, bundled DIA license, and the app's hardened-runtime signature were checked.
The native release opened with existing DDA preferences, and its DIA mode and
populated chromatogram viewer were observed in the native webview. Notarization
is still pending because Apple rejected the saved Keychain profile with HTTP 401;
the user must refresh that credential before public distribution.

## Reproduce checks

From `metabokit-gui`:

```sh
cargo test --workspace
deno test ui/dia.test.js
cargo build --release -p metabokit-dia --examples
target/release/examples/run /path/to/param.txt /new/output
target/release/examples/compare /original/output /new/output
target/release/examples/validate /path/to/param.txt /original/output /new/checks
target/release/examples/integrate /copy/of/output   # writes only integration/
```

`integrate` exports with the folder's saved windows and compares every area
with the engine's reports; run it on a copy without edits for the parity check.

`validate` refuses an existing destination, checks multiple resource settings
and cancellation, and cleans up its temporary cases. It keeps a `verified` run.

With Node and Playwright available, `node scripts/ui-smoke.cjs` runs an isolated
browser test with a mocked native bridge. Set `METABOKIT_DIA_RESULTS` to a real
run to render its actual targets and traces through the native `inspect` helper.
This complements the Rust tests; it does not itself test Tauri IPC.

The macOS bundle is now named `MetaboKit GUI.app`; its existing
`app.metabokit.dda` identifier is retained to preserve local preferences.
Distribution still requires Developer ID signing and Apple notarization; see
`MACOS_DISTRIBUTION.md`.
