//! Native DIA engine. Numerical kernels and output schemas follow SLING's
//! MetaboKit DIA 0.1.0. All paths are explicit; the process cwd is never changed.
mod common;
mod cwt;
mod fillall;
pub mod integration;
pub mod params;
mod parse;
pub mod peak;
mod print_t;
mod readlib;
mod score;
mod table;
#[cfg(test)]
mod tests;
pub mod visualizer;

use metabokit_core::progress::{Cancel, Event, Reporter, Stage};
pub use params::Settings;
use std::path::PathBuf;
use std::time::Instant;
pub type Failure = Box<dyn std::error::Error + Send + Sync>;

struct Param {
    mzml_fs: Vec<PathBuf>,
    ispos: bool,
    lib: Vec<String>,
    library_labels: std::collections::BTreeMap<String, String>,
    ms1tol: f32,
    ms2tol: f32,
    ms2_score: f32,
    min_peaks: u8,
    rt_shift: f32,
    peak_w: (f32, f32),
    win_s: Vec<(f32, f32)>,
    peak_shape: f32,
    sn_score: f32,
    rt_search: f32,
    ms1ms2: f32,
    impute_width: f32,
    i_rt: f32,
    i_mz: f32,
    output_dir: PathBuf,
    libs_dir: Option<PathBuf>,
    cancel: Cancel,
}
impl Param {
    fn misc(&self) -> PathBuf {
        self.output_dir.join("misc")
    }
    fn check(&self) -> std::io::Result<()> {
        if self.cancel.is_cancelled() {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Analysis cancelled",
            ))
        } else {
            Ok(())
        }
    }
}
struct MzAucC {
    mz: f32,
    auc: f32,
    score: f32,
    s_n: f32,
}
struct Cpd {
    i: usize,
    ent_i: usize,
    rt: f32,
    sc: f32,
    shape: f32,
    smooth: f32,
    pre: MzAucC,
    mz_auc_c: Vec<MzAucC>,
}
struct Ms {
    rt: f32,
    mz_i: Vec<(f32, f32)>,
}
struct Msms {
    ms1mz: f32,
    rt: f32,
    mz_i_l: Vec<(f32, f32)>,
    #[allow(dead_code)]
    ce: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub output_dir: PathBuf,
    pub samples: usize,
    pub library_entries: usize,
    pub annotations: usize,
    pub elapsed_seconds: f64,
    pub reports: Vec<String>,
}

pub fn run(
    settings: &Settings,
    reporter: &dyn Reporter,
    cancel: &Cancel,
) -> Result<Outcome, Failure> {
    use std::io::Write;
    let started = Instant::now();
    settings.validate()?;
    cancel.check()?;
    // Only create a fresh destination: incomplete or completed runs are never
    // silently overwritten. Scan caches and reports remain available on error.
    if settings.output_dir.exists() && std::fs::read_dir(&settings.output_dir)?.next().is_some() {
        return Err("Output folder is not empty. Choose a new output folder for this run.".into());
    }
    let p = settings.to_param(cancel.clone())?;
    reporter.stage(Stage::Library);
    let lib = readlib::get_cpds(&p)?;
    if !lib.iter().any(|e| e.rt.is_some() && !e.mz_i.is_empty()) {
        return Err("DIA requires a library with RETENTIONTIME and fragment peaks. No usable entries were found.".into());
    }
    std::fs::create_dir_all(p.misc())?;
    serde_json::to_writer_pretty(
        std::fs::File::create(p.output_dir.join("dia-settings.json"))?,
        settings,
    )?;
    let mut buf = std::io::BufWriter::new(std::fs::File::create(p.misc().join("param.bin"))?);
    for v in [p.i_mz, p.ms1tol, p.ms2tol] {
        buf.write_all(&v.to_le_bytes())?;
    }
    buf.write_all(&[p.win_s.len() as u8])?;
    for (lo, hi) in &p.win_s {
        buf.write_all(&lo.to_le_bytes())?;
        buf.write_all(&hi.to_le_bytes())?;
    }
    buf.flush()?;
    let threads = settings.effective_threads();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()?;
    let files_in_flight = settings.max_files_in_flight.max(1).min(threads);
    reporter.metric("threads", threads.to_string());
    reporter.metric("resident samples", files_in_flight.to_string());
    reporter.stage(Stage::Processing);
    let mut all = Vec::with_capacity(p.mzml_fs.len());
    use rayon::prelude::*;
    for batch in p.mzml_fs.chunks(files_in_flight) {
        p.check()?;
        let base = all.len();
        let results = pool.install(|| {
            batch
                .par_iter()
                .enumerate()
                .map(|(i, path)| -> Result<Vec<Cpd>, Failure> {
                    p.check()?;
                    let bn = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .ok_or("Invalid sample filename")?;
                    reporter.emit(Event::Sample {
                        name: bn.into(),
                        index: base + i,
                        total: p.mzml_fs.len(),
                    });
                    parse::mzml(path, &p)?;
                    let ms1 = common::get_ms1(&p.misc(), bn)?;
                    if ms1.len() < 3 {
                        return Err(format!("{bn}: fewer than three MS1 scans").into());
                    }
                    cwt::cwt(bn, &ms1, &lib, &p)?;
                    p.check()?;
                    let ms2 = common::get_ms2(&p.misc(), bn)?;
                    if ms2.is_empty() {
                        return Err(format!("{bn}: no MS2 scans").into());
                    }
                    let result = score::score(&p, bn, &lib, &ms1, &ms2)?;
                    reporter.info(format!("{bn}: {} library matches", result.len()));
                    Ok(result)
                })
                .collect::<Vec<_>>()
        });
        for result in results {
            all.push(result?);
            reporter.progress(all.len() as u64, p.mzml_fs.len() as u64);
        }
    }
    let annotations = all.iter().map(Vec::len).sum();
    p.check()?;
    reporter.stage(Stage::Reporting);
    let widths = table::print_tab(&p, &lib, all)?;
    let library_entries = lib.len();
    drop(lib);
    p.check()?;
    reporter.stage(Stage::GapFilling);
    // Filling has its own bounded pool so it cannot restore every sample at once.
    let fill_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(files_in_flight)
        .build()?;
    fill_pool.install(|| fillall::fill(&p, &widths))?;
    p.check()?;
    reporter.info("Generating target list");
    print_t::print_tar(&p)?;
    p.check()?;
    if !settings.keep_cache {
        for path in &p.mzml_fs {
            let bn = path.file_name().unwrap().to_string_lossy();
            for prefix in ["ms1", "ms2", "ms1f"] {
                std::fs::remove_file(p.misc().join(format!("{prefix}_{bn}.bin")))?;
            }
        }
    }
    let outcome = Outcome {
        output_dir: p.output_dir.clone(),
        samples: p.mzml_fs.len(),
        library_entries,
        annotations,
        elapsed_seconds: started.elapsed().as_secs_f64(),
        reports: vec![
            "Report.csv".into(),
            "Report_fill_all.csv".into(),
            "target_list.csv".into(),
        ],
    };
    serde_json::to_writer_pretty(
        std::fs::File::create(p.output_dir.join("dia-run.json"))?,
        &outcome,
    )?;
    Ok(outcome)
}
