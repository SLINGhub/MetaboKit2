//! Exercise real-data parity under resource settings and cancellation.
//! Usage: validate PARAM ORIGINAL_OUTPUT NEW_VALIDATION_FOLDER
//! The supplied validation folder must not exist. Only its generated cases
//! are cleaned up; the `verified` run remains for visual inspection.
use metabokit_core::progress::{Cancel, Event, Reporter, Silent};
use metabokit_dia::{Failure, Settings};
use std::{fs, io::Read, path::Path};
fn equal(a: &Path, b: &Path) -> Result<(), Failure> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Err(format!("Size mismatch: {}", b.display()).into());
    }
    let mut a = fs::File::open(a)?;
    let mut bfile = fs::File::open(b)?;
    let mut x = [0; 65536];
    let mut y = [0; 65536];
    loop {
        let n = a.read(&mut x)?;
        if n == 0 {
            return Ok(());
        }
        bfile.read_exact(&mut y[..n])?;
        if x[..n] != y[..n] {
            return Err(format!("Byte mismatch: {}", b.display()).into());
        }
    }
}
struct StopAtSample(Cancel);
impl Reporter for StopAtSample {
    fn emit(&self, event: Event) {
        if matches!(event, Event::Sample { .. }) {
            self.0.cancel();
        }
    }
}
fn main() -> Result<(), Failure> {
    let args: Vec<_> = std::env::args().collect();
    let original = Path::new(
        args.get(2)
            .ok_or("usage: validate PARAM ORIGINAL_OUTPUT NEW_VALIDATION_FOLDER")?,
    );
    let root = Path::new(args.get(3).ok_or("Missing new validation folder")?);
    fs::create_dir(root)?; // Refuse an existing destination, even if empty.
    let settings = Settings::load(Path::new(&args[1]))?;
    for (name, threads, resident, keep) in [
        ("verified", 5, 1, true),
        ("single-worker", 1, 1, true),
        ("two-resident", 5, 2, true),
        ("no-cache", 5, 1, false),
    ] {
        let mut s = settings.clone();
        s.output_dir = root.join(name);
        s.threads = threads;
        s.max_files_in_flight = resident;
        s.keep_cache = keep;
        let out = metabokit_dia::run(&s, &Silent, &Cancel::new())?;
        for report in &out.reports {
            equal(&original.join(report), &s.output_dir.join(report))?;
        }
        for entry in fs::read_dir(original.join("misc"))? {
            let entry = entry?;
            let file = entry.file_name();
            let file_text = file.to_string_lossy();
            let cache = file_text.starts_with("ms1_")
                || file_text.starts_with("ms2_")
                || file_text.starts_with("ms1f_");
            if !keep && cache {
                assert!(!s.output_dir.join("misc").join(&file).exists());
            } else {
                equal(&entry.path(), &s.output_dir.join("misc").join(&file))?;
            }
        }
        println!(
            "PASS {name}: {} matches, {:.3} s; identical reports and retained caches",
            out.annotations, out.elapsed_seconds
        );
        if name != "verified" {
            fs::remove_dir_all(&s.output_dir)?;
        }
    }
    let mut s = settings;
    s.output_dir = root.join("cancelled");
    let cancel = Cancel::new();
    assert!(metabokit_dia::run(&s, &StopAtSample(cancel.clone()), &cancel).is_err());
    assert!(s.output_dir.join("dia-settings.json").is_file());
    assert!(!s.output_dir.join("dia-run.json").exists());
    fs::remove_dir_all(&s.output_dir)?;
    println!("PASS cancellation: stopped during processing; no successful-run marker");
    Ok(())
}
