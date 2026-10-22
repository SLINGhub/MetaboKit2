//! Export integrated areas for a result folder, then compare them with the
//! engine's reports. With no edits and the `none` baseline every identified
//! area must equal Report.csv and every gap-filled area Report_fill_all.csv.
//! Writes only to OUTPUT/integration/.
use metabokit_core::progress::{Cancel, Silent};
use std::path::Path;

fn table(
    path: &Path,
) -> Result<(csv::StringRecord, Vec<csv::StringRecord>), metabokit_dia::Failure> {
    let mut rdr = csv::Reader::from_path(path)?;
    let headers = rdr.headers()?.clone();
    Ok((headers, rdr.records().collect::<Result<_, _>>()?))
}
fn main() -> Result<(), metabokit_dia::Failure> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(args.get(1).ok_or("usage: integrate OUTPUT")?);
    let started = std::time::Instant::now();
    let out = metabokit_dia::integration::export(root, &Silent, &Cancel::new())?;
    println!(
        "exported {} rows × {} samples in {:.2?}",
        out.rows,
        out.samples,
        started.elapsed()
    );
    let (headers, report) = table(&root.join("Report.csv"))?;
    let (_, filled) = table(&root.join("Report_fill_all.csv"))?;
    let (_, integrated) = table(&out.report)?;
    let areas: Vec<_> = (0..headers.len())
        .filter(|&i| headers[i].starts_with("AREA_"))
        .collect();
    let (mut identified, mut gap_filled, mut differ) = (0, 0, vec![]);
    for (row, ((r, f), n)) in report.iter().zip(&filled).zip(&integrated).enumerate() {
        for &c in &areas {
            let expected = if r[c].is_empty() {
                gap_filled += 1;
                &f[c]
            } else {
                identified += 1;
                &r[c]
            };
            if expected != &n[c] {
                differ.push(format!(
                    "row {} {}: engine {expected}, integrated {}",
                    row + 2,
                    &headers[c],
                    &n[c]
                ));
            }
            if n.iter()
                .enumerate()
                .any(|(i, v)| !areas.contains(&i) && v != &f[i])
            {
                differ.push(format!("row {}: non-area column changed", row + 2));
            }
        }
    }
    println!(
        "identified cells {identified}, gap-filled cells {gap_filled}, differing {}",
        differ.len()
    );
    for d in differ.iter().take(20) {
        println!("  {d}");
    }
    // The interactive path reads the caches separately; spot-check it against
    // the engine's unrounded areas in misc/analyte.bin.
    let session = metabokit_dia::visualizer::open(root, "", 0)?;
    let (mut checked, mut traces) = (0, 0);
    for target in (0..session.total).step_by(23) {
        for sample in 0..session.samples.len() {
            let d = metabokit_dia::visualizer::detail(root, target, sample, 0.3, None, None)?;
            if !d.present {
                continue;
            }
            checked += 1;
            if d.precursor_peak.area.to_bits() != d.area.to_bits() {
                differ.push(format!(
                    "target {target} sample {sample}: precursor {} vs {}",
                    d.precursor_peak.area, d.area
                ));
            }
            for f in &d.fragments {
                traces += 1;
                if f.peak.area.to_bits() != f.area.to_bits() {
                    differ.push(format!(
                        "target {target} sample {sample}: fragment {} {} vs {}",
                        f.mz, f.peak.area, f.area
                    ));
                }
            }
        }
    }
    println!(
        "viewer spot check: {checked} identified samples, {traces} fragment traces, differing {}",
        differ.len()
    );
    for d in differ.iter().take(20) {
        println!("  {d}");
    }
    if !differ.is_empty() {
        return Err("integrated areas differ from the engine's".into());
    }
    Ok(())
}
