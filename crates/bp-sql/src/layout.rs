//! Deterministic relationship-aware placement, shared by imports and Arrange.

use bp_commands::{Command, Prop};
use bp_model::kurbo::{Rect, Vec2};
use bp_model::{Document, ElementId, Endpoint, PageId, PortId, Routing};
use bp_scene::shape_geometry;
use bp_shapes::Libraries;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const HORIZONTAL_GAP: f64 = 160.0;
const VERTICAL_GAP: f64 = 96.0;
const COMPONENT_GAP: f64 = 160.0;
const MARGIN: f64 = 40.0;

/// Arrange visible ERD tables and their editable relationships as one command
/// batch. Locked tables retain their positions; hidden layers are untouched.
/// The caller applies the batch with a single `History::apply`.
pub fn arrange_page(doc: &Document, page: PageId) -> Result<Vec<Command>, String> {
    arrange(doc, page, None)
}

pub(crate) fn arrange_import(
    doc: &Document,
    page: PageId,
    tables: &BTreeSet<ElementId>,
) -> Result<Vec<Command>, String> {
    arrange(doc, page, Some(tables))
}

#[derive(Clone)]
struct TableNode {
    id: ElementId,
    bounds: Rect,
    movable: bool,
}

fn arrange(
    doc: &Document,
    page: PageId,
    subset: Option<&BTreeSet<ElementId>>,
) -> Result<Vec<Command>, String> {
    if !doc.pages.contains_key(&page) {
        return Err(format!("Page {page} does not exist"));
    }
    let mut visible = doc.paint_order(page);
    if let Some(tables) = subset {
        // A chosen import layer may be hidden. New elements still need valid
        // placement for when that layer is shown; existing hidden elements
        // remain outside layout and are never edited.
        let included: BTreeSet<_> = visible.iter().map(|element| element.id).collect();
        visible.extend(doc.elements.values().filter(|element| {
            !included.contains(&element.id)
                && doc.page_of(element.id) == Some(page)
                && (tables.contains(&element.id)
                    || element.as_connector().is_some_and(|connector| {
                        connector.endpoints().iter().all(|endpoint| {
                            endpoint.element().is_some_and(|id| tables.contains(&id))
                        })
                    }))
        }));
    }
    let mut elements: Vec<_> = visible
        .iter()
        .copied()
        .filter(|element| {
            element.as_shape().is_some_and(|shape| shape.erd.is_some())
                && subset.is_none_or(|tables| tables.contains(&element.id))
        })
        .collect();
    // Decoded SQL names give the same ordering regardless of hash iteration or
    // import order. IDs break ties only for genuinely duplicate diagram names.
    elements.sort_by(|a, b| {
        let left = a.as_shape().unwrap();
        let right = b.as_shape().unwrap();
        (&left.erd.as_ref().unwrap().schema, &left.text)
            .cmp(&(&right.erd.as_ref().unwrap().schema, &right.text))
            .then_with(|| a.order.cmp(&b.order))
            .then_with(|| a.id.cmp(&b.id))
    });
    let nodes: Vec<_> = elements
        .iter()
        .map(|element| TableNode {
            id: element.id,
            bounds: shape_geometry(Libraries::builtin(), element.as_shape().unwrap()).bounds,
            movable: subset.is_some() || !doc.is_locked(element.id),
        })
        .collect();
    if !nodes.iter().any(|node| node.movable) {
        return Ok(Vec::new());
    }
    let indices: BTreeMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id, i))
        .collect();
    let mut graph = vec![BTreeSet::new(); nodes.len()];
    for element in &visible {
        let Some(connector) = element.as_connector() else {
            continue;
        };
        let direction = connector
            .foreign_key_endpoints()
            .and_then(|(owner, referenced)| Some((owner.element()?, referenced.element()?)))
            .or_else(|| {
                crate::export::infer_foreign_key(doc, &connector.source, &connector.target)
                    .map(|(owner, _, referenced, _)| (owner, referenced))
            })
            .or_else(|| Some((connector.source.element()?, connector.target.element()?)));
        let Some((owner, referenced)) = direction else {
            continue;
        };
        if let (Some(child), Some(parent)) = (indices.get(&owner), indices.get(&referenced)) {
            graph[*parent].insert(*child);
        }
    }
    let components = weak_components(&graph);
    let blocks: Vec<_> = components
        .iter()
        .map(|component| component_layout(component, &graph, &nodes))
        .collect();
    let fixed: Vec<_> = visible
        .iter()
        .filter(|element| {
            indices
                .get(&element.id)
                .is_none_or(|index| !nodes[*index].movable)
        })
        .filter_map(|element| element.as_shape())
        .map(|shape| shape_geometry(Libraries::builtin(), shape).bounds)
        .collect();
    let origin_x = if subset.is_some() {
        fixed
            .iter()
            .map(|rect| rect.x1 + HORIZONTAL_GAP)
            .fold(MARGIN, f64::max)
    } else {
        MARGIN
    };
    // Pack disconnected components on shelves, retaining the left-to-right
    // dependency structure within each component.
    let total_area: f64 = blocks
        .iter()
        .map(|block| (block.width + COMPONENT_GAP) * (block.height + COMPONENT_GAP))
        .sum();
    let shelf_width = blocks
        .iter()
        .map(|block| block.width)
        .fold(total_area.sqrt() * 1.35, f64::max);
    let mut occupied = fixed;
    let mut proposed = BTreeMap::new();
    let mut shelf_x = 0.0;
    let mut shelf_y = MARGIN;
    let mut shelf_height = 0.0_f64;
    for block in blocks {
        if shelf_x > 0.0 && shelf_x + block.width > shelf_width {
            shelf_x = 0.0;
            shelf_y += shelf_height + COMPONENT_GAP;
            shelf_height = 0.0;
        }
        // Move an entire component below fixed obstacles rather than shifting
        // individual rows, so its crossing-reduction ordering is preserved.
        let mut offset = Vec2::new(origin_x + shelf_x, shelf_y);
        loop {
            let mut shift = 0.0_f64;
            for (&index, rect) in &block.positions {
                if !nodes[index].movable {
                    continue;
                }
                let positioned = *rect + offset;
                for obstacle in &occupied {
                    if overlaps(positioned, obstacle.inflate(MARGIN, MARGIN)) {
                        shift = shift.max(obstacle.y1 + VERTICAL_GAP - positioned.y0);
                    }
                }
            }
            if shift <= 0.0 {
                break;
            }
            offset.y += shift;
        }
        for (&index, rect) in &block.positions {
            if nodes[index].movable {
                let bounds = *rect + offset;
                proposed.insert(nodes[index].id, bounds);
                occupied.push(bounds);
            }
        }
        shelf_x += block.width + COMPONENT_GAP;
        shelf_height = shelf_height.max(offset.y - shelf_y + block.height);
    }
    let mut commands = Vec::new();
    for node in &nodes {
        if let Some(bounds) = proposed.get(&node.id)
            && doc.elements[&node.id].as_shape().unwrap().bounds != *bounds
        {
            commands.push(Command::Set {
                id: node.id,
                prop: Prop::Bounds(*bounds),
            });
        }
    }
    let mut connectors: Vec<_> = visible
        .iter()
        .copied()
        .filter(|element| {
            element.as_connector().is_some() && (subset.is_some() || !doc.is_locked(element.id))
        })
        .collect();
    connectors.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
    for element in connectors {
        let connector = element.as_connector().unwrap();
        let (Some(source), Some(target)) = (connector.source.element(), connector.target.element())
        else {
            continue;
        };
        if !indices.contains_key(&source)
            || !indices.contains_key(&target)
            || (!proposed.contains_key(&source) && !proposed.contains_key(&target))
        {
            continue;
        }
        let source_bounds = proposed
            .get(&source)
            .copied()
            .unwrap_or(nodes[indices[&source]].bounds);
        let target_bounds = proposed
            .get(&target)
            .copied()
            .unwrap_or(nodes[indices[&target]].bounds);
        let dx = target_bounds.center().x - source_bounds.center().x;
        let same_column = dx.abs() < HORIZONTAL_GAP / 2.0;
        let source_left = !same_column && dx < 0.0;
        let target_left = !same_column && dx > 0.0;
        let new_source = facing_endpoint(&connector.source, source_left);
        let new_target = facing_endpoint(&connector.target, target_left);
        if connector.source != new_source {
            commands.push(Command::Set {
                id: element.id,
                prop: Prop::Source(new_source),
            });
        }
        if connector.target != new_target {
            commands.push(Command::Set {
                id: element.id,
                prop: Prop::Target(new_target),
            });
        }
        if !connector.waypoints.is_empty() {
            commands.push(Command::Set {
                id: element.id,
                prop: Prop::Waypoints(Vec::new()),
            });
        }
        if connector.routing != Routing::Orthogonal {
            commands.push(Command::Set {
                id: element.id,
                prop: Prop::Routing(Routing::Orthogonal),
            });
        }
    }
    Ok(commands)
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

