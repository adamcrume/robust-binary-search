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

use rand::Rng;
use robust_binary_search::*;
use std::cmp;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::process;

fn steps_required<R: Rng>(rng: &mut R, flakiness: f64, stiffness: f64) -> f64 {
    let size = 1 << 20;
    let mut max = 0;
    let count = 100;
    for _ in 0..count {
        let mut searcher = Searcher::new(size);
        let mut i = 0;
        let index = (rng.gen::<f64>() * size as f64) as usize;
        let max_steps = 1000;
        max = cmp::max(
            max,
            loop {
                i += 1;
                let test_index = searcher.next_index().unwrap();
                if test_index == index || i == max_steps {
                    break i;
                }
                let heads = if rng.gen::<f64>() < flakiness {
                    rng.gen::<f32>() < 0.5
                } else {
                    test_index >= index
                };
                searcher.report_with_stiffness(test_index, heads, stiffness);
            },
        );
    }
    max as f64
}

fn log_interpolate(index: usize, buckets: usize, min: f64, max: f64) -> f64 {
    (min.ln() + index as f64 / buckets as f64 * (max / min).ln()).exp()
}

fn main() -> Result<(), Box<dyn Error>> {
    // optimal stiffness is approximately
    // min(2.6/x**0.37, 0.58/x**0.97, 0.19/x**2.4)
    // where x is the flakiness (0 is deterministic, 1 is fully random)
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: main <output_file>");
        process::exit(1);
    }
    let mut f = File::create(&args[1])?;
    let min_flakiness = 0.001;
    let max_flakiness = 1.0;
    let flakiness_buckets = 50;
    let stiffness_buckets = 1000;
    let min_stiffness = 0.1;
    let max_stiffness = 128.0;
    for flakiness_index in 0..flakiness_buckets {
        let flakiness = log_interpolate(
            flakiness_index,
            flakiness_buckets,
            min_flakiness,
            max_flakiness,
        );
        let mut rng = rand::thread_rng();
        let mut searcher = Searcher::new(stiffness_buckets);
        let to_stiffness = |i| log_interpolate(i, stiffness_buckets, min_stiffness, max_stiffness);
        let window = 1.5;
        for i in 0..1000 {
            let test_index = searcher.next_index().unwrap();
            let steps1 = steps_required(&mut rng, flakiness, to_stiffness(test_index) / window);
            let steps2 = steps_required(&mut rng, flakiness, to_stiffness(test_index) * window);
            let heads = if steps1 < steps2 {
                true
            } else {
                if steps1 > steps2 {
                    false
                } else {
                    rng.gen::<f32>() < 0.5
                }
            };
            searcher.report(test_index, heads, 0.5);
            let lower_bound = searcher.confidence_percentile_ceil(0.1);
            let upper_bound = searcher.confidence_percentile_ceil(0.9);
            println!(
                "{} {} {} {} {} {}",
                flakiness,
                i,
                to_stiffness(test_index),
                to_stiffness(lower_bound),
                to_stiffness(upper_bound),
                searcher.likelihood(searcher.best_index())
            );
            if lower_bound == upper_bound {
                break;
            }
        }

        writeln!(f, "{} {}", flakiness, to_stiffness(searcher.best_index()))?;
        f.sync_data()?;
    }
    Ok(())
}
