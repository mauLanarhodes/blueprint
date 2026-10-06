//! `blueprint-cli`: headless export for CI pipelines and docs builds.
//!
//! ```text
//! blueprint-cli export plan.blueprint plan.svg
//! blueprint-cli export plan.blueprint --page 2 --embed-fonts
//! blueprint-cli info plan.blueprint
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

mod sql;

const USAGE: &str = "\
Usage:
  blueprint-cli export <input.blueprint> [output.svg] [--page <n|name>] [--embed-fonts]
      Export one page as SVG (the first page unless --page is given).
      --embed-fonts makes the SVG look the same on machines without Inter.
  blueprint-cli info <input.blueprint>
      Show pages, layers and element counts.
  blueprint-cli import-sql <input.sql> [output.blueprint] [--dialect postgres] [--preview]
      Preview a PostgreSQL schema or import it into a new editable ERD project.
  blueprint-cli export-sql <input.blueprint> [output.sql] [--page <n|name>] [--dialect postgres] [--preview]
      Preview or export one ERD page as PostgreSQL DDL.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("export") => parse_export(&args[1..]).and_then(|e| export(&e)),
        Some("info") if args.len() == 2 => info(args[1].clone().into()),
        Some("import-sql") => sql::run(&args[1..], true),
        Some("export-sql") => sql::run(&args[1..], false),
        Some("--version" | "-V") => {
            println!("blueprint-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(String::new()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) if message.is_empty() => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

struct Export {
    input: PathBuf,
    output: Option<PathBuf>,
    page: Option<String>,
    embed_fonts: bool,
}

fn parse_export(args: &[String]) -> Result<Export, String> {
    let mut positional = Vec::new();
    let mut page = None;
    let mut embed_fonts = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--page" => {
                page = Some(
                    args.get(i + 1)
                        .ok_or("--page needs a page number or name")?
                        .clone(),
                );
                i += 1;
            }
            "--embed-fonts" => embed_fonts = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => positional.push(PathBuf::from(path)),
        }
        i += 1;
    }
    let mut positional = positional.into_iter();
    let input = positional.next().ok_or_else(String::new)?;
    let output = positional.next();
    if positional.next().is_some() {
        return Err(String::new());
    }
    Ok(Export {
        input,
        output,
        page,
        embed_fonts,
    })
}

fn export(e: &Export) -> Result<(), String> {
    let doc = bp_io::load(&e.input).map_err(|err| format!("{}: {err}", e.input.display()))?;
    let pages = doc.pages_sorted();
    let page = match &e.page {
        None => pages.first().ok_or("the document has no pages")?.id,
        Some(want) => {
            let by_number = want
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|i| pages.get(i));
            by_number
                .or_else(|| pages.iter().find(|p| p.name == *want))
                .ok_or_else(|| {
                    format!("no page {want:?} (the document has {} pages)", pages.len())
                })?
                .id
        }
    };
    let options = bp_export::SvgOptions {
        embed_fonts: e.embed_fonts,
        ..Default::default()
    };
    let svg = bp_export::page_to_svg(&doc, page, &options);
    let output = e.output.clone().unwrap_or_else(|| default_output(&e.input));
    if e.input.canonicalize().ok() == output.canonicalize().ok() {
        return Err("the export output must be different from the input project".into());
    }
    bp_io::atomic_write(&output, svg.as_bytes())
        .map_err(|err| format!("{}: {err}", output.display()))?;
    println!("Wrote {}", output.display());
    Ok(())
}

fn default_output(input: &std::path::Path) -> PathBuf {
    let stem = input.with_extension("");
    if input
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        && stem
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(bp_io::FILE_EXTENSION))
    {
        stem.with_extension("svg")
    } else {
        input.with_extension("svg")
    }
}

fn info(input: PathBuf) -> Result<(), String> {
    let doc = bp_io::load(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    println!("{} (format {})", input.display(), doc.schema_version);
    let tree = doc.tree();
    for page in doc.pages_sorted() {
        let elements = doc.page_elements(page.id);
        let connectors = elements.iter().filter(|e| e.is_connector()).count();
        println!(
            "  {}: {} shapes, {connectors} connectors",
            page.name,
            elements.iter().filter(|e| e.is_shape()).count(),
        );
        for layer in doc.layers_of(page.id).into_iter().rev() {
            let mut all = Vec::new();
            tree.subtree(bp_model::Parent::Layer(layer.id), &mut all);
            let flags = match (layer.visible, layer.locked) {
                (true, false) => "",
                (false, false) => " (hidden)",
                (true, true) => " (locked)",
                (false, true) => " (hidden, locked)",
            };
            println!("    {}{flags}: {} elements", layer.name, all.len());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::{Document, ElementId};

    #[test]
    fn export_names_preserve_dots_in_the_project_name() {
        for (input, expected) in [
            ("flow.blueprint", "flow.svg"),
            ("flow.v2.blueprint", "flow.v2.svg"),
            ("flow.v2.blueprint.json", "flow.v2.svg"),
            ("flow.v2.json", "flow.v2.svg"),
            ("flow.v2.BLUEPRINT.JSON", "flow.v2.svg"),
        ] {
            assert_eq!(
                default_output(std::path::Path::new(input)),
                PathBuf::from(expected)
            );
        }
    }

    #[test]
    fn exporting_cannot_overwrite_the_source_project() {
        let dir = std::env::temp_dir().join(format!("blueprint-cli-{}", ElementId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        // Input format is detected from contents, so a project can have an SVG extension.
        let input = dir.join("project.svg");
        let doc = Document::new();
        bp_io::save(&doc, &input).unwrap();
        let original = std::fs::read(&input).unwrap();
        for output in [
            None,
            Some(input.clone()),
            Some(dir.join(".").join("project.svg")),
        ] {
            let error = export(&Export {
                input: input.clone(),
                output,
                page: None,
                embed_fonts: false,
            })
            .unwrap_err();
            assert!(error.contains("different from the input"));
            assert_eq!(std::fs::read(&input).unwrap(), original);
        }
        assert_eq!(bp_io::load(&input).unwrap(), doc);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
