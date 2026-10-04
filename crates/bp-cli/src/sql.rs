//! Headless schema previews share the desktop parser and undoable conversion.

use bp_model::{DiagramKind, Document, Parent, SqlDialect};
use std::path::PathBuf;

pub(super) fn run(args: &[String], importing: bool) -> Result<(), String> {
    let mut paths = Vec::new();
    let mut page = None;
    let mut preview = false;
    let mut dialect = SqlDialect::PostgreSql;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--preview" => preview = true,
            "--dialect" => {
                i += 1;
                dialect = match args.get(i).map(String::as_str) {
                    Some("postgres" | "postgresql") => SqlDialect::PostgreSql,
                    Some(other) => {
                        return Err(format!(
                            "unsupported SQL dialect {other:?}; available: postgres"
                        ));
                    }
                    None => return Err("--dialect needs a dialect (postgres)".into()),
                };
            }
            "--page" if !importing => {
                i += 1;
                page = Some(
                    args.get(i)
                        .ok_or("--page needs a page number or name")?
                        .clone(),
                );
            }
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            path => paths.push(PathBuf::from(path)),
        }
        i += 1;
    }
    if paths.is_empty() || paths.len() > 2 {
        return Err(String::new());
    }
    let input = &paths[0];
    if importing {
        let source =
            std::fs::read_to_string(input).map_err(|e| format!("{}: {e}", input.display()))?;
        let parsed = bp_sql::parse(&source, dialect)?;
        println!(
            "{} tables, {} columns, {} foreign keys",
            parsed.schema.tables.len(),
            parsed
                .schema
                .tables
                .iter()
                .map(|table| table.columns.len())
                .sum::<usize>(),
            parsed.schema.foreign_keys.len()
        );
        warnings(&parsed.warnings);
        if preview {
            return Ok(());
        }
        if parsed.schema.tables.is_empty() {
            return Err("no supported tables to import".into());
        }
        let output = paths
            .get(1)
            .ok_or("provide a new output.blueprint path, or use --preview")?;
        if !output
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("blueprint") || e.eq_ignore_ascii_case("json"))
        {
            return Err(
                "the import output must be a new .blueprint or .blueprint.json project".into(),
            );
        }
        // Import never replaces the original script or an existing project.
        if output.exists() || std::fs::symlink_metadata(output).is_ok() {
            return Err("the import output already exists; choose a new project path".into());
        }
        let mut doc = Document::new();
        let page = doc.first_page().ok_or("the document has no pages")?;
        doc.pages.get_mut(&page).unwrap().diagram_kind = Some(DiagramKind::Erd);
        let parent = Parent::Layer(doc.layers_of(page)[0].id);
        let (_, commands) = bp_sql::import_commands(&doc, parent, &parsed)?;
        bp_commands::History::new()
            .apply(&mut doc, "Import SQL schema", commands)
            .map_err(|e| e.to_string())?;
        bp_io::save(&doc, output).map_err(|e| format!("{}: {e}", output.display()))?;
        println!("Wrote {}", output.display());
    } else {
        let doc = bp_io::load(input).map_err(|e| format!("{}: {e}", input.display()))?;
        let pages = doc.pages_sorted();
        let selected = match page {
            None => pages.first().ok_or("the document has no pages")?,
            Some(want) => want
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|n| pages.get(n))
                .or_else(|| pages.iter().find(|p| p.name == want))
                .ok_or_else(|| format!("no page {want:?}"))?,
        };
        let generated = bp_sql::export_page(&doc, selected.id, dialect)?;
        warnings(&generated.warnings);
        if preview {
            print!("{}", generated.sql);
            return Ok(());
        }
        let output = paths
            .get(1)
            .cloned()
            .unwrap_or_else(|| super::default_output(input).with_extension("sql"));
        if output.exists() || std::fs::symlink_metadata(&output).is_ok() {
            return Err(
                "the SQL output already exists; choose a new path to preserve source scripts"
                    .into(),
            );
        }
        bp_io::write_sql(&output, &generated.sql, Some(input)).map_err(|e| e.to_string())?;
        println!("Wrote {}", output.display());
    }
    Ok(())
}

fn warnings(warnings: &[bp_sql::Warning]) {
    for warning in warnings {
        if warning.line == 0 {
            eprintln!("warning: {}", warning.message);
        } else {
            eprintln!("line {}: {}", warning.line, warning.message);
        }
    }
}
