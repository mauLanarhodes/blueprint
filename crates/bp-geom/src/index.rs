//! An R-tree over boxes, for "what is under the cursor" and "what is near
//! this box" queries without scanning every element.

use kurbo::{Point, Rect};
use rstar::{AABB, RTree, RTreeObject};

#[derive(Clone, Debug, PartialEq)]
struct Entry<T> {
    rect: Rect,
    value: T,
}

impl<T> RTreeObject for Entry<T> {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners([self.rect.x0, self.rect.y0], [self.rect.x1, self.rect.y1])
    }
}

/// Boxes with a value attached. Build it in one go; it is rebuilt rather
/// than updated, which bulk loading makes cheap.
#[derive(Clone, Debug)]
pub struct SpatialIndex<T> {
    tree: RTree<Entry<T>>,
}

impl<T> Default for SpatialIndex<T> {
    fn default() -> Self {
        Self { tree: RTree::new() }
    }
}

impl<T> SpatialIndex<T> {
    pub fn new(items: impl IntoIterator<Item = (Rect, T)>) -> Self {
        let entries = items
            .into_iter()
            .map(|(rect, value)| Entry {
                rect: rect.abs(),
                value,
            })
            .collect();
        Self {
            tree: RTree::bulk_load(entries),
        }
    }

    pub fn len(&self) -> usize {
        self.tree.size()
    }

    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }

    pub fn insert(&mut self, rect: Rect, value: T) {
        self.tree.insert(Entry {
            rect: rect.abs(),
            value,
        });
    }

    /// Removes the entry with exactly this box and value; returns whether
    /// there was one.
    pub fn remove(&mut self, rect: Rect, value: T) -> bool
    where
        T: PartialEq,
    {
        self.tree
            .remove(&Entry {
                rect: rect.abs(),
                value,
            })
            .is_some()
    }

    /// Every entry whose box touches `rect`.
    pub fn query_rect(&self, rect: Rect) -> impl Iterator<Item = (Rect, &T)> {
        let r = rect.abs();
        let aabb = AABB::from_corners([r.x0, r.y0], [r.x1, r.y1]);
        self.tree
            .locate_in_envelope_intersecting(aabb)
            .map(|e| (e.rect, &e.value))
    }

    /// Every entry whose box is within `tolerance` of `p`.
    pub fn query_point(&self, p: Point, tolerance: f64) -> impl Iterator<Item = (Rect, &T)> {
        self.query_rect(Rect::from_center_size(
            p,
            (tolerance * 2.0, tolerance * 2.0),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_boxes_by_point_and_rect() {
        let index = SpatialIndex::new((0..100).map(|i| {
            let x = f64::from(i) * 10.0;
            (Rect::new(x, 0.0, x + 5.0, 5.0), i)
        }));
        assert_eq!(index.len(), 100);
        let mut hits: Vec<_> = index
            .query_point(Point::new(12.0, 2.0), 0.5)
            .map(|(_, v)| *v)
            .collect();
        hits.sort();
        assert_eq!(hits, vec![1]);
        assert!(
            index
                .query_point(Point::new(7.5, 2.0), 1.0)
                .next()
                .is_none()
        );
        let mut hits: Vec<_> = index
            .query_rect(Rect::new(0.0, 0.0, 25.0, 1.0))
            .map(|(_, v)| *v)
            .collect();
        hits.sort();
        assert_eq!(hits, vec![0, 1, 2]);
        assert!(SpatialIndex::<u8>::default().is_empty());
    }

    #[test]
    fn insert_and_remove_entries() {
        let mut index = SpatialIndex::new([(Rect::new(0.0, 0.0, 1.0, 1.0), 'a')]);
        index.insert(Rect::new(10.0, 10.0, 11.0, 11.0), 'b');
        assert_eq!(index.len(), 2);
        assert!(index.remove(Rect::new(0.0, 0.0, 1.0, 1.0), 'a'));
        assert!(
            !index.remove(Rect::new(0.0, 0.0, 1.0, 1.0), 'a'),
            "already gone"
        );
        let hits: Vec<_> = index
            .query_point(Point::new(10.5, 10.5), 0.1)
            .map(|(_, v)| *v)
            .collect();
        assert_eq!(hits, ['b']);
    }
}
