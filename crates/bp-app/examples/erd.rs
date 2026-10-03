//! Builds an editable sample ERD:
//! `cargo run -p bp-app --example erd -- examples/orders.blueprint.json`

use bp_app::BlueprintApp;
use bp_commands::{ColumnProp, Command, Prop};
use bp_model::kurbo::Rect;
use bp_model::{ElementId, Endpoint, PortId, ShapeRef};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "orders.blueprint.json".into());
    let mut app = BlueprintApp::new(&egui::Context::default(), None);
    let customers = table(&mut app, "customers", Rect::new(40.0, 60.0, 390.0, 240.0));
    let orders = table(&mut app, "orders", Rect::new(570.0, 140.0, 920.0, 350.0));
    column(&mut app, customers, "email", "VARCHAR(255)", true);
    column(&mut app, customers, "name", "TEXT", false);
    let customer_id = column(&mut app, orders, "customer_id", "BIGINT", false);
    let status = column(&mut app, orders, "status", "TEXT", false);
    app.set_table_column(
        orders,
        status,
        ColumnProp::DefaultValue(Some("'pending'".into())),
    );
    let total = column(&mut app, orders, "total", "NUMERIC(12,2)", false);
    app.set_table_column(orders, total, ColumnProp::DefaultValue(Some("0".into())));
    let primary = app.doc.elements[&customers]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[0]
        .id;
    app.insert_connector(
        Endpoint::glued(orders, Some(PortId::column(customer_id, true).as_str())),
        Endpoint::glued(customers, Some(PortId::column(primary, false).as_str())),
    )
    .expect("valid row relationship");
    let note = app
        .insert_shape(
            ShapeRef::new("erd", "note"),
            Rect::new(40.0, 300.0, 390.0, 420.0),
        )
        .unwrap();
    app.apply(
        "Schema note",
        [Command::Set {
            id: note,
            prop: Prop::Text(
                "Each order belongs to one customer.\nA customer can have zero or many orders."
                    .into(),
            ),
        }],
    );
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    bp_io::save(&app.doc, &output)?;
    println!("Wrote {}", output.display());
    Ok(())
}

fn table(app: &mut BlueprintApp, name: &str, bounds: Rect) -> ElementId {
    let id = app
        .insert_shape(ShapeRef::new("erd", "table"), bounds)
        .unwrap();
    app.apply(
        "Rename table",
        [Command::Set {
            id,
            prop: Prop::Text(name.into()),
        }],
    );
    id
}

fn column(
    app: &mut BlueprintApp,
    table: ElementId,
    name: &str,
    data_type: &str,
    unique: bool,
) -> bp_model::ColumnId {
    let id = app.add_table_column(table).unwrap();
    app.set_table_column(table, id, ColumnProp::Name(name.into()));
    app.set_table_column(table, id, ColumnProp::DataType(data_type.into()));
    app.set_table_column(table, id, ColumnProp::Nullable(false));
    app.set_table_column(table, id, ColumnProp::Unique(unique));
    id
}
