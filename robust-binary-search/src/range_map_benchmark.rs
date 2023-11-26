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

#![allow(dead_code)] // TODO: remove
#![allow(unused_imports)] // TODO: remove

use bench_compare::ParametricBencher;
use rand::thread_rng;
use rand::Rng;
use rand_distr::Normal;
use robust_binary_search::range_map;
use robust_binary_search::range_map2;
use robust_binary_search::range_map3;
use robust_binary_search::range_map4;
use robust_binary_search::range_map5;
use robust_binary_search::range_map6;
use std::error::Error;
use std::iter;

fn main() -> Result<(), Box<dyn Error>> {
    let indexes = {
        let mut rng = rand::thread_rng();
        let dist = Normal::new(500_000.0f32, 10_000.0).unwrap();
        iter::repeat_with(|| rng.sample(dist).max(0.0).min(999_999.0) as usize)
            .take(128_000)
            .collect::<Vec<_>>()
    };
    let mut b = ParametricBencher::default();
    b.set_samples(1000);
    //   b.add_params([10usize, 100, 1000, 2_000, 4_000, 8_000, 16_000, 32_000]);
    //    b.add_params([4_000, 8_000, 16_000, 32_000, 64_000]);
    // b.add_params([4_000, 8_000, 16_000]);
    b.add_params([10usize, 100, 1000]);
    b.add_test("map", |n| {
        let mut m = range_map::RangeMap::new(1_000_000, 0);
        for i in &indexes[0..*n] {
            m.split(*i);
        }
        m
    });
    // b.add_test("map2", |n| {
    //     let mut m = range_map2::RangeMap::new(1_000_000, 0);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    // b.add_test("map3", |n| {
    //     let mut m = range_map3::RangeMap::new(1_000_000, 0);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    b.add_test("map4", |n| {
        let mut m = range_map4::RangeMap::new(1_000_000, 0);
        for i in &indexes[0..*n] {
            let _ = m.split(*i);
        }
        m
    });
    // b.add_test("map5", |n| {
    //     let mut m = range_map5::RangeMap::new(1_000_000, 0);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    // b.add_test("map52", |n| {
    //     let mut m = range_map5::RangeMap::new2(1_000_000, 0, 10_000);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    // b.add_test("map53", |n| {
    //     let mut m = range_map5::RangeMap::new2(1_000_000, 0, 30_000);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    // b.add_test("map6", |n| {
    //     let mut m = range_map6::RangeMap::new(1_000_000, 0, 10);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    // b.add_test("map62", |n| {
    //     let mut m = range_map6::RangeMap::new(1_000_000, 0, 30);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    // b.add_test("map63", |n| {
    //     let mut m = range_map6::RangeMap::new(1_000_000, 0, 100);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    // b.add_test("map64", |n| {
    //     let mut m = range_map6::RangeMap::new(1_000_000, 0, 300);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    // b.add_test("map65", |n| {
    //     let mut m = range_map6::RangeMap::new(1_000_000, 0, 1000);
    //     for i in &indexes[0..*n] {
    //         m.split(*i);
    //     }
    //     m
    // });
    println!("{}", b.run(&mut thread_rng()));
    Ok(())
}
