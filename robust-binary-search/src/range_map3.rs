// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::ops::Bound;
use std::ops::RangeBounds;

const MAX_RANGE_SIZE: usize = 1000;

/// A single entry in a RangeMap, which corresponds to a range of individual values.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RangeMapEntry<T> {
    /// Beginning index of the range within the conceptual vector of individual values.
    offset: usize,
    /// Number of indices captured by the range.
    len: usize,
    /// Value of all individual values within the range.
    value: T,
}

impl<T> RangeMapEntry<T> {
    #[allow(dead_code)]
    /// Returns the index of the first individual value in the range.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the length of the range.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns offset() + len().
    pub fn end(&self) -> usize {
        self.offset + self.len
    }

    /// Returns the value of the range.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Returns the value of the range.
    pub fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    fn contains(&self, index: usize) -> bool {
        self.offset <= index && index < self.end()
    }
}

#[derive(Clone, Debug)]
struct Node<T> {
    values: Vec<RangeMapEntry<T>>,
}

impl<T> Node<T> {
    pub fn contains(&self, index: usize) -> bool {
        let start = self.values[0].offset;
        let end = self.values[self.values.len() - 1].end();
        start <= index && index < end
    }

    fn intersects_range<R: RangeBounds<usize>>(&self, range: &R) -> bool {
        let start = self.values[0].offset;
        let end = self.values[self.values.len() - 1].end();
        match range.start_bound() {
            Bound::Included(b) => {
                if *b >= end {
                    return false;
                }
            }
            Bound::Excluded(b) => {
                if *b + 1 >= end {
                    return false;
                }
            }
            Bound::Unbounded => (),
        }
        match range.end_bound() {
            Bound::Included(b) => {
                if *b < start {
                    return false;
                }
            }
            Bound::Excluded(b) => {
                if *b <= start {
                    return false;
                }
            }
            Bound::Unbounded => (),
        }
        true
    }

    /// Takes an individual element index and returns the RangeMapEntry index.
    fn range_index(&self, index: usize) -> Option<usize> {
        for (i, w) in self.values.iter().enumerate() {
            if index >= w.offset && index < w.end() {
                return Some(i);
            }
        }
        None
    }

    pub fn range_for_index(&self, index: usize) -> Option<&RangeMapEntry<T>> {
        self.range_index(index).map(|i| &self.values[i])
    }
}

/// A RangeMap is essentially a fixed-length vector optimized for long stretches of equal values.
/// The RangeMap is partitioned into contiguous RangeMapEntries. For example, a map with the
/// entries:
///
/// ```text
/// RangeMapEntry {
///   offset: 0,
///   len: 1,
///   value: 'a'
/// }
/// RangeMapEntry {
///   offset: 1,
///   len: 2,
///   value: 'b'
/// }
/// RangeMapEntry {
///   offset: 3,
///   len: 3,
///   value: 'c'
/// }
/// RangeMapEntry {
///   offset: 6,
///   len: 4,
///   value: 'd'
/// }
/// ```
///
/// represents the data:
///
/// ```text
/// ['a', 'b', 'b', 'c', 'c', 'c', 'd', 'd', 'd', 'd']
/// ```
///
/// Note that neighboring entries may contain the same value.
#[derive(Clone, Debug)]
pub struct RangeMap<T> {
    /// Entries within the map. Invariants:
    ///
    /// 1. Must be non-empty.
    /// 2. values[0].offset() == 0
    /// 3. values[i - 1].end() == values[i].offset()
    /// 4. The length of each entry must be non-zero.
    valueses: Vec<Node<T>>,
}

impl<T: Clone> RangeMap<T> {
    /// Creates a new RangeMap with the given size and initial value. It contains a single entry
    /// spanning the entire range.
    pub fn new(size: usize, value: T) -> Self {
        RangeMap {
            valueses: vec![Node {
                values: vec![RangeMapEntry {
                    offset: 0,
                    len: size,
                    value,
                }],
            }],
        }
    }

    /// Returns the length of the entire range.
    pub fn len(&self) -> usize {
        let values = &self.valueses[self.valueses.len() - 1].values;
        values[values.len() - 1].end()
    }

    /// Returns an iterator over entries.
    pub fn ranges(&self) -> impl DoubleEndedIterator<Item = &RangeMapEntry<T>> {
        self.valueses.iter().map(|v| v.values.iter()).flatten()
    }

    /// Returns an iterator over mutable entries.
    pub fn ranges_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>> {
        self.valueses
            .iter_mut()
            .map(|v| v.values.iter_mut())
            .flatten()
    }

    /// Returns the entry containing the given index.
    pub fn range_for_index(&self, index: usize) -> &RangeMapEntry<T> {
        self.valueses
            .iter()
            .filter_map(|r| r.range_for_index(index))
            .next()
            .unwrap()
    }

