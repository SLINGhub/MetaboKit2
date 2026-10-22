//! Dump the first page and one trace through the same API used by the GUI.
use std::path::Path;
fn main() -> Result<(), metabokit_dia::Failure> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(
        args.get(1)
            .ok_or("usage: inspect OUTPUT [TARGET] [SAMPLE]")?,
    );
    let session = metabokit_dia::visualizer::open(root, "", 0)?;
    let target = args.get(2).map(|v| v.parse()).transpose()?.unwrap_or(0);
    let sample = args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(0);
    let detail = metabokit_dia::visualizer::detail(root, target, sample, 0.3, None, None)?;
    let overview = metabokit_dia::integration::overview(root)?;
    let windows = metabokit_dia::integration::target(root, target)?;
    println!(
        "{}",
        serde_json::json!({"session":session,"detail":detail,"overview":overview,"windows":windows})
    );
    Ok(())
}
