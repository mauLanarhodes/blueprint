use bp_commands::{Command, History, Prop, edit::Clip};
use bp_model::*;
use std::collections::BTreeMap;

fn shape(doc: &Document) -> Element {
    Element::shape(
        ShapeRef::new("basic", "rectangle"),
        Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id),
        OrderKey::first(),
        kurbo::Rect::new(0., 0., 100., 100.),
    )
}
fn resource() -> TerraformResource {
    TerraformResource {
        key: "primary".into(),
        provider: ProviderPin {
            provider: TerraformProvider::Aws,
            version: "6.68.0".into(),
        },
        resource_type: "aws_s3_bucket".into(),
        properties: BTreeMap::new(),
        depends_on: vec![],
    }
}

#[test]
fn generic_edits_are_validated_undoable_and_copies_remap_resource_targets() {
    let mut doc = Document::new();
    let mut history = History::new();
    let mut first = shape(&doc);
    let mut second = shape(&doc);
    first.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Resources {
        resources: vec![resource()],
    });
    let mut dependent = resource();
    dependent.resource_type = "aws_sqs_queue".into();
    dependent.properties.insert(
        "policy".into(),
        ResourceValue::Reference(ResourceReference {
            target: ResourceTarget {
                element: Some(first.id),
                resource: "primary".into(),
            },
            output: vec![OutputStep::Attribute("arn".into())],
        }),
    );
    second.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Resources {
        resources: vec![dependent],
    });
    history
        .apply(
            &mut doc,
            "Insert",
            [
                Command::Insert(Box::new(first.clone())),
                Command::Insert(Box::new(second.clone())),
            ],
        )
        .unwrap();
    let before = doc.clone();
    let mut invalid = resource();
    invalid.provider.version = "latest".into();
    assert!(
        history
            .apply(
                &mut doc,
                "Invalid",
                [Command::Set {
                    id: first.id,
                    prop: Prop::Infrastructure(Some(AwsResource::Resources {
                        resources: vec![invalid]
                    }))
                }]
            )
            .is_err()
    );
    assert_eq!(doc, before);
    let clip = Clip::copy(&doc, &[first.id, second.id], |_, _| None);
    let (ids, commands) = clip.paste(&doc, first.parent, kurbo::Vec2::new(20., 20.));
    history.apply(&mut doc, "Paste", commands).unwrap();
    let copies: Vec<_> = ids
        .iter()
        .map(|id| {
            doc.elements[id]
                .as_shape()
                .unwrap()
                .infrastructure
                .as_ref()
                .unwrap()
        })
        .collect();
    let mut targets = Vec::new();
    for copy in copies {
        if let AwsResource::Resources { resources } = copy {
            targets.extend(resources.iter().flat_map(TerraformResource::targets));
        }
    }
    assert_eq!(targets.len(), 1);
    assert!(ids.contains(&targets[0].element.unwrap()));
    let pasted = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, pasted);
    let clip = Clip::copy(&doc, &[second.id], |_, _| None);
    let (isolated, commands) = clip.paste(&doc, second.parent, kurbo::Vec2::ZERO);
    history.apply(&mut doc, "Isolated", commands).unwrap();
    let AwsResource::Resources { resources } = doc.elements[&isolated[0]]
        .as_shape()
        .unwrap()
        .infrastructure
        .as_ref()
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(resources[0].targets()[0].element, None);
}
