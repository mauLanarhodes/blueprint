use bp_commands::{
    Command, History, Prop, edit,
    network::{self, NetworkError},
};
use bp_model::kurbo::{Point, Rect, Vec2};
use bp_model::{
    AwsResource, Document, Element, ElementId, Endpoint, NetworkContainer, OrderKey, Parent,
    ShapeRef,
};

struct Fixture {
    doc: Document,
    history: History,
    vpc: ElementId,
    other_vpc: ElementId,
    subnet: ElementId,
    other_subnet: ElementId,
    service: ElementId,
}

fn insert(
    doc: &mut Document,
    parent: Parent,
    bounds: Rect,
    kind: Option<NetworkContainer>,
    resource: Option<AwsResource>,
) -> ElementId {
    let mut element = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        doc.next_order_key(parent),
        bounds,
    );
    let shape = element.as_shape_mut().unwrap();
    shape.network_container = kind;
    shape.infrastructure = resource;
    let id = element.id;
    doc.elements.insert(id, element);
    id
}

impl Fixture {
    fn new() -> Self {
        let mut doc = Document::new();
        let layer = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
        let vpc = insert(
            &mut doc,
            layer,
            Rect::new(0.0, 0.0, 1000.0, 600.0),
            Some(NetworkContainer::Vpc),
            Some(AwsResource::Vpc {
                cidr: "10.0.0.0/16".into(),
            }),
        );
        let other_vpc = insert(
            &mut doc,
            layer,
            Rect::new(1200.0, 0.0, 2200.0, 600.0),
            Some(NetworkContainer::Vpc),
            Some(AwsResource::Vpc {
                cidr: "10.0.0.0/16".into(),
            }),
        );
        let subnet = insert(
            &mut doc,
            Parent::Element(vpc),
            Rect::new(32.0, 100.0, 432.0, 500.0),
            Some(NetworkContainer::Subnet),
            Some(AwsResource::Subnet {
                vpc: Some(vpc),
                cidr: "10.0.1.0/24".into(),
                availability_zone: "us-east-1a".into(),
            }),
        );
        let other_subnet = insert(
            &mut doc,
            Parent::Element(vpc),
            Rect::new(500.0, 100.0, 900.0, 500.0),
            Some(NetworkContainer::Subnet),
            Some(AwsResource::Subnet {
                vpc: Some(vpc),
                cidr: "10.0.2.0/24".into(),
                availability_zone: "us-east-1b".into(),
            }),
        );
        let service = insert(
            &mut doc,
            layer,
            Rect::new(80.0, 220.0, 144.0, 284.0),
            None,
            None,
        );
        doc.validate().unwrap();
        Self {
            doc,
            history: History::new(),
            vpc,
            other_vpc,
            subnet,
            other_subnet,
            service,
        }
    }
    fn apply(&mut self, commands: Vec<Command>) {
        self.history
            .apply(&mut self.doc, "Network edit", commands)
            .unwrap();
        self.doc.validate().unwrap();
    }
}

#[test]
fn services_attach_transfer_and_detach_without_touching_resource_configuration() {
    let mut f = Fixture::new();
    let before = f.doc.elements[&f.service].clone();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    assert_eq!(f.doc.elements[&f.service].parent, Parent::Element(f.subnet));
    assert_eq!(
        edit::bounds(&f.doc, f.service),
        Some(before.as_shape().unwrap().bounds)
    );
    f.apply(
        network::drop(
            &f.doc,
            &[f.service],
            Some(f.other_subnet),
            Vec2::new(468.0, 0.0),
        )
        .unwrap(),
    );
    assert_eq!(
        f.doc.elements[&f.service].parent,
        Parent::Element(f.other_subnet)
    );
    let after_attach = f.doc.clone();
    f.apply(network::drop(&f.doc, &[f.service], None, Vec2::new(0.0, 600.0)).unwrap());
    assert!(matches!(
        f.doc.elements[&f.service].parent,
        Parent::Layer(_)
    ));
    assert!(f.history.undo(&mut f.doc));
    assert_eq!(f.doc, after_attach);
    assert!(f.history.redo(&mut f.doc));
    assert!(matches!(
        f.doc.elements[&f.service].parent,
        Parent::Layer(_)
    ));
}

