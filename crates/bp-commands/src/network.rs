//! Atomic network edits shared by canvas gestures and the deployment inspector.
//! All coordinates are absolute. Planning never changes the supplied document.

use crate::{Command, Prop, edit};
use bp_model::kurbo::{Point, Rect, Vec2};
use bp_model::{
    AwsResource, Document, Element, ElementId, NetworkContainer, OrderKey, PageId, Parent,
    StyleValues, TextAlign, VerticalAlign,
};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum NetworkError {
    #[error("Element no longer exists")]
    MissingElement,
    #[error("The element, an ancestor, or a carried descendant is locked")]
    Locked,
    #[error("Hidden layers cannot receive or move network content")]
    Hidden,
    #[error("This container cannot receive the selected elements")]
    WrongTarget,
    #[error("Network membership must stay on the same page")]
    CrossPage,
    #[error("A container cannot be placed inside itself or its descendants")]
    Cycle,
    #[error("Content must fit inside the container, below its header and padding")]
    OutsideContent,
    #[error("The boundary is smaller than the minimum usable container size")]
    TooSmall,
    #[error("An assigned subnet must stay inside its configured VPC")]
    AssignedSubnet,
    #[error("Enter a valid IPv4 network CIDR with prefix /16 through /28")]
    InvalidCidr,
    #[error("Subnet CIDR is outside this VPC")]
    CidrOutsideVpc,
    #[error("Subnet CIDR overlaps another subnet in this VPC")]
    CidrOverlap,
    #[error("There is no available content space for this subnet")]
    NoSpace,
    #[error("{0}")]
    Invalid(String),
}

fn editable(doc: &Document, id: ElementId) -> Result<(), NetworkError> {
    if !doc.elements.contains_key(&id) {
        return Err(NetworkError::MissingElement);
    }
    if doc.is_locked(id) {
        return Err(NetworkError::Locked);
    }
    if !doc
        .layer_of(id)
        .and_then(|id| doc.layers.get(&id))
        .is_some_and(|layer| layer.visible)
    {
        return Err(NetworkError::Hidden);
    }
    Ok(())
}

/// Reject the whole gesture when moving any carried element would bypass a lock.
pub fn can_move(doc: &Document, ids: &[ElementId]) -> Result<(), NetworkError> {
    if ids.iter().any(|id| !doc.elements.contains_key(id)) {
        return Err(NetworkError::MissingElement);
    }
    let mut moved = HashSet::new();
    for id in edit::top_level(doc, ids) {
        editable(doc, id)?;
        moved.insert(id);
        for descendant in doc.descendants(id) {
            editable(doc, descendant)?;
            moved.insert(descendant);
        }
    }
    for element in doc.elements.values() {
        if let Some(connector) = element.as_connector()
            && !connector.waypoints.is_empty()
            && connector
                .endpoints()
                .iter()
                .all(|endpoint| endpoint.element().is_some_and(|id| moved.contains(&id)))
        {
            editable(doc, element.id)?;
        }
    }
    Ok(())
}

