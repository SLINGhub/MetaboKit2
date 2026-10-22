use metabokit_core::progress::{Cancel, Event, Reporter};
struct Console;
impl Reporter for Console {
    fn emit(&self, event: Event) {
        eprintln!("{event:?}");
    }
}
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<_> = std::env::args().collect();
    let mut settings = metabokit_dia::Settings::load(std::path::Path::new(
        args.get(1).ok_or("Usage: run PARAM OUTPUT [THREADS]")?,
    ))?;
    settings.output_dir = args.get(2).ok_or("Missing output path")?.into();
    if let Some(n) = args.get(3) {
        settings.threads = n.parse()?;
    }
    let result = metabokit_dia::run(&settings, &Console, &Cancel::new())?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
