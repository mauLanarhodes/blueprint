//! Higher-level edits built from [`Command`]s: deleting with everything
//! that depends on the deleted elements, moving, scaling, grouping,
//! z-order, alignment and copy/paste. Each function only plans commands;
//! apply them with [`crate::History::apply`] to get one undo step.

use crate::{Command, Prop};
use bp_model::kurbo::{Affine, Point, Rect, Vec2};
use bp_model::{Document, Element, ElementId, ElementKind, Endpoint, OrderKey, PageId, Parent};
use std::collections::{BTreeSet, HashMap, HashSet};

/// `ids` without any element whose ancestor is also in `ids`, in paint
/// order. Moving or grouping these moves everything selected exactly once.
pub fn top_level(doc: &Document, ids: &[ElementId]) -> Vec<ElementId> {
    let set: HashSet<_> = ids.iter().copied().collect();
    let mut out: Vec<_> = ids
        .iter()
        .copied()
        .filter(|id| doc.elements.contains_key(id))
        .filter(|id| !doc.ancestors(*id).iter().any(|a| set.contains(a)))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    sort_paint_order(doc, &mut out);
    out
}

/// Sorts `ids` bottom to top across the whole document.
pub fn sort_paint_order(doc: &Document, ids: &mut [ElementId]) {
    let rank = paint_ranks(doc);
    ids.sort_by_key(|id| rank.get(id).copied().unwrap_or(usize::MAX));
}

fn paint_ranks(doc: &Document) -> HashMap<ElementId, usize> {
    let tree = doc.tree();
    let mut all = Vec::new();
    for page in doc.pages_sorted() {
        for layer in doc.layers_of(page.id) {
            tree.subtree(Parent::Layer(layer.id), &mut all);
        }
    }
    all.iter().enumerate().map(|(i, e)| (e.id, i)).collect()
}

/// Commands that delete `ids`, their descendants and every connector glued
/// to anything deleted, in an order the commands accept.
pub fn remove(doc: &Document, ids: &[ElementId]) -> Vec<Command> {
    let mut doomed: HashSet<ElementId> = HashSet::new();
    for &id in ids {
        if doc.elements.contains_key(&id) {
            doomed.insert(id);
            doomed.extend(doc.descendants(id));
        }
    }
    let attached: Vec<_> = doc
        .elements
        .values()
        .filter(|e| {
            e.as_connector().is_some_and(|c| {
                c.endpoints()
                    .iter()
                    .any(|end| end.element().is_some_and(|t| doomed.contains(&t)))
            })
        })
        .map(|e| e.id)
        .collect();
    doomed.extend(attached);

    let mut order: Vec<_> = doomed.into_iter().collect();
    sort_paint_order(doc, &mut order);
    // Connectors first (nothing hangs off them), then everything else with
    // children before their parents.
    let (connectors, others): (Vec<_>, Vec<_>) = order
        .into_iter()
        .partition(|id| doc.elements[id].is_connector());
    connectors
        .into_iter()
        .chain(others.into_iter().rev())
        .map(Command::Remove)
        .collect()
}

/// Commands that delete `page` with all its layers and elements.
pub fn remove_page(doc: &Document, page: PageId) -> Vec<Command> {
    let ids: Vec<_> = doc.page_elements(page).iter().map(|e| e.id).collect();
    let mut commands = remove(doc, &ids);
    commands.extend(
        doc.layers_of(page)
            .iter()
            .map(|l| Command::RemoveLayer(l.id)),
    );
    commands.push(Command::RemovePage(page));
    commands
}

/// The box an element occupies, as far as the model knows: a shape's
/// bounds, or the union of a group's shapes. Connectors have no stored
/// geometry (their route is derived), so they return `None`.
pub fn bounds(doc: &Document, id: ElementId) -> Option<Rect> {
    let element = doc.elements.get(&id)?;
    match &element.kind {
        ElementKind::Shape(s) => Some(s.bounds),
        ElementKind::Connector(_) => None,
        ElementKind::Group => doc
            .descendants(id)
            .iter()
            .filter_map(|d| doc.elements.get(d)?.as_shape().map(|s| s.bounds))
            .reduce(|a, b| a.union(b)),
    }
}