/// Shape bounds plus the external cloud label, using the scene's text metrics.
/// Groups include every descendant, while connector routes remain free to cross boundaries.
pub fn occupied_bounds(doc: &Document, id: ElementId) -> Option<Rect> {
    let element = doc.elements.get(&id)?;
    if let Some(shape) = element.as_shape() {
        if shape.network_container.is_none()
            && let Some(table) = &shape.erd
        {
            return Some(erd_bounds(shape, table));
        }
        let mut bounds = shape.bounds;
        if shape.network_container.is_none() && shape.shape.is_cloud() && !shape.text.is_empty() {
            let style = shape.style.resolve(&StyleValues::default());
            let width = bounds.width().max(120.0);
            let height = bp_text::layout(
                &shape.text,
                bp_text::Face::new(style.bold, style.italic),
                style.font_size,
                Some((width - 12.0).max(1.0)),
            )
            .height;
            let label = Rect::new(
                bounds.center().x - width / 2.0,
                bounds.y1 + 6.0,
                bounds.center().x + width / 2.0,
                bounds.y1 + 6.0 + height + 8.0,
            );
            bounds = bounds.union(label);
        } else if shape.network_container.is_none()
            && shape.erd.is_none()
            && !shape.text.trim().is_empty()
        {
            let def = bp_shapes::Libraries::builtin().resolve(&shape.shape);
            let style = shape.style.resolve(&def.default_style());
            let area = def.text_box(bounds);
            let inner = area.inset((
                -6.0_f64.min(area.width() / 4.0),
                -4.0_f64.min(area.height() / 4.0),
            ));
            let layout = bp_text::layout(
                &shape.text,
                bp_text::Face::new(style.bold, style.italic),
                style.font_size,
                Some(inner.width().max(1.0)),
            );
            let top = match style.vertical_align {
                VerticalAlign::Top => inner.y0,
                VerticalAlign::Middle => inner.center().y - layout.height / 2.0,
                VerticalAlign::Bottom => inner.y1 - layout.height,
            };
            for (i, line) in layout.lines.iter().enumerate() {
                let x = match style.text_align {
                    TextAlign::Left => inner.x0,
                    TextAlign::Center => inner.center().x - line.width / 2.0,
                    TextAlign::Right => inner.x1 - line.width,
                };
                let y = top + i as f64 * layout.line_height;
                bounds = bounds.union(Rect::new(x, y, x + line.width, y + layout.line_height));
            }
        }
        Some(bounds)
    } else if element.is_group() {
        doc.descendants(id)
            .into_iter()
            .filter_map(|id| occupied_bounds(doc, id))
            .reduce(|a, b| a.union(b))
    } else {
        None
    }
}

/// Match the smart table's derived scene bounds, including expanded typed rows.
fn erd_bounds(shape: &bp_model::Shape, table: &bp_model::ErdTable) -> Rect {
    use bp_model::TableDisplay;
    use bp_text::Face;
    let requested = shape.bounds;
    let style = shape.style.resolve(
        &bp_shapes::Libraries::builtin()
            .resolve(&shape.shape)
            .default_style(),
    );
    let columns: Vec<_> = table
        .columns
        .iter()
        .filter(|column| match table.display {
            TableDisplay::All => true,
            TableDisplay::KeysOnly => column.primary_key || column.foreign_key || column.unique,
            TableDisplay::Collapsed => false,
        })
        .collect();
    let mut badge_width = 0.0_f64;
    let mut name_width = 0.0_f64;
    let mut type_width = 0.0_f64;
    for column in &columns {
        let mut badges = Vec::new();
        if column.primary_key {
            badges.push("PK");
        }
        if column.foreign_key {
            badges.push("FK");
        }
        if column.unique {
            badges.push("UK");
        }
        if !column.nullable {
            badges.push("NOT NULL");
        }
        if !badges.is_empty() {
            badge_width = badge_width
                .max(bp_text::measure(&badges.join(" "), Face::Bold, style.font_size * 0.8) + 12.0);
        }
        name_width = name_width.max(
            bp_text::measure(
                &column.name,
                Face::new(column.primary_key || style.bold, style.italic),
                style.font_size,
            ) + 12.0,
        );
        let mut data_type = column.data_type.clone();
        if let Some(default) = &column.default_value {
            data_type.push_str(" = ");
            data_type.push_str(default);
        }
        type_width = type_width.max(
            bp_text::measure(
                &data_type,
                Face::new(style.bold, style.italic),
                style.font_size,
            ) + 12.0,
        );
    }
    let width = requested
        .width()
        .max(220.0)
        .max(bp_text::measure(&shape.text, Face::new(true, style.italic), style.font_size) + 24.0)
        .max(badge_width + name_width + type_width + 32.0);
    let header = (style.font_size * bp_text::LINE_HEIGHT + 14.0).max(34.0);
    let row = (style.font_size * bp_text::LINE_HEIGHT + 12.0).max(28.0);
    let height = if table.display == TableDisplay::Collapsed {
        header
    } else {
        requested.height().max(header + columns.len() as f64 * row)
    };
    Rect::new(
        requested.x0,
        requested.y0,
        requested.x0 + width,
        requested.y0 + height,
    )
}

