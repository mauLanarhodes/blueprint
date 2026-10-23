//! Native project files retain canonical container parents and resource identity.

use bp_model::kurbo::Rect;
use bp_model::{AwsResource, Document, Element, NetworkContainer, Parent, ShapeRef};

fn network() -> Document {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    let layer = Parent::Layer(doc.layers_of(page)[0].id);
    let mut vpc = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        layer,
        doc.next_order_key(layer),
        Rect::new(0.0, 0.0, 960.0, 520.0),
    );
    let vpc_id = vpc.id;
    vpc.as_shape_mut().unwrap().network_container = Some(NetworkContainer::Vpc);
    vpc.as_shape_mut().unwrap().infrastructure = Some(AwsResource::Vpc {
        cidr: "10.0.0.0/16".into(),
    });
    vpc.as_shape_mut().unwrap().text = "Production VPC".into();
    doc.elements.insert(vpc_id, vpc);
    let parent = Parent::Element(vpc_id);
    let mut subnet = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        doc.next_order_key(parent),
        Rect::new(20.0, 80.0, 460.0, 440.0),
    );
    let subnet_id = subnet.id;
    let shape = subnet.as_shape_mut().unwrap();
    shape.network_container = Some(NetworkContainer::Subnet);
    shape.text = "Private subnet".into();
    shape.infrastructure = Some(AwsResource::Subnet {
        vpc: Some(vpc_id),
        cidr: "10.0.1.0/24".into(),
        availability_zone: "us-east-1a".into(),
    });
    doc.elements.insert(subnet_id, subnet);
    let parent = Parent::Element(subnet_id);
    let mut service = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        doc.next_order_key(parent),
        Rect::new(60.0, 180.0, 156.0, 276.0),
    );
    service.as_shape_mut().unwrap().text = "Worker".into();
    doc.elements.insert(service.id, service);
    doc
}

#[test]
fn nested_network_json_and_zip_round_trip_without_resource_or_geometry_changes() {
    let doc = network();
    for bytes in [
        bp_io::to_json_bytes(&doc).unwrap(),
        bp_io::to_zip_bytes(&doc).unwrap(),
    ] {
        let loaded = bp_io::from_bytes(&bytes).unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(loaded.validate(), Ok(()));
        let subnet = loaded
            .elements
            .values()
            .find(|element| element.network_container() == Some(NetworkContainer::Subnet))
            .unwrap();
        let Some(AwsResource::Subnet { vpc: Some(vpc), .. }) =
            &subnet.as_shape().unwrap().infrastructure
        else {
            panic!("configured subnet")
        };
        assert_eq!(subnet.parent, Parent::Element(*vpc));
    }
}

#[test]
fn format8_network_shapes_keep_layout_and_configuration_until_explicit_conversion() {
    let mut doc = network();
    let layer = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let mut order = doc.next_order_key(layer);
    for element in doc.elements.values_mut() {
        element.parent = layer;
        element.order = order.clone();
        order = bp_model::OrderKey::after(&order);
        element.as_shape_mut().unwrap().network_container = None;
    }
    let mut old = serde_json::to_value(&doc).unwrap();
    old["schema_version"] = 8.into();
    let loaded = bp_io::from_bytes(&serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(loaded, doc, "migration only updates the format version");
    assert!(
        loaded
            .elements
            .values()
            .all(|element| element.network_container().is_none())
    );
}

#[test]
fn unassigned_subnet_container_remains_loadable_for_later_configuration() {
    let mut doc = network();
    let layer = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
    let subnet = doc
        .elements
        .values_mut()
        .find(|element| element.network_container() == Some(NetworkContainer::Subnet))
        .unwrap();
    subnet.parent = layer;
    subnet.order = bp_model::OrderKey::after(&subnet.order);
    if let Some(AwsResource::Subnet { vpc, .. }) =
        &mut subnet.as_shape_mut().unwrap().infrastructure
    {
        *vpc = None;
    }
    let bytes = bp_io::to_json_bytes(&doc).unwrap();
    assert_eq!(bp_io::from_bytes(&bytes).unwrap(), doc);
}
