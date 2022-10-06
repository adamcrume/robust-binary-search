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

use crate::CompressedDAG;
use crate::CompressedDAGNodeRef;
use crate::FlakinessTracker;
use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::rc::Rc;

/// Calculates vote inversions over a graph, which can be used to estimate flakiness.
#[derive(Clone, Debug)]
pub(crate) struct CompressedDAGFlakinessTracker {
    graph: Rc<CompressedDAG>,
    votes: BTreeMap<usize, FlakinessTracker>,
}

impl CompressedDAGFlakinessTracker {
    /// Creates a CompressedDAGFlakinessTracker for the given graph.
    pub fn new(graph: Rc<CompressedDAG>) -> Self {
        Self {
            graph,
            votes: BTreeMap::new(),
        }
    }

    /// Adds a vote to the internal statistics. With low flakiness, true votes are expected not to
    /// appear in the ancestors of false votes.
    pub fn report(&mut self, node: CompressedDAGNodeRef, heads: bool) {
        self.votes
            .entry(node.segment)
            .or_insert_with(FlakinessTracker::default)
            .report(node.index, heads);
    }

    /// Returns the number of inversions and four times the number of "random" inverions.
    /// The "random" inversions is the number of inversions that would be expected if the votes were
    /// cast at the same nodes but were randomly half heads and half tails. It is scaled by four
    /// to avoid loss of precision.
    fn inversions(&self) -> (usize, usize) {
        let mut votes_at_segment = HashMap::new();
        let graph: &CompressedDAG = self.graph.borrow();
        for segment in self.votes.keys() {
            let inputs = graph.node(*segment).inputs();
            if !inputs.is_empty() {
                let (input_heads, input_votes) = self
                    .votes
                    .get(&inputs[0])
                    .map(|v| (v.total_heads(), v.total_votes()))
                    .unwrap_or((0, 0));
                let (mut heads, mut votes) = *votes_at_segment.get(&inputs[0]).unwrap_or(&(0, 0));
                heads += input_heads;
                votes += input_votes;
                for ancestor in graph.node(*segment).remainder_ancestors() {
                    let (ancestor_heads, ancestor_votes) = self
                        .votes
                        .get(ancestor)
                        .map(|v| (v.total_heads(), v.total_votes()))
                        .unwrap_or((0, 0));
                    heads += ancestor_heads;
                    votes += ancestor_votes;
                }
                votes_at_segment.insert(segment, (heads, votes));
            }
        }
        let mut inversions = 0;
        let mut random_inversions = 0;
        for (segment, votes) in &self.votes {
            let (segment_heads, segment_votes) = *votes_at_segment.get(&segment).unwrap_or(&(0, 0));
            let (inv, rand_inv) = votes.inversions();
            inversions += votes.total_tails() * segment_heads + inv;
            random_inversions += votes.total_votes() * segment_votes + rand_inv;
        }
        (inversions, random_inversions)
    }

    /// Returns the estimated flakiness based on the votes, where 0.0 is deterministic and 1.0 is
    /// complete randomness.
    pub fn flakiness(&self) -> f64 {
        // See note in FlakinessTracker::flakiness.
        let (inv, rand_inv) = self.inversions();
        let tmp = 1.0 - (inv + 1) as f64 / (rand_inv as f64 / 4.0 + 4.0 / 3.0);
        1.0 - tmp.max(0.0).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CompressedDAGSegment;

    macro_rules! assert_flakiness {
        ($tracker:expr, $flakiness:expr) => {
            let flakiness = $tracker.flakiness();
            assert!(
                (flakiness - $flakiness).abs() < 1e-4,
                "flakiness = {}",
                flakiness
            );
        };
    }

    #[test]
    fn empty() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        assert_eq!(tracker.inversions(), (0, 0));
        assert_flakiness!(tracker, 0.5);
    }

