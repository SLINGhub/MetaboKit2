use super::*;
use metabokit_core::progress::{Cancel, Silent};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "metabokit-dia-test-{}-{}-{id}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn settings(root: &Path) -> Settings {
    let file = root.join("sample.mzML");
    fs::write(&file, "").unwrap();
    let lib = root.join("library.txt");
    fs::write(&lib, "").unwrap();
    Settings {
        mzml_files: vec![file],
        libraries: vec![format!("user {}", lib.display())],
        windows: vec![(400., 600.)],
        output_dir: root.join("out"),
        ..Default::default()
    }
}

#[test]
fn invalid_settings_and_duplicate_cache_names_are_rejected() {
    let scratch = Scratch::new();
    let mut s = settings(&scratch.0);
    assert!(s.validate().is_ok());
    s.windows = vec![(500., 400.)];
    assert!(s.validate().is_err());
    s.windows = vec![(400., 600.)];
    s.ms1_tol = f32::NAN;
    assert!(s.validate().is_err());
    s.ms1_tol = 0.008;
    s.max_files_in_flight = 0;
    assert!(s.validate().is_err());
    s.max_files_in_flight = 1;
    s.mzml_files.push(s.mzml_files[0].clone());
    assert!(s.validate().is_err());
    s.mzml_files.pop();
    s.libraries = vec!["unknown-library".into()];
    assert!(s.validate().is_err());
}