fn contains(outer: Rect, inner: Rect) -> bool {
    const EPSILON: f64 = 0.001;
    inner.x0 >= outer.x0 - EPSILON
        && inner.y0 >= outer.y0 - EPSILON
        && inner.x1 <= outer.x1 + EPSILON
        && inner.y1 <= outer.y1 + EPSILON
}

pub fn validate_layout(doc: &Document, ids: &[ElementId]) -> Result<(), NetworkError> {
    for &id in ids {
        let element = doc.elements.get(&id).ok_or(NetworkError::MissingElement)?;
        if let Some(kind) = element.network_container() {
            let bounds = element.as_shape().expect("container is shape").bounds;
            let min = kind.min_size();
            if bounds.width() < min.width || bounds.height() < min.height {
                return Err(NetworkError::TooSmall);
            }
        }
        if let Some(occupied) = occupied_bounds(doc, id) {
            for ancestor in doc.ancestors(id) {
                let parent = &doc.elements[&ancestor];
                if let Some(kind) = parent.network_container()
                    && !contains(
                        kind.content_rect(parent.as_shape().expect("container is shape").bounds),
                        occupied,
                    )
                {
                    return Err(NetworkError::OutsideContent);
                }
            }
        }
    }
    Ok(())
}

fn staged(doc: &Document, commands: &[Command]) -> Result<Document, NetworkError> {
    let mut next = doc.clone();
    for command in commands {
        command
            .clone()
            .apply(&mut next)
            .map_err(|e| NetworkError::Invalid(e.to_string()))?;
    }
    next.validate()
        .map_err(|e| NetworkError::Invalid(e.to_string()))?;
    Ok(next)
}