#[test]
fn nested_movement_translates_each_descendant_once_and_keeps_connector_endpoints() {
    let mut f = Fixture::new();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    let original = f.doc.clone();
    let connector = Element::connector(
        Endpoint::glued(f.service, None),
        Endpoint::glued(f.other_subnet, None),
        Parent::Element(f.vpc),
        OrderKey::first(),
    );
    let connector_id = connector.id;
    f.doc.elements.insert(connector.id, connector);
    f.apply(
        network::drop(
            &f.doc,
            &[f.vpc, f.subnet, f.service],
            None,
            Vec2::new(40.0, 60.0),
        )
        .unwrap(),
    );
    for id in [f.vpc, f.subnet, f.other_subnet, f.service] {
        assert_eq!(
            edit::bounds(&f.doc, id).unwrap(),
            edit::bounds(&original, id).unwrap() + Vec2::new(40.0, 60.0)
        );
    }
    assert_eq!(
        f.doc.elements[&connector_id]
            .as_connector()
            .unwrap()
            .source
            .element(),
        Some(f.service)
    );
}

#[test]
fn resize_changes_only_boundary_and_enforces_header_minimum_and_contents() {
    let mut f = Fixture::new();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    let service = f.doc.elements[&f.service].clone();
    f.apply(network::resize(&f.doc, f.subnet, Rect::new(32.0, 100.0, 460.0, 540.0)).unwrap());
    assert_eq!(f.doc.elements[&f.service], service);
    assert_eq!(
        network::resize(&f.doc, f.subnet, Rect::new(32.0, 100.0, 200.0, 250.0)),
        Err(NetworkError::TooSmall)
    );
    assert_eq!(
        network::resize(&f.doc, f.subnet, Rect::new(100.0, 100.0, 460.0, 540.0)),
        Err(NetworkError::OutsideContent)
    );
    assert_eq!(
        network::resize(&f.doc, f.subnet, Rect::new(32.0, 100.0, 1200.0, 540.0)),
        Err(NetworkError::OutsideContent)
    );
    f.apply(network::resize(&f.doc, f.vpc, Rect::new(-10.0, -10.0, 1100.0, 650.0)).unwrap());
    assert_eq!(f.doc.elements[&f.service], service);
}

#[test]
fn subnet_transfer_synchronizes_parent_and_resource_in_one_undo_step() {
    let mut f = Fixture::new();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    let original = f.doc.clone();
    f.apply(
        network::drop(
            &f.doc,
            &[f.subnet],
            Some(f.other_vpc),
            Vec2::new(1200.0, 0.0),
        )
        .unwrap(),
    );
    assert_eq!(
        f.doc.elements[&f.subnet].parent,
        Parent::Element(f.other_vpc)
    );
    match f.doc.elements[&f.subnet]
        .as_shape()
        .unwrap()
        .infrastructure
        .as_ref()
        .unwrap()
    {
        AwsResource::Subnet {
            vpc,
            cidr,
            availability_zone,
        } => {
            assert_eq!(*vpc, Some(f.other_vpc));
            assert_eq!(cidr, "10.0.1.0/24");
            assert_eq!(availability_zone, "us-east-1a");
        }
        _ => panic!(),
    }
    let final_doc = f.doc.clone();
    assert!(f.history.undo(&mut f.doc));
    assert_eq!(f.doc, original);
    assert!(f.history.redo(&mut f.doc));
    assert_eq!(f.doc, final_doc);
    assert_eq!(
        network::drop(&f.doc, &[f.subnet], None, Vec2::ZERO),
        Err(NetworkError::AssignedSubnet)
    );
}