fn facing_endpoint(endpoint: &Endpoint, left: bool) -> Endpoint {
    let Endpoint::Glued { element, port } = endpoint else {
        return endpoint.clone();
    };
    Endpoint::Glued {
        element: *element,
        port: Some(port.as_ref().and_then(PortId::column_id).map_or_else(
            || PortId::new(if left { "w" } else { "e" }),
            |id| PortId::column(id, left),
        )),
    }
}

fn weak_components(graph: &[BTreeSet<usize>]) -> Vec<Vec<usize>> {
    let mut neighbors = graph.to_vec();
    for (source, targets) in graph.iter().enumerate() {
        for &target in targets {
            neighbors[target].insert(source);
        }
    }
    let mut visited = vec![false; graph.len()];
    let mut components = Vec::new();
    for first in 0..graph.len() {
        if visited[first] {
            continue;
        }
        visited[first] = true;
        let mut pending = VecDeque::from([first]);
        let mut component = Vec::new();
        while let Some(node) = pending.pop_front() {
            component.push(node);
            for &next in &neighbors[node] {
                if !visited[next] {
                    visited[next] = true;
                    pending.push_back(next);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components
}

struct Block {
    positions: BTreeMap<usize, Rect>,
    width: f64,
    height: f64,
}

fn component_layout(component: &[usize], graph: &[BTreeSet<usize>], nodes: &[TableNode]) -> Block {
    let groups = strongly_connected(component, graph);
    let mut group_of = vec![usize::MAX; nodes.len()];
    for (group, members) in groups.iter().enumerate() {
        for &member in members {
            group_of[member] = group;
        }
    }
    let mut children = vec![BTreeSet::new(); groups.len()];
    let mut parents = children.clone();
    for &source in component {
        for &target in &graph[source] {
            if group_of[source] != group_of[target] {
                children[group_of[source]].insert(group_of[target]);
                parents[group_of[target]].insert(group_of[source]);
            }
        }
    }
    let mut indegree: Vec<_> = parents.iter().map(BTreeSet::len).collect();
    let mut pending: BTreeSet<_> = (0..groups.len()).filter(|&g| indegree[g] == 0).collect();
    let mut rank = vec![0; groups.len()];
    while let Some(group) = pending.pop_first() {
        for &child in &children[group] {
            rank[child] = rank[child].max(rank[group] + 1);
            indegree[child] -= 1;
            if indegree[child] == 0 {
                pending.insert(child);
            }
        }
    }
    let mut layers = vec![Vec::new(); rank.iter().copied().max().unwrap_or(0) + 1];
    for (group, &layer) in rank.iter().enumerate() {
        layers[layer].push(group);
    }
    // Alternating stable barycentric sweeps keep cycles together and reduce
    // crossings between dependency columns without random optimization.
    for _ in 0..4 {
        sweep(&mut layers, &parents, true);
        sweep(&mut layers, &children, false);
    }
    let layer_heights: Vec<f64> = layers
        .iter()
        .map(|layer| {
            let members: Vec<_> = layer.iter().flat_map(|&g| &groups[g]).copied().collect();
            members
                .iter()
                .map(|&i| nodes[i].bounds.height())
                .sum::<f64>()
                + VERTICAL_GAP * members.len().saturating_sub(1) as f64
        })
        .collect();
    let height = layer_heights.iter().copied().fold(0.0, f64::max);
    let mut x = 0.0;
    let mut positions = BTreeMap::new();
    for (layer, &layer_height) in layers.iter().zip(&layer_heights) {
        let mut y = (height - layer_height) / 2.0;
        let mut width = 0.0_f64;
        for &group in layer {
            for &member in &groups[group] {
                let bounds = nodes[member].bounds;
                positions.insert(
                    member,
                    Rect::new(x, y, x + bounds.width(), y + bounds.height()),
                );
                width = width.max(bounds.width());
                y += bounds.height() + VERTICAL_GAP;
            }
        }
        x += width + HORIZONTAL_GAP;
    }
    Block {
        positions,
        width: (x - HORIZONTAL_GAP).max(0.0),
        height,
    }
}

fn sweep(layers: &mut [Vec<usize>], neighbors: &[BTreeSet<usize>], forward: bool) {
    let sequence: Vec<_> = if forward {
        (1..layers.len()).collect()
    } else {
        (0..layers.len().saturating_sub(1)).rev().collect()
    };
    let mut positions = vec![0.0; neighbors.len()];
    for layer in layers.iter() {
        for (index, &group) in layer.iter().enumerate() {
            positions[group] = (index as f64 + 0.5) / layer.len() as f64;
        }
    }
    for layer in sequence {
        layers[layer].sort_by(|&left, &right| {
            let center = |group: usize| {
                if neighbors[group].is_empty() {
                    positions[group]
                } else {
                    neighbors[group].iter().map(|&g| positions[g]).sum::<f64>()
                        / neighbors[group].len() as f64
                }
            };
            center(left)
                .total_cmp(&center(right))
                .then_with(|| left.cmp(&right))
        });
        for (index, &group) in layers[layer].iter().enumerate() {
            positions[group] = (index as f64 + 0.5) / layers[layer].len() as f64;
        }
    }
}

// Iterative Kosaraju avoids recursion limits on large imported schemas.
fn strongly_connected(component: &[usize], graph: &[BTreeSet<usize>]) -> Vec<Vec<usize>> {
    let mut visited = vec![false; graph.len()];
    let mut finished = Vec::new();
    for &first in component {
        if visited[first] {
            continue;
        }
        let mut pending = vec![(first, false)];
        while let Some((node, exiting)) = pending.pop() {
            if exiting {
                finished.push(node);
            } else if !visited[node] {
                visited[node] = true;
                pending.push((node, true));
                pending.extend(graph[node].iter().rev().map(|&next| (next, false)));
            }
        }
    }
    let mut reverse = vec![BTreeSet::new(); graph.len()];
    for &source in component {
        for &target in &graph[source] {
            reverse[target].insert(source);
        }
    }
    visited.fill(false);
    let mut groups = Vec::new();
    for &first in finished.iter().rev() {
        if visited[first] {
            continue;
        }
        let mut members = Vec::new();
        let mut pending = vec![first];
        visited[first] = true;
        while let Some(node) = pending.pop() {
            members.push(node);
            for &next in &reverse[node] {
                if !visited[next] {
                    visited[next] = true;
                    pending.push(next);
                }
            }
        }
        members.sort_unstable();
        groups.push(members);
    }
    groups.sort_by_key(|group| group[0]);
    groups
}
