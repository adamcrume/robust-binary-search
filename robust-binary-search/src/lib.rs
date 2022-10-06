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

use log::trace;
use std::borrow::Borrow;
use std::cmp;
use std::rc::Rc;

#[doc(hidden)]
pub mod flakiness_tracker;
use flakiness_tracker::*;
mod range_map;
use range_map::*;

mod dag;

/// Reference to a node in a CompressedDAG.
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub struct CompressedDAGNodeRef {
    /// Index of the segment in the CompressedDAG.
    pub segment: usize,
    /// Index of the expanded node within the segment.
    pub index: usize,
}

/// A segment in a CompressedDAG. This is a node in a DAG but corresponds to a linear sequence of
/// nodes in a conceptual expanded graph. The size is the number of nodes in the expanded graph
/// represented by this segment.
#[derive(Clone, Debug)]
pub struct CompressedDAGSegment {
    len: usize,
}

impl CompressedDAGSegment {
    /// Creates a CompressedDAGSegment of a given size.
    pub fn new(len: usize) -> Self {
        CompressedDAGSegment { len }
    }

    /// Returns the size of the segment.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the segment is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A DAG whose nodes are CompressedDAGSegments, which represent sequences of nodes in a conceptual
/// expanded graph. For example, given the graph:
///
/// ```text
///   B-C-D
///  /     \
/// A       G
///  \     /
///   E---F
/// ```
///
/// this can be expressed in a CompressedDAG as:
///
/// ```text
///   B'
///  / \
/// A'  G'
///  \ /
///   E'
/// ```
///
/// where `A'` and `G'` are segments of size 1 corresponding to `A` and `G`, `E'` is a segment of
/// size 2 corresponding to `E` and `F`, and `B'` is a segment of size 3 corresponding to `B`, `C`,
/// and `D`.
///
/// More formally, the nodes represented by a segment must be in a linear formation (i.e. directed,
/// acyclic, connected, with each node having at most one incoming edge from another node in the
/// segment and at most one outgoing edge to another node in the segment), with only the first node
/// allowing edges from outside the segment, and only the last node allowing edges to outside the
/// segment.
///
/// This representation allows many common graphs to be represented in a more compact form than
/// directly as a DAG.
pub type CompressedDAG = dag::DAG<CompressedDAGSegment>;

mod compressed_dag_flakiness_tracker;
use compressed_dag_flakiness_tracker::*;

/// Finds the index such that the sum of values at indices [0, i] (inclusive) is as close as
/// possible to the argument. Returns the index and the sum.
fn confidence_percentile_nearest(range_map: &RangeMap<f64>, percentile: f64) -> (usize, f64) {
    let mut sum = 0.0;
    let mut index = 0;
    let mut best_index = 0;
    let mut best_percentile = f64::NEG_INFINITY;
    for w in range_map.ranges() {
        let delta = w.len() as f64 * w.value();
        trace!(
            "percentile = {}, sum = {}, w.value = {}",
            percentile,
            sum,
            w.value()
        );
        trace!(
            "(percentile - sum) / w.value() - 0.5 = {}",
            (percentile - sum) / w.value() - 0.5
        );
        let ix = index
            + cmp::min(
                w.len() - 1,
                ((percentile - sum) / w.value() - 0.5).max(0.0) as usize,
            );
        let ix_percentile = sum + (ix - index + 1) as f64 * w.value();
        trace!("ix = {} ix_percentile = {}", ix, ix_percentile);
        if (ix_percentile - percentile).abs() < (best_percentile - percentile).abs() {
            best_index = ix;
            best_percentile = ix_percentile;
        }
        sum += delta;
        index += w.len();
    }
    assert!(best_percentile > f64::NEG_INFINITY);
    trace!(
        "confidence_percentile_nearest returning {:?}",
        (best_index, best_percentile)
    );
    (best_index, best_percentile)
}

/// Finds the smallest index such that the sum of values at indices [0, i] (inclusive) is greater
/// than or equal to the argument. Returns the index and the sum. If no sum is greater than or equal
/// to the argument, returns the last index and the sum over all values.
fn confidence_percentile_ceil(range_map: &RangeMap<f64>, percentile: f64) -> (usize, f64) {
    let mut sum = 0.0;
    let mut index = 0;
    for w in range_map.ranges() {
        let delta = w.len() as f64 * w.value();
        if sum + delta >= percentile {
            let ix = index + ((percentile - sum) / w.value() - 1e-9) as usize;
            let ret = (ix, sum + (ix - index + 1) as f64 * w.value());
            trace!("confidence_percentile_ceil returning {:?}", ret);
            return ret;
        }
        sum += delta;
        index += w.len();
    }
    (range_map.len() - 1, sum)
}

// Does not normalize.
fn report_range(weights: &mut RangeMap<f64>, index: usize, heads: bool, stiffness: f64) {
    if heads {
        for w in weights.split(index).0 {
            *w.value_mut() *= 1.0 + stiffness;
        }
        let (left, _right) = weights.split(index + 1);
        *left.rev().next().unwrap().value_mut() *= 1.0 + stiffness;
    } else {
        weights.split(index);
        let (_left, right) = weights.split(index + 1);
        for w in right {
            *w.value_mut() *= 1.0 + stiffness;
        }
    }
}

/// Performs a robust binary search over a linear range.
#[derive(Clone, Debug)]
pub struct Searcher {
    weights: RangeMap<f64>,
    len: usize,
}

impl Searcher {
    /// Creates a new Searcher over a range with the given number of testable indices.
    pub fn new(len: usize) -> Self {
        Searcher {
            weights: RangeMap::new(len + 1, 1.0 / (len as f64 + 1.0)),
            len,
        }
    }

