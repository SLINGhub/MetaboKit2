use crate::{Failure, Param};
use metabokit_core::progress::Cancel;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub mzml_files: Vec<PathBuf>,
    pub output_dir: PathBuf,
    pub libs_dir: Option<PathBuf>,
    pub libraries: Vec<String>,
    /// Original source labels, independent of resolved file locations.
    pub library_labels: std::collections::BTreeMap<String, String>,
    pub positive: bool,
    pub ms1_tol: f32,
    pub ms2_tol: f32,
    pub ms2_score: f32,
    pub min_peaks: u8,
    pub rt_shift: f32,
    pub peak_width: (f32, f32),
    pub windows: Vec<(f32, f32)>,
    pub peak_shape: f32,
    pub sn_score: f32,
    pub integration_rt: f32,
    pub integration_mz: f32,
    pub impute_width: f32,
    pub threads: usize,
    pub max_files_in_flight: usize,
    pub keep_cache: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            mzml_files: vec![],
            output_dir: PathBuf::new(),
            libs_dir: None,
            libraries: vec![],
            library_labels: Default::default(),
            positive: true,
            ms1_tol: 0.008,
            ms2_tol: 0.05,
            ms2_score: 0.5,
            min_peaks: 1,
            rt_shift: 0.1,
            peak_width: (0.05, 0.8),
            windows: vec![],
            peak_shape: 0.1,
            sn_score: 0.4,
            integration_rt: 2.0,
            integration_mz: 0.01,
            impute_width: 0.0,
            threads: 0,
            max_files_in_flight: 1,
            keep_cache: true,
        }
    }
}
impl Settings {
    pub fn effective_threads(&self) -> usize {
        let available = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        if self.threads == 0 {
            available.saturating_sub(1).max(1)
        } else {
            self.threads.min(available).max(1)
        }
    }
    pub fn validate(&self) -> Result<(), Failure> {
        if self.mzml_files.is_empty() {
            return Err("Choose a dataset containing centroided DIA mzML files.".into());
        }
        let mut names = HashSet::new();
        for path in &self.mzml_files {
            if !path.is_file() {
                return Err(format!("Sample does not exist: {}", path.display()).into());
            }
            if !path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("mzml"))
            {
                return Err("DIA supports mzML input only.".into());
            }
            if !names.insert(path.file_name()) {
                return Err("Sample basenames must be unique, including across subfolders.".into());
            }
        }
        if self.output_dir.as_os_str().is_empty() {
            return Err("Choose an output folder.".into());
        }
        if self.libraries.is_empty() {
            return Err(
                "Choose a DIA library containing retention times and MS2 fragments.".into(),
            );
        }
        for lib in &self.libraries {
            if let Some(file) = lib
                .strip_prefix("user ")
                .or_else(|| lib.strip_prefix("csv "))
            {
                if !Path::new(file).is_file() {
                    return Err(format!("Library not found: {file}").into());
                }
            } else {
                let builtins = [
                    "nist",
                    "Atlas",
                    "Atlas_filtered",
                    "MSDIAL",
                    "hmdb",
                    "sling",
                    "MassBank",
                    "FiehnHILIC",
                ];
                let name = builtins
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(lib))
                    .ok_or_else(|| format!("Unknown DIA library: {lib}"))?;
                let stem = if *name == "Atlas_filtered" {
                    "Atlas"
                } else {
                    name
                };
                let dir = self
                    .libs_dir
                    .as_ref()
                    .ok_or("Choose a libs folder for built-in libraries")?;
                let path = dir.join(format!(
                    "{stem}_{}",
                    if self.positive { "pos" } else { "neg" }
                ));
                if !path.is_file() {
                    return Err(format!("Library not found: {}", path.display()).into());
                }
            }
        }
        if self.windows.is_empty()
            || self.windows.len() > 255
            || self
                .windows
                .iter()
                .any(|(l, r)| !l.is_finite() || !r.is_finite() || *l < 0.0 || l >= r)
        {
            return Err("Enter 1–255 valid DIA isolation windows (lower m/z, upper m/z), or import param.txt.".into());
        }
        for (name, value) in [
            ("MS1 tolerance", self.ms1_tol),
            ("MS2 tolerance", self.ms2_tol),
            ("RT shift", self.rt_shift),
            ("Integration RT", self.integration_rt),
            ("Integration m/z", self.integration_mz),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be finite and greater than zero.").into());
            }
        }
        for (name, value) in [
            ("MS2 score", self.ms2_score),
            ("Peak shape", self.peak_shape),
            ("Smoothness", self.sn_score),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("{name} must be between 0 and 1.").into());
            }
        }
        if !self.peak_width.0.is_finite()
            || !self.peak_width.1.is_finite()
            || self.peak_width.0 <= 0.0
            || self.peak_width.0 >= self.peak_width.1
        {
            return Err("Peak width must have positive, increasing bounds.".into());
        }
        if !self.impute_width.is_finite() || self.impute_width < 0.0 {
            return Err("Imputation width must be zero (automatic) or positive.".into());
        }
        if self.max_files_in_flight == 0 {
            return Err("Resident samples must be at least one.".into());
        }
        Ok(())
    }
    pub(crate) fn to_param(&self, cancel: Cancel) -> Result<Param, Failure> {
        Ok(Param {
            mzml_fs: self.mzml_files.clone(),
            ispos: self.positive,
            lib: self.libraries.clone(),
            library_labels: self.library_labels.clone(),
            ms1tol: self.ms1_tol,
            ms2tol: self.ms2_tol,
            ms2_score: self.ms2_score,
            min_peaks: self.min_peaks,
            rt_shift: self.rt_shift,
            peak_w: self.peak_width,
            win_s: self.windows.clone(),
            peak_shape: self.peak_shape,
            sn_score: self.sn_score,
            rt_search: self.rt_shift * 1.5,
            ms1ms2: 0.01,
            impute_width: self.impute_width,
            i_rt: self.integration_rt,
            i_mz: self.integration_mz,
            output_dir: self.output_dir.clone(),
            libs_dir: self.libs_dir.clone(),
            cancel,
        })
    }
    /// Import the original parameter format with paths resolved from its own
    /// directory, never the app bundle or process working directory.
    pub fn load(path: &Path) -> Result<Self, Failure> {
        let text = std::fs::read_to_string(path)?;
        if path.extension().is_some_and(|e| e == "json") {
            return Ok(serde_json::from_str(&text)?);
        }
        let t: toml::Table = text.parse()?;
        let root = path.parent().unwrap_or(Path::new("."));
        let resolve = |s: &str| {
            let p = Path::new(s);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                root.join(p)
            }
        };
        let num = |k: &str| -> Result<f32, Failure> {
            let v = t
                .get(k)
                .ok_or_else(|| format!("param.txt is missing {k}"))?;
            v.as_float()
                .or_else(|| v.as_integer().map(|n| n as f64))
                .map(|n| n as f32)
                .ok_or_else(|| format!("Invalid numeric parameter: {k}").into())
        };
        let array = |k: &str| -> Result<Vec<f32>, Failure> {
            t.get(k)
                .and_then(|v| v.as_array())
                .ok_or_else(|| format!("Invalid array: {k}"))?
                .iter()
                .map(|v| {
                    v.as_float()
                        .or_else(|| v.as_integer().map(|n| n as f64))
                        .map(|n| n as f32)
                        .ok_or_else(|| format!("Invalid number in {k}").into())
                })
                .collect()
        };
        let widths = array("length_of_ion_chromatogram")?;
        let windows = array("window_setting")?;
        if widths.len() != 2 || windows.len() % 2 != 0 {
            return Err("Peak widths and isolation windows must contain complete pairs.".into());
        }
        let mut settings = Self {
            ms1_tol: num("ms1tol")?,
            ms2_tol: num("ms2tol")?,
            ms2_score: num("MS2_score")?,
            min_peaks: u8::try_from(
                t.get("min_peaks")
                    .and_then(|v| v.as_integer())
                    .ok_or("Invalid min_peaks")?,
            )?,
            rt_shift: num("RT_shift")?,
            peak_width: (widths[0], widths[1]),
            windows: windows.chunks_exact(2).map(|x| (x[0], x[1])).collect(),
            peak_shape: num("peak_shape")?,
            sn_score: if t.contains_key("sn_score") {
                num("sn_score")?
            } else {
                0.0
            },
            integration_rt: num("integration_RT")?,
            integration_mz: num("integration_mz")?,
            impute_width: num("impute_width")?,
            threads: usize::try_from(
                t.get("num_threads")
                    .and_then(|v| v.as_integer())
                    .unwrap_or(0),
            )?,
            output_dir: root.join("results_dia"),
            libs_dir: root.join("libs").is_dir().then(|| root.join("libs")),
            ..Self::default()
        };
        settings.libraries = t
            .get("library")
            .and_then(|v| v.as_array())
            .ok_or("Missing library array")?
            .iter()
            .map(|v| -> Result<String, Failure> {
                let s = v.as_str().ok_or("Invalid library value")?;
                for prefix in ["user ", "csv "] {
                    if let Some(p) = s.strip_prefix(prefix) {
                        let resolved = format!("{prefix}{}", resolve(p.trim()).display());
                        settings
                            .library_labels
                            .insert(resolved.clone(), p.trim().into());
                        return Ok(resolved);
                    }
                }
                Ok(s.into())
            })
            .collect::<Result<_, _>>()?;
        if root.join("file_order.txt").is_file() {
            let mut rdr = csv::ReaderBuilder::new()
                .delimiter(b'\t')
                .comment(Some(b'#'))
                .has_headers(false)
                .trim(csv::Trim::All)
                .from_path(root.join("file_order.txt"))?;
            let mut recs = rdr.records();
            let first = recs.next().ok_or("Empty file_order.txt")??;
            let dir = resolve(first.get(0).ok_or("Empty file_order directory")?);
            settings.mzml_files = recs
                .map(|r| Ok(dir.join(r?.get(0).ok_or("Empty file_order entry")?)))
                .collect::<Result<_, Failure>>()?;
        } else {
            let pattern = t
                .get("mzML_files")
                .and_then(|v| v.as_str())
                .ok_or("Missing mzML_files pattern")?;
            settings.mzml_files =
                glob::glob(&resolve(pattern).to_string_lossy())?.collect::<Result<_, _>>()?;
        }
        if let Some(p) = settings.mzml_files.first() {
            settings.positive = metabokit_core::mzml::sniff_polarity(p)?
                .ok_or("Could not determine scan polarity")?;
        }
        Ok(settings)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dataset {
    pub settings: Settings,
    pub notes: Vec<String>,
    pub total_bytes: u64,
}
pub fn discover(root: &Path) -> Result<Dataset, Failure> {
    let root = std::fs::canonicalize(root)?;
    let mut notes = vec![];
    let mut settings = if root.join("param.txt").is_file() {
        notes.push("Imported the original DIA param.txt, preserving sample order, library selection, tolerances, and isolation windows.".into());
        Settings::load(&root.join("param.txt"))?
    } else {
        let scan = metabokit_core::discover::scan(&root)?;
        notes.push("No DIA param.txt found. Review the default settings and enter the acquisition's isolation windows.".into());
        Settings {
            mzml_files: scan
                .params
                .mzml_files
                .into_iter()
                .filter(|p| {
                    p.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("mzml"))
                })
                .collect(),
            output_dir: root.join("results_dia"),
            libs_dir: scan.params.libs_dir,
            libraries: scan
                .params
                .libraries
                .into_iter()
                .filter_map(|l| match l {
                    metabokit_core::params::LibrarySource::Msp(p) => {
                        Some(format!("user {}", p.display()))
                    }
                    _ => None,
                })
                .collect(),
            ..Default::default()
        }
    };
    if settings.output_dir.exists() {
        let base = settings.output_dir.clone();
        let mut i = 2;
        while settings.output_dir.exists() {
            settings.output_dir = base.with_file_name(format!("results_dia_{i}"));
            i += 1;
        }
    }
    let total_bytes = settings
        .mzml_files
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    notes.push("One resident sample by default; worker threads are used within feature detection. DDA and DIA share a single run slot.".into());
    Ok(Dataset {
        settings,
        notes,
        total_bytes,
    })
}
