use bp_commands::{ColumnProp, Command, CommandError, History, Prop, edit};
use bp_model::kurbo::{Point, Rect, Vec2};
use bp_model::{
    ColumnId, Document, Element, ElementId, Endpoint, ErdColumn, OrderKey, Parent, PortId,
    ShapeRef, SqlDialect, TableDisplay,
};

fn table_fixture() -> (Document, ElementId, ColumnId, Parent) {
    let mut doc = Document::new();
    let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let table = Element::shape(
        ShapeRef::new("erd", "table"),
        parent,
        OrderKey::first(),
        Rect::new(0.0, 0.0, 240.0, 120.0),
    );
    let id = table.id;
    let column = table.as_shape().unwrap().erd.as_ref().unwrap().columns[0].id;
    doc.elements.insert(id, table);
    (doc, id, column, parent)
}

#[test]
fn column_and_table_properties_undo_to_the_exact_document() {
    let (mut doc, id, column, _) = table_fixture();
    let before = doc.clone();
    let mut history = History::new();
    let props = [
        ColumnProp::Name("account_id".into()),
        ColumnProp::DataType("UUID".into()),
        ColumnProp::Order(OrderKey::after(&OrderKey::first())),
        ColumnProp::PrimaryKey(false),
        ColumnProp::ForeignKey(true),
        ColumnProp::Unique(true),
        ColumnProp::Nullable(true),
        ColumnProp::DefaultValue(Some("uuid()".into())),
    ];
    let mut commands: Vec<_> = props
        .into_iter()
        .map(|prop| Command::SetColumn { id, column, prop })
        .collect();
    commands.extend([
        Command::Set {
            id,
            prop: Prop::TableDisplay(TableDisplay::KeysOnly),
        },
        Command::Set {
            id,
            prop: Prop::SqlDialect(SqlDialect::MySql),
        },
    ]);
    history.apply(&mut doc, "Edit column", commands).unwrap();
    assert_eq!(doc.validate(), Ok(()));
    let after = doc.clone();
    assert_ne!(before, after);
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, after);
}

#[test]
fn column_removal_restores_original_sequence_and_relationships() {
    let (mut doc, id, column, parent) = table_fixture();
    let mut history = History::new();
    let row = ErdColumn::new("name", "TEXT", OrderKey::after(&OrderKey::first()));
    history
        .apply(
            &mut doc,
            "Add",
            [Command::InsertColumn {
                id,
                column: Box::new(row),
            }],
        )
        .unwrap();
    let connection = Element::connector(
        Endpoint::Glued {
            element: id,
            port: Some(PortId::column(column, false)),
        },
        Endpoint::Free(Point::new(400.0, 70.0)),
        parent,
        doc.next_order_key(parent),
    );
    let connection_id = connection.id;
    history
        .apply(&mut doc, "Connect", [Command::Insert(Box::new(connection))])
        .unwrap();
    let before = doc.clone();
    assert_eq!(
        Command::RemoveColumn { id, column }.apply(&mut doc),
        Err(CommandError::InUseColumn { id, column })
    );
    assert_eq!(doc, before);
    let commands = edit::remove_column(&doc, id, column);
    history.apply(&mut doc, "Delete column", commands).unwrap();
    assert!(!doc.elements.contains_key(&connection_id));
    assert_eq!(
        doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns
            .len(),
        1
    );
    assert_eq!(doc.validate(), Ok(()));
    let after = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert_eq!(doc.validate(), Ok(()));
    assert!(history.redo(&mut doc));
    assert_eq!(doc, after);
}

#[test]
fn coalescing_keeps_columns_separate_and_respects_remove_reinsert() {
    let (mut doc, id, column, _) = table_fixture();
    let before = doc.clone();
    let row = doc.elements[&id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[0]
        .clone();
    let second = ErdColumn::new("name", "TEXT", OrderKey::after(&row.order));
    let second_id = second.id;
    let mut history = History::new();
    history
        .apply(
            &mut doc,
            "Edit",
            [
                Command::SetColumn {
                    id,
                    column,
                    prop: ColumnProp::Name("earlier".into()),
                },
                Command::RemoveColumn { id, column },
                Command::InsertColumn {
                    id,
                    column: Box::new(row),
                },
                Command::InsertColumn {
                    id,
                    column: Box::new(second),
                },
                Command::SetColumn {
                    id,
                    column: second_id,
                    prop: ColumnProp::Name("other".into()),
                },
                Command::SetColumn {
                    id,
                    column,
                    prop: ColumnProp::Name("final".into()),
                },
                Command::SetColumn {
                    id,
                    column,
                    prop: ColumnProp::Name("final again".into()),
                },
            ],
        )
        .unwrap();
    let after = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, after);
    let table = doc.elements[&id].as_shape().unwrap().erd.as_ref().unwrap();
    assert_eq!(table.column(column).unwrap().name, "final again");
    assert_eq!(table.column(second_id).unwrap().name, "other");
}