    /// Same as `report` but with a specified stiffness. Only public for use by the tuner, not for
    /// public use.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    #[doc(hidden)]
    pub fn report_with_stiffness(&mut self, index: usize, heads: bool, stiffness: f64) {
        assert!(index < self.len);
        report_range(&mut self.weights, index, heads, stiffness);
        let weight_sum: f64 = self
            .weights
            .ranges()
            .map(|w| w.value() * w.len() as f64)
            .sum();
        for w in self.weights.ranges_mut() {
            *w.value_mut() /= weight_sum;
        }
    }

    /// Adds a vote to the internal statistics. With low flakiness, false votes are expected to have
    /// smaller indices than true votes.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    pub fn report(&mut self, index: usize, heads: bool, flakiness: f64) {
        self.report_with_stiffness(index, heads, optimal_stiffness(flakiness));
    }

    /// Returns the next index that should be tested. Can return values in the range 0 to len,
    /// exclusive.
    pub fn next_index(&self) -> usize {
        cmp::min(
            confidence_percentile_nearest(&self.weights, 0.5).0,
            self.len - 1,
        )
    }

    /// Returns the current estimate of the best index. Can return values in the range 0 to len,
    /// inclusive.
    pub fn best_index(&self) -> usize {
        confidence_percentile_ceil(&self.weights, 0.5).0
    }

    /// Only public for use by the tuner, not for public use.
    #[doc(hidden)]
    pub fn confidence_percentile_ceil(&self, percentile: f64) -> usize {
        confidence_percentile_ceil(&self.weights, percentile).0
    }

    /// Returns the likelihood of the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    pub fn likelihood(&self, index: usize) -> f64 {
        *self.weights.range_for_index(index).value()
    }
}

/// INTERNAL ONLY.
///
/// Returns the stiffness which should be optimal for the given flakiness.
#[doc(hidden)]
pub fn optimal_stiffness(flakiness: f64) -> f64 {
    // Values calculated by tuner.rs
    (2.6 / flakiness.powf(0.37))
        .min(0.58 / flakiness.powf(0.97))
        .min(0.19 / flakiness.powf(2.4))
}

