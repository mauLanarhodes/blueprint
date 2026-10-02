//! What clicking selects, given groups and the group being edited.
//!
//! A click on a grouped shape selects the outermost group, like other
//! editors. Double-clicking enters that group (its *scope*), after which
//! clicks select its children directly. Escape leaves the group.

use crate::app::BlueprintApp;
use bp_model::{Element, ElementId, Parent};

impl BlueprintApp {
    /// The element a click on `hit` selects: its ancestor directly inside
    /// the group being edited, or its outermost ancestor.
    pub fn selectable(&self, hit: ElementId) -> ElementId {
        let mut chain = vec![hit];
        chain.extend(self.doc.ancestors(hit));
        if let Some(scope) = self.scope
            && let Some(pos) = chain.iter().position(|e| *e == scope)
        {
            return chain[pos.saturating_sub(1)];
        }
        *chain.last().expect("non-empty")
    }

    /// Whether `id` is inside the group being edited (always true when no
    /// group is being edited).
    pub fn inside_scope(&self, id: ElementId) -> bool {
        self.scope
            .is_none_or(|scope| self.doc.ancestors(id).contains(&scope))
    }

    /// Clicking something outside the group being edited leaves the group.
    pub fn leave_scope_unless_inside(&mut self, hit: ElementId) {
        if !self.inside_scope(hit) {
            self.scope = None;
        }
    }

    /// The group to enter when double-clicking `hit`, if it is grouped
    /// below the current scope.
    pub fn group_to_enter(&self, hit: ElementId) -> Option<ElementId> {
        let ancestors = self.doc.ancestors(hit);
        let groups: Vec<ElementId> = ancestors
            .iter()
            .copied()
            .filter(|a| self.doc.elements.get(a).is_some_and(Element::is_group))
            .collect();
        match self.scope {
            None => groups.last().copied(),
            Some(scope) => {
                let inside = ancestors.iter().position(|a| *a == scope)?;
                // The next group down from the scope.
                ancestors[..inside]
                    .iter()
                    .rev()
                    .find(|a| groups.contains(a))
                    .copied()
            }
        }
    }

    pub fn is_selected(&self, id: ElementId) -> bool {
        self.selection.contains(&id)
    }

    pub fn select_only(&mut self, id: ElementId) {
        self.selection = vec![id];
    }

    pub fn toggle_selected(&mut self, id: ElementId) {
        if let Some(i) = self.selection.iter().position(|s| *s == id) {
            self.selection.remove(i);
        } else {
            // Selecting something removes its ancestors and descendants,
            // so nothing is selected twice.
            let ancestors = self.doc.ancestors(id);
            let descendants = self.doc.descendants(id);
            self.selection
                .retain(|s| !ancestors.contains(s) && !descendants.contains(s));
            self.selection.push(id);
        }
    }

    /// Drops selected elements that no longer exist, left the page or sit
    /// on a hidden layer.
    pub fn prune_selection(&mut self) {
        let page = self.page;
        let doc = &self.doc;
        let visible = |id: &ElementId| {
            doc.layer_of(*id)
                .and_then(|l| doc.layers.get(&l))
                .is_some_and(|l| l.visible && l.page == page)
        };
        self.selection
            .retain(|id| doc.elements.contains_key(id) && visible(id));
        if self
            .scope
            .is_some_and(|s| !doc.elements.contains_key(&s) || !visible(&s))
        {
            self.scope = None;
        }
    }

    /// The selected elements that can be edited (not locked).
    pub fn editable_selection(&self) -> Vec<ElementId> {
        self.selection
            .iter()
            .copied()
            .filter(|id| !self.doc.is_locked(*id))
            .collect()
    }

    /// Everything selectable at the current scope on visible, unlocked
    /// layers of this page.
    pub fn all_selectable(&self) -> Vec<ElementId> {
        let parent_ok = |e: &Element| match (self.scope, e.parent) {
            (Some(scope), Parent::Element(p)) => p == scope,
            (Some(_), Parent::Layer(_)) => false,
            (None, Parent::Layer(l)) => self.doc.layers.get(&l).is_some_and(|l| !l.locked),
            (None, Parent::Element(_)) => false,
        };
        self.doc
            .paint_order(self.page)
            .into_iter()
            .filter(|e| parent_ok(e))
            .map(|e| e.id)
            .collect()
    }

    /// Whether clicks may land on `id`: its layer must be unlocked.
    pub fn is_hittable(&self, id: ElementId) -> bool {
        self.doc
            .layer_of(id)
            .and_then(|l| self.doc.layers.get(&l))
            .is_some_and(|l| !l.locked)
    }
}