#[test]
fn invalid_transfers_reject_cidr_and_overlap_without_changing_anything() {
    let mut f = Fixture::new();
    f.doc
        .elements
        .get_mut(&f.other_vpc)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .infrastructure = Some(AwsResource::Vpc {
        cidr: "172.16.0.0/16".into(),
    });
    let before = f.doc.clone();
    assert_eq!(
        network::drop(
            &f.doc,
            &[f.subnet],
            Some(f.other_vpc),
            Vec2::new(1200.0, 0.0)
        ),
        Err(NetworkError::CidrOutsideVpc)
    );
    assert_eq!(f.doc, before);
    f.doc
        .elements
        .get_mut(&f.other_vpc)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .infrastructure = Some(AwsResource::Vpc {
        cidr: "10.0.0.0/16".into(),
    });
    insert(
        &mut f.doc,
        Parent::Element(f.other_vpc),
        Rect::new(1700.0, 100.0, 2100.0, 500.0),
        Some(NetworkContainer::Subnet),
        Some(AwsResource::Subnet {
            vpc: Some(f.other_vpc),
            cidr: "10.0.1.128/25".into(),
            availability_zone: String::new(),
        }),
    );
    assert_eq!(
        network::drop(
            &f.doc,
            &[f.subnet],
            Some(f.other_vpc),
            Vec2::new(1200.0, 0.0)
        ),
        Err(NetworkError::CidrOverlap)
    );
}

#[test]
fn inspector_reassignment_uses_available_space_and_same_transfer_validation() {
    let mut f = Fixture::new();
    let original = f.doc.clone();
    f.apply(network::reassign_subnet(&f.doc, f.subnet, f.other_vpc).unwrap());
    assert_eq!(
        f.doc.elements[&f.subnet].parent,
        Parent::Element(f.other_vpc)
    );
    assert_eq!(
        edit::bounds(&f.doc, f.subnet).unwrap().origin(),
        Point::new(1216.0, 72.0)
    );
    assert!(f.history.undo(&mut f.doc));
    assert_eq!(f.doc, original);
}

#[test]
fn locked_descendants_hidden_targets_cycles_and_multiselection_are_atomic() {
    let mut f = Fixture::new();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    f.doc.elements.get_mut(&f.service).unwrap().locked = true;
    assert_eq!(
        network::drop(&f.doc, &[f.vpc], None, Vec2::new(20.0, 0.0)),
        Err(NetworkError::Locked)
    );
    assert!(edit::translate(&f.doc, &[f.vpc], Vec2::new(20.0, 0.0)).is_empty());
    f.doc.elements.get_mut(&f.service).unwrap().locked = false;
    assert_eq!(
        network::drop(&f.doc, &[f.subnet], Some(f.service), Vec2::ZERO),
        Err(NetworkError::Cycle)
    );
    f.doc.elements.get_mut(&f.other_subnet).unwrap().locked = true;
    assert_eq!(
        network::drop(
            &f.doc,
            &[f.service],
            Some(f.other_subnet),
            Vec2::new(468.0, 0.0)
        ),
        Err(NetworkError::Locked)
    );
    f.doc.elements.get_mut(&f.other_subnet).unwrap().locked = false;
    let layer = Parent::Layer(f.doc.layer_of(f.service).unwrap());
    let too_wide = insert(
        &mut f.doc,
        layer,
        Rect::new(80.0, 220.0, 600.0, 300.0),
        None,
        None,
    );
    let before = f.doc.clone();
    assert_eq!(
        network::drop(&f.doc, &[f.service, too_wide], Some(f.subnet), Vec2::ZERO),
        Err(NetworkError::OutsideContent)
    );
    assert_eq!(f.doc, before);
    f.doc
        .layers
        .get_mut(&f.doc.layer_of(f.service).unwrap())
        .unwrap()
        .visible = false;
    assert_eq!(
        network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO),
        Err(NetworkError::Hidden)
    );
}

#[test]
fn cancellation_restores_positions_membership_and_history() {
    let mut f = Fixture::new();
    let original = f.doc.clone();
    f.history.begin("Drag service");
    let commands = network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap();
    f.history
        .apply(&mut f.doc, "Drag service", commands)
        .unwrap();
    f.history.cancel(&mut f.doc);
    assert_eq!(f.doc, original);
}

