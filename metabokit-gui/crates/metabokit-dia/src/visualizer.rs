//! Read original DIA cache files on demand. No whole-run scan cache is retained.
use crate::integration::{self, Baseline, Source, Window};
use crate::peak::{self, Peak};
use crate::{Failure, common::*};
use serde::Serialize;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

#[derive(Serialize)]
pub struct Target {
    pub id: usize,
    pub name: String,
    pub mz: f32,
    /// Samples whose integration window was edited.
    pub edited: usize,
}
#[derive(Serialize)]
pub struct Session {
    pub samples: Vec<String>,
    pub targets: Vec<Target>,
    pub total: usize,
    pub caches: bool,
}
pub(crate) fn samples(root: &Path) -> Result<Vec<String>, Failure> {
    Ok(csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(false)
        .from_path(root.join("misc/file_order.txt"))?
        .records()
        .map(|r| Ok(r?.get(0).ok_or("Missing sample name")?.to_string()))
        .collect::<Result<_, Failure>>()?)
}
pub(crate) fn has_caches(root: &Path, samples: &[String]) -> bool {
    samples.iter().all(|s| {
        root.join("misc").join(format!("ms1_{s}.bin")).is_file()
            && root.join("misc").join(format!("ms2_{s}.bin")).is_file()
    })
}
fn skip_sample(r: &mut BufReader<File>, n: i64) -> Result<(), Failure> {
    let present = unpack_u8(r)?;
    if present > 1 {
        return Err("Invalid DIA analyte cache".into());
    }
    r.seek_relative(12 + i64::from(present) * (12 + 8 * n))?;
    Ok(())
}
pub fn open(root: &Path, search: &str, offset: usize) -> Result<Session, Failure> {
    let samples = samples(root)?;
    // Edit markers are a convenience; a damaged edits file is reported by the
    // integration panel rather than hiding the target list.
    let edits = integration::load(root).unwrap_or_default();
    let mut r = BufReader::new(File::open(root.join("misc/analyte.bin"))?);
    let mut targets = vec![];
    let mut total = 0;
    let query = search.to_lowercase();
    let mut id = 0;
    while !r.fill_buf()?.is_empty() {
        let name = unpack_string(&mut r)?;
        let mz = unpack_f32(&mut r)?;
        let n = i64::from(unpack_u16(&mut r)?);
        r.seek_relative(8 * n)?;
        for _ in &samples {
            skip_sample(&mut r, n)?;
        }
        if name.to_lowercase().contains(&query) {
            if total >= offset && targets.len() < 100 {
                let edited = edits
                    .targets
                    .get(&id)
                    .filter(|t| t.name == name)
                    .map_or(0, |t| t.windows.len());
                targets.push(Target {
                    id,
                    name,
                    mz,
                    edited,
                });
            }
            total += 1;
        }
        id += 1;
    }
    let caches = has_caches(root, &samples);
    Ok(Session {
        samples,
        targets,
        total,
        caches,
    })
}

