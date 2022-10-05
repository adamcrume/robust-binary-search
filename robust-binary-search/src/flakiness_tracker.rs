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

/// INTERNAL ONLY.
///
/// Calculates vote inversions in a linear range, which can be used to estimate flakiness.
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct FlakinessTracker {
    /// Maps index to number of number of tails votes and number of heads votes.
    votes: BTreeMap<usize, (usize, usize)>,
    total_heads: usize,
    total_tails: usize,
}

impl FlakinessTracker {
    /// Adds a vote to the internal statistics. With low flakiness, false votes are expected to have
    /// smaller indices than true votes.
    pub fn report(&mut self, index: usize, heads: bool) {
        let value = self.votes.entry(index).or_insert((0, 0));
        value.0 += if heads { 0 } else { 1 };
        value.1 += if heads { 1 } else { 0 };
        if heads {
            self.total_heads += 1;
        } else {
            self.total_tails += 1;
        }
    }

    /// Returns the number of inversions and four times the number of "random" inversions.
    /// The "random" inversions is the number of inversions that would be expected if the votes were
    /// cast at the same indices but were randomly half heads and half tails. It is scaled by four
    /// to avoid loss of precision.
    pub fn inversions(&self) -> (usize, usize) {
        let mut headstotal = 0;
        let mut inverted = 0;
        let mut random_inversions = 0;
        let mut total_votes = 0;
        for (tails, heads) in self.votes.values() {
            let votes = heads + tails;
            random_inversions += votes * votes + votes * total_votes;
            inverted += tails * headstotal + tails * heads;
            headstotal += heads;
            total_votes += votes;
        }
        (inverted, random_inversions)
    }

    pub fn inversions_before(&self, split_ix: usize) -> (usize, usize) {
        let mut headstotal = 0;
        let mut inverted = 0;
        let mut random_inversions = 0;
        let mut total_votes = 0;
        for (key, (tails, heads)) in &self.votes {
            if *key >= split_ix {
                break;
            }
            let votes = heads + tails;
            random_inversions += votes * votes + votes * total_votes;
            inverted += tails * headstotal + tails * heads;
            headstotal += heads;
            total_votes += votes;
        }
        (inverted, random_inversions)
    }

    pub fn inversions_on_or_after(&self, split_ix: usize) -> (usize, usize) {
        let mut headstotal = 0;
        let mut inverted = 0;
        let mut random_inversions = 0;
        let mut total_votes = 0;
        for (key, (tails, heads)) in &self.votes {
            if *key < split_ix {
                continue;
            }
            let votes = heads + tails;
            random_inversions += votes * votes + votes * total_votes;
            inverted += tails * headstotal + tails * heads;
            headstotal += heads;
            total_votes += votes;
        }
        (inverted, random_inversions)
    }

    /// Returns the number of true votes.
    pub fn total_heads(&self) -> usize {
        self.total_heads
    }

    /// Returns the number of false votes.
    pub fn total_tails(&self) -> usize {
        self.total_tails
    }

    /// Returns the total number of votes.
    pub fn total_votes(&self) -> usize {
        self.total_heads + self.total_tails
    }

    /// Returns the estimated flakiness based on the votes, where 0.0 is deterministic and 1.0 is
    /// complete randomness.
    pub fn flakiness(&self) -> f64 {
        // The formula used here is provided by flakiness_tuner.rs (and fit by
        // recovered_flakiness.plt), plus some numerical niceties and a Bayesian prior.
        // ar^2 + br - f = 0
        // (-b + sqrt(b^2 + 4af))/(2a)
        let (inv, rand_inv) = self.inversions();
        let r = (inv + 1) as f64 / (rand_inv as f64 + 7.6143);
        (0.1698 * r * r + 3.7844 * r).min(1.0).max(0.0)
    }

    pub fn split_flakiness(&self, split_ix: usize, prior: f64) -> (f64, f64) {
        let mut tails_before = 0;
        let mut heads_before = 0;
        let mut tails_after = 0;
        let mut heads_after = 0;
        for (key, (tails, heads)) in &self.votes {
            if *key < split_ix {
                tails_before += tails;
                heads_before += heads;
            } else {
                tails_after += tails;
                heads_after += heads;
            }
        }
        let total_before = heads_before + tails_before;
        let total_after = heads_after + tails_after;
        let total = total_before + total_after;
        // // P(H|B)
        // let p_heads_before = (heads_before + 1) as f64 / (total_before + 2) as f64;
        // // P(H|A)
        // let p_heads_after = (heads_after + 1) as f64 / (total_after + 2) as f64;
        // // P(B)
        // let p_before = (total_before + 1) as f64 / (total + 2) as f64;
        // // P(A)
        // let p_after = (total_after + 1) as f64 / (total + 2) as f64;
        // // P(H)
        // let p_heads = (total_heads + 1) as f64 / (total + 2) as f64;
        // // P(A|H)
        // let p_after_heads = p_heads_after * p_after / p_heads;

        // P(B|H)
        let p_before_heads =
            (heads_before as f64 + prior) / (self.total_heads as f64 + 2.0 * prior);
        // P(A|T)
        let p_after_tails = (tails_after as f64 + prior) / (self.total_tails as f64 + 2.0 * prior);
        let flakiness_heads = (2.0 * p_before_heads).min(1.0);
        let flakiness_tails = (2.0 * p_after_tails).min(1.0);
        (flakiness_tails, flakiness_heads)
    }