#[test]
fn complete_vpc_copy_remaps_nested_parents_and_configured_references() {
    let mut f = Fixture::new();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    let clip = edit::Clip::copy(&f.doc, &[f.vpc], |_, _| None);
    let parent = Parent::Layer(f.doc.layer_of(f.vpc).unwrap());
    let (roots, commands) = clip.paste(&f.doc, parent, Vec2::new(0.0, 700.0));
    f.apply(commands);
    assert_eq!(roots.len(), 1);
    let copy = roots[0];
    assert_ne!(copy, f.vpc);
    let subnets: Vec<_> = f
        .doc
        .descendants(copy)
        .into_iter()
        .filter(|id| f.doc.elements[id].network_container() == Some(NetworkContainer::Subnet))
        .collect();
    assert_eq!(subnets.len(), 2);
    for id in subnets {
        assert!(
            matches!(f.doc.elements[&id].as_shape().unwrap().infrastructure,Some(AwsResource::Subnet {vpc:Some(parent),..}) if parent==copy)
        );
    }
}

#[test]
fn explicit_conversion_preserves_ids_resources_authored_connectors_and_is_undoable() {
    let mut f = Fixture::new();
    for id in [f.vpc, f.other_vpc, f.subnet, f.other_subnet] {
        f.doc
            .elements
            .get_mut(&id)
            .unwrap()
            .as_shape_mut()
            .unwrap()
            .network_container = None;
    }
    let layer = Parent::Layer(f.doc.layer_of(f.vpc).unwrap());
    for id in [f.subnet, f.other_subnet] {
        f.doc.elements.get_mut(&id).unwrap().parent = layer;
    }
    let connector = Element::connector(
        Endpoint::glued(f.vpc, None),
        Endpoint::glued(f.subnet, None),
        layer,
        OrderKey::first(),
    );
    let cid = connector.id;
    f.doc.elements.insert(cid, connector);
    let original = f.doc.clone();
    let page = f.doc.first_page().unwrap();
    f.apply(network::convert(&f.doc, page).unwrap());
    assert_eq!(f.doc.elements[&f.subnet].parent, Parent::Element(f.vpc));
    for id in [f.vpc, f.other_vpc, f.subnet, f.other_subnet] {
        assert_eq!(
            f.doc.elements[&id].as_shape().unwrap().infrastructure,
            original.elements[&id].as_shape().unwrap().infrastructure
        );
    }
    assert_eq!(f.doc.elements[&cid], original.elements[&cid]);
    assert!(network::convert(&f.doc, page).unwrap().is_empty());
    assert!(f.history.undo(&mut f.doc));
    assert_eq!(f.doc, original);
}

#[test]
fn visual_and_configured_vpc_disagreement_is_invalid_but_legacy_icons_are_readable() {
    let mut f = Fixture::new();
    f.doc.elements.get_mut(&f.subnet).unwrap().parent =
        Parent::Layer(f.doc.layer_of(f.vpc).unwrap());
    assert!(f.doc.validate().is_err());
    f.doc
        .elements
        .get_mut(&f.subnet)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .network_container = None;
    assert!(f.doc.validate().is_ok());
    f.doc
        .elements
        .get_mut(&f.subnet)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .network_container = Some(NetworkContainer::Subnet);
    f.doc
        .elements
        .get_mut(&f.subnet)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .infrastructure = Some(AwsResource::Subnet {
        vpc: None,
        cidr: String::new(),
        availability_zone: String::new(),
    });
    assert!(f.doc.validate().is_ok());
    let commands = vec![Command::Set {
        id: f.subnet,
        prop: Prop::Infrastructure(None),
    }];
    f.apply(commands);
}