/// One sample's entry in `misc/analyte.bin`. Gap-filled samples carry the
/// cross-sample median RT, imputation half-width and library mass.
pub(crate) struct Record {
    pub present: bool,
    pub rt: f32,
    pub half_width: f32,
    pub mz: f32,
    pub area: f32,
    pub shape: f32,
    pub smoothness: f32,
    /// Per fragment (correlation, area); empty for gap-filled samples.
    pub metrics: Vec<(f32, f32)>,
}
pub(crate) struct Analyte {
    pub name: String,
    pub mass: f32,
    /// Library fragments sorted by m/z, as in Report.csv.
    pub fragments: Vec<(f32, f32)>,
    pub records: Vec<Record>,
}
fn read_record(r: &mut BufReader<File>, n: usize, metrics: bool) -> Result<Record, Failure> {
    let present = match unpack_u8(r)? {
        0 => false,
        1 => true,
        _ => return Err("Invalid DIA analyte cache".into()),
    };
    let rt = unpack_f32(r)?;
    let half_width = unpack_f32(r)?;
    let mz = unpack_f32(r)?;
    let mut record = Record {
        present,
        rt,
        half_width,
        mz,
        area: 0.0,
        shape: 0.0,
        smoothness: 0.0,
        metrics: vec![],
    };
    if present {
        if metrics {
            record.area = unpack_f32(r)?;
            record.shape = unpack_f32(r)?;
            record.smoothness = unpack_f32(r)?;
            record.metrics = (0..n).map(|_| unpack_f32_2(r)).collect::<Result<_, _>>()?;
        } else {
            r.seek_relative(12 + 8 * n as i64)?;
        }
    }
    Ok(record)
}
/// Name, library mass and sorted library fragments.
type Header = (String, f32, Vec<(f32, f32)>);
fn read_header(r: &mut BufReader<File>) -> Result<Header, Failure> {
    let name = unpack_string(r)?;
    let mass = unpack_f32(r)?;
    let n = unpack_u16(r)? as usize;
    if n > 10 {
        return Err("Unexpected DIA fragment count".into());
    }
    let fragments = (0..n).map(|_| unpack_f32_2(r)).collect::<Result<_, _>>()?;
    Ok((name, mass, fragments))
}
/// Streams to one target and reads every sample's record for it.
pub(crate) fn analyte(root: &Path, target: usize, samples: usize) -> Result<Analyte, Failure> {
    let mut r = BufReader::new(File::open(root.join("misc/analyte.bin"))?);
    for _ in 0..target {
        if r.fill_buf()?.is_empty() {
            return Err("Target not found".into());
        }
        unpack_string(&mut r)?;
        unpack_f32(&mut r)?;
        let n = i64::from(unpack_u16(&mut r)?);
        r.seek_relative(8 * n)?;
        for _ in 0..samples {
            skip_sample(&mut r, n)?;
        }
    }
    if r.fill_buf()?.is_empty() {
        return Err("Target not found".into());
    }
    let (name, mass, fragments) = read_header(&mut r)?;
    let records = (0..samples)
        .map(|_| read_record(&mut r, fragments.len(), true))
        .collect::<Result<_, _>>()?;
    Ok(Analyte {
        name,
        mass,
        fragments,
        records,
    })
}
/// Every target without per-fragment metrics, for whole-run integration.
pub(crate) fn analytes(root: &Path, samples: usize) -> Result<Vec<Analyte>, Failure> {
    let mut r = BufReader::new(File::open(root.join("misc/analyte.bin"))?);
    let mut all = vec![];
    while !r.fill_buf()?.is_empty() {
        let (name, mass, fragments) = read_header(&mut r)?;
        let records = (0..samples)
            .map(|_| read_record(&mut r, fragments.len(), false))
            .collect::<Result<_, _>>()?;
        all.push(Analyte {
            name,
            mass,
            fragments,
            records,
        });
    }
    Ok(all)
}

