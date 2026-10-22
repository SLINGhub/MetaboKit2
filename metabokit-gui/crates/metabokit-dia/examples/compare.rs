//! Byte-for-byte report/cache comparison with a run of the original engine.
//! Usage: cargo run --release -p metabokit-dia --example compare -- ORIGINAL PORTED
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};
fn same(a: &Path, b: &Path) -> std::io::Result<bool> {
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    let mut a = BufReader::new(File::open(a)?);
    let mut b = BufReader::new(File::open(b)?);
    let mut x = [0; 65536];
    let mut y = [0; 65536];
    loop {
        let n = a.read(&mut x)?;
        if n == 0 {
            return Ok(true);
        }
        b.read_exact(&mut y[..n])?;
        if x[..n] != y[..n] {
            return Ok(false);
        }
    }
}
fn main() -> Result<(), metabokit_dia::Failure> {
    let args: Vec<_> = std::env::args().collect();
    let a = Path::new(args.get(1).ok_or("usage: compare ORIGINAL PORTED")?);
    let b = Path::new(args.get(2).ok_or("usage: compare ORIGINAL PORTED")?);
    let mut files = vec![
        "Report.csv".into(),
        "Report_fill_all.csv".into(),
        "target_list.csv".into(),
    ];
    for entry in std::fs::read_dir(a.join("misc"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            files.push(Path::new("misc").join(entry.file_name()));
        }
    }
    files.sort();
    for file in &files {
        if !same(&a.join(file), &b.join(file))? {
            return Err(format!("Mismatch: {}", file.display()).into());
        }
        println!("MATCH {}", file.display());
    }
    println!(
        "{} files identical, including all numerical outputs and scan/feature caches.",
        files.len()
    );
    Ok(())
}