#[test]
fn layout_only_subnet_movement_does_not_require_completed_deployment_settings() {
    let mut f = Fixture::new();
    if let Some(AwsResource::Subnet { cidr, .. }) = &mut f
        .doc
        .elements
        .get_mut(&f.subnet)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .infrastructure
    {
        cidr.clear();
    }
    let commands = network::drop(&f.doc, &[f.subnet], Some(f.vpc), Vec2::new(10.0, 10.0)).unwrap();
    assert!(!commands.iter().any(|c| matches!(
        c,
        Command::Set {
            prop: Prop::Infrastructure(_),
            ..
        }
    )));
    f.apply(commands);
}

#[test]
fn cross_page_targets_are_rejected_before_membership_can_change() {
    let mut f = Fixture::new();
    let page = bp_model::Page::new("Other page", OrderKey::first());
    let page_id = page.id;
    f.doc.pages.insert(page.id, page);
    let layer = bp_model::Layer::new(page_id, "Other layer", OrderKey::first());
    let layer_id = layer.id;
    f.doc.layers.insert(layer.id, layer);
    let vpc = insert(
        &mut f.doc,
        Parent::Layer(layer_id),
        Rect::new(0.0, 0.0, 1000.0, 600.0),
        Some(NetworkContainer::Vpc),
        Some(AwsResource::Vpc {
            cidr: "10.0.0.0/16".into(),
        }),
    );
    assert_eq!(
        network::drop(&f.doc, &[f.subnet], Some(vpc), Vec2::ZERO),
        Err(NetworkError::CrossPage)
    );
}

#[test]
fn copying_only_a_subnet_clears_external_vpc_reference_and_keeps_services() {
    let mut f = Fixture::new();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    let clip = edit::Clip::copy(&f.doc, &[f.subnet], |_, _| None);
    let parent = Parent::Layer(f.doc.layer_of(f.subnet).unwrap());
    let (roots, commands) = clip.paste(&f.doc, parent, Vec2::new(0.0, 700.0));
    f.apply(commands);
    assert_eq!(f.doc.descendants(roots[0]).len(), 1);
    assert!(matches!(
        f.doc.elements[&roots[0]].as_shape().unwrap().infrastructure,
        Some(AwsResource::Subnet { vpc: None, .. })
    ));
}

#[test]
fn conversion_preserves_existing_containers_when_adding_a_legacy_network() {
    let mut f = Fixture::new();
    let before = f.doc.elements[&f.vpc].clone();
    let subnet_before = f.doc.elements[&f.subnet].clone();
    f.doc
        .elements
        .get_mut(&f.other_vpc)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .network_container = None;
    let page = f.doc.first_page().unwrap();
    f.apply(network::convert(&f.doc, page).unwrap());
    assert_eq!(f.doc.elements[&f.vpc], before);
    assert_eq!(f.doc.elements[&f.subnet], subnet_before);
}

#[test]
fn cloud_labels_are_included_in_drop_and_boundary_constraints() {
    let mut f = Fixture::new();
    let reference = ShapeRef::new("aws", "label-test@v1");
    f.doc.icons.insert(
        reference.clone(),
        bp_model::CloudIcon {
            reference: reference.clone(),
            name: "Label test".into(),
            provider: bp_model::CloudProvider::Aws,
            category: "Test".into(),
            kind: bp_model::IconKind::Service,
            pack_version: "v1".into(),
            source_path: "test.svg".into(),
            svg: std::sync::Arc::from("<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
        },
    );
    let shape = f
        .doc
        .elements
        .get_mut(&f.service)
        .unwrap()
        .as_shape_mut()
        .unwrap();
    shape.shape = reference;
    shape.text = "A deliberately long cloud service name with multiple wrapped lines".into();
    let footprint = network::occupied_bounds(&f.doc, f.service).unwrap();
    assert!(footprint.height() > 100.0);
    assert_eq!(
        network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::new(0.0, 180.0)),
        Err(NetworkError::OutsideContent)
    );
    assert_eq!(
        network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::new(0.0, -100.0)),
        Err(NetworkError::OutsideContent)
    );
}