/// Tolerances and isolation windows written by the run to `misc/param.bin`.
pub(crate) struct CacheParams {
    pub i_mz: f32,
    pub ms1tol: f32,
    pub ms2tol: f32,
    pub windows: Vec<(f32, f32)>,
}
pub(crate) fn cache_params(root: &Path) -> Result<CacheParams, Failure> {
    let mut param = BufReader::new(File::open(root.join("misc/param.bin"))?);
    let i_mz = unpack_f32(&mut param)?;
    let ms1tol = unpack_f32(&mut param)?;
    let ms2tol = unpack_f32(&mut param)?;
    let nw = unpack_u8(&mut param)?;
    let windows = (0..nw)
        .map(|_| unpack_f32_2(&mut param))
        .collect::<Result<_, _>>()?;
    Ok(CacheParams {
        i_mz,
        ms1tol,
        ms2tol,
        windows,
    })
}
/// The engine's isolation-window choice: the containing window with the
/// largest margin around the precursor mass.
pub(crate) fn isolation(windows: &[(f32, f32)], mass: f32) -> Option<usize> {
    let mut selected = None;
    let mut prev = 0.0;
    for (i, w) in windows.iter().enumerate() {
        if w.0 < mass && mass < w.1 {
            let cur = (mass - w.0).min(w.1 - mass);
            if cur > prev {
                selected = Some(i);
                prev = cur;
            }
        }
    }
    selected
}
fn rounded(value: f32, digits: usize) -> f32 {
    format!("{value:.digits$}").parse().unwrap_or(value)
}
/// Which ions to trace for one target in one sample, using the engine's own
/// m/z values and tolerances. Gap filling reads the precursor m/z, fragment
/// m/z and median RT back from Report.csv's formatted columns, so the same
/// rounding is applied here.
pub(crate) struct Spec {
    pub mz: f32,
    pub tolerance: f32,
    pub isolation: Option<usize>,
    pub fragments: Vec<f32>,
    pub engine: Window,
}
pub(crate) fn spec(analyte: &Analyte, record: &Record, p: &CacheParams) -> Spec {
    if record.present {
        Spec {
            mz: record.mz,
            tolerance: p.i_mz,
            isolation: isolation(&p.windows, analyte.mass),
            fragments: analyte.fragments.iter().map(|f| f.0).collect(),
            engine: Window {
                start: record.rt - record.half_width,
                end: record.rt + record.half_width,
            },
        }
    } else {
        let mass = rounded(analyte.mass, 4);
        let rt = rounded(record.rt, 2);
        Spec {
            mz: mass,
            tolerance: p.ms1tol,
            isolation: isolation(&p.windows, mass),
            fragments: analyte.fragments.iter().map(|f| rounded(f.0, 4)).collect(),
            engine: Window {
                start: rt - record.half_width,
                end: rt + record.half_width,
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub name: String,
    pub mz: f32,
    pub rt: f32,
    pub present: bool,
    pub area: f32,
    pub shape: f32,
    pub smoothness: f32,
    pub precursor: Vec<(f32, f32)>,
    pub fragments: Vec<Trace>,
    /// Displayed RT interval: the requested half-width, widened to the window.
    pub range: (f32, f32),
    pub window: Window,
    pub engine_window: Window,
    pub source: Source,
    pub baseline: Baseline,
    pub precursor_peak: Peak,
}
#[derive(Serialize)]
pub struct Trace {
    pub mz: f32,
    pub correlation: f32,
    pub area: f32,
    pub points: Vec<(f32, f32)>,
    pub peak: Peak,
}
/// One target/sample's traces, integrated over its saved window, or over a
/// proposed `window`/`baseline` when given. Nothing is written.
pub fn detail(
    root: &Path,
    target: usize,
    sample: usize,
    width: f32,
    window: Option<Window>,
    baseline: Option<Baseline>,
) -> Result<Detail, Failure> {
    if !width.is_finite() || !(0.05..=5.0).contains(&width) {
        return Err("Invalid chromatogram half-width".into());
    }
    let window = window.map(Window::validate).transpose()?;
    let samples = samples(root)?;
    let bn = samples.get(sample).ok_or("Sample not found")?;
    if Path::new(bn).file_name().and_then(|x| x.to_str()) != Some(bn) {
        return Err("Invalid sample name in cache".into());
    }
    let mut analyte = analyte(root, target, samples.len())?;
    let p = cache_params(root)?;
    let record = analyte.records.swap_remove(sample);
    let spec = spec(&analyte, &record, &p);
    let edits = integration::load(root)?;
    let (saved, source, effective) = edits.resolve(target, &analyte.name, bn, spec.engine);
    let window = window.unwrap_or(saved);
    let baseline = baseline.unwrap_or(effective);
    let view = (record.rt - width, record.rt + width);
    let range = (view.0.min(window.start), view.1.max(window.end));
    // The percentile baseline looks one window width either side.
    let span = window.end - window.start;
    let reach = (
        range.0.min(window.start - span),
        range.1.max(window.end + span),
    );
    let isolation = spec.isolation.map_or((0.0, 0.0), |i| p.windows[i]);
    let precursor = extract(
        &root.join("misc").join(format!("ms1_{bn}.bin")),
        false,
        reach,
        isolation,
        &[spec.mz],
        spec.tolerance,
    )?
    .remove(0);
    let traces = extract(
        &root.join("misc").join(format!("ms2_{bn}.bin")),
        true,
        reach,
        isolation,
        &spec.fragments,
        p.ms2tol,
    )?;
    let shown = |points: &[(f32, f32)]| {
        points
            .iter()
            .copied()
            .filter(|x| range.0 <= x.0 && x.0 < range.1)
            .collect::<Vec<_>>()
    };
    let metrics = if record.present {
        record.metrics
    } else {
        vec![(0.0, 0.0); analyte.fragments.len()]
    };
    Ok(Detail {
        name: analyte.name,
        mz: record.mz,
        rt: record.rt,
        present: record.present,
        area: record.area,
        shape: record.shape,
        smoothness: record.smoothness,
        precursor_peak: peak::integrate(&precursor, window, baseline),
        precursor: shown(&precursor),
        fragments: spec
            .fragments
            .into_iter()
            .zip(metrics)
            .zip(traces)
            .map(|((mz, (correlation, area)), points)| Trace {
                mz,
                correlation,
                area,
                peak: peak::integrate(&points, window, baseline),
                points: shown(&points),
            })
            .collect(),
        range,
        window,
        engine_window: spec.engine,
        source,
        baseline,
    })
}
fn extract(
    path: &Path,
    ms2: bool,
    rt: (f32, f32),
    window: (f32, f32),
    mzs: &[f32],
    tolerance: f32,
) -> Result<Vec<Vec<(f32, f32)>>, Failure> {
    let mut reader = BufReader::new(File::open(path)?);
    if !ms2 {
        reader.skip_until(0)?;
    }
    let mut out = vec![vec![]; mzs.len()];
    while !reader.fill_buf()?.is_empty() {
        let pre = if ms2 { unpack_f32(&mut reader)? } else { 0.0 };
        let time = unpack_f32(&mut reader)?;
        if ms2 {
            unpack_f32(&mut reader)?;
        } // collision energy is part of the original cache
        let n = unpack_u32(&mut reader)?;
        if time >= rt.1 {
            break;
        }
        if time < rt.0 || (ms2 && !(window.0 < pre && pre < window.1)) {
            reader.seek_relative(i64::from(n) * 8)?;
            continue;
        }
        let mut maxima = vec![0f32; mzs.len()];
        for _ in 0..n {
            let (mz, intensity) = unpack_f32_2(&mut reader)?;
            for (wanted, max) in mzs.iter().zip(&mut maxima) {
                if mz >= wanted - tolerance && mz < wanted + tolerance {
                    *max = max.max(intensity);
                }
            }
        }
        for (trace, intensity) in out.iter_mut().zip(maxima) {
            trace.push((time, intensity));
        }
    }
    Ok(out)
}
/// The acquisition start time written at the head of each MS1 cache.
pub(crate) fn acquired(root: &Path, sample: &str) -> Option<String> {
    let mut reader =
        BufReader::new(File::open(root.join("misc").join(format!("ms1_{sample}.bin"))).ok()?)
            .take(64);
    let mut stamp = vec![];
    reader.read_until(0, &mut stamp).ok()?;
    (stamp.pop() == Some(0))
        .then(|| String::from_utf8(stamp).ok())
        .flatten()
        .filter(|s| !s.is_empty())
}
