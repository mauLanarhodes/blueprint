//! A working four-service starting point, shared by the editor and CLI.

use bp_model::kurbo::{Point, Rect};
use bp_model::{
    AwsDeployment, AwsResource, AwsResourceKind, DiagramKind, Document, Element, Endpoint,
    OrderKey, Parent, ShapeRef,
};

pub fn elements(parent: Parent, center: Point, mut order: OrderKey) -> Vec<Element> {
    let mut elements = Vec::new();
    for (kind, title, x, y) in [
        (
            AwsResourceKind::S3Bucket,
            "Amazon S3 · objects",
            -230.0,
            -125.0,
        ),
        (
            AwsResourceKind::DynamoDbTable,
            "DynamoDB · data",
            -230.0,
            35.0,
        ),
        (
            AwsResourceKind::SnsTopic,
            "Amazon SNS · events",
            30.0,
            -125.0,
        ),
        (AwsResourceKind::SqsQueue, "Amazon SQS · inbox", 30.0, 35.0),
    ] {
        let mut element = Element::shape(
            ShapeRef::new("basic", "rounded-rectangle"),
            parent,
            order.clone(),
            Rect::new(
                center.x + x,
                center.y + y,
                center.x + x + 200.0,
                center.y + y + 85.0,
            ),
        );
        let resource = kind.create(element.id);
        let shape = element.as_shape_mut().unwrap();
        shape.text = title.into();
        shape.infrastructure = Some(resource);
        order = OrderKey::after(&order);
        elements.push(element);
    }
    let topic = elements[2].id;
    let queue = elements[3].id;
    if let Some(AwsResource::SqsQueue { topic: source, .. }) =
        &mut elements[3].as_shape_mut().unwrap().infrastructure
    {
        *source = Some(topic);
    }
    elements.push(Element::connector(
        Endpoint::glued(topic, None),
        Endpoint::glued(queue, None),
        parent,
        order,
    ));
    elements
}

pub fn document(target: AwsDeployment) -> Document {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    let data = doc.pages.get_mut(&page).unwrap();
    data.name = "AWS deployment trial".into();
    data.diagram_kind = Some(DiagramKind::Cloud);
    data.deployment = Some(target);
    let parent = Parent::Layer(doc.layers_of(page)[0].id);
    for element in elements(parent, Point::new(300.0, 240.0), OrderKey::first()) {
        doc.elements.insert(element.id, element);
    }
    doc
}