/// Performs a robust binary search over a linear range and automatically infers the flakiness based
/// on the votes.
#[derive(Clone, Debug)]
pub struct AutoSearcher {
    searcher: Searcher,
    flakiness_tracker: FlakinessTracker,
}

impl AutoSearcher {
    /// Creates a new AutoSearcher over a range with the given number of testable indices.
    pub fn new(len: usize) -> Self {
        AutoSearcher {
            searcher: Searcher::new(len),
            flakiness_tracker: FlakinessTracker::default(),
        }
    }

    /// Adds a vote to the internal statistics. With low flakiness, false votes are expected to have
    /// smaller indices than true votes.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    pub fn report(&mut self, index: usize, heads: bool) {
        self.flakiness_tracker.report(index, heads);
        self.searcher
            .report(index, heads, self.flakiness_tracker.flakiness());
    }

    /// Returns the next index that should be tested. Can return values in the range 0 to len,
    /// exclusive.
    pub fn next_index(&self) -> usize {
        self.searcher.next_index()
    }

    /// Returns the current estimate of the best index. Can return values in the range 0 to len,
    /// inclusive.
    pub fn best_index(&self) -> usize {
        self.searcher.best_index()
    }

    /// Returns the likelihood of the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    pub fn likelihood(&self, index: usize) -> f64 {
        self.searcher.likelihood(index)
    }
}

/// Performs a robust binary search over a CompressedDAG.
#[derive(Clone, Debug)]
pub struct CompressedDAGSearcher {
    graph: Rc<CompressedDAG>,
    segment_range_maps: Vec<RangeMap<f64>>,
}

impl CompressedDAGSearcher {
    /// Creates a new CompressedDAGSearcher.
    pub fn new(graph: Rc<CompressedDAG>) -> Self {
        let n = graph
            .nodes()
            .iter()
            .map(|node| node.value().len())
            .sum::<usize>();
        let segment_range_maps = graph
            .nodes()
            .iter()
            .map(|node| RangeMap::new(node.value().len(), 1.0 / n as f64))
            .collect();
        CompressedDAGSearcher {
            graph,
            segment_range_maps,
        }
    }

    /// Returns the sums at the beginning and end of every segment. Each vector entry corresponds to
    /// a single segment. The first entry in the tuple is the sum of all weights in the segment's
    /// ancestors (i.e. source segments will have a start of 0.0), and the second entry is the sum
    /// of all weights in the segment and its ancestors.
    fn segment_percentile_ranges(&self) -> Vec<(f64, f64)> {
        let mut segment_ranges = Vec::<(f64, f64)>::new();
        let mut segment_sums = Vec::<f64>::new();
        let graph: &CompressedDAG = self.graph.borrow();
        for (i, range_map) in self.segment_range_maps.iter().enumerate() {
            let inputs = graph.node(i).inputs();
            let start = if inputs.is_empty() {
                0.0
            } else {
                let mut start = segment_ranges[inputs[0]].1;
                for ancestor in graph.node(i).remainder_ancestors() {
                    start += segment_sums[*ancestor];
                }
                start
            };
            let mut segment_sum = 0.0;
            for range in range_map.ranges() {
                segment_sum += range.value() * range.len() as f64;
            }
            segment_sums.push(segment_sum);
            let end = start + segment_sum;
            assert!(
                (0.0..=1.0 + 1e-11).contains(&start) && (0.0..=1.0 + 1e-11).contains(&end),
                "i = {} of {}, start = {}, end = {}",
                i,
                self.segment_range_maps.len(),
                start,
                end
            );
            segment_ranges.push((start, end));
        }
        segment_ranges
    }

