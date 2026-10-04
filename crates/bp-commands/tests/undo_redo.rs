//! Property test: any sequence of edits, valid or not, can be undone back
//! to the starting document and redone to the final one, and the document
//! stays valid after every step.

use bp_commands::edit::{self, Reorder};
use bp_commands::{ColumnProp, Command, History, LayerProp, Prop};
use bp_model::kurbo::{Point, Rect, Vec2};
use bp_model::{
    Color, Document, Element, ElementId, Endpoint, ErdColumn, Layer, OrderKey, Page, Paint, Parent,
    PortId, ShapeRef, SqlDialect, TableDisplay,
};
use proptest::prelude::*;

/// One random edit, interpreted against the document as it is when the
/// edit runs (the numbers pick among whatever exists then).
#[derive(Clone, Debug)]
struct Op {
    kind: u8,
    a: u16,
    b: u16,
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(
        (0u8..22, any::<u16>(), any::<u16>()).prop_map(|(kind, a, b)| Op { kind, a, b }),
        1..40,
    )
}

fn pick<T: Copy>(items: &[T], n: u16) -> Option<T> {
    (!items.is_empty()).then(|| items[usize::from(n) % items.len()])
}

fn commands_for(doc: &Document, op: &Op) -> Vec<Command> {
    let elements: Vec<ElementId> = doc.elements.keys().copied().collect();
    let shapes: Vec<ElementId> = doc
        .elements
        .values()
        .filter(|e| e.is_shape())
        .map(|e| e.id)
        .collect();
    let groups: Vec<ElementId> = doc
        .elements
        .values()
        .filter(|e| e.is_group())
        .map(|e| e.id)
        .collect();
    let layers: Vec<_> = doc.layers.keys().copied().collect();
    let tables: Vec<_> = doc
        .elements
        .values()
        .filter(|element| element.as_shape().is_some_and(|shape| shape.erd.is_some()))
        .map(|element| element.id)
        .collect();
    let row = pick(&tables, op.a).and_then(|id| {
        let table = doc.elements[&id].as_shape().unwrap().erd.as_ref().unwrap();
        let columns: Vec<_> = table.columns.iter().map(|column| column.id).collect();
        pick(&columns, op.b).map(|column| (id, column))
    });
    let layer = Parent::Layer(pick(&layers, op.a).expect("a layer"));
    let x = f64::from(op.b % 500);
    match op.kind {
        0 | 1 => {
            let el = Element::shape(
                ShapeRef::new("basic", "rectangle"),
                layer,
                doc.next_order_key(layer),
                Rect::new(x, x, x + 40.0, x + 20.0),
            );
            vec![Command::Insert(Box::new(el))]
        }
        2 => match (pick(&shapes, op.a), pick(&shapes, op.b)) {
            (Some(a), Some(b)) => {
                let el = Element::connector(
                    Endpoint::glued(a, Some("e")),
                    Endpoint::glued(b, None),
                    layer,
                    doc.next_order_key(layer),
                );
                vec![Command::Insert(Box::new(el))]
            }
            _ => vec![],
        },
        // A raw remove may be refused (children, connectors attached).
        3 => pick(&elements, op.a)
            .map(Command::Remove)
            .into_iter()
            .collect(),
        4 => pick(&elements, op.a)
            .map(|id| edit::remove(doc, &[id]))
            .unwrap_or_default(),
        5 => pick(&shapes, op.a)
            .map(|id| {
                vec![Command::Set {
                    id,
                    prop: Prop::Bounds(Rect::new(x, 0.0, x + 9.0, 9.0)),
                }]
            })
            .unwrap_or_default(),
        6 => pick(&elements, op.a)
            .map(|id| {
                vec![Command::Set {
                    id,
                    prop: Prop::Fill(Some(Paint::Color(Color::rgb(op.b as u8, 0, 0)))),
                }]
            })
            .unwrap_or_default(),
        // Reparenting may be refused (cycles, connectors as parents).
        7 => match (pick(&elements, op.a), pick(&elements, op.b)) {
            (Some(child), Some(parent)) => {
                vec![Command::Set {
                    id: child,
                    prop: Prop::Parent(Parent::Element(parent)),
                }]
            }
            _ => vec![],
        },
        8 => {
            let ids: Vec<_> = [pick(&elements, op.a), pick(&elements, op.b)]
                .into_iter()
                .flatten()
                .collect();
            edit::group(doc, &ids).map(|(_, c)| c).unwrap_or_default()
        }
        9 => edit::ungroup(doc, &groups).1,
        10 => pick(&elements, op.a)
            .map(|id| edit::translate(doc, &[id], Vec2::new(x, -x)))
            .unwrap_or_default(),
        11 => pick(&elements, op.a)
            .map(|id| {
                let how = [
                    Reorder::Front,
                    Reorder::Forward,
                    Reorder::Backward,
                    Reorder::Back,
                ][usize::from(op.b) % 4];
                edit::reorder(doc, &[id], how)
            })
            .unwrap_or_default(),
        12 => {
            let first = doc.first_page().expect("a page");
            let page = Page::new("P", OrderKey::after(&doc.pages[&first].order));
            let layer = Layer::new(page.id, "L", OrderKey::first());
            vec![
                Command::InsertPage(Box::new(page)),
                Command::InsertLayer(Box::new(layer)),
            ]
        }
        13 => match pick(&layers, op.b) {
            Some(id) => vec![
                Command::SetLayer {
                    id,
                    prop: LayerProp::Visible(op.a.is_multiple_of(2)),
                },
                Command::Set {
                    id: pick(&elements, op.a).unwrap_or_default(),
                    prop: Prop::Target(Endpoint::Free(Point::new(x, x))),
                },
            ],
            None => vec![],
        },
        14 => vec![Command::Insert(Box::new(Element::shape(
            ShapeRef::new("erd", "table"),
            layer,
            doc.next_order_key(layer),
            Rect::new(x, x, x + 240.0, x + 100.0),
        )))],
        15 => pick(&tables, op.a)
            .map(|id| {
                let table = doc.elements[&id].as_shape().unwrap().erd.as_ref().unwrap();
                let order = table
                    .columns
                    .iter()
                    .map(|column| &column.order)
                    .max()
                    .map_or_else(OrderKey::first, OrderKey::after);
                vec![Command::InsertColumn {
                    id,
                    column: Box::new(ErdColumn::new(format!("column_{}", op.b), "TEXT", order)),
                }]
            })
            .unwrap_or_default(),
        16 => row
            .map(|(id, column)| vec![Command::RemoveColumn { id, column }])
            .unwrap_or_default(),
        17 => row
            .map(|(id, column)| edit::remove_column(doc, id, column))
            .unwrap_or_default(),
        18 => row
            .map(|(id, column)| {
                let flag = op.a.is_multiple_of(2);
                let prop = match op.b % 9 {
                    0 => ColumnProp::Name(format!("renamed_{}", op.a)),
                    1 => ColumnProp::DataType("UUID".into()),
                    2 => ColumnProp::Order(OrderKey::after(
                        &doc.elements[&id]
                            .as_shape()
                            .unwrap()
                            .erd
                            .as_ref()
                            .unwrap()
                            .column(column)
                            .unwrap()
                            .order,
                    )),
                    3 => ColumnProp::PrimaryKey(flag),
                    4 => ColumnProp::ForeignKey(flag),
                    5 => ColumnProp::Unique(flag),
                    6 => ColumnProp::Nullable(flag),
                    7 => ColumnProp::DefaultValue(Some("0".into())),
                    _ => ColumnProp::DefaultValue(None),
                };
                vec![Command::SetColumn { id, column, prop }]
            })
            .unwrap_or_default(),
        19 => pick(&tables, op.a)
            .map(|id| {
                vec![
                    Command::Set {
                        id,
                        prop: Prop::TableDisplay(
                            TableDisplay::ALL[usize::from(op.b) % TableDisplay::ALL.len()],
                        ),
                    },
                    Command::Set {
                        id,
                        prop: Prop::SqlDialect(
                            SqlDialect::ALL[usize::from(op.a) % SqlDialect::ALL.len()],
                        ),
                    },
                ]
            })
            .unwrap_or_default(),
        20 => match (row, pick(&tables, op.b)) {
            (Some((source, column)), Some(target)) => {
                let Some(target_column) = doc.elements[&target]
                    .as_shape()
                    .unwrap()
                    .erd
                    .as_ref()
                    .unwrap()
                    .columns
                    .first()
                else {
                    return vec![];
                };
                vec![Command::Insert(Box::new(Element::connector(
                    Endpoint::Glued {
                        element: source,
                        port: Some(PortId::column(column, false)),
                    },
                    Endpoint::Glued {
                        element: target,
                        port: Some(PortId::column(target_column.id, true)),
                    },
                    layer,
                    doc.next_order_key(layer),
                )))]
            }
            _ => vec![],
        },
        21 => pick(&shapes, op.a)
            .map(|id| {
                vec![Command::Set {
                    id,
                    prop: Prop::Shape(ShapeRef::new(
                        if doc.elements[&id].as_shape().unwrap().erd.is_some() {
                            "basic"
                        } else {
                            "erd"
                        },
                        if doc.elements[&id].as_shape().unwrap().erd.is_some() {
                            "rectangle"
                        } else {
                            "table"
                        },
                    )),
                }]
            })
            .unwrap_or_default(),
        _ => vec![],
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn undo_and_redo_restore_every_state(ops in ops()) {
        let mut doc = Document::new();
        let mut history = History::new();
        let start = doc.clone();
        let mut states = vec![doc.clone()];
        for op in &ops {
            let commands = commands_for(&doc, op);
            let before = doc.clone();
            match history.apply(&mut doc, "op", commands) {
                Ok(()) => {
                    if doc != before {
                        states.push(doc.clone());
                    }
                }
                Err(_) => prop_assert_eq!(&doc, &before, "a failed step changes nothing"),
            }
            prop_assert_eq!(doc.validate(), Ok(()));
        }
        let end = doc.clone();
        // Undo walks back through every recorded state.
        let mut undone = 0;
        while history.undo(&mut doc) {
            undone += 1;
            prop_assert_eq!(doc.validate(), Ok(()));
            prop_assert!(states.contains(&doc));
        }
        prop_assert_eq!(&doc, &start);
        for _ in 0..undone {
            prop_assert!(history.redo(&mut doc));
            prop_assert_eq!(doc.validate(), Ok(()));
        }
        prop_assert_eq!(doc, end);
    }
}