/// Union of [`bounds`] over `ids`.
pub fn union_bounds(doc: &Document, ids: &[ElementId]) -> Option<Rect> {
    ids.iter()
        .filter_map(|id| bounds(doc, *id))
        .reduce(|a, b| a.union(b))
}

/// Commands that map every shape among `ids` (and their descendants)
/// through `transform`, which must keep boxes axis-aligned (translate and
/// scale). Connectors move with them: a selected connector's free ends and
/// waypoints, and the waypoints of any connector whose both ends are
/// attached to moved shapes.
pub fn transform(doc: &Document, ids: &[ElementId], transform: Affine) -> Vec<Command> {
    let mut moved: HashSet<ElementId> = HashSet::new();
    for id in top_level(doc, ids) {
        moved.insert(id);
        moved.extend(doc.descendants(id));
    }
    let mut commands = Vec::new();
    for element in doc.elements.values() {
        match &element.kind {
            ElementKind::Shape(s) if moved.contains(&element.id) => {
                let bounds = transform.transform_rect_bbox(s.bounds);
                commands.push(Command::Set {
                    id: element.id,
                    prop: Prop::Bounds(bounds),
                });
            }
            ElementKind::Connector(c) => {
                let selected = moved.contains(&element.id);
                let carried = c
                    .endpoints()
                    .iter()
                    .all(|end| end.element().is_some_and(|t| moved.contains(&t)));
                if !(selected || carried) {
                    continue;
                }
                if selected {
                    for (end, make) in [
                        (&c.source, Prop::Source as fn(Endpoint) -> Prop),
                        (&c.target, Prop::Target as fn(Endpoint) -> Prop),
                    ] {
                        if let Endpoint::Free(p) = end {
                            commands.push(Command::Set {
                                id: element.id,
                                prop: make(Endpoint::Free(transform * *p)),
                            });
                        }
                    }
                }
                if !c.waypoints.is_empty() {
                    let waypoints = c.waypoints.iter().map(|p| transform * *p).collect();
                    commands.push(Command::Set {
                        id: element.id,
                        prop: Prop::Waypoints(waypoints),
                    });
                }
            }
            _ => {}
        }
    }
    commands
}

/// Commands that move `ids` by `delta`.
pub fn translate(doc: &Document, ids: &[ElementId], delta: Vec2) -> Vec<Command> {
    if delta == Vec2::ZERO {
        return Vec::new();
    }
    transform(doc, ids, Affine::translate(delta))
}

/// The transform that maps `from` onto `to` (both axis-aligned boxes).
pub fn rect_to_rect(from: Rect, to: Rect) -> Affine {
    let sx = if from.width() > 0.0 {
        to.width() / from.width()
    } else {
        1.0
    };
    let sy = if from.height() > 0.0 {
        to.height() / from.height()
    } else {
        1.0
    };
    Affine::translate(to.origin().to_vec2())
        * Affine::scale_non_uniform(sx, sy)
        * Affine::translate(-from.origin().to_vec2())
}

/// Commands that put `ids` into a new group, at the stacking position of
/// the topmost of them. Returns the group's id with the commands.
pub fn group(doc: &Document, ids: &[ElementId]) -> Option<(ElementId, Vec<Command>)> {
    let members = top_level(doc, ids);
    let top = *members.last()?;
    let top_el = &doc.elements[&top];
    let parent = top_el.parent;
    let group = Element::group(parent, after_sibling(doc, top_el));
    let g = group.id;
    let mut commands = vec![Command::Insert(Box::new(group))];
    let mut order = OrderKey::first();
    for (i, id) in members.into_iter().enumerate() {
        if i > 0 {
            order = OrderKey::after(&order);
        }
        commands.push(Command::Set {
            id,
            prop: Prop::Parent(Parent::Element(g)),
        });
        commands.push(Command::Set {
            id,
            prop: Prop::Order(order.clone()),
        });
    }
    Some((g, commands))
}