/// Plans one completed drag, including positions, ordering, membership and VPC configuration.
/// `delta` is relative to the original document, never an incremental drag-frame offset.
pub fn drop(
    doc: &Document,
    ids: &[ElementId],
    target: Option<ElementId>,
    delta: Vec2,
) -> Result<Vec<Command>, NetworkError> {
    if !delta.x.is_finite() || !delta.y.is_finite() {
        return Err(NetworkError::Invalid("Position must be finite".into()));
    }
    can_move(doc, ids)?;
    let roots = edit::top_level(doc, ids);
    if let Some(target) = target {
        editable(doc, target)?;
        if roots
            .iter()
            .any(|id| *id == target || doc.ancestors(target).contains(id))
        {
            return Err(NetworkError::Cycle);
        }
        if roots
            .iter()
            .any(|id| doc.page_of(*id) != doc.page_of(target))
        {
            return Err(NetworkError::CrossPage);
        }
    }
    let mut commands = edit::translate(doc, &roots, delta);
    let mut next_order = target.map(|id| doc.next_order_key(Parent::Element(id)));
    for &id in &roots {
        let element = &doc.elements[&id];
        let new_parent = match (element.network_container(), target) {
            (Some(NetworkContainer::Vpc), None) => element.parent,
            (Some(NetworkContainer::Vpc), Some(_)) => return Err(NetworkError::WrongTarget),
            (Some(NetworkContainer::Subnet), Some(vpc)) => {
                if doc.elements[&vpc].network_container() != Some(NetworkContainer::Vpc) {
                    return Err(NetworkError::WrongTarget);
                }
                if let Some(AwsResource::Subnet { .. }) =
                    element.as_shape().and_then(|s| s.infrastructure.as_ref())
                {
                    if !matches!(
                        doc.elements[&vpc]
                            .as_shape()
                            .and_then(|s| s.infrastructure.as_ref()),
                        Some(AwsResource::Vpc { .. })
                    ) {
                        return Err(NetworkError::WrongTarget);
                    }
                    let mut resource = element
                        .as_shape()
                        .expect("container shape")
                        .infrastructure
                        .clone()
                        .expect("checked resource");
                    if let AwsResource::Subnet {
                        vpc: configured, ..
                    } = &mut resource
                    {
                        *configured = Some(vpc);
                    }
                    if element.as_shape().and_then(|s| s.infrastructure.as_ref()) != Some(&resource)
                    {
                        commands.push(Command::Set {
                            id,
                            prop: Prop::Infrastructure(Some(resource)),
                        });
                    }
                }
                Parent::Element(vpc)
            }
            (Some(NetworkContainer::Subnet), None) => {
                if matches!(
                    element.as_shape().and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::Subnet { vpc: Some(_), .. })
                ) {
                    return Err(NetworkError::AssignedSubnet);
                }
                Parent::Layer(doc.layer_of(id).ok_or(NetworkError::MissingElement)?)
            }
            (None, Some(subnet)) => {
                if doc.elements[&subnet].network_container() != Some(NetworkContainer::Subnet)
                    || std::iter::once(id).chain(doc.descendants(id)).any(|id| {
                        let element = &doc.elements[&id];
                        element.network_container().is_some()
                            || matches!(
                                element.as_shape().and_then(|s| s.infrastructure.as_ref()),
                                Some(AwsResource::Vpc { .. } | AwsResource::Subnet { .. })
                            )
                    })
                {
                    return Err(NetworkError::WrongTarget);
                }
                if doc.ancestors(id).contains(&subnet) {
                    element.parent
                } else {
                    Parent::Element(subnet)
                }
            }
            (None, None) => {
                if doc
                    .ancestors(id)
                    .iter()
                    .any(|id| doc.elements[id].network_container().is_some())
                {
                    Parent::Layer(doc.layer_of(id).ok_or(NetworkError::MissingElement)?)
                } else {
                    element.parent
                }
            }
        };
        if element.parent != new_parent {
            commands.push(Command::Set {
                id,
                prop: Prop::Parent(new_parent),
            });
            let order = next_order
                .take()
                .unwrap_or_else(|| doc.next_order_key(new_parent));
            next_order = Some(OrderKey::after(&order));
            commands.push(Command::Set {
                id,
                prop: Prop::Order(order),
            });
        }
    }
    let next = staged(doc, &commands)?;
    let mut moved = Vec::new();
    for &id in &roots {
        moved.push(id);
        moved.extend(next.descendants(id));
    }
    validate_layout(&next, &moved)?;
    for &id in &roots {
        if next.elements[&id].network_container() == Some(NetworkContainer::Subnet)
            && let Some(AwsResource::Subnet { vpc: Some(vpc), .. }) = next.elements[&id].as_shape().and_then(|s| s.infrastructure.as_ref())
            && doc.elements[&id].as_shape().and_then(|s| s.infrastructure.as_ref())
                .is_some_and(|resource| matches!(resource, AwsResource::Subnet { vpc: previous, .. } if *previous != Some(*vpc)))
        { validate_subnet_vpc(&next, id, *vpc)?; }
    }
    Ok(commands)
}

/// IPv4 CIDRs are canonical networks; silently masking host bits would change intent.
fn cidr(value: &str) -> Result<(u32, u32), NetworkError> {
    let (address, prefix) = value
        .trim()
        .split_once('/')
        .ok_or(NetworkError::InvalidCidr)?;
    let address = address
        .parse::<std::net::Ipv4Addr>()
        .map_err(|_| NetworkError::InvalidCidr)?;
    let prefix: u32 = prefix.parse().map_err(|_| NetworkError::InvalidCidr)?;
    if !(16..=28).contains(&prefix) {
        return Err(NetworkError::InvalidCidr);
    }
    let address = u32::from(address);
    let mask = u32::MAX << (32 - prefix);
    if address & mask != address {
        return Err(NetworkError::InvalidCidr);
    }
    Ok((address, address | !mask))
}

