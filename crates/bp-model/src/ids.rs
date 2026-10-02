use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// A new, globally unique, time-ordered id (UUIDv7).
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(
    /// Identifies a page in a document.
    PageId
);
id_type!(
    /// Identifies a layer on a page.
    LayerId
);
id_type!(
    /// Identifies a shape, connector or other element.
    ElementId
);

/// A hasher for id keys in runtime caches. UUIDv7s end in 62 random bits,
/// so one multiply spreads them well; std's default SipHash would cost
/// several times more on maps that are probed thousands of times a frame.
/// Not for untrusted keys (no protection against crafted collisions).
#[derive(Clone, Copy, Debug, Default)]
pub struct IdHasher(u64);

impl std::hash::Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Ok(id) = <[u8; 16]>::try_from(bytes) {
            // The random half of a UUIDv7, folded with the time half.
            let hi = u64::from_le_bytes(id[..8].try_into().expect("8 bytes"));
            let lo = u64::from_le_bytes(id[8..].try_into().expect("8 bytes"));
            self.0 = (lo ^ hi.rotate_left(32) ^ self.0).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        } else {
            // Anything else: FNV-1a.
            let mut h = self.0 ^ 0xcbf2_9ce4_8422_2325;
            for b in bytes {
                h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
            }
            self.0 = h;
        }
    }

    fn write_usize(&mut self, _: usize) {
        // Slice length prefixes carry no information for fixed-size ids.
    }
}

/// A hash map keyed by ids, with [`IdHasher`].
pub type IdMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<IdHasher>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_maps_work() {
        let mut map: IdMap<ElementId, usize> = IdMap::default();
        let ids: Vec<ElementId> = (0..1000).map(|_| ElementId::new()).collect();
        for (i, id) in ids.iter().enumerate() {
            map.insert(*id, i);
        }
        assert_eq!(map.len(), 1000);
        assert!(ids.iter().enumerate().all(|(i, id)| map[id] == i));
    }
}