    pub fn split_flakiness2(&self, split_ix: usize, prior: f64) -> (f64, f64) {
        let mut tails_before = 0;
        let mut heads_before = 0;
        let mut tails_after = 0;
        let mut heads_after = 0;
        for (key, (tails, heads)) in &self.votes {
            if *key < split_ix {
                tails_before += tails;
                heads_before += heads;
            } else {
                tails_after += tails;
                heads_after += heads;
            }
        }
        let total_before = heads_before + tails_before;
        let total_after = heads_after + tails_after;
        let total = total_before + total_after;
        // P(H|B)
        let p_heads_before = (heads_before as f64 + prior) / (total_before as f64 + 2.0 * prior);
        // P(T|A)
        let p_tails_after = (tails_after as f64 + prior) / (total_after as f64 + 2.0 * prior);
        let flakiness_before = (2.0 * p_heads_before).min(1.0);
        let flakiness_after = (2.0 * p_tails_after).min(1.0);
        (flakiness_before, flakiness_after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let tracker = FlakinessTracker::default();
        assert_eq!(tracker.inversions(), (0, 0));
        assert!(
            (tracker.flakiness() - 0.5).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn one_head() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, true);
        assert_eq!(tracker.inversions(), (0, 1));
        assert!(
            (tracker.flakiness() - 0.4416).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn one_tail() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, true);
        assert_eq!(tracker.inversions(), (0, 1));
        assert!(
            (tracker.flakiness() - 0.4416).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn two_heads_same_bucket() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, true);
        tracker.report(0, true);
        assert_eq!(tracker.inversions(), (0, 4));
        assert!(
            (tracker.flakiness() - 0.3271).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn two_heads_different_buckets() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, true);
        tracker.report(1, true);
        assert_eq!(tracker.inversions(), (0, 3));
        assert!(
            (tracker.flakiness() - 0.3581).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn two_tails_same_bucket() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, false);
        tracker.report(0, false);
        assert_eq!(tracker.inversions(), (0, 4));
        assert!(
            (tracker.flakiness() - 0.3271).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn two_tails_different_buckets() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, false);
        tracker.report(1, false);
        assert_eq!(tracker.inversions(), (0, 3));
        assert!(
            (tracker.flakiness() - 0.3581).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn one_head_one_tail_same_bucket() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, false);
        tracker.report(0, true);
        assert_eq!(tracker.inversions(), (1, 4));
        assert!(
            (tracker.flakiness() - 0.6567).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn one_head_one_tail_inverted() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, true);
        tracker.report(1, false);
        assert_eq!(tracker.inversions(), (1, 3));
        assert!(
            (tracker.flakiness() - 0.7191).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn one_head_one_tail_not_inverted() {
        let mut tracker = FlakinessTracker::default();
        tracker.report(0, false);
        tracker.report(1, true);
        assert_eq!(tracker.inversions(), (0, 3));
        assert!(
            (tracker.flakiness() - 0.3580).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn hundred_heads_same_bucket() {
        let mut tracker = FlakinessTracker::default();
        for _ in 0..100 {
            tracker.report(0, true);
        }
        assert_eq!(tracker.inversions(), (0, 10000));
        assert!(
            (tracker.flakiness() - 0.0004).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn hundred_heads_one_tail_same_bucket() {
        let mut tracker = FlakinessTracker::default();
        for _ in 0..100 {
            tracker.report(0, true);
        }
        tracker.report(0, false);
        assert_eq!(tracker.inversions(), (100, 10201));
        assert!(
            (tracker.flakiness() - 0.0375).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn hundred_heads_hundred_tails_same_bucket() {
        let mut tracker = FlakinessTracker::default();
        for _ in 0..100 {
            tracker.report(0, false);
            tracker.report(0, true);
        }
        assert_eq!(tracker.inversions(), (10000, 40000));
        assert!(
            (tracker.flakiness() - 0.9566).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn hundred_heads_hundred_tails_inverted() {
        let mut tracker = FlakinessTracker::default();
        for _ in 0..100 {
            tracker.report(0, true);
            tracker.report(1, false);
        }
        assert_eq!(tracker.inversions(), (10000, 30000));
        assert!(
            (tracker.flakiness() - 0.9999).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn hundred_heads_hundred_tails_not_inverted() {
        let mut tracker = FlakinessTracker::default();
        for _ in 0..100 {
            tracker.report(0, false);
            tracker.report(1, true);
        }
        assert_eq!(tracker.inversions(), (0, 30000));
        assert!(
            (tracker.flakiness() - 0.0001).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }

    #[test]
    fn half_not_flaky() {
        let mut tracker = FlakinessTracker::default();
        for i in 0..500 {
            tracker.report(i, false);
        }
        for i in (500..1000).step_by(2) {
            tracker.report(i, false);
            tracker.report(i + 1, true);
        }
        assert_eq!(tracker.inversions_before(500), (0, 125250));
        assert_eq!(tracker.inversions_on_or_after(500), (31125, 125250));
        // TODO: better testing
        assert!(
            (tracker.flakiness() - 0.236).abs() < 1e-4,
            "flakiness = {}",
            tracker.flakiness()
        );
    }
}