#[test]
fn invalid_column_edits_and_ports_preserve_document() {
    let (mut doc, id, column, parent) = table_fixture();
    let before = doc.clone();
    let existing = doc.elements[&id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[0]
        .clone();
    assert_eq!(
        Command::InsertColumn {
            id,
            column: Box::new(existing)
        }
        .apply(&mut doc),
        Err(CommandError::DuplicateColumn { id, column })
    );
    let missing = ColumnId::new();
    assert_eq!(
        Command::SetColumn {
            id,
            column: missing,
            prop: ColumnProp::Nullable(false)
        }
        .apply(&mut doc),
        Err(CommandError::MissingColumn {
            id,
            column: missing
        })
    );
    let invalid_order: OrderKey = serde_json::from_str("\"V0\"").unwrap();
    assert!(
        Command::SetColumn {
            id,
            column,
            prop: ColumnProp::Order(invalid_order)
        }
        .apply(&mut doc)
        .is_err()
    );
    let mut history = History::new();
    let connection = Element::connector(
        Endpoint::Glued {
            element: id,
            port: Some(PortId::column(missing, true)),
        },
        Endpoint::Free(Point::ZERO),
        parent,
        doc.next_order_key(parent),
    );
    assert!(
        history
            .apply(
                &mut doc,
                "Invalid connection",
                [Command::Insert(Box::new(connection))]
            )
            .is_err()
    );
    assert_eq!(doc, before);
    let connection = Element::connector(
        Endpoint::glued(id, Some("w")),
        Endpoint::Free(Point::ZERO),
        parent,
        doc.next_order_key(parent),
    );
    let connector_id = connection.id;
    history
        .apply(
            &mut doc,
            "Connection",
            [Command::Insert(Box::new(connection))],
        )
        .unwrap();
    let connected = doc.clone();
    assert!(
        Command::Set {
            id: connector_id,
            prop: Prop::Source(Endpoint::Glued {
                element: id,
                port: Some(PortId::column(missing, true))
            })
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, connected);
}

#[test]
fn changing_shape_references_cannot_leave_table_metadata_mismatched() {
    let (mut doc, id, _, parent) = table_fixture();
    let before = doc.clone();
    assert!(
        Command::Set {
            id,
            prop: Prop::Shape(ShapeRef::new("basic", "rectangle"))
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    let rectangle = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        doc.next_order_key(parent),
        Rect::new(0.0, 0.0, 100.0, 100.0),
    );
    let rectangle_id = rectangle.id;
    doc.elements.insert(rectangle_id, rectangle);
    let before = doc.clone();
    assert!(
        Command::Set {
            id: rectangle_id,
            prop: Prop::Shape(ShapeRef::new("erd", "table"))
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    let inverse = Command::Set {
        id: rectangle_id,
        prop: Prop::Shape(ShapeRef::new("basic", "ellipse")),
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(doc.validate(), Ok(()));
    inverse.apply(&mut doc).unwrap();
    assert_eq!(doc, before);
}

#[test]
fn copying_tables_preserves_column_data_and_remaps_relationship_elements() {
    let (mut doc, id, column, parent) = table_fixture();
    let mut history = History::new();
    let other = Element::shape(
        ShapeRef::new("erd", "table"),
        parent,
        doc.next_order_key(parent),
        Rect::new(400.0, 0.0, 640.0, 120.0),
    );
    let other_id = other.id;
    let other_column = other.as_shape().unwrap().erd.as_ref().unwrap().columns[0].id;
    history
        .apply(&mut doc, "Other", [Command::Insert(Box::new(other))])
        .unwrap();
    let connector = Element::connector(
        Endpoint::Glued {
            element: id,
            port: Some(PortId::column(column, false)),
        },
        Endpoint::Glued {
            element: other_id,
            port: Some(PortId::column(other_column, true)),
        },
        parent,
        doc.next_order_key(parent),
    );
    history
        .apply(&mut doc, "Connect", [Command::Insert(Box::new(connector))])
        .unwrap();
    let before = doc.clone();
    let clip = edit::Clip::copy(&doc, &[id, other_id], |_, _| None);
    let (roots, commands) = clip.paste(&doc, parent, Vec2::new(20.0, 30.0));
    history.apply(&mut doc, "Paste", commands).unwrap();
    assert_eq!(doc.validate(), Ok(()));
    let pasted = doc.elements[&roots[0]].as_shape().unwrap();
    assert_eq!(pasted.erd, before.elements[&id].as_shape().unwrap().erd);
    let connector = doc.elements[&roots[2]].as_connector().unwrap();
    assert!(
        matches!(&connector.source, Endpoint::Glued {element,port:Some(port)} if *element == roots[0] && port.column_id() == Some(column))
    );
    assert!(
        matches!(&connector.target, Endpoint::Glued {element,port:Some(port)} if *element == roots[1] && port.column_id() == Some(other_column))
    );
    history.undo(&mut doc);
    assert_eq!(doc, before);
}
