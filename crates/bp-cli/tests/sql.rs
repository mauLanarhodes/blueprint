//! Exercise the real CLI and native file format, including destination safety.

use bp_model::ElementId;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blueprint-cli"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn sql_preview_import_export_round_trip_and_source_preservation() {
    let dir = std::env::temp_dir().join(format!("bp-cli-sql-{}", ElementId::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.sql");
    let project = dir.join("diagram.blueprint");
    let exported = dir.join("export.sql");
    let again = dir.join("again.blueprint.json");
    let ddl = "CREATE TABLE parent (id INTEGER PRIMARY KEY);\nCREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id));\nSELECT 1;\n";
    std::fs::write(&source, ddl).unwrap();
    let s = source.to_str().unwrap();
    let p = project.to_str().unwrap();
    let e = exported.to_str().unwrap();
    let a = again.to_str().unwrap();
    let preview = cli(&["import-sql", s, p, "--preview"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("2 tables"));
    assert!(String::from_utf8_lossy(&preview.stderr).contains("line 3"));
    assert!(!project.exists());
    let imported = cli(&["import-sql", s, p]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert_eq!(std::fs::read_to_string(&source).unwrap(), ddl);
    let doc = bp_io::load(&project).unwrap();
    assert_eq!(
        doc.elements
            .values()
            .filter(|el| el.as_shape().is_some_and(|shape| shape.erd.is_some()))
            .count(),
        2
    );
    let preview = cli(&["export-sql", p, e, "--preview", "--page", "1"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("CREATE TABLE"));
    assert!(!exported.exists());
    let generated = cli(&["export-sql", p, e, "--dialect", "postgres"]);
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    assert!(cli(&["import-sql", e, a]).status.success());
    let reparsed = bp_sql::parse(
        &std::fs::read_to_string(&exported).unwrap(),
        bp_model::SqlDialect::PostgreSql,
    )
    .unwrap();
    let original = bp_sql::parse(ddl, bp_model::SqlDialect::PostgreSql).unwrap();
    assert_eq!(reparsed.schema, original.schema);
    let original_project = std::fs::read(&project).unwrap();
    assert!(!cli(&["export-sql", p, p]).status.success());
    assert!(!cli(&["export-sql", p, s]).status.success());
    assert!(!cli(&["import-sql", s, p]).status.success());
    assert!(
        !cli(&["export-sql", p, e, "--dialect", "mysql"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&project).unwrap(), original_project);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), ddl);
    std::fs::remove_dir_all(dir).unwrap();
}