/// A key just above `element` among its siblings (below the next one).
fn after_sibling(doc: &Document, element: &Element) -> OrderKey {
    let next = doc
        .elements
        .values()
        .filter(|e| e.parent == element.parent && e.order > element.order)
        .map(|e| &e.order)
        .min();
    OrderKey::between(Some(&element.order), next)
}

/// Commands that dissolve the groups among `ids`, putting each group's
/// children where the group was. Returns the freed children too.
pub fn ungroup(doc: &Document, ids: &[ElementId]) -> (Vec<ElementId>, Vec<Command>) {
    let tree = doc.tree();
    let mut freed = Vec::new();
    let mut commands = Vec::new();
    for &id in ids {
        let Some(group) = doc.elements.get(&id).filter(|e| e.is_group()) else {
            continue;
        };
        let next = doc
            .elements
            .values()
            .filter(|e| e.parent == group.parent && e.order > group.order)
            .map(|e| e.order.clone())
            .min();
        let mut previous = group.order.clone();
        for child in tree.children(Parent::Element(id)) {
            let order = OrderKey::between(Some(&previous), next.as_ref());
            commands.push(Command::Set {
                id: child.id,
                prop: Prop::Parent(group.parent),
            });
            commands.push(Command::Set {
                id: child.id,
                prop: Prop::Order(order.clone()),
            });
            previous = order;
            freed.push(child.id);
        }
        commands.push(Command::Remove(id));
    }
    (freed, commands)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reorder {
    Front,
    Forward,
    Backward,
    Back,
}

/// Commands that restack `ids` among their siblings.
pub fn reorder(doc: &Document, ids: &[ElementId], how: Reorder) -> Vec<Command> {
    let tree = doc.tree();
    let mut commands = Vec::new();
    let mut by_parent: HashMap<Parent, Vec<ElementId>> = HashMap::new();
    for id in top_level(doc, ids) {
        by_parent
            .entry(doc.elements[&id].parent)
            .or_default()
            .push(id);
    }
    for (parent, moving) in by_parent {
        let siblings = tree.children(parent);
        let moving_set: HashSet<_> = moving.iter().copied().collect();
        let keys = restack(siblings, &moving_set, how);
        for (id, order) in keys {
            commands.push(Command::Set {
                id,
                prop: Prop::Order(order),
            });
        }
    }
    commands
}

/// New order keys for the `moving` siblings. Only the moving elements get
/// new keys, so concurrent edits to the others never conflict.
fn restack(
    siblings: &[&Element],
    moving: &HashSet<ElementId>,
    how: Reorder,
) -> Vec<(ElementId, OrderKey)> {
    let ids: Vec<ElementId> = siblings.iter().map(|e| e.id).collect();
    let key = |id: ElementId| {
        siblings
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.order.clone())
            .expect("sibling")
    };
    let stay: Vec<ElementId> = ids
        .iter()
        .copied()
        .filter(|i| !moving.contains(i))
        .collect();
    let mov: Vec<ElementId> = ids.iter().copied().filter(|i| moving.contains(i)).collect();
    if mov.is_empty() || stay.is_empty() {
        return Vec::new();
    }
    // Where the moving block goes: between `below` and `above` (keys of
    // staying siblings; `None` is an open end).
    let (below, above): (Option<OrderKey>, Option<OrderKey>) = match how {
        Reorder::Front => (Some(key(*stay.last().expect("non-empty"))), None),
        Reorder::Back => (None, Some(key(stay[0]))),
        Reorder::Forward => {
            // Above the first staying sibling that is above the topmost
            // moving one.
            let top = ids
                .iter()
                .position(|i| *i == *mov.last().expect("non-empty"))
                .expect("present");
            let Some(pass) = ids[top + 1..].iter().find(|i| !moving.contains(i)) else {
                return Vec::new();
            };
            let after_pass = ids
                .iter()
                .skip_while(|i| *i != pass)
                .skip(1)
                .find(|i| !moving.contains(i))
                .map(|i| key(*i));
            (Some(key(*pass)), after_pass)
        }
        Reorder::Backward => {
            let bottom = ids.iter().position(|i| *i == mov[0]).expect("present");
            let Some(pass) = ids[..bottom].iter().rev().find(|i| !moving.contains(i)) else {
                return Vec::new();
            };
            let before_pass = ids
                .iter()
                .rev()
                .skip_while(|i| *i != pass)
                .skip(1)
                .find(|i| !moving.contains(i))
                .map(|i| key(*i));
            (before_pass, Some(key(*pass)))
        }
    };
    let mut out = Vec::new();
    let mut previous = below;
    for id in mov {
        let order = OrderKey::between(previous.as_ref(), above.as_ref());
        previous = Some(order.clone());
        out.push((id, order));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

/// Commands that align the boxes of `ids` to their common bounds.
pub fn align(doc: &Document, ids: &[ElementId], how: Align) -> Vec<Command> {
    let items = top_level(doc, ids);
    let Some(all) = union_bounds(doc, &items) else {
        return Vec::new();
    };
    let mut commands = Vec::new();
    for id in items {
        let Some(b) = bounds(doc, id) else { continue };
        let delta = match how {
            Align::Left => Vec2::new(all.x0 - b.x0, 0.0),
            Align::CenterX => Vec2::new(all.center().x - b.center().x, 0.0),
            Align::Right => Vec2::new(all.x1 - b.x1, 0.0),
            Align::Top => Vec2::new(0.0, all.y0 - b.y0),
            Align::CenterY => Vec2::new(0.0, all.center().y - b.center().y),
            Align::Bottom => Vec2::new(0.0, all.y1 - b.y1),
        };
        commands.extend(translate(doc, &[id], delta));
    }
    commands
}

/// Commands that space the boxes of `ids` evenly between the outermost
/// two, horizontally or vertically.
pub fn distribute(doc: &Document, ids: &[ElementId], horizontal: bool) -> Vec<Command> {
    let mut items: Vec<(ElementId, Rect)> = top_level(doc, ids)
        .into_iter()
        .filter_map(|id| Some((id, bounds(doc, id)?)))
        .collect();
    if items.len() < 3 {
        return Vec::new();
    }
    let start = |r: &Rect| if horizontal { r.x0 } else { r.y0 };
    let size = |r: &Rect| if horizontal { r.width() } else { r.height() };
    items.sort_by(|a, b| start(&a.1).total_cmp(&start(&b.1)));
    let first = start(&items[0].1);
    let last = items
        .last()
        .map(|(_, r)| start(r) + size(r))
        .expect("non-empty");
    let total: f64 = items.iter().map(|(_, r)| size(r)).sum();
    let gap = (last - first - total) / (items.len() - 1) as f64;
    let mut cursor = first;
    let mut commands = Vec::new();
    for (id, r) in &items {
        let shift = cursor - start(r);
        let delta = if horizontal {
            Vec2::new(shift, 0.0)
        } else {
            Vec2::new(0.0, shift)
        };
        commands.extend(translate(doc, &[*id], delta));
        cursor += size(r) + gap;
    }
    commands
}

/// Elements copied out of a document, self-contained: every reference
/// points inside the clip or has been turned into a free position.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    /// Parents always come before their children. An element whose parent
    /// is not in the clip is top-level.
    pub elements: Vec<Element>,
}

impl Clip {
    /// Copies `ids` with their descendants. Connectors are copied when they
    /// are selected or join two copied shapes; an end attached to a shape
    /// that is not copied becomes a free point, placed by `resolve_end`.
    pub fn copy(
        doc: &Document,
        ids: &[ElementId],
        resolve_end: impl Fn(ElementId, bool) -> Option<Point>,
    ) -> Clip {
        let roots = top_level(doc, ids);
        let mut chosen: Vec<ElementId> = Vec::new();
        let tree = doc.tree();
        for &id in &roots {
            chosen.push(id);
            let mut subtree = Vec::new();
            tree.subtree(Parent::Element(id), &mut subtree);
            chosen.extend(subtree.iter().map(|e| e.id));
        }
        let mut set: HashSet<ElementId> = chosen.iter().copied().collect();
        // Connectors that join two copied shapes come along too.
        let joining: Vec<_> = doc
            .elements
            .values()
            .filter(|e| !set.contains(&e.id))
            .filter(|e| {
                e.as_connector().is_some_and(|c| {
                    c.endpoints()
                        .iter()
                        .all(|end| end.element().is_some_and(|t| set.contains(&t)))
                })
            })
            .map(|e| e.id)
            .collect();
        for id in joining {
            set.insert(id);
            chosen.push(id);
        }

        let mut elements = Vec::new();
        for id in chosen {
            let mut element = doc.elements[&id].clone();
            if let ElementKind::Connector(c) = &mut element.kind {
                for (end, is_source) in [(&mut c.source, true), (&mut c.target, false)] {
                    if let Endpoint::Glued {
                        element: target, ..
                    } = end
                        && !set.contains(target)
                    {
                        let at = resolve_end(id, is_source).unwrap_or_default();
                        *end = Endpoint::Free(at);
                    }
                }
            }
            elements.push(element);
        }
        Clip { elements }
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Commands that paste the clip into `parent`, on top of its children,
    /// moved by `offset`. Every element gets a fresh id. Elements whose
    /// parent is not in the clip become top-level. Returns the ids of the
    /// pasted top-level elements.
    pub fn paste(
        &self,
        doc: &Document,
        parent: Parent,
        offset: Vec2,
    ) -> (Vec<ElementId>, Vec<Command>) {
        let fresh: HashMap<ElementId, ElementId> = self
            .elements
            .iter()
            .map(|e| (e.id, ElementId::new()))
            .collect();
        let shift = Affine::translate(offset);
        let mut order = doc.next_order_key(parent);
        let mut roots = Vec::new();
        let mut commands = Vec::new();
        for original in &self.elements {
            let mut element = original.clone();
            element.id = fresh[&original.id];
            match element.parent {
                Parent::Element(p) if fresh.contains_key(&p) => {
                    element.parent = Parent::Element(fresh[&p]);
                }
                _ => {
                    element.parent = parent;
                    element.order = order.clone();
                    order = OrderKey::after(&order);
                    roots.push(element.id);
                }
            }
            match &mut element.kind {
                ElementKind::Shape(s) => s.bounds = shift.transform_rect_bbox(s.bounds),
                ElementKind::Connector(c) => {
                    for end in [&mut c.source, &mut c.target] {
                        match end {
                            Endpoint::Glued {
                                element: target, ..
                            } => {
                                // A dangling reference can't happen after
                                // `copy`, but a hand-edited clip could have
                                // one; the insert command rejects it.
                                if let Some(new) = fresh.get(target) {
                                    *target = *new;
                                }
                            }
                            Endpoint::Free(p) => *p = shift * *p,
                        }
                    }
                    for p in &mut c.waypoints {
                        *p = shift * *p;
                    }
                }
                ElementKind::Group => {}
            }
            commands.push(Command::Insert(Box::new(element)));
        }
        (roots, commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::History;
    use bp_model::ShapeRef;

    struct Fixture {
        doc: Document,
        history: History,
        layer: Parent,
    }

    impl Fixture {
        fn new() -> Self {
            let doc = Document::new();
            let page = doc.first_page().unwrap();
            let layer = Parent::Layer(doc.layers_of(page)[0].id);
            Self {
                doc,
                history: History::new(),
                layer,
            }
        }

        fn run(&mut self, commands: Vec<Command>) {
            self.history.apply(&mut self.doc, "test", commands).unwrap();
            assert_eq!(self.doc.validate(), Ok(()));
        }

        fn shape(&mut self, parent: Parent, x: f64, y: f64) -> ElementId {
            let order = self.doc.next_order_key(parent);
            let el = Element::shape(
                ShapeRef::new("basic", "rectangle"),
                parent,
                order,
                Rect::new(x, y, x + 10.0, y + 10.0),
            );
            let id = el.id;
            self.run(vec![Command::Insert(Box::new(el))]);
            id
        }

        fn connect(&mut self, a: ElementId, b: ElementId) -> ElementId {
            let order = self.doc.next_order_key(self.layer);
            let mut el = Element::connector(
                Endpoint::glued(a, None),
                Endpoint::glued(b, None),
                self.layer,
                order,
            );
            if let ElementKind::Connector(c) = &mut el.kind {
                c.waypoints = vec![Point::new(50.0, 50.0)];
            }
            let id = el.id;
            self.run(vec![Command::Insert(Box::new(el))]);
            id
        }

        fn x(&self, id: ElementId) -> f64 {
            self.doc.elements[&id].as_shape().unwrap().bounds.x0
        }

        fn paint(&self) -> Vec<ElementId> {
            let page = self.doc.first_page().unwrap();
            self.doc.paint_order(page).iter().map(|e| e.id).collect()
        }
    }

    #[test]
    fn removing_a_group_takes_children_and_attached_connectors() {
        let mut f = Fixture::new();
        let (g, group_cmds) = {
            let a = f.shape(f.layer, 0.0, 0.0);
            let b = f.shape(f.layer, 20.0, 0.0);
            let outside = f.shape(f.layer, 40.0, 0.0);
            let c1 = f.connect(a, outside);
            let c2 = f.connect(a, b);
            let (g, cmds) = group(&f.doc, &[a, b]).unwrap();
            let _ = (c1, c2);
            (g, cmds)
        };
        f.run(group_cmds);
        assert_eq!(f.doc.elements.len(), 6);
        f.run(remove(&f.doc, &[g]));
        assert_eq!(f.doc.elements.len(), 1, "only the outside shape is left");
        f.history.undo(&mut f.doc);
        assert_eq!(f.doc.elements.len(), 6);
        assert_eq!(f.doc.validate(), Ok(()));
    }

    #[test]
    fn moving_shapes_carries_connectors_between_them() {
        let mut f = Fixture::new();
        let a = f.shape(f.layer, 0.0, 0.0);
        let b = f.shape(f.layer, 100.0, 0.0);
        let c = f.shape(f.layer, 200.0, 0.0);
        let ab = f.connect(a, b);
        let bc = f.connect(b, c);
        f.run(translate(&f.doc, &[a, b], Vec2::new(5.0, 7.0)));
        assert_eq!(f.x(a), 5.0);
        assert_eq!(f.x(b), 105.0);
        assert_eq!(f.x(c), 200.0);
        let wp = |f: &Fixture, id| f.doc.elements[&id].as_connector().unwrap().waypoints[0];
        assert_eq!(wp(&f, ab), Point::new(55.0, 57.0), "both ends moved");
        assert_eq!(wp(&f, bc), Point::new(50.0, 50.0), "one end moved");
    }

    #[test]
    fn group_and_ungroup_keep_stacking() {
        let mut f = Fixture::new();
        let a = f.shape(f.layer, 0.0, 0.0);
        let b = f.shape(f.layer, 0.0, 0.0);
        let c = f.shape(f.layer, 0.0, 0.0);
        let d = f.shape(f.layer, 0.0, 0.0);
        let (g, cmds) = group(&f.doc, &[d, b]).unwrap();
        f.run(cmds);
        // The group takes the place of the topmost member (d).
        assert_eq!(f.paint(), vec![a, c, g, b, d]);
        let (freed, cmds) = ungroup(&f.doc, &[g]);
        assert_eq!(freed, vec![b, d]);
        f.run(cmds);
        assert_eq!(f.paint(), vec![a, c, b, d]);
    }

    #[test]
    fn reorder_moves_blocks() {
        let mut f = Fixture::new();
        let ids: Vec<_> = (0..4).map(|_| f.shape(f.layer, 0.0, 0.0)).collect();
        let [a, b, c, d] = ids[..] else {
            unreachable!()
        };
        f.run(reorder(&f.doc, &[a], Reorder::Forward));
        assert_eq!(f.paint(), vec![b, a, c, d]);
        f.run(reorder(&f.doc, &[a, b], Reorder::Front));
        assert_eq!(f.paint(), vec![c, d, b, a]);
        f.run(reorder(&f.doc, &[a], Reorder::Back));
        assert_eq!(f.paint(), vec![a, c, d, b]);
        f.run(reorder(&f.doc, &[b], Reorder::Backward));
        assert_eq!(f.paint(), vec![a, c, b, d]);
        // Already at the top: nothing to do.
        assert!(reorder(&f.doc, &[d], Reorder::Forward).is_empty());
    }

    #[test]
    fn align_and_distribute() {
        let mut f = Fixture::new();
        let a = f.shape(f.layer, 0.0, 0.0);
        let b = f.shape(f.layer, 30.0, 5.0);
        let c = f.shape(f.layer, 100.0, 9.0);
        f.run(align(&f.doc, &[a, b, c], Align::Top));
        let y = |f: &Fixture, id| f.doc.elements[&id].as_shape().unwrap().bounds.y0;
        assert_eq!((y(&f, a), y(&f, b), y(&f, c)), (0.0, 0.0, 0.0));
        f.run(distribute(&f.doc, &[c, a, b], true));
        assert_eq!((f.x(a), f.x(b), f.x(c)), (0.0, 50.0, 100.0));
    }

    #[test]
    fn copy_paste_remaps_references() {
        let mut f = Fixture::new();
        let a = f.shape(f.layer, 0.0, 0.0);
        let b = f.shape(f.layer, 100.0, 0.0);
        let outside = f.shape(f.layer, 300.0, 0.0);
        let (g, cmds) = group(&f.doc, &[a, b]).unwrap();
        f.run(cmds);
        let joined = f.connect(a, b);
        let dangling = f.connect(b, outside);

        let clip = Clip::copy(&f.doc, &[g, dangling], |_, _| Some(Point::new(9.0, 9.0)));
        // Group, its two shapes, the selected connector and the joining one.
        assert_eq!(clip.elements.len(), 5);
        let json = serde_json::to_string(&clip).unwrap();
        let clip: Clip = serde_json::from_str(&json).unwrap();

        let (roots, cmds) = clip.paste(&f.doc, f.layer, Vec2::new(10.0, 0.0));
        f.run(cmds);
        assert_eq!(f.doc.elements.len(), 6 + 5);
        assert_eq!(roots.len(), 3, "group + two connectors at the top level");
        let pasted_group = roots[0];
        let kids = f.doc.descendants(pasted_group);
        assert_eq!(kids.len(), 2);
        assert!(kids.iter().all(|k| f.x(*k) == 10.0 || f.x(*k) == 110.0));
        for root in &roots[1..] {
            let c = f.doc.elements[root].as_connector().unwrap();
            for end in c.endpoints() {
                match end {
                    Endpoint::Glued { element, .. } => assert!(kids.contains(element)),
                    Endpoint::Free(p) => assert_eq!(*p, Point::new(19.0, 9.0)),
                }
            }
        }
        let _ = joined;
    }

    #[test]
    fn remove_page_removes_everything_on_it() {
        let mut f = Fixture::new();
        let first = f.doc.first_page().unwrap();
        let page = bp_model::Page::new("Two", OrderKey::after(&f.doc.pages[&first].order));
        let layer = bp_model::Layer::new(page.id, "L", OrderKey::first());
        let (p, l) = (page.id, layer.id);
        f.run(vec![
            Command::InsertPage(Box::new(page)),
            Command::InsertLayer(Box::new(layer)),
        ]);
        let a = f.shape(Parent::Layer(l), 0.0, 0.0);
        let b = f.shape(Parent::Layer(l), 50.0, 0.0);
        f.connect(a, b);
        f.run(remove_page(&f.doc, p));
        assert_eq!(f.doc.pages.len(), 1);
        assert!(f.doc.elements.is_empty());
    }

    #[test]
    fn rect_to_rect_maps_corners() {
        let t = rect_to_rect(
            Rect::new(0.0, 0.0, 10.0, 20.0),
            Rect::new(5.0, 5.0, 25.0, 15.0),
        );
        assert_eq!(t * Point::new(0.0, 0.0), Point::new(5.0, 5.0));
        assert_eq!(t * Point::new(10.0, 20.0), Point::new(25.0, 15.0));
    }
}
