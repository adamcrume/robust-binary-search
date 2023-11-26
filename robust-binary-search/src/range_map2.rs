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

use std::collections::BTreeMap;
use std::ops::RangeBounds;

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
    values: BTreeMap<usize, RangeMapEntry<T>>,
}

impl<T: Clone> RangeMap<T> {
    /// Creates a new RangeMap with the given size and initial value. It contains a single entry
    /// spanning the entire range.
    pub fn new(size: usize, value: T) -> Self {
        RangeMap {
            values: BTreeMap::from([(
                0,
                RangeMapEntry {
                    offset: 0,
                    len: size,
                    value,
                },
            )]),
        }
    }

    /// Returns the length of the entire range.
    pub fn len(&self) -> usize {
        self.values.last_key_value().unwrap().1.end()
    }

    /// Returns an iterator over entries.
    pub fn ranges(&self) -> impl DoubleEndedIterator<Item = &RangeMapEntry<T>> {
        self.values.iter().map(|x| x.1)
    }

    /// Returns an iterator over mutable entries.
    pub fn ranges_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>> {
        self.values.iter_mut().map(|x| x.1)
    }

    /// Returns the entry containing the given index.
    pub fn range_for_index(&self, index: usize) -> &RangeMapEntry<T> {
        self.values.range(..=index).next_back().unwrap().1
    }

    /// Ensures that `index-1` and `index` are in different RangeMapEntrys.
    fn _split(&mut self, index: usize)
    where
        T: std::fmt::Debug,
    {
        let entry = self.values.range_mut(..=index).next_back().unwrap().1;
        if entry.offset == index {
            return;
        }
        let end = entry.end();
        if index == end {
            return;
        }
        entry.len = index - entry.offset;
        let value = entry.value.clone();
        self.values.insert(
            index,
            RangeMapEntry {
                offset: index,
                len: end - index,
                value,
            },
        );
    }

    /// Ensures that `index-1` and `index` are in different RangeMapEntrys.
    /// Returns iterators for the left and right side of the split.
    pub fn split(&mut self, index: usize)
    where
        T: std::fmt::Debug,
    {
        // TODO: inline
        self._split(index);
    }

    // TODO: better signature
    // TODO: document
    pub fn range<R: RangeBounds<usize>>(
        &self,
        range: R,
    ) -> impl DoubleEndedIterator<Item = &RangeMapEntry<T>> {
        self.values.range(range).map(|x| x.1)
    }

    // TODO: better signature
    // TODO: document
    pub fn range_mut<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>> {
        self.values.range_mut(range).map(|x| x.1)
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
}