    /// Returns the node whose percentile (i.e. the sum of weights over the node and its ancestors)
    /// is nearest the argument.
    fn confidence_percentile_nearest(&self, percentile: f64) -> CompressedDAGNodeRef {
        let segment_ranges = self.segment_percentile_ranges();
        trace!("segment_ranges = {:?}", segment_ranges);
        let mut best_node = CompressedDAGNodeRef {
            segment: 0,
            index: 0,
        };
        let mut best_value = f64::NEG_INFINITY;
        for (i, range) in segment_ranges.iter().enumerate() {
            let (ix, mut value) =
                confidence_percentile_nearest(&self.segment_range_maps[i], percentile - range.0);
            value += range.0;
            if (percentile - value).abs() < (percentile - best_value).abs() {
                best_node = CompressedDAGNodeRef {
                    segment: i,
                    index: ix,
                };
                best_value = value;
            }
        }
        assert!(best_value > f64::NEG_INFINITY);
        best_node
    }

    /// Returns the node whose percentile (i.e. the sum of weights over the node and its ancestors)
    /// is smallest but greater than or equal to the argument.
    pub fn confidence_percentile_ceil(&self, percentile: f64) -> CompressedDAGNodeRef {
        let segment_ranges = self.segment_percentile_ranges();
        let mut min_end = 0;
        let mut min_end_segment = 0;
        let mut min_end_value = f64::INFINITY;
        for (i, range) in segment_ranges.iter().enumerate() {
            let (ix, mut value) =
                confidence_percentile_ceil(&self.segment_range_maps[i], percentile - range.0);
            value += range.0;
            trace!(
                "i = {}, ix = {}, value = {}, min_end_value = {}",
                i,
                ix,
                value,
                min_end_value
            );
            if value < min_end_value && value >= percentile {
                min_end = ix;
                min_end_segment = i;
                min_end_value = value;
            }
        }
        let ret = CompressedDAGNodeRef {
            segment: min_end_segment,
            index: min_end,
        };
        trace!(
            "CompressedDAGSearcher::confidence_percentile_ceil returning {:?}",
            ret
        );
        ret
    }

    /// Returns the current estimate of the best node.
    pub fn best_node(&self) -> CompressedDAGNodeRef {
        self.confidence_percentile_ceil(0.5)
    }

    /// Returns the next node that should be tested.
    pub fn next_node(&self) -> CompressedDAGNodeRef {
        self.confidence_percentile_nearest(0.5)
    }

    /// Adds a vote to the internal statistics. With low flakiness, nodes with false votes are
    /// expected not to nodes with true votes as ancestors.
    ///
    /// # Panics
    ///
    /// Panics if the node is out of range.
    pub fn report(&mut self, node: CompressedDAGNodeRef, heads: bool, flakiness: f64) {
        let stiffness = optimal_stiffness(flakiness);
        let graph: &CompressedDAG = self.graph.borrow();
        if heads {
            for segment in graph.node(node.segment).ancestors() {
                for w in self.segment_range_maps[*segment].ranges_mut() {
                    *w.value_mut() *= 1.0 + stiffness;
                }
            }
        } else {
            let ancestor_segments = graph.node(node.segment).ancestors();
            for segment in 0..graph.nodes().len() {
                if ancestor_segments.contains(&segment) || segment == node.segment {
                    continue;
                }
                for w in self.segment_range_maps[segment].ranges_mut() {
                    *w.value_mut() *= 1.0 + stiffness;
                }
            }
        }
        report_range(
            &mut self.segment_range_maps[node.segment],
            node.index,
            heads,
            stiffness,
        );
        let weight_sum: f64 = self
            .segment_range_maps
            .iter()
            .map(|range_map| {
                range_map
                    .ranges()
                    .map(|w| w.value() * w.len() as f64)
                    .sum::<f64>()
            })
            .sum();
        for range_map in &mut self.segment_range_maps {
            for w in range_map.ranges_mut() {
                *w.value_mut() /= weight_sum;
            }
        }
    }

    /// Returns the likelihood of the given index.
    ///
    /// # Panics
    ///
    /// Panics if the node is out of range.
    pub fn likelihood(&self, node: CompressedDAGNodeRef) -> f64 {
        *self.segment_range_maps[node.segment]
            .range_for_index(node.index)
            .value()
    }
}

