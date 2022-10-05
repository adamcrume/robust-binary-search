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
use robust_binary_search::flakiness_tracker::*;
use robust_binary_search::*;
use std::cell::RefCell;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::ops::DerefMut;
use std::process;

fn estimate_flakiness<R: Rng>(rng: &mut R, p_before: f64, p_after: f64) -> (f64, f64) {
    let size = 1 << 20;
    let mut tracker = FlakinessTracker::default();
    let mut searcher = Searcher::new(size);
    let mut i = 0;
    let index = (rng.gen::<f64>() * size as f64) as usize;
    let max_steps = 10000;
    loop {
        i += 1;
        let test_index = searcher.next_index();
        if test_index == index || i == max_steps {
            return tracker.split_flakiness2(test_index, 0.1);
        }
        let heads = if test_index >= index {
            if rng.gen::<f64>() < p_after {
                rng.gen::<f32>() < 0.5
            } else {
                true
            }
        } else {
            if rng.gen::<f64>() < p_before {
                rng.gen::<f32>() < 0.5
            } else {
                false
            }
        };
        tracker.report(test_index, heads);
        let estimated_flakiness = tracker.flakiness();
        searcher.report(test_index, heads, estimated_flakiness);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: main <output_file>");
        process::exit(1);
    }
    let mut f = File::create(&args[1])?;
    let rng = RefCell::new(rand::thread_rng());
    let m = 80;
    for i_after in 0..m {
        let p_after = i_after as f64 / 100.0;
        for i_before in 0..m {
            let p_before = i_before as f64 / 100.0;
            let mut total_tails = 0.0;
            let mut total_heads = 0.0;
            let n = 100;
            for _ in 0..n {
                let r = estimate_flakiness(rng.borrow_mut().deref_mut(), p_before, p_after);
                total_tails += r.0;
                total_heads += r.1;
            }
            writeln!(
                f,
                "{} {} {} {}",
                p_before,
                p_after,
                total_tails / n as f64,
                total_heads / n as f64
            );
            f.sync_data()?;
        }
    }
    Ok(())
}
