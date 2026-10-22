//! Reviewed peak integration for DIA result folders.
//!
//! Each target/sample is integrated over a retention-time window. By default
//! that is the engine's own window (`RT ± half-width`, or the gap-filling
//! window), so the `None` baseline reproduces Report.csv and the gap-filled
//! values of Report_fill_all.csv. Edited windows and baseline choices are kept
//! in `integration/windows.json`; exports are written beside it. The engine's
//! reports and caches are never modified.
use crate::peak::{self, Peak};
use crate::visualizer::{self, Analyte};
use crate::{Failure, common};
use metabokit_core::progress::{Cancel, Event, Reporter};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const DIR: &str = "integration";
const EDITS: &str = "windows.json";
const REPORT: &str = "Report_integrated.csv";
const PEAKS: &str = "peaks.csv";
const SUMMARY: &str = "export.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Baseline {
    /// No baseline subtraction: MetaboKit's own area.
    #[default]
    None,
    /// Flat, at the 5th percentile of the trace from one window width before
    /// the window to one after it.
    Percentile,
    /// Flat, at the lowest intensity inside the window.
    VerticalDrop,
    /// A line between the window's first and last scans.
    ValleyToValley,
}
impl Baseline {
    fn label(self) -> &'static str {
        match self {
            Baseline::None => "none",
            Baseline::Percentile => "percentile",
            Baseline::VerticalDrop => "vertical-drop",
            Baseline::ValleyToValley => "valley-to-valley",
        }
    }
}