#[test]
fn carried_connector_waypoints_obey_locks_outside_the_container_tree() {
    let mut f = Fixture::new();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    let layer = Parent::Layer(f.doc.layer_of(f.vpc).unwrap());
    let mut connector = Element::connector(
        Endpoint::glued(f.service, None),
        Endpoint::glued(f.other_subnet, None),
        layer,
        OrderKey::first(),
    );
    connector.locked = true;
    connector
        .as_connector_mut()
        .unwrap()
        .waypoints
        .push(Point::new(460.0, 300.0));
    f.doc.elements.insert(connector.id, connector);
    assert_eq!(
        network::can_move(&f.doc, &[f.vpc]),
        Err(NetworkError::Locked)
    );
}

#[test]
fn grouping_services_from_different_subnets_cannot_bypass_content_bounds() {
    let mut f = Fixture::new();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO).unwrap());
    let other = insert(
        &mut f.doc,
        Parent::Element(f.other_subnet),
        Rect::new(550.0, 220.0, 614.0, 284.0),
        None,
        None,
    );
    assert!(edit::group(&f.doc, &[f.service, other]).is_none());
    assert!(edit::group(&f.doc, &[f.vpc]).is_none());
}

#[test]
fn overflowing_ordinary_shape_labels_constrain_placement() {
    let mut f = Fixture::new();
    let shape = f
        .doc
        .elements
        .get_mut(&f.service)
        .unwrap()
        .as_shape_mut()
        .unwrap();
    shape.text =
        "A long ordinary service label that overflows the shape with many wrapped lines".into();
    shape.style.font_size = Some(28.0);
    let footprint = network::occupied_bounds(&f.doc, f.service).unwrap();
    assert!(footprint.height() > edit::bounds(&f.doc, f.service).unwrap().height());
    assert_eq!(
        network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO),
        Err(NetworkError::OutsideContent)
    );
}

#[test]
fn legacy_network_resource_icons_cannot_be_dropped_as_services_into_subnets() {
    let mut f = Fixture::new();
    let layer = Parent::Layer(f.doc.layer_of(f.vpc).unwrap());
    let legacy = insert(
        &mut f.doc,
        layer,
        Rect::new(80.0, 220.0, 144.0, 284.0),
        None,
        Some(AwsResource::Vpc {
            cidr: "10.0.0.0/16".into(),
        }),
    );
    assert_eq!(
        network::drop(&f.doc, &[legacy], Some(f.subnet), Vec2::ZERO),
        Err(NetworkError::WrongTarget)
    );
    assert!(network::drop(&f.doc, &[legacy], None, Vec2::new(10.0, 0.0)).is_ok());
    f.doc.elements.get_mut(&legacy).unwrap().parent = Parent::Element(f.subnet);
    assert!(f.doc.validate().is_err());
}

#[test]
fn inspector_placement_skips_occupied_space_and_rejects_a_full_destination() {
    let mut f = Fixture::new();
    insert(
        &mut f.doc,
        Parent::Element(f.other_vpc),
        Rect::new(1216.0, 72.0, 1616.0, 472.0),
        Some(NetworkContainer::Subnet),
        Some(AwsResource::Subnet {
            vpc: Some(f.other_vpc),
            cidr: "10.0.3.0/24".into(),
            availability_zone: String::new(),
        }),
    );
    let at = network::available_position(&f.doc, f.subnet, f.other_vpc).unwrap();
    assert_eq!(at, Point::new(1632.0, 72.0));
    assert_eq!(
        network::available_position(&f.doc, f.subnet, f.other_vpc).unwrap(),
        at
    );
    f.doc
        .elements
        .get_mut(&f.other_vpc)
        .unwrap()
        .as_shape_mut()
        .unwrap()
        .bounds = Rect::new(1200.0, 0.0, 1648.0, 504.0);
    let before = f.doc.clone();
    assert_eq!(
        network::reassign_subnet(&f.doc, f.subnet, f.other_vpc),
        Err(NetworkError::NoSpace)
    );
    assert_eq!(f.doc, before);
}