/// Performs a robust binary search over a CompressedDAG and automatically infers the flakiness
/// based on the votes.
#[derive(Clone, Debug)]
pub struct AutoCompressedDAGSearcher {
    searcher: CompressedDAGSearcher,
    flakiness_tracker: CompressedDAGFlakinessTracker,
}

impl AutoCompressedDAGSearcher {
    /// Creates a new AutoCompressedDAGSearcher.
    pub fn new(graph: Rc<CompressedDAG>) -> Self {
        Self {
            searcher: CompressedDAGSearcher::new(graph.clone()),
            flakiness_tracker: CompressedDAGFlakinessTracker::new(graph),
        }
    }

    /// Adds a vote to the internal statistics. With low flakiness, nodes with false votes are
    /// expected not to nodes with true votes as ancestors.
    ///
    /// # Panics
    ///
    /// Panics if the node is out of range.
    pub fn report(&mut self, node: CompressedDAGNodeRef, heads: bool) {
        self.flakiness_tracker.report(node, heads);
        self.searcher
            .report(node, heads, self.flakiness_tracker.flakiness());
    }

    /// Returns the next node that should be tested.
    pub fn next_node(&self) -> CompressedDAGNodeRef {
        self.searcher.next_node()
    }

    /// Returns the current estimate of the best node.
    pub fn best_node(&self) -> CompressedDAGNodeRef {
        self.searcher.best_node()
    }

    /// Returns the likelihood of the given index.
    ///
    /// # Panics
    ///
    /// Panics if the node is out of range.
    pub fn likelihood(&self, index: CompressedDAGNodeRef) -> f64 {
        self.searcher.likelihood(index)
    }

