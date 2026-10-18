//! Working AWS starting points, shared by the editor and CLI.

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

/// The four-service trial plus a Python worker consuming the selected queue.
pub fn elements_with_lambda(parent: Parent, center: Point, order: OrderKey) -> Vec<Element> {
    let mut elements = elements(parent, center, order);
    let queue = elements[3].id;
    let order = OrderKey::after(&elements.last().unwrap().order);
    let mut function = Element::shape(
        ShapeRef::new("basic", "rounded-rectangle"),
        parent,
        order.clone(),
        Rect::new(
            center.x + 290.0,
            center.y + 35.0,
            center.x + 490.0,
            center.y + 120.0,
        ),
    );
    let mut resource = AwsResourceKind::LambdaFunction.create(function.id);
    if let AwsResource::LambdaFunction { queue: source, .. } = &mut resource {
        *source = Some(queue);
    }
    let shape = function.as_shape_mut().unwrap();
    shape.text = "AWS Lambda · worker".into();
    shape.infrastructure = Some(resource);
    let connector = Element::connector(
        Endpoint::glued(queue, None),
        Endpoint::glued(function.id, None),
        parent,
        OrderKey::after(&order),
    );
    elements.push(function);
    elements.push(connector);
    elements
}

pub fn document(target: AwsDeployment) -> Document {
    document_for(target, false)
}

pub fn document_with_lambda(target: AwsDeployment) -> Document {
    document_for(target, true)
}

fn document_for(target: AwsDeployment, with_lambda: bool) -> Document {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    let data = doc.pages.get_mut(&page).unwrap();
    data.name = "AWS deployment trial".into();
    data.diagram_kind = Some(DiagramKind::Cloud);
    data.deployment = Some(target);
    let parent = Parent::Layer(doc.layers_of(page)[0].id);
    let center = Point::new(300.0, 240.0);
    let elements = if with_lambda {
        elements_with_lambda(parent, center, OrderKey::first())
    } else {
        elements(parent, center, OrderKey::first())
    };
    for element in elements {
        doc.elements.insert(element.id, element);
    }
    doc
}