pub fn validate_subnet_vpc(
    doc: &Document,
    subnet: ElementId,
    vpc: ElementId,
) -> Result<(), NetworkError> {
    editable(doc, vpc)?;
    if doc.page_of(subnet) != doc.page_of(vpc) {
        return Err(NetworkError::CrossPage);
    }
    let Some(AwsResource::Vpc { cidr: vpc_cidr }) = doc
        .elements
        .get(&vpc)
        .and_then(Element::as_shape)
        .and_then(|s| s.infrastructure.as_ref())
    else {
        return Err(NetworkError::WrongTarget);
    };
    let Some(AwsResource::Subnet {
        cidr: subnet_cidr, ..
    }) = doc
        .elements
        .get(&subnet)
        .and_then(Element::as_shape)
        .and_then(|s| s.infrastructure.as_ref())
    else {
        return Err(NetworkError::WrongTarget);
    };
    let (vpc_start, vpc_end) = cidr(vpc_cidr)?;
    let (start, end) = cidr(subnet_cidr)?;
    if start < vpc_start || end > vpc_end {
        return Err(NetworkError::CidrOutsideVpc);
    }
    for sibling in doc
        .elements
        .values()
        .filter(|e| e.id != subnet && doc.page_of(e.id) == doc.page_of(subnet))
    {
        if let Some(AwsResource::Subnet {
            vpc: Some(parent),
            cidr: other,
            ..
        }) = sibling.as_shape().and_then(|s| s.infrastructure.as_ref())
            && *parent == vpc
        {
            let (other_start, other_end) = cidr(other)?;
            if start <= other_end && other_start <= end {
                return Err(NetworkError::CidrOverlap);
            }
        }
    }
    Ok(())
}

/// Returns the first available position in row-major order. Existing coordinates are preferred.
pub fn available_position(
    doc: &Document,
    id: ElementId,
    target: ElementId,
) -> Result<Point, NetworkError> {
    let element = doc.elements.get(&id).ok_or(NetworkError::MissingElement)?;
    let bounds = edit::bounds(doc, id).ok_or(NetworkError::WrongTarget)?;
    let target_element = doc
        .elements
        .get(&target)
        .ok_or(NetworkError::MissingElement)?;
    let kind = target_element
        .network_container()
        .ok_or(NetworkError::WrongTarget)?;
    let content = kind.content_rect(
        target_element
            .as_shape()
            .ok_or(NetworkError::WrongTarget)?
            .bounds,
    );
    let occupied = occupied_bounds(doc, id).ok_or(NetworkError::WrongTarget)?;
    let children: Vec<_> = doc
        .tree()
        .children(Parent::Element(target))
        .iter()
        .filter(|e| e.id != element.id)
        .filter_map(|e| occupied_bounds(doc, e.id))
        .collect();
    let valid = |at: Point| {
        let rect = occupied + (at - bounds.origin());
        contains(content, rect)
            && children
                .iter()
                .all(|other| rect.intersect(other.inflate(16.0, 16.0)).area() <= 0.0)
    };
    if valid(bounds.origin()) {
        return Ok(bounds.origin());
    }
    let mut xs = vec![content.x0 + bounds.x0 - occupied.x0];
    let mut ys = vec![content.y0 + bounds.y0 - occupied.y0];
    for child in &children {
        xs.push(child.x1 + 16.0 + bounds.x0 - occupied.x0);
        ys.push(child.y1 + 16.0 + bounds.y0 - occupied.y0);
    }
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    for y in ys {
        for &x in &xs {
            let at = Point::new(x, y);
            if valid(at) {
                return Ok(at);
            }
        }
    }
    Err(NetworkError::NoSpace)
}

pub fn reassign_subnet(
    doc: &Document,
    id: ElementId,
    vpc: ElementId,
) -> Result<Vec<Command>, NetworkError> {
    if doc.elements.get(&id).and_then(Element::network_container) != Some(NetworkContainer::Subnet)
    {
        return Err(NetworkError::WrongTarget);
    }
    validate_subnet_vpc(doc, id, vpc)?;
    let at = available_position(doc, id, vpc)?;
    let from = edit::bounds(doc, id)
        .ok_or(NetworkError::MissingElement)?
        .origin();
    drop(doc, &[id], Some(vpc), at - from)
}

/// Checks any shape's new bounds against its containing boundaries.
pub fn validate_bounds(doc: &Document, id: ElementId, bounds: Rect) -> Result<(), NetworkError> {
    editable(doc, id)?;
    let next = staged(
        doc,
        &[Command::Set {
            id,
            prop: Prop::Bounds(bounds),
        }],
    )?;
    let mut ids = vec![id];
    ids.extend(next.descendants(id));
    validate_layout(&next, &ids)
}