/// Where a sample's window came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Engine,
    Manual,
    /// Interpolated from reference samples along the acquisition sequence.
    Sequence,
}
impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Engine => "engine",
            Source::Manual => "manual",
            Source::Sequence => "sequence",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub start: f32,
    pub end: f32,
}
impl Window {
    pub fn validate(self) -> Result<Self, Failure> {
        if !self.start.is_finite()
            || !self.end.is_finite()
            || self.start < 0.0
            || self.end <= self.start
            || self.end - self.start > 10.0
        {
            return Err(
                "An integration window needs a start before its end, at most 10 minutes apart."
                    .into(),
            );
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Edited {
    pub start: f32,
    pub end: f32,
    pub source: Source,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetEdits {
    /// Guards against applying edits to a different result folder.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Baseline>,
    /// Keyed by sample file name.
    #[serde(default)]
    pub windows: BTreeMap<String, Edited>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edits {
    pub version: u32,
    #[serde(default)]
    pub baseline: Baseline,
    /// Keyed by target id (position in misc/analyte.bin).
    #[serde(default)]
    pub targets: BTreeMap<usize, TargetEdits>,
}
impl Default for Edits {
    fn default() -> Self {
        Self {
            version: 1,
            baseline: Baseline::None,
            targets: BTreeMap::new(),
        }
    }
}
impl Edits {
    /// Saved window, its source and the effective baseline for one sample.
    pub(crate) fn resolve(
        &self,
        target: usize,
        name: &str,
        sample: &str,
        engine: Window,
    ) -> (Window, Source, Baseline) {
        let edits = self.targets.get(&target).filter(|t| t.name == name);
        let baseline = edits.and_then(|t| t.baseline).unwrap_or(self.baseline);
        match edits.and_then(|t| t.windows.get(sample)) {
            Some(e) => (
                Window {
                    start: e.start,
                    end: e.end,
                },
                e.source,
                baseline,
            ),
            None => (engine, Source::Engine, baseline),
        }
    }
    fn windows(&self) -> usize {
        self.targets.values().map(|t| t.windows.len()).sum()
    }
}

pub fn load(root: &Path) -> Result<Edits, Failure> {
    let path = root.join(DIR).join(EDITS);
    if !path.exists() {
        return Ok(Edits::default());
    }
    let edits: Edits = serde_json::from_slice(&std::fs::read(&path)?)
        .map_err(|e| format!("integration/{EDITS} could not be read: {e}"))?;
    if edits.version != 1 {
        return Err(format!("integration/{EDITS} has an unsupported version").into());
    }
    Ok(edits)
}
/// Replaces `path` only after the new contents are completely written.
fn replace(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let tmp = path.with_extension("tmp");
    let result = std::fs::File::create(&tmp)
        .map_err(Failure::from)
        .and_then(|mut f| {
            write(&mut f)?;
            f.sync_all()?;
            Ok(())
        });
    match result.and_then(|_| std::fs::rename(&tmp, path).map_err(Failure::from)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}
fn save(root: &Path, edits: &mut Edits) -> Result<(), Failure> {
    edits
        .targets
        .retain(|_, t| t.baseline.is_some() || !t.windows.is_empty());
    std::fs::create_dir_all(root.join(DIR))?;
    replace(&root.join(DIR).join(EDITS), |f| {
        serde_json::to_writer_pretty(&mut *f, edits)?;
        f.write_all(b"\n")?;
        Ok(())
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleWindow {
    pub present: bool,
    pub rt: f32,
    pub engine: Window,
    pub window: Window,
    pub source: Source,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetWindows {
    pub id: usize,
    pub name: String,
    pub baseline: Baseline,
    /// True when this target overrides the project baseline.
    pub custom_baseline: bool,
    pub samples: Vec<SampleWindow>,
}
/// Every sample's window for one target, for the sequence overview.
pub fn target(root: &Path, target: usize) -> Result<TargetWindows, Failure> {
    let samples = visualizer::samples(root)?;
    let analyte = visualizer::analyte(root, target, samples.len())?;
    let p = visualizer::cache_params(root)?;
    let edits = load(root)?;
    let custom = edits
        .targets
        .get(&target)
        .filter(|t| t.name == analyte.name)
        .and_then(|t| t.baseline);
    let samples = samples
        .iter()
        .zip(&analyte.records)
        .map(|(sample, record)| {
            let engine = visualizer::spec(&analyte, record, &p).engine;
            let (window, source, _) = edits.resolve(target, &analyte.name, sample, engine);
            SampleWindow {
                present: record.present,
                rt: record.rt,
                engine,
                window,
                source,
            }
        })
        .collect();
    Ok(TargetWindows {
        id: target,
        name: analyte.name,
        baseline: custom.unwrap_or(edits.baseline),
        custom_baseline: custom.is_some(),
        samples,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowChange {
    pub sample: usize,
    /// `None` restores the engine window.
    pub window: Option<Window>,
    pub source: Source,
}
/// Saves window edits for one target and returns its updated windows.
pub fn set_windows(
    root: &Path,
    target: usize,
    changes: &[WindowChange],
) -> Result<TargetWindows, Failure> {
    let samples = visualizer::samples(root)?;
    let analyte = visualizer::analyte(root, target, samples.len())?;
    let p = visualizer::cache_params(root)?;
    let mut edits = load(root)?;
    let entry = edits.targets.entry(target).or_default();
    if entry.name != analyte.name {
        // Stale entries (another folder's edits) are replaced, never merged.
        *entry = TargetEdits {
            name: analyte.name.clone(),
            ..Default::default()
        };
    }
    for change in changes {
        let sample = samples.get(change.sample).ok_or("Sample not found")?;
        let engine = visualizer::spec(&analyte, &analyte.records[change.sample], &p).engine;
        match change.window.map(Window::validate).transpose()? {
            Some(w) if w != engine => {
                if change.source == Source::Engine {
                    return Err("An edited window needs a manual or sequence source.".into());
                }
                entry.windows.insert(
                    sample.clone(),
                    Edited {
                        start: w.start,
                        end: w.end,
                        source: change.source,
                    },
                );
            }
            _ => {
                entry.windows.remove(sample);
            }
        }
    }
    save(root, &mut edits)?;
    self::target(root, target)
}
/// Sets one target's baseline (`None` follows the project), or the project
/// default when `target` is `None`.
pub fn set_baseline(
    root: &Path,
    target: Option<usize>,
    baseline: Option<Baseline>,
) -> Result<(), Failure> {
    let mut edits = load(root)?;
    match target {
        None => edits.baseline = baseline.ok_or("Choose a project baseline.")?,
        Some(target) => {
            let samples = visualizer::samples(root)?;
            let name = visualizer::analyte(root, target, samples.len())?.name;
            let entry = edits.targets.entry(target).or_default();
            if entry.name != name {
                *entry = TargetEdits {
                    name,
                    ..Default::default()
                };
            }
            entry.baseline = baseline;
        }
    }
    save(root, &mut edits)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Acquisition {
    pub name: String,
    /// The mzML run's startTimeStamp, from the MS1 cache.
    pub acquired: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub report: PathBuf,
    pub peaks: PathBuf,
    pub samples: usize,
    pub targets: usize,
    pub rows: usize,
    pub edited_windows: usize,
    pub custom_baselines: usize,
    pub baseline: Baseline,
    pub elapsed_seconds: f64,
    /// Unix time in seconds.
    pub finished_at: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub samples: Vec<Acquisition>,
    pub baseline: Baseline,
    pub edited_windows: usize,
    pub edited_targets: usize,
    pub exported: Option<Exported>,
    /// Windows or baselines changed after the last export.
    pub stale: bool,
}
pub fn overview(root: &Path) -> Result<Overview, Failure> {
    let edits = load(root)?;
    let samples = visualizer::samples(root)?
        .into_iter()
        .map(|name| Acquisition {
            acquired: visualizer::acquired(root, &name),
            name,
        })
        .collect();
    let dir = root.join(DIR);
    let exported: Option<Exported> = std::fs::read(dir.join(SUMMARY))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .filter(|e: &Exported| e.report.is_file());
    let modified = |name: &str| {
        std::fs::metadata(dir.join(name))
            .and_then(|m| m.modified())
            .ok()
    };
    let stale = exported.is_some() && modified(EDITS) > modified(SUMMARY);
    Ok(Overview {
        samples,
        baseline: edits.baseline,
        edited_windows: edits.windows(),
        edited_targets: edits
            .targets
            .values()
            .filter(|t| !t.windows.is_empty())
            .count(),
        exported,
        stale,
    })
}

fn ms1_trace(scans: &[crate::Ms], mz: f32, tol: f32, lo: f32, hi: f32) -> Vec<(f32, f32)> {
    let (mz0, mz1) = (mz - tol, mz + tol);
    scans[scans.partition_point(|x| x.rt < lo)..]
        .iter()
        .take_while(|x| x.rt < hi)
        .map(|sc| {
            let pos0 = sc.mz_i.partition_point(|x| x.0 < mz0);
            let i = sc.mz_i[pos0..]
                .iter()
                .take_while(|x| x.0 < mz1)
                .map(|x| x.1)
                .reduce(f32::max)
                .unwrap_or(0.0);
            (sc.rt, i)
        })
        .collect()
}
fn ms2_trace(
    scans: &[(f32, &Vec<(f32, f32)>)],
    mz: f32,
    tol: f32,
    lo: f32,
    hi: f32,
) -> Vec<(f32, f32)> {
    let (mz0, mz1) = (mz - tol, mz + tol);
    scans[scans.partition_point(|x| x.0 < lo)..]
        .iter()
        .take_while(|x| x.0 < hi)
        .map(|(rt, mz_i)| {
            let pos0 = mz_i.partition_point(|x| x.0 < mz0);
            let i = mz_i[pos0..]
                .iter()
                .take_while(|x| x.0 < mz1)
                .map(|x| x.1)
                .reduce(f32::max)
                .unwrap_or(0.0);
            (*rt, i)
        })
        .collect()
}

struct Resolved {
    window: Window,
    source: Source,
}
/// Re-integrates every target in every sample with the saved windows and
/// baselines. Writes `integration/Report_integrated.csv` (Report_fill_all.csv
/// with integrated areas) and the long-format `integration/peaks.csv`.
pub fn export(root: &Path, reporter: &dyn Reporter, cancel: &Cancel) -> Result<Exported, Failure> {
    let started = Instant::now();
    let samples = visualizer::samples(root)?;
    if !visualizer::has_caches(root, &samples) {
        return Err(
            "Scan caches are absent. Rerun the analysis with caches kept to integrate.".into(),
        );
    }
    let p = visualizer::cache_params(root)?;
    let edits = load(root)?;
    let analytes = visualizer::analytes(root, samples.len())?;
    for (&id, t) in &edits.targets {
        if analytes.get(id).is_none_or(|a| a.name != t.name) {
            return Err(format!(
                "integration/{EDITS} does not match this result folder (target {} “{}”).",
                id + 1,
                t.name
            )
            .into());
        }
    }
    let baselines: Vec<Baseline> = (0..analytes.len())
        .map(|t| {
            edits
                .targets
                .get(&t)
                .and_then(|e| e.baseline)
                .unwrap_or(edits.baseline)
        })
        .collect();
    // Report rows: the precursor, then each fragment in m/z order.
    let mut offsets = Vec::with_capacity(analytes.len());
    let mut rows = 0;
    for a in &analytes {
        offsets.push(rows);
        rows += 1 + a.fragments.len();
    }
    let mut areas = vec![0f32; rows * samples.len()];
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir)?;
    let peaks_path = dir.join(PEAKS);
    let peaks_tmp = peaks_path.with_extension("tmp");
    let written = (|| -> Result<(), Failure> {
        let mut long = csv::Writer::from_path(&peaks_tmp)?;
        long.write_record([
            "id_number",
            "name",
            "MS_level",
            "precursor_m/z",
            "fragment_m/z",
            "sample",
            "detection",
            "window",
            "baseline",
            "rt_start",
            "rt_end",
            "rt_apex",
            "area",
            "height",
            "FWHM",
            "scans",
        ])?;
        for (s, sample) in samples.iter().enumerate() {
            cancel.check()?;
            reporter.emit(Event::Sample {
                name: sample.clone(),
                index: s,
                total: samples.len(),
            });
            let misc = root.join("misc");
            let ms1 = common::get_ms1(&misc, sample)?;
            let ms2 = common::get_ms2(&misc, sample)?;
            let swaths: Vec<_> = p
                .windows
                .iter()
                .map(|w| crate::score::get_swath(&ms2, *w))
                .collect();
            let stem = Path::new(sample)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(sample)
                .to_string();
            let results = analytes
                .par_iter()
                .enumerate()
                .map(|(t, a)| -> Result<(Resolved, Vec<Peak>), Failure> {
                    cancel.check()?;
                    let record = &a.records[s];
                    let spec = visualizer::spec(a, record, &p);
                    let (window, source, _) = edits.resolve(t, &a.name, sample, spec.engine);
                    let span = window.end - window.start;
                    let (lo, hi) = (window.start - span, window.end + span);
                    let mut peaks = Vec::with_capacity(1 + spec.fragments.len());
                    peaks.push(peak::integrate(
                        &ms1_trace(&ms1, spec.mz, spec.tolerance, lo, hi),
                        window,
                        baselines[t],
                    ));
                    for &f in &spec.fragments {
                        let trace = spec
                            .isolation
                            .map(|i| ms2_trace(&swaths[i], f, p.ms2tol, lo, hi))
                            .unwrap_or_default();
                        peaks.push(peak::integrate(&trace, window, baselines[t]));
                    }
                    Ok((Resolved { window, source }, peaks))
                })
                .collect::<Result<Vec<_>, _>>()?;
            for (t, ((resolved, peaks), a)) in results.iter().zip(&analytes).enumerate() {
                let present = a.records[s].present;
                for (k, peak) in peaks.iter().enumerate() {
                    areas[(offsets[t] + k) * samples.len() + s] = peak.area;
                    write_peak(
                        &mut long,
                        t,
                        a,
                        k,
                        &stem,
                        present,
                        resolved,
                        baselines[t],
                        peak,
                    )?;
                }
            }
            reporter.progress(s as u64 + 1, samples.len() as u64);
        }
        long.flush()?;
        long.into_inner().map_err(|e| e.to_string())?.sync_all()?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&peaks_tmp);
        return Err(e);
    }
    cancel.check()?;
    reporter.info("Writing Report_integrated.csv");
    let report_path = dir.join(REPORT);
    if let Err(e) = write_report(root, &report_path, &samples, &analytes, &offsets, &areas) {
        let _ = std::fs::remove_file(&peaks_tmp);
        return Err(e);
    }
    std::fs::rename(&peaks_tmp, &peaks_path)?;
    let exported = Exported {
        report: report_path,
        peaks: peaks_path,
        samples: samples.len(),
        targets: analytes.len(),
        rows,
        edited_windows: edits.windows(),
        custom_baselines: edits
            .targets
            .values()
            .filter(|t| t.baseline.is_some())
            .count(),
        baseline: edits.baseline,
        elapsed_seconds: started.elapsed().as_secs_f64(),
        finished_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    replace(&dir.join(SUMMARY), |f| {
        serde_json::to_writer_pretty(&mut *f, &exported)?;
        Ok(())
    })?;
    Ok(exported)
}
#[allow(clippy::too_many_arguments)]
fn write_peak(
    w: &mut csv::Writer<std::fs::File>,
    t: usize,
    a: &Analyte,
    k: usize,
    sample: &str,
    present: bool,
    resolved: &Resolved,
    baseline: Baseline,
    peak: &Peak,
) -> Result<(), Failure> {
    let opt = |v: Option<f32>, digits: usize| v.map_or(String::new(), |v| format!("{v:.digits$}"));
    w.write_record([
        (t + 1).to_string(),
        a.name.clone(),
        if k == 0 { "1" } else { "2" }.into(),
        format!("{:.4}", a.mass),
        if k == 0 {
            String::new()
        } else {
            format!("{:.4}", a.fragments[k - 1].0)
        },
        sample.into(),
        if present { "identified" } else { "gap-filled" }.into(),
        resolved.source.label().into(),
        baseline.label().into(),
        format!("{:.3}", resolved.window.start),
        format!("{:.3}", resolved.window.end),
        opt(peak.apex, 3),
        format!("{:.1}", peak.area),
        if peak.scans == 0 {
            String::new()
        } else {
            format!("{:.1}", peak.height)
        },
        opt(peak.fwhm, 3),
        peak.scans.to_string(),
    ])?;
    Ok(())
}
/// Copies Report_fill_all.csv (or Report.csv) with each AREA_ column replaced
/// by the integrated area. Rows are matched to targets by id_number, MS level
/// and fragment m/z; any mismatch aborts the export.
fn write_report(
    root: &Path,
    out: &Path,
    samples: &[String],
    analytes: &[Analyte],
    offsets: &[usize],
    areas: &[f32],
) -> Result<(), Failure> {
    let source = ["Report_fill_all.csv", "Report.csv"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .ok_or("Report.csv is missing from this result folder.")?;
    let mismatch = || -> Failure { "Report.csv does not match misc/analyte.bin.".into() };
    let mut rdr = csv::ReaderBuilder::new().from_path(&source)?;
    let headers = rdr.headers()?.clone();
    let column = |name: &str| headers.iter().position(|h| h == name);
    let (id_col, level_col, frag_col) = (
        column("id_number").ok_or_else(mismatch)?,
        column("MS_level").ok_or_else(mismatch)?,
        column("fragment_m/z").ok_or_else(mismatch)?,
    );
    let area_cols = samples
        .iter()
        .map(|s| {
            let stem = Path::new(s)
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or(s);
            column(&format!("AREA_{stem}"))
                .ok_or_else(|| format!("Report.csv has no AREA column for {s}.").into())
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    replace(out, |f| {
        let mut wtr = csv::Writer::from_writer(&mut *f);
        wtr.write_record(&headers)?;
        let (mut current, mut fragment) = (usize::MAX, 0);
        let mut seen = 0;
        let mut row = Vec::with_capacity(headers.len());
        for record in rdr.records() {
            let record = record?;
            let id: usize = record[id_col].parse().map_err(|_| mismatch())?;
            let t = id
                .checked_sub(1)
                .filter(|t| *t < analytes.len())
                .ok_or_else(mismatch)?;
            let k = match &record[level_col] {
                "1" => {
                    current = t;
                    fragment = 0;
                    0
                }
                "2" if current == t && fragment < analytes[t].fragments.len() => {
                    if record[frag_col] != format!("{:.4}", analytes[t].fragments[fragment].0) {
                        return Err(mismatch());
                    }
                    fragment += 1;
                    fragment
                }
                _ => return Err(mismatch()),
            };
            let trace = offsets[t] + k;
            row.clear();
            row.extend(record.iter().map(str::to_owned));
            for (s, &col) in area_cols.iter().enumerate() {
                row[col] = format!("{:.1}", areas[trace * samples.len() + s]);
            }
            wtr.write_record(&row)?;
            seen += 1;
        }
        if seen != areas.len() / samples.len().max(1) {
            return Err(mismatch());
        }
        wtr.flush()?;
        Ok(())
    })
}
