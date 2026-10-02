//! Fractional order keys.
//!
//! An [`OrderKey`] is a short string that sorts lexicographically. There is
//! always room for a new key between any two existing keys, so moving one
//! element never renumbers the others, and two people reordering shapes at
//! the same time merge cleanly.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Base-62 digits in ASCII order, so string order equals numeric order.
const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: usize = DIGITS.len();

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrderKey(String);

impl OrderKey {
    /// The key to use when a list is empty.
    pub fn first() -> Self {
        Self::between(None, None)
    }

    /// A key that sorts after `key`.
    pub fn after(key: &OrderKey) -> Self {
        Self::between(Some(key), None)
    }

    /// A key that sorts before `key`.
    pub fn before(key: &OrderKey) -> Self {
        Self::between(None, Some(key))
    }

    /// A key strictly between `a` and `b`; `None` means an open end.
    ///
    /// # Panics
    /// If `a` does not sort before `b`, or either key is invalid.
    pub fn between(a: Option<&OrderKey>, b: Option<&OrderKey>) -> Self {
        if let (Some(a), Some(b)) = (a, b) {
            assert!(a < b, "OrderKey::between: {a} must sort before {b}");
        }
        let a = a.map_or(&[][..], |k| k.0.as_bytes());
        Self(midpoint(a, b.map(|k| k.0.as_bytes())))
    }

    /// Parses a stored key, rejecting anything `between` could not handle.
    pub fn parse(s: &str) -> Option<Self> {
        let key = Self(s.to_owned());
        key.is_valid().then_some(key)
    }

    /// Valid keys are non-empty, use only base-62 digits and never end in `0`.
    pub fn is_valid(&self) -> bool {
        !self.0.is_empty() && !self.0.ends_with('0') && self.0.bytes().all(|c| DIGITS.contains(&c))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OrderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn digit(c: u8) -> usize {
    DIGITS
        .iter()
        .position(|&d| d == c)
        .expect("invalid order key digit")
}

/// Returns a key between `a` (empty = the start) and `b` (`None` = the end).
fn midpoint(a: &[u8], b: Option<&[u8]>) -> String {
    if let Some(b) = b {
        // Skip the common prefix, treating a missing digit in `a` as `0`.
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(DIGITS[0]) == b[n] {
            n += 1;
        }
        if n > 0 {
            let prefix = String::from_utf8_lossy(&b[..n]);
            let rest = midpoint(a.get(n..).unwrap_or(&[]), Some(&b[n..]));
            return format!("{prefix}{rest}");
        }
    }

    let digit_a = a.first().map_or(0, |&c| digit(c));
    let digit_b = b.map_or(BASE, |b| digit(b[0]));

    if digit_b - digit_a > 1 {
        let mid = (digit_a + digit_b).div_ceil(2);
        char::from(DIGITS[mid]).to_string()
    } else if let Some(b) = b.filter(|b| b.len() > 1) {
        char::from(b[0]).to_string()
    } else {
        let rest = midpoint(a.get(1..).unwrap_or(&[]), None);
        format!("{}{rest}", char::from(DIGITS[digit_a]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_key_is_valid() {
        let k = OrderKey::first();
        assert!(k.is_valid());
        assert_eq!(k.as_str(), "V");
    }

    #[test]
    fn appending_keeps_order() {
        let mut keys = vec![OrderKey::first()];
        for _ in 0..500 {
            keys.push(OrderKey::after(keys.last().unwrap()));
        }
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
        assert!(keys.iter().all(OrderKey::is_valid));
    }

    #[test]
    fn prepending_keeps_order() {
        let mut keys = vec![OrderKey::first()];
        for _ in 0..500 {
            keys.insert(0, OrderKey::before(&keys[0]));
        }
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
        assert!(keys.iter().all(OrderKey::is_valid));
    }

    #[test]
    fn repeated_bisection_keeps_order() {
        let low = OrderKey::first();
        let mut high = OrderKey::after(&low);
        for _ in 0..200 {
            let mid = OrderKey::between(Some(&low), Some(&high));
            assert!(low < mid && mid < high, "{low} < {mid} < {high}");
            assert!(mid.is_valid());
            high = mid;
        }
    }

    #[test]
    fn parse_rejects_bad_keys() {
        assert!(OrderKey::parse("V").is_some());
        assert!(OrderKey::parse("").is_none());
        assert!(OrderKey::parse("V0").is_none());
        assert!(OrderKey::parse("a-b").is_none());
    }
}