#[test]
fn cancellation_and_output_protection_leave_existing_files_untouched() {
    let scratch = Scratch::new();
    let s = settings(&scratch.0);
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(run(&s, &Silent, &cancel).is_err());
    assert!(!s.output_dir.exists());
    fs::create_dir(&s.output_dir).unwrap();
    fs::write(s.output_dir.join("keep.txt"), "keep").unwrap();
    assert!(
        run(&s, &Silent, &Cancel::new())
            .unwrap_err()
            .to_string()
            .contains("not empty")
    );
    assert_eq!(
        fs::read_to_string(s.output_dir.join("keep.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn original_parameter_import_preserves_source_labels_and_resolves_paths() {
    let scratch = Scratch::new();
    let text = r#"mzML_files = 'data/*.mzML'
length_of_ion_chromatogram = [0.05,0.8]
library = ['user library.txt']
ms1tol=0.008
ms2tol=0.05
integration_RT=2
integration_mz=0.01
MS2_score=0.5
min_peaks=1
peak_shape=0.1
sn_score=0.4
RT_shift=0.1
num_threads=5
impute_width=0
window_setting=[200,480,479,496]
"#;
    let path = scratch.0.join("param.txt");
    fs::write(&path, text).unwrap();
    let s = Settings::load(&path).unwrap();
    assert_eq!(s.windows, vec![(200., 480.), (479., 496.)]);
    assert_eq!(
        s.libraries[0],
        format!("user {}", scratch.0.join("library.txt").display())
    );
    assert_eq!(s.library_labels[&s.libraries[0]], "library.txt");
    let saved = scratch.0.join("preset.json");
    fs::write(&saved, serde_json::to_vec(&s).unwrap()).unwrap();
    assert_eq!(
        Settings::load(&saved).unwrap().library_labels,
        s.library_labels
    );
    fs::write(&path, text.replace("min_peaks=1", "")).unwrap();
    assert!(Settings::load(&path).is_err());
}

#[test]
fn library_fragment_selection_and_labels_follow_original_rules() {
    let scratch = Scratch::new();
    let mut s = settings(&scratch.0);
    fs::write(scratch.0.join("library.txt"),"Name: Example\nPrecursorMZ: 500\nPrecursorType: [M+H]+\nRetentionTime: 2\nNum Peaks: 4\n100 900\n150 200 *\n200 100 *\n499.9 800 *\n\n").unwrap();
    s.library_labels
        .insert(s.libraries[0].clone(), "library.txt".into());
    let lib = readlib::get_cpds(&s.to_param(Cancel::new()).unwrap()).unwrap();
    assert_eq!(lib.len(), 1);
    assert_eq!(lib[0].mz_i, vec![(150., 200.), (200., 100.)]);
    assert_eq!(lib[0].name, "Example M+H (library.txt)");
    assert_eq!(lib[0].rt, Some(2.));
    fs::write(
        scratch.0.join("library.txt"),
        "Name: Bad\nPrecursorMZ: NaN\nNum Peaks: 1\n100 10\n\n",
    )
    .unwrap();
    assert!(readlib::get_cpds(&s.to_param(Cancel::new()).unwrap()).is_err());
}

fn f32s(w: &mut impl Write, xs: &[f32]) {
    for x in xs {
        w.write_all(&x.to_le_bytes()).unwrap();
    }
}
fn scan(w: &mut impl Write, pre: Option<f32>, rt: f32, mz: f32, intensity: f32) {
    if let Some(pre) = pre {
        f32s(w, &[pre]);
    }
    f32s(w, &[rt]);
    if pre.is_some() {
        f32s(w, &[35.]);
    }
    w.write_all(&1u32.to_le_bytes()).unwrap();
    f32s(w, &[mz, intensity]);
}

/// Two samples, 103 targets of mass 500 with one fragment at 150; each target
/// is identified in `one.mzML` and gap-filled in `two.mzML`.
fn viewer_fixture(scratch: &Scratch) -> PathBuf {
    let misc = scratch.0.join("misc");
    fs::create_dir(&misc).unwrap();
    fs::write(misc.join("file_order.txt"), "one.mzML\ntwo.mzML\n").unwrap();
    let mut a = fs::File::create(misc.join("analyte.bin")).unwrap();
    for i in 0..103 {
        a.write_all(format!("Target {i}\0").as_bytes()).unwrap();
        f32s(&mut a, &[500.]);
        a.write_all(&1u16.to_le_bytes()).unwrap();
        f32s(&mut a, &[150., 1000.]);
        a.write_all(&[1]).unwrap();
        f32s(&mut a, &[1., 0.2, 500., 120., 0.9, 0.8, 0.75, 240.]);
        a.write_all(&[0]).unwrap();
        f32s(&mut a, &[1., 0.2, 500.]);
    }
    drop(a);
    let mut p = fs::File::create(misc.join("param.bin")).unwrap();
    f32s(&mut p, &[0.01, 0.008, 0.05]);
    p.write_all(&[2]).unwrap();
    f32s(&mut p, &[400., 550., 490., 600.]);
    drop(p);
    for sample in ["one.mzML", "two.mzML"] {
        let mut ms1 = fs::File::create(misc.join(format!("ms1_{sample}.bin"))).unwrap();
        ms1.write_all(b"date\0").unwrap();
        let mut ms2 = fs::File::create(misc.join(format!("ms2_{sample}.bin"))).unwrap();
        for (rt, intensity) in [(0.6, 2.), (0.8, 10.), (1., 20.), (1.2, 10.), (1.4, 2.)] {
            scan(&mut ms1, None, rt, 500., intensity);
            scan(&mut ms2, Some(475.), rt, 150., intensity * 2.);
            scan(&mut ms2, Some(580.), rt, 150., 999.);
        }
    }
    misc
}

#[test]
fn paged_viewer_matches_original_traces_and_reads_ms2_collision_energy() {
    let scratch = Scratch::new();
    let misc = viewer_fixture(&scratch);
    let first = visualizer::open(&scratch.0, "", 0).unwrap();
    assert_eq!(first.targets.len(), 100);
    assert_eq!(first.total, 103);
    assert!(first.caches);
    let second = visualizer::open(&scratch.0, "", 100).unwrap();
    assert_eq!(second.targets.len(), 3);
    assert_eq!(second.targets[0].id, 100);
    assert_eq!(
        visualizer::open(&scratch.0, "target 102", 0).unwrap().total,
        1
    );
    let d = visualizer::detail(&scratch.0, 102, 0, 0.3, None, None).unwrap();
    let ms1 = common::get_ms1(&misc, "one.mzML").unwrap();
    let ms2 = common::get_ms2(&misc, "one.mzML").unwrap();
    assert_eq!(d.precursor, common::get_chrom((500., 1., 0.3), &ms1, 0.01));
    assert_eq!(
        d.fragments[0].points,
        common::get_chrom2((150., 1., 0.3), &score::get_swath(&ms2, (400., 550.)), 0.05)
    );
    assert_eq!(d.fragments[0].correlation, 0.75);
    assert_eq!(d.fragments[0].area, 240.);
    assert!(
        !visualizer::detail(&scratch.0, 1, 1, 0.3, None, None)
            .unwrap()
            .present
    );
    assert!(visualizer::detail(&scratch.0, 999, 0, 0.3, None, None).is_err());
    assert!(visualizer::detail(&scratch.0, 0, 0, f32::NAN, None, None).is_err());
    fs::remove_file(misc.join("ms2_one.mzML.bin")).unwrap();
    assert!(!visualizer::open(&scratch.0, "", 0).unwrap().caches);
}

fn report_fixture(root: &Path) {
    let mut report = String::from(
        "id_number,name,MS_level,precursor_m/z,fragment_m/z,RT,AREA_one,AREA_two,RT_one,RT_two\n",
    );
    for i in 0..103 {
        let id = i + 1;
        report += &format!("{id},Target {i},1,500.0000,,1.00,120.0,9.9,1.00,\n");
        report += &format!("{id},Target {i},2,500.0000,150.0000,1.00,240.0,9.9,1.00,\n");
    }
    fs::write(root.join("Report_fill_all.csv"), report).unwrap();
}
fn area_cell(csv_text: &str, row: usize, column: &str) -> String {
    let mut lines = csv_text.lines();
    let header: Vec<_> = lines.next().unwrap().split(',').collect();
    let col = header.iter().position(|h| *h == column).unwrap();
    lines
        .nth(row)
        .unwrap()
        .split(',')
        .nth(col)
        .unwrap()
        .to_string()
}

#[test]
fn integration_windows_are_saved_resolved_and_restored() {
    use integration::{Baseline, Source, Window, WindowChange};
    let scratch = Scratch::new();
    viewer_fixture(&scratch);
    let root = &scratch.0;
    let t = integration::target(root, 0).unwrap();
    assert_eq!(t.baseline, Baseline::None);
    // Identified: RT ± half-width; gap-filled: the same, around the report RT.
    assert_eq!(
        t.samples[0].window,
        Window {
            start: 1. - 0.2,
            end: 1. + 0.2
        }
    );
    assert_eq!(t.samples[1].engine, t.samples[0].engine);
    assert!(t.samples.iter().all(|s| s.source == Source::Engine));
    let edit = Window {
        start: 0.7,
        end: 1.3,
    };
    let t = integration::set_windows(
        root,
        0,
        &[WindowChange {
            sample: 1,
            window: Some(edit),
            source: Source::Manual,
        }],
    )
    .unwrap();
    assert_eq!(
        (t.samples[1].window, t.samples[1].source),
        (edit, Source::Manual)
    );
    assert_eq!(t.samples[0].source, Source::Engine);
    assert_eq!(
        visualizer::open(root, "target 0", 0).unwrap().targets[0].edited,
        1
    );
    // The detail view integrates the saved window unless a proposal is given.
    let d = visualizer::detail(root, 0, 1, 0.3, None, None).unwrap();
    assert_eq!((d.window, d.source), (edit, Source::Manual));
    let proposal = Window {
        start: 0.9,
        end: 1.1,
    };
    let d = visualizer::detail(
        root,
        0,
        1,
        0.3,
        Some(proposal),
        Some(Baseline::VerticalDrop),
    )
    .unwrap();
    assert_eq!(
        (d.window, d.precursor_peak.scans, d.baseline),
        (proposal, 1, Baseline::VerticalDrop)
    );
    assert!(
        visualizer::detail(
            root,
            0,
            1,
            0.3,
            Some(Window {
                start: 1.1,
                end: 1.0
            }),
            None
        )
        .is_err()
    );
    // Restoring the engine window removes the edit, and the file stays tidy.
    integration::set_windows(
        root,
        0,
        &[WindowChange {
            sample: 1,
            window: None,
            source: Source::Manual,
        }],
    )
    .unwrap();
    assert!(integration::load(root).unwrap().targets.is_empty());
    integration::set_baseline(root, Some(0), Some(Baseline::ValleyToValley)).unwrap();
    integration::set_baseline(root, None, Some(Baseline::Percentile)).unwrap();
    let t = integration::target(root, 0).unwrap();
    assert!(t.custom_baseline && t.baseline == Baseline::ValleyToValley);
    assert_eq!(
        integration::target(root, 1).unwrap().baseline,
        Baseline::Percentile
    );
    let o = integration::overview(root).unwrap();
    assert_eq!(o.samples[0].acquired.as_deref(), Some("date"));
    assert_eq!(
        (o.baseline, o.edited_windows, o.exported.is_none()),
        (Baseline::Percentile, 0, true)
    );
    assert!(
        integration::set_windows(
            root,
            0,
            &[WindowChange {
                sample: 9,
                window: Some(edit),
                source: Source::Manual
            }],
        )
        .is_err()
    );
}

#[test]
fn export_replaces_only_area_columns_and_rejects_mismatched_reports() {
    use integration::{Source, Window, WindowChange};
    let scratch = Scratch::new();
    let misc = viewer_fixture(&scratch);
    let root = &scratch.0;
    report_fixture(root);
    let source = fs::read_to_string(root.join("Report_fill_all.csv")).unwrap();
    let out = integration::export(root, &Silent, &Cancel::new()).unwrap();
    assert_eq!(
        (out.samples, out.targets, out.rows, out.edited_windows),
        (2, 103, 206, 0)
    );
    let report = fs::read_to_string(&out.report).unwrap();
    // Engine windows, no baseline: the engine's own trapezoid over its scans.
    let ms1 = common::get_ms1(&misc, "one.mzML").unwrap();
    let expected = peak::integrate(
        &common::get_chrom((500., 1., 0.2), &ms1, 0.01),
        Window {
            start: 0.8,
            end: 1.2,
        },
        integration::Baseline::None,
    );
    assert_eq!(
        area_cell(&report, 0, "AREA_one"),
        format!("{:.1}", expected.area)
    );
    let engine_area: f32 = common::get_chrom((500., 1., 0.2), &ms1, 0.01)
        .windows(2)
        .map(|w| (w[0].1 + w[1].1) * (w[1].0 - w[0].0))
        .sum::<f32>()
        * 30.;
    assert_eq!(expected.area, engine_area);
    // Everything but AREA_ columns is copied through unchanged.
    for (a, b) in source.lines().zip(report.lines()) {
        let (a, b): (Vec<_>, Vec<_>) = (a.split(',').collect(), b.split(',').collect());
        assert_eq!(a.len(), b.len());
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            if ![6, 7].contains(&i) {
                assert_eq!(x, y);
            }
        }
    }
    let peaks = fs::read_to_string(&out.peaks).unwrap();
    assert_eq!(peaks.lines().count(), 1 + 206 * 2);
    assert!(
        peaks
            .lines()
            .nth(1)
            .unwrap()
            .contains(",one,identified,engine,none,")
    );
    integration::set_windows(
        root,
        0,
        &[WindowChange {
            sample: 0,
            window: Some(Window {
                start: 0.9,
                end: 1.1,
            }),
            source: Source::Sequence,
        }],
    )
    .unwrap();
    assert!(integration::overview(root).unwrap().stale);
    let out = integration::export(root, &Silent, &Cancel::new()).unwrap();
    let edited = fs::read_to_string(&out.report).unwrap();
    assert_eq!(area_cell(&edited, 0, "AREA_one"), "0.0"); // a single scan has no area
    assert_eq!(
        area_cell(&edited, 2, "AREA_one"),
        area_cell(&report, 2, "AREA_one")
    );
    assert!(
        fs::read_to_string(&out.peaks)
            .unwrap()
            .contains(",one,identified,sequence,none,0.900,1.100,")
    );
    // A report that doesn't belong to this cache is refused, leaving outputs intact.
    fs::write(
        root.join("Report_fill_all.csv"),
        source.replacen("150.0000", "151.0000", 1),
    )
    .unwrap();
    assert!(
        integration::export(root, &Silent, &Cancel::new())
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    assert_eq!(fs::read_to_string(&out.report).unwrap(), edited);
    assert!(!root.join("integration/peaks.tmp").exists());
    fs::write(root.join("Report_fill_all.csv"), &source).unwrap();
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(integration::export(root, &Silent, &cancel).is_err());
    assert_eq!(fs::read_to_string(&out.report).unwrap(), edited);
    // Edits recorded for another result folder are never applied silently.
    let edits = root.join("integration/windows.json");
    let text = fs::read_to_string(&edits)
        .unwrap()
        .replace("Target 0", "Other");
    fs::write(&edits, text).unwrap();
    assert!(integration::export(root, &Silent, &Cancel::new()).is_err());
}