/// Resizes only the boundary. Descendants and connector geometry are untouched.
pub fn resize(doc: &Document, id: ElementId, bounds: Rect) -> Result<Vec<Command>, NetworkError> {
    if doc
        .elements
        .get(&id)
        .and_then(Element::network_container)
        .is_none()
    {
        return Err(NetworkError::WrongTarget);
    }
    validate_bounds(doc, id, bounds)?;
    Ok(vec![Command::Set {
        id,
        prop: Prop::Bounds(bounds),
    }])
}

/// Explicit conversion uses configured references and preserves every resource ID and connector.
/// Already converted networks retain their authored layout.
pub fn convert(doc: &Document, page: PageId) -> Result<Vec<Command>, NetworkError> {
    fn push(
        next: &mut Document,
        out: &mut Vec<Command>,
        command: Command,
    ) -> Result<(), NetworkError> {
        command
            .clone()
            .apply(next)
            .map_err(|e| NetworkError::Invalid(e.to_string()))?;
        out.push(command);
        Ok(())
    }
    fn size(doc: &Document, id: ElementId) -> bp_model::kurbo::Size {
        let default = NetworkContainer::Subnet.default_size();
        let children = doc
            .descendants(id)
            .into_iter()
            .filter_map(|id| occupied_bounds(doc, id))
            .reduce(|a, b| a.union(b));
        children.map_or(default, |r| {
            bp_model::kurbo::Size::new(
                default.width.max(r.width() + 32.0),
                default.height.max(r.height() + 88.0),
            )
        })
    }
    fn place_subnet(
        doc: &Document,
        next: &mut Document,
        out: &mut Vec<Command>,
        id: ElementId,
        at: Point,
        size: bp_model::kurbo::Size,
        vpc: Option<ElementId>,
    ) -> Result<(), NetworkError> {
        let descendants = doc.descendants(id);
        if let Some(children) = descendants
            .iter()
            .filter_map(|id| occupied_bounds(doc, *id))
            .reduce(|a, b| a.union(b))
        {
            let delta = Point::new(at.x + 16.0, at.y + 72.0) - children.origin();
            for command in edit::translate(doc, &descendants, delta) {
                push(next, out, command)?;
            }
        }
        push(
            next,
            out,
            Command::Set {
                id,
                prop: Prop::Bounds(Rect::from_origin_size(at, size)),
            },
        )?;
        let parent = vpc.map_or_else(
            || {
                doc.layer_of(id)
                    .map(Parent::Layer)
                    .ok_or(NetworkError::MissingElement)
            },
            |id| Ok(Parent::Element(id)),
        )?;
        push(
            next,
            out,
            Command::Set {
                id,
                prop: Prop::Parent(parent),
            },
        )?;
        Ok(())
    }
    let resources: Vec<_> = doc
        .page_elements(page)
        .into_iter()
        .filter(|e| {
            e.as_shape().is_some_and(|s| {
                matches!(
                    s.infrastructure,
                    Some(AwsResource::Vpc { .. } | AwsResource::Subnet { .. })
                )
            })
        })
        .map(|e| e.id)
        .collect();
    let legacy: Vec<_> = resources
        .iter()
        .copied()
        .filter(|id| doc.elements[id].network_container().is_none())
        .collect();
    if legacy.is_empty() {
        return Ok(Vec::new());
    }
    can_move(doc, &legacy)?;
    let mut next = doc.clone();
    let mut commands = Vec::new();
    for &id in &legacy {
        let kind = match doc.elements[&id]
            .as_shape()
            .and_then(|s| s.infrastructure.as_ref())
        {
            Some(AwsResource::Vpc { .. }) => NetworkContainer::Vpc,
            _ => NetworkContainer::Subnet,
        };
        push(
            &mut next,
            &mut commands,
            Command::Set {
                id,
                prop: Prop::NetworkContainer(Some(kind)),
            },
        )?;
    }
    let vpcs: Vec<_> = resources
        .iter()
        .copied()
        .filter(|id| {
            matches!(
                doc.elements[id]
                    .as_shape()
                    .and_then(|s| s.infrastructure.as_ref()),
                Some(AwsResource::Vpc { .. })
            )
        })
        .collect();
    let start = legacy
        .iter()
        .filter_map(|id| edit::bounds(doc, *id))
        .reduce(|a, b| a.union(b))
        .map_or(Point::ORIGIN, |r| r.origin());
    let mut vpc_x = vpcs
        .iter()
        .filter(|id| doc.elements[id].network_container().is_some())
        .filter_map(|id| edit::bounds(doc, *id))
        .map(|r| r.x1 + 64.0)
        .fold(start.x, f64::max);
    for vpc in vpcs {
        let subnets: Vec<_>=resources.iter().copied().filter(|id|matches!(doc.elements[id].as_shape().and_then(|s|s.infrastructure.as_ref()),Some(AwsResource::Subnet {vpc:Some(parent),..}) if *parent==vpc)).collect();
        if doc.elements[&vpc].network_container().is_some() {
            // Insert only the legacy subnet into this existing boundary, or fail atomically.
            for id in subnets.into_iter().filter(|id| legacy.contains(id)) {
                editable(doc, vpc)?;
                let bounds = doc.elements[&id].as_shape().expect("resource shape").bounds;
                let size = size(doc, id);
                push(
                    &mut next,
                    &mut commands,
                    Command::Set {
                        id,
                        prop: Prop::Bounds(Rect::from_origin_size(bounds.origin(), size)),
                    },
                )?;
                let at = available_position(&next, id, vpc)?;
                place_subnet(doc, &mut next, &mut commands, id, at, size, Some(vpc))?;
            }
            continue;
        }
        let columns = subnets.len().clamp(1, 2);
        let rows = subnets.len().div_ceil(2).max(1);
        let subnet_size = subnets
            .iter()
            .map(|id| size(doc, *id))
            .fold(NetworkContainer::Subnet.default_size(), |a, b| {
                bp_model::kurbo::Size::new(a.width.max(b.width), a.height.max(b.height))
            });
        let vpc_bounds = Rect::from_origin_size(
            Point::new(vpc_x, start.y),
            bp_model::kurbo::Size::new(
                (subnet_size.width + 16.0) * columns as f64 + 16.0,
                (subnet_size.height + 16.0) * rows as f64 + 72.0,
            ),
        );
        push(
            &mut next,
            &mut commands,
            Command::Set {
                id: vpc,
                prop: Prop::Bounds(vpc_bounds),
            },
        )?;
        if matches!(doc.elements[&vpc].parent, Parent::Element(_)) {
            push(
                &mut next,
                &mut commands,
                Command::Set {
                    id: vpc,
                    prop: Prop::Parent(Parent::Layer(
                        doc.layer_of(vpc).ok_or(NetworkError::MissingElement)?,
                    )),
                },
            )?;
        }
        for (i, id) in subnets.into_iter().enumerate() {
            let at = Point::new(
                vpc_x + 16.0 + (i % 2) as f64 * (subnet_size.width + 16.0),
                start.y + 72.0 + (i / 2) as f64 * (subnet_size.height + 16.0),
            );
            place_subnet(
                doc,
                &mut next,
                &mut commands,
                id,
                at,
                subnet_size,
                Some(vpc),
            )?;
        }
        vpc_x = vpc_bounds.x1 + 64.0;
    }
    for &id in &legacy {
        if matches!(
            doc.elements[&id]
                .as_shape()
                .and_then(|s| s.infrastructure.as_ref()),
            Some(AwsResource::Subnet { vpc: None, .. })
        ) {
            let at = doc.elements[&id]
                .as_shape()
                .expect("resource shape")
                .bounds
                .origin();
            place_subnet(doc, &mut next, &mut commands, id, at, size(doc, id), None)?;
        }
    }
    next.validate()
        .map_err(|e| NetworkError::Invalid(e.to_string()))?;
    let all: Vec<_> = legacy
        .iter()
        .flat_map(|id| std::iter::once(*id).chain(next.descendants(*id)))
        .collect();
    validate_layout(&next, &all)?;
    Ok(commands)
}