#[test]
fn empty_container_boundaries_resize_independently() {
    let mut f = Fixture::new();
    let bounds = Rect::new(1200.0, 0.0, 1800.0, 300.0);
    f.apply(network::resize(&f.doc, f.other_vpc, bounds).unwrap());
    assert_eq!(edit::bounds(&f.doc, f.other_vpc), Some(bounds));
    assert_eq!(
        edit::bounds(&f.doc, f.subnet),
        Some(Rect::new(32.0, 100.0, 432.0, 500.0))
    );
}

#[test]
fn conversion_sizes_for_carried_contents_and_keeps_their_relative_positions() {
    let mut f = Fixture::new();
    for id in [f.vpc, f.other_vpc, f.subnet, f.other_subnet] {
        f.doc
            .elements
            .get_mut(&id)
            .unwrap()
            .as_shape_mut()
            .unwrap()
            .network_container = None;
    }
    f.doc.elements.get_mut(&f.service).unwrap().parent = Parent::Element(f.subnet);
    let second = insert(
        &mut f.doc,
        Parent::Element(f.subnet),
        Rect::new(600.0, 400.0, 700.0, 500.0),
        None,
        None,
    );
    let relative = edit::bounds(&f.doc, second).unwrap().origin()
        - edit::bounds(&f.doc, f.service).unwrap().origin();
    let page = f.doc.first_page().unwrap();
    f.apply(network::convert(&f.doc, page).unwrap());
    assert_eq!(
        edit::bounds(&f.doc, second).unwrap().origin()
            - edit::bounds(&f.doc, f.service).unwrap().origin(),
        relative
    );
    assert!(
        edit::bounds(&f.doc, f.subnet).unwrap().width()
            > NetworkContainer::Subnet.default_size().width
    );
    network::validate_layout(&f.doc, &[f.vpc, f.subnet, f.service, second]).unwrap();
}

#[test]
fn smart_table_derived_rows_are_included_in_container_fit() {
    let mut f = Fixture::new();
    let mut table = bp_model::ErdTable::default();
    for i in 0..20 {
        table.columns.push(bp_model::ErdColumn::new(
            format!("field_{i}"),
            "VARCHAR(255)",
            OrderKey::first(),
        ));
    }
    let shape = f
        .doc
        .elements
        .get_mut(&f.service)
        .unwrap()
        .as_shape_mut()
        .unwrap();
    shape.shape = ShapeRef::new("erd", "table");
    shape.erd = Some(table);
    shape.text = "Service records".into();
    let footprint = network::occupied_bounds(&f.doc, f.service).unwrap();
    assert!(footprint.width() >= 220.0 && footprint.height() > 600.0);
    assert_eq!(
        network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::ZERO),
        Err(NetworkError::OutsideContent)
    );
}

#[test]
fn moving_a_group_child_inside_its_subnet_preserves_the_group_and_transfer_changes_membership() {
    let mut f = Fixture::new();
    let group = Element::group(
        Parent::Element(f.subnet),
        f.doc.next_order_key(Parent::Element(f.subnet)),
    );
    let group_id = group.id;
    f.doc.elements.insert(group.id, group);
    f.doc.elements.get_mut(&f.service).unwrap().parent = Parent::Element(group_id);
    f.doc.validate().unwrap();
    f.apply(network::drop(&f.doc, &[f.service], Some(f.subnet), Vec2::new(12.0, 12.0)).unwrap());
    assert_eq!(f.doc.elements[&f.service].parent, Parent::Element(group_id));
    f.apply(
        network::drop(
            &f.doc,
            &[f.service],
            Some(f.other_subnet),
            Vec2::new(468.0, 0.0),
        )
        .unwrap(),
    );
    assert_eq!(
        f.doc.elements[&f.service].parent,
        Parent::Element(f.other_subnet)
    );
    assert!(f.history.undo(&mut f.doc));
    assert_eq!(f.doc.elements[&f.service].parent, Parent::Element(group_id));
}
