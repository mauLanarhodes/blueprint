//! `blueprint-cli`: headless export for CI pipelines and docs builds.
//!
//! ```text
//! blueprint-cli export plan.blueprint plan.svg
//! blueprint-cli info plan.blueprint
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Usage:
  blueprint-cli export <input.blueprint> [output.svg]   Export the first page as SVG
  blueprint-cli info <input.blueprint>                  Show pages, layers and element counts
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["export", input] => export(input.into(), None),
        ["export", input, output] => export(input.into(), Some(output.into())),
        ["info", input] => info(input.into()),
        ["--version" | "-V"] => {
            println!("blueprint-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn export(input: PathBuf, output: Option<PathBuf>) -> Result<(), String> {
    let doc = bp_io::load(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let page = doc.first_page().ok_or("the document has no pages")?;
    let svg = bp_export::page_to_svg(&doc, page, &Default::default());
    let output = output.unwrap_or_else(|| input.with_extension("svg"));
    std::fs::write(&output, svg).map_err(|e| format!("{}: {e}", output.display()))?;
    println!("Wrote {}", output.display());
    Ok(())
}

fn info(input: PathBuf) -> Result<(), String> {
    let doc = bp_io::load(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    println!("{} (format {})", input.display(), doc.schema_version);
    for page in doc.pages_sorted() {
        println!(
            "  {}: {} shapes",
            page.name,
            doc.elements_on_page(page.id).len()
        );
        for layer in doc.layers_of(page.id) {
            let count = doc
                .elements
                .values()
                .filter(|e| e.layer == layer.id)
                .count();
            let flags = match (layer.visible, layer.locked) {
                (true, false) => "",
                (false, false) => " (hidden)",
                (true, true) => " (locked)",
                (false, true) => " (hidden, locked)",
            };
            println!("    {}{flags}: {count}", layer.name);
        }
    }
    Ok(())
}