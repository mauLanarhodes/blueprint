use bp_commands::{Command, CommandError, History, Prop, edit::Clip};
use bp_model::kurbo::{Rect, Vec2};
use bp_model::{
    CloudIcon, CloudProvider, Document, Element, IconKind, ModelError, OrderKey, Parent, ShapeRef,
};
use std::sync::Arc;

fn icon() -> CloudIcon {
    CloudIcon {
        reference: ShapeRef::new("aws", "sample@2026-10"),
        name: "Sample service".into(),
        provider: CloudProvider::Aws,
        category: "Compute".into(),
        kind: IconKind::Service,
        pack_version: "2026-10".into(),
        source_path: "Compute/Sample.svg".into(),
        svg: Arc::from(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="orange"/></svg>"#,
        ),
    }
}

fn shape(doc: &Document) -> Element {
    Element::shape(
        icon().reference,
        Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id),
        OrderKey::first(),
        Rect::new(0.0, 0.0, 64.0, 64.0),
    )
}

#[test]
fn icon_and_shape_are_one_undo_step_and_identical_insertion_preserves_existing_asset() {
    let mut doc = Document::new();
    let original = doc.clone();
    let element = shape(&doc);
    let mut history = History::new();
    history
        .apply(
            &mut doc,
            "Insert cloud service",
            [
                Command::InsertIcon(Box::new(icon())),
                Command::Insert(Box::new(element)),
            ],
        )
        .unwrap();
    let placed = doc.clone();
    assert!(Arc::ptr_eq(
        &doc.icons[&icon().reference].svg,
        &placed.icons[&icon().reference].svg
    ));
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, placed);
    history
        .apply(
            &mut doc,
            "Insert existing icon",
            [Command::InsertIcon(Box::new(icon()))],
        )
        .unwrap();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, placed);
}

#[test]
fn missing_conflicting_and_used_icons_are_rejected_without_changes() {
    let mut doc = Document::new();
    let element = shape(&doc);
    let id = element.id;
    let mut history = History::new();
    assert!(matches!(
        history.apply(
            &mut doc,
            "Missing",
            [Command::Insert(Box::new(element.clone()))]
        ),
        Err(CommandError::Invalid(ModelError::MissingIcon { .. }))
    ));
    history
        .apply(
            &mut doc,
            "Icon and shape",
            [
                Command::InsertIcon(Box::new(icon())),
                Command::Insert(Box::new(element)),
            ],
        )
        .unwrap();
    let before = doc.clone();
    let mut conflict = icon();
    conflict.svg = Arc::from("<svg/>");
    assert!(matches!(
        history.apply(
            &mut doc,
            "Conflicting",
            [
                Command::Set {
                    id,
                    prop: Prop::Text("Changed".into())
                },
                Command::InsertIcon(Box::new(conflict)),
            ]
        ),
        Err(CommandError::ConflictingIcon(_))
    ));
    assert_eq!(doc, before);
    assert!(matches!(
        Command::RemoveIcon {
            reference: icon().reference
        }
        .apply(&mut doc),
        Err(CommandError::IconInUse(_))
    ));
    assert_eq!(doc, before);
    let reference = ShapeRef::new("azure", "missing@v1");
    assert!(matches!(
        Command::Set {
            id,
            prop: Prop::Shape(reference)
        }
        .apply(&mut doc),
        Err(CommandError::Invalid(ModelError::MissingIcon { .. }))
    ));
    assert_eq!(doc, before);
}

#[test]
fn clipboard_is_self_contained_and_duplicate_paste_shares_one_asset() {
    let mut source = Document::new();
    let element = shape(&source);
    let id = element.id;
    source.icons.insert(icon().reference.clone(), icon());
    source.elements.insert(id, element);
    let mut unused = icon();
    unused.reference = ShapeRef::new("aws", "unused@2026-10");
    source.icons.insert(unused.reference.clone(), unused);
    let clip = Clip::copy(&source, &[id], |_, _| None);
    assert_eq!(clip.icons.len(), 1);
    let clip: Clip = serde_json::from_slice(&serde_json::to_vec(&clip).unwrap()).unwrap();
    let mut destination = Document::new();
    let original = destination.clone();
    let parent = Parent::Layer(destination.layers_of(destination.first_page().unwrap())[0].id);
    let mut history = History::new();
    for offset in [Vec2::new(10.0, 20.0), Vec2::new(50.0, 60.0)] {
        let (_, commands) = clip.paste(&destination, parent, offset);
        history
            .apply(&mut destination, "Paste cloud icon", commands)
            .unwrap();
        assert_eq!(destination.icons.len(), 1);
        assert_eq!(destination.validate(), Ok(()));
    }
    let pasted = destination.clone();
    assert!(history.undo(&mut destination));
    assert_eq!(destination.icons.len(), 1);
    assert!(history.undo(&mut destination));
    assert_eq!(destination, original);
    assert!(history.redo(&mut destination));
    assert!(history.redo(&mut destination));
    assert_eq!(destination, pasted);
    let mut damaged = clip;
    damaged.icons.clear();
    let mut fresh = Document::new();
    let before = fresh.clone();
    let parent = Parent::Layer(fresh.layers_of(fresh.first_page().unwrap())[0].id);
    let commands = damaged.paste(&fresh, parent, Vec2::ZERO).1;
    assert!(
        history
            .apply(&mut fresh, "Damaged clipboard", commands)
            .is_err()
    );
    assert_eq!(fresh, before);
}

#[test]
fn changing_icon_references_keeps_asset_dependencies_in_replay_order() {
    let mut doc = Document::new();
    let element = shape(&doc);
    let id = element.id;
    doc.icons.insert(icon().reference.clone(), icon());
    doc.elements.insert(id, element);
    let before = doc.clone();
    let mut replacement = icon();
    replacement.reference = ShapeRef::new("aws", "replacement@2026-10");
    let mut history = History::new();
    history
        .apply(
            &mut doc,
            "Replace icon",
            [
                Command::Set {
                    id,
                    prop: Prop::Shape(ShapeRef::new("basic", "rectangle")),
                },
                Command::RemoveIcon {
                    reference: icon().reference,
                },
                Command::InsertIcon(Box::new(replacement.clone())),
                Command::Set {
                    id,
                    prop: Prop::Shape(replacement.reference),
                },
            ],
        )
        .unwrap();
    let after = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, after);
    assert_eq!(doc.validate(), Ok(()));
}