    #[test]
    fn one_head() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (0, 1));
        assert_flakiness!(tracker, 0.3930);
    }

    #[test]
    fn one_tail() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            false,
        );
        assert_eq!(tracker.inversions(), (0, 1));
        assert_flakiness!(tracker, 0.3930);
    }

    #[test]
    fn two_heads_same_bucket() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (0, 4));
        assert_flakiness!(tracker, 0.2441);
    }

    #[test]
    fn two_heads_different_buckets() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 1,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (0, 3));
        assert_flakiness!(tracker, 0.2789);
    }

    #[test]
    fn two_tails_same_bucket() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            false,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            false,
        );
        assert_eq!(tracker.inversions(), (0, 4));
        assert_flakiness!(tracker, 0.2441);
    }

    #[test]
    fn two_tails_different_buckets() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            false,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 1,
            },
            false,
        );
        assert_eq!(tracker.inversions(), (0, 3));
        assert_flakiness!(tracker, 0.2789);
    }

    #[test]
    fn one_head_one_tail_same_bucket() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            false,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (1, 4));
        assert_flakiness!(tracker, 0.622);
    }

    #[test]
    fn one_head_one_tail_inverted() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 1,
            },
            false,
        );
        assert_eq!(tracker.inversions(), (1, 3));
        assert_flakiness!(tracker, 0.8);
    }

    #[test]
    fn one_head_one_tail_not_inverted() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            false,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 1,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (0, 3));
        assert_flakiness!(tracker, 0.2789);
    }

    #[test]
    fn flakiness_scan_one_index() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let graph = Rc::new(graph);
        for i in 0..100 {
            let mut tracker = CompressedDAGFlakinessTracker::new(graph.clone());
            for _ in 0..i {
                tracker.report(
                    CompressedDAGNodeRef {
                        segment: 0,
                        index: 0,
                    },
                    false,
                );
            }
            for _ in i..100 {
                tracker.report(
                    CompressedDAGNodeRef {
                        segment: 0,
                        index: 0,
                    },
                    true,
                );
            }
            let expected_flakiness = if i < 50 { i } else { 100 - i } as f64 / 50.0;
            assert!(
                (tracker.flakiness() - expected_flakiness).abs() < 0.02,
                "i = {}, flakiness = {}, expected_flakiness = {}",
                i,
                tracker.flakiness(),
                expected_flakiness
            );
        }
    }

    #[test]
    fn flakiness_scan_two_indexes() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let graph = Rc::new(graph);
        for i in 0..100 {
            let mut tracker = CompressedDAGFlakinessTracker::new(graph.clone());
            for _ in 0..i {
                tracker.report(
                    CompressedDAGNodeRef {
                        segment: 0,
                        index: 0,
                    },
                    true,
                );
                tracker.report(
                    CompressedDAGNodeRef {
                        segment: 0,
                        index: 1,
                    },
                    true,
                );
            }
            for _ in i..100 {
                tracker.report(
                    CompressedDAGNodeRef {
                        segment: 0,
                        index: 0,
                    },
                    false,
                );
                tracker.report(
                    CompressedDAGNodeRef {
                        segment: 0,
                        index: 1,
                    },
                    false,
                );
            }
            let expected_flakiness = if i < 50 { i } else { 100 - i } as f64 / 50.0;
            assert!(
                (tracker.flakiness() - expected_flakiness).abs() < 0.02,
                "i = {}, flakiness = {}, expected_flakiness = {}",
                i,
                tracker.flakiness(),
                expected_flakiness
            );
        }
    }

    #[test]
    fn hundred_heads_same_bucket() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        for _ in 0..100 {
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 0,
                },
                true,
            );
        }
        assert_eq!(tracker.inversions(), (0, 10000));
        assert_flakiness!(tracker, 0.0002);
    }

    #[test]
    fn hundred_heads_one_tail_same_bucket() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        for _ in 0..100 {
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 0,
                },
                true,
            );
        }
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            false,
        );
        assert_eq!(tracker.inversions(), (100, 10201));
        assert_flakiness!(tracker, 0.02);
    }

    #[test]
    fn hundred_heads_hundred_tails_same_bucket() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        for _ in 0..100 {
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 0,
                },
                true,
            );
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 0,
                },
                false,
            );
        }
        assert_eq!(tracker.inversions(), (10000, 40000));
        assert_flakiness!(tracker, 0.9942);
    }

    #[test]
    fn hundred_heads_hundred_tails_different_buckets() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        for _ in 0..100 {
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 0,
                },
                true,
            );
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 0,
                },
                false,
            );
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 1,
                },
                true,
            );
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 1,
                },
                false,
            );
        }
        assert_eq!(tracker.inversions(), (30000, 120000));
        assert_flakiness!(tracker, 0.9967);
    }

    #[test]
    fn hundred_heads_hundred_tails_inverted() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        for _ in 0..100 {
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 0,
                },
                true,
            );
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 1,
                },
                false,
            );
        }
        assert_eq!(tracker.inversions(), (10000, 30000));
        assert_flakiness!(tracker, 1.0);
    }

    #[test]
    fn hundred_heads_hundred_tails_not_inverted() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        for _ in 0..100 {
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 0,
                },
                false,
            );
            tracker.report(
                CompressedDAGNodeRef {
                    segment: 0,
                    index: 1,
                },
                true,
            );
        }
        assert_eq!(tracker.inversions(), (0, 30000));
        assert_flakiness!(tracker, 0.0);
    }

    #[test]
    fn two_heads_sequential_segments() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 1,
                index: 0,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (0, 3));
        assert_flakiness!(tracker, 0.2789);
    }

    #[test]
    fn one_head_one_tail_sequential_segments_inverted() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 1,
                index: 0,
            },
            false,
        );
        assert_eq!(tracker.inversions(), (1, 3));
        assert_flakiness!(tracker, 0.8);
    }

    #[test]
    fn one_head_one_tail_sequential_segments_not_inverted() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            false,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 1,
                index: 0,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (0, 3));
        assert_flakiness!(tracker, 0.2789);
    }

    #[test]
    fn two_heads_parallel_segments() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 1,
                index: 0,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (0, 2));
        assert_flakiness!(tracker, 0.3258);
    }

    #[test]
    fn three_heads_join() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0, 1]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 1,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 2,
                index: 0,
            },
            true,
        );
        assert_eq!(tracker.inversions(), (0, 5));
        assert_flakiness!(tracker, 0.2171);
    }

    #[test]
    fn half_inverted_join() {
        let mut graph = CompressedDAG::default();
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![]);
        graph.add_node(CompressedDAGSegment::new(10), vec![0, 1]);
        let mut tracker = CompressedDAGFlakinessTracker::new(Rc::new(graph));
        tracker.report(
            CompressedDAGNodeRef {
                segment: 0,
                index: 0,
            },
            true,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 1,
                index: 0,
            },
            false,
        );
        tracker.report(
            CompressedDAGNodeRef {
                segment: 2,
                index: 0,
            },
            false,
        );
        assert_eq!(tracker.inversions(), (1, 5));
        assert_flakiness!(tracker, 0.5248);
    }
}