    /// Returns the estimated flakiness.
    pub fn flakiness(&self) -> f64 {
        self.flakiness_tracker.flakiness()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_FLAKINESS: f64 = 0.01;

    macro_rules! assert_index {
        ($searcher:expr, $next:expr, $best:expr, $heads:expr, $flakiness:expr) => {
            assert_eq!($searcher.next_index(), $next, "next_index");
            assert_eq!($searcher.best_index(), $best, "best_index");
            $searcher.report($next, $heads, $flakiness);
        };
    }

    // Each test should run until a cycle repeats itself three times, and the
    // best_index is stable. The cycle may consist of a single element.

    #[test]
    fn one_element_zero() {
        let mut s = Searcher::new(1);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn one_element_one() {
        let mut s = Searcher::new(1);
        assert_index!(s, 0, 0, false, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn two_elements_zero() {
        let mut s = Searcher::new(2);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn two_elements_one() {
        let mut s = Searcher::new(2);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn two_elements_two() {
        let mut s = Searcher::new(2);
        assert_index!(s, 1, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 2, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 2, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 2, false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn three_elements_zero() {
        let mut s = Searcher::new(3);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn three_elements_one() {
        let mut s = Searcher::new(3);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn three_elements_two() {
        let mut s = Searcher::new(3);
        assert_index!(s, 1, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 2, 2, true, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 2, false, DEFAULT_FLAKINESS);
        assert_index!(s, 2, 2, true, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 2, false, DEFAULT_FLAKINESS);
        assert_index!(s, 2, 2, true, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 2, false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn three_elements_three() {
        let mut s = Searcher::new(3);
        assert_index!(s, 1, 1, false, DEFAULT_FLAKINESS);
        assert_index!(s, 2, 2, false, DEFAULT_FLAKINESS);
        assert_index!(s, 2, 3, false, DEFAULT_FLAKINESS);
        assert_index!(s, 2, 3, false, DEFAULT_FLAKINESS);
        assert_index!(s, 2, 3, false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn many_elements_first() {
        let mut s = Searcher::new(1024);
        assert_index!(s, 512, 512, true, DEFAULT_FLAKINESS);
        assert_index!(s, 272, 273, true, DEFAULT_FLAKINESS);
        assert_index!(s, 144, 145, true, DEFAULT_FLAKINESS);
        assert_index!(s, 76, 77, true, DEFAULT_FLAKINESS);
        assert_index!(s, 40, 41, true, DEFAULT_FLAKINESS);
        assert_index!(s, 21, 21, true, DEFAULT_FLAKINESS);
        assert_index!(s, 11, 11, true, DEFAULT_FLAKINESS);
        assert_index!(s, 5, 6, true, DEFAULT_FLAKINESS);
        assert_index!(s, 2, 3, true, DEFAULT_FLAKINESS);
        assert_index!(s, 1, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 1, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
        assert_index!(s, 0, 0, true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn many_elements_last() {
        let mut s = Searcher::new(1024);
        assert_index!(s, 512, 512, false, DEFAULT_FLAKINESS);
        assert_index!(s, 751, 752, false, DEFAULT_FLAKINESS);
        assert_index!(s, 879, 879, false, DEFAULT_FLAKINESS);
        assert_index!(s, 947, 947, false, DEFAULT_FLAKINESS);
        assert_index!(s, 983, 983, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1002, 1003, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1012, 1013, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1018, 1018, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1021, 1021, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1022, 1023, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1023, 1023, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1023, 1024, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1023, 1024, false, DEFAULT_FLAKINESS);
        assert_index!(s, 1023, 1024, false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_confidence_percentile_nearest_singleton() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(1), vec![]);
        let searcher = CompressedDAGSearcher::new(Rc::new(graph));
        assert_eq!(
            searcher.confidence_percentile_nearest(0.5),
            CompressedDAGNodeRef {
                segment: 0,
                index: 0
            }
        );
    }

    #[test]
    fn graph_confidence_percentile_nearest_single_segment() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let searcher = CompressedDAGSearcher::new(Rc::new(graph));
        assert_eq!(
            searcher.confidence_percentile_nearest(0.5),
            CompressedDAGNodeRef {
                segment: 0,
                index: 4
            }
        );
    }

    #[test]
    fn graph_confidence_percentile_nearest_parallel_segments() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let searcher = CompressedDAGSearcher::new(Rc::new(graph));
        assert_eq!(
            searcher.confidence_percentile_nearest(0.5),
            CompressedDAGNodeRef {
                segment: 0,
                index: 9
            }
        );
    }

    #[test]
    fn graph_confidence_percentile_nearest_parallel_unequal_segments() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let searcher = CompressedDAGSearcher::new(Rc::new(graph));
        assert_eq!(
            searcher.confidence_percentile_nearest(0.5),
            CompressedDAGNodeRef {
                segment: 0,
                index: 54
            }
        );
    }

    #[test]
    fn graph_confidence_percentile_nearest_parallel_unequal_segments2() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        let searcher = CompressedDAGSearcher::new(Rc::new(graph));
        assert_eq!(
            searcher.confidence_percentile_nearest(0.5),
            CompressedDAGNodeRef {
                segment: 1,
                index: 54
            }
        );
    }

    #[test]
    fn graph_confidence_percentile_nearest_sequential_segments() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0]);
        graph.add_node(CompressedDAGSegment::new(10), vec![1]);
        let searcher = CompressedDAGSearcher::new(Rc::new(graph));
        assert_eq!(
            searcher.confidence_percentile_nearest(0.5),
            CompressedDAGNodeRef {
                segment: 1,
                index: 4
            }
        );
    }

    #[test]
    fn graph_confidence_percentile_nearest_fork() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0]);
        let searcher = CompressedDAGSearcher::new(Rc::new(graph));
        assert_eq!(
            searcher.confidence_percentile_nearest(0.5),
            CompressedDAGNodeRef {
                segment: 1,
                index: 4
            }
        );
    }

    #[test]
    fn graph_confidence_percentile_nearest_merge() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0, 1]);
        let searcher = CompressedDAGSearcher::new(Rc::new(graph));
        assert_eq!(
            searcher.confidence_percentile_nearest(0.5),
            CompressedDAGNodeRef {
                segment: 0,
                index: 9
            }
        );
    }

    macro_rules! assert_graph_index {
        ($searcher:expr, $next:expr, $best:expr, $heads:expr, $flakiness:expr) => {
            assert_eq!(
                $searcher.next_node(),
                CompressedDAGNodeRef {
                    segment: $next.0,
                    index: $next.1
                },
                "next_index"
            );
            assert_eq!(
                $searcher.best_node(),
                CompressedDAGNodeRef {
                    segment: $best.0,
                    index: $best.1
                },
                "best_index"
            );
            $searcher.report(
                CompressedDAGNodeRef {
                    segment: $next.0,
                    index: $next.1,
                },
                $heads,
                $flakiness,
            );
        };
    }

    #[test]
    fn graph_two_elements_zero() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(2), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 0), (0, 0), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 0), (0, 0), true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_two_elements_one() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(2), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 0), (0, 0), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 0), (0, 1), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 0), (0, 1), false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_many_elements_last() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(1024), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 511), (0, 511), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 750), (0, 751), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 878), (0, 878), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 946), (0, 946), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 982), (0, 982), false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_parallel_first_first() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 99), (0, 99), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 52), (0, 53), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 27), (0, 28), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 14), (0, 14), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 7), (0, 7), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 3), (0, 4), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 1), (0, 2), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 0), (0, 1), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 0), (0, 0), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 0), (0, 0), true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_parallel_first_last() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 99), (0, 99), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 52), (0, 53), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 77), (0, 78), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 90), (0, 91), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 97), (0, 98), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 68), (1, 69), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 99), (0, 99), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 98), (0, 98), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 99), (0, 99), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 98), (0, 99), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 99), (0, 99), false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_parallel_last_first() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 99), (0, 99), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 52), (1, 53), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 27), (1, 28), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 14), (1, 14), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 7), (1, 7), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 3), (1, 4), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 1), (1, 2), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 0), (1, 1), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 0), (1, 0), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 0), (1, 0), true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_parallel_last_last() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 99), (0, 99), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 52), (1, 53), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 77), (1, 78), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 90), (1, 91), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 97), (1, 98), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 68), (0, 69), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 99), (1, 99), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 98), (1, 98), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 99), (1, 99), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 98), (1, 99), false, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_parallel_first_half() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 99), (0, 99), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 52), (0, 53), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 27), (0, 28), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 40), (0, 41), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 47), (0, 48), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 51), (0, 51), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 49), (0, 49), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 50), (0, 51), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 49), (0, 50), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (0, 50), (0, 50), true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_parallel_second_half() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (0, 99), (0, 99), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 52), (1, 53), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 27), (1, 28), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 40), (1, 41), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 47), (1, 48), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 51), (1, 51), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 49), (1, 49), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 50), (1, 51), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 49), (1, 50), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (1, 50), (1, 50), true, DEFAULT_FLAKINESS);
    }

    #[test]
    fn graph_fork_join() {
        //      /-1-\
        // *-0-*     *-3-*
        //      \-2-/
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(100), vec![]);
        graph.add_node(CompressedDAGSegment::new(100), vec![0]);
        graph.add_node(CompressedDAGSegment::new(100), vec![0]);
        graph.add_node(CompressedDAGSegment::new(100), vec![1, 2]);
        let mut s = CompressedDAGSearcher::new(Rc::new(graph));
        assert_graph_index!(s, (1, 99), (1, 99), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 99), (2, 99), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 49), (2, 50), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 76), (2, 76), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 62), (2, 62), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 54), (2, 55), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 50), (2, 50), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 31), (2, 31), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 49), (2, 49), false, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 50), (2, 50), true, DEFAULT_FLAKINESS);
        assert_graph_index!(s, (2, 49), (2, 50), false, DEFAULT_FLAKINESS);
    }
}