    /// Ensures that `index-1` and `index` are in different RangeMapEntrys.
    pub fn split(&mut self, index: usize)
    where
        T: std::fmt::Debug,
    {
        if index == self.len() {
            return;
        }
        let (range_ix, range) = self
            .valueses
            .iter_mut()
            .enumerate()
            .filter(|(_, r)| r.contains(index))
            .next()
            .unwrap();
        let (entry_ix, entry) = range
            .values
            .iter_mut()
            .enumerate()
            .filter(|(_, e)| e.contains(index))
            .next()
            .unwrap();
        if entry.offset == index {
            return;
        }
        let end = entry.end();
        if index == end {
            return;
        }
        entry.len = index - entry.offset;
        let value = entry.value.clone();
        range.values.insert(
            entry_ix + 1,
            RangeMapEntry {
                offset: index,
                len: end - index,
                value,
            },
        );
        if range.values.len() > MAX_RANGE_SIZE {
            let i = range.values.len() / 2;
            let range2 = range.values.drain(i..).collect::<Vec<_>>();
            self.valueses.insert(range_ix + 1, Node { values: range2 });
        }
    }

    // TODO: better signature
    // TODO: document
    pub fn range<'a, R: RangeBounds<usize> + Clone + 'a>(
        &'a self,
        range: R,
    ) -> impl DoubleEndedIterator<Item = &RangeMapEntry<T>> {
        self.valueses
            .iter()
            .filter({
                let range = range.clone();
                move |r| r.intersects_range(&range)
            })
            .map(|r| r.values.iter())
            .flatten()
            .filter(move |e| range.contains(&e.offset))
    }

    // TODO: better signature
    // TODO: document
    pub fn range_mut<'a, R: RangeBounds<usize> + Clone + 'a>(
        &'a mut self,
        range: R,
    ) -> impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>> {
        self.valueses
            .iter_mut()
            .filter({
                let range = range.clone();
                move |r| r.intersects_range(&range)
            })
            .map(|r| r.values.iter_mut())
            .flatten()
            .filter(move |e| range.contains(&e.offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_for_index_empty() {
        let m = RangeMap::new(10, 0.0);
        assert_eq!(
            m.range_for_index(0),
            &RangeMapEntry {
                offset: 0,
                len: 10,
                value: 0.0
            }
        );
        assert_eq!(
            m.range_for_index(9),
            &RangeMapEntry {
                offset: 0,
                len: 10,
                value: 0.0
            }
        );
    }

    #[test]
    fn split() {
        let mut m = RangeMap::new(10, 0.0);
        assert_eq!(
            m.ranges().collect::<Vec<_>>(),
            vec![&RangeMapEntry {
                offset: 0,
                len: 10,
                value: 0.0
            }]
        );
        m.split(5);
        assert_eq!(
            m.range(0..5).collect::<Vec<_>>(),
            vec![&RangeMapEntry {
                offset: 0,
                len: 5,
                value: 0.0
            }]
        );
        assert_eq!(
            m.range(5..).collect::<Vec<_>>(),
            vec![&RangeMapEntry {
                offset: 5,
                len: 5,
                value: 0.0
            }]
        );
        assert_eq!(
            m.range_for_index(0),
            &RangeMapEntry {
                offset: 0,
                len: 5,
                value: 0.0
            }
        );
        assert_eq!(
            m.range_for_index(4),
            &RangeMapEntry {
                offset: 0,
                len: 5,
                value: 0.0
            }
        );
        assert_eq!(
            m.range_for_index(5),
            &RangeMapEntry {
                offset: 5,
                len: 5,
                value: 0.0
            }
        );
        assert_eq!(
            m.range_for_index(9),
            &RangeMapEntry {
                offset: 5,
                len: 5,
                value: 0.0
            }
        );
    }

    #[test]
    fn node_intersects_range() {
        let node = Node {
            values: vec![RangeMapEntry {
                offset: 10,
                len: 5,
                value: (),
            }],
        };
        assert!(node.intersects_range(&..));
        assert!(node.intersects_range(&(11..=11)));
        assert!(!node.intersects_range(&..10));
        assert!(node.intersects_range(&..=10));
        assert!(node.intersects_range(&(14..)));
        assert!(!node.intersects_range(&(15..)));
        assert!(!node.intersects_range(&(16..)));
        assert!(node.intersects_range(&(Bound::Excluded(13), Bound::Unbounded)));
        assert!(!node.intersects_range(&(Bound::Excluded(14), Bound::Unbounded)));
        assert!(!node.intersects_range(&(Bound::Excluded(15), Bound::Unbounded)));
    }
}
