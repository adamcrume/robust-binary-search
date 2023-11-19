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

fn sample_inversions<R: Rng>(rng: &mut R, p: f64) -> (usize, usize) {
    let size = 1 << 20;
    let mut tracker = FlakinessTracker::default();
    let mut searcher = Searcher::new(size);
    let mut i = 0;
    let index = (rng.gen::<f64>() * size as f64) as usize;
    let max_steps = 10000;
    loop {
        i += 1;
        let test_index = searcher.next_index().unwrap();
        if test_index == index || i == max_steps {
            break;
        }
        let heads = if rng.gen::<f64>() < p {
            rng.gen::<f32>() < 0.5
        } else {
            test_index >= index
        };
        tracker.report(test_index, heads);
        let estimated_flakiness = tracker.flakiness();
        searcher.report_with_stiffness(test_index, heads, optimal_stiffness(estimated_flakiness));
    }
    tracker.inversions()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: main <output_file>");
        process::exit(1);
    }
    let mut f = File::create(&args[1])?;
    let rng = RefCell::new(rand::thread_rng());
    for i in 0..80 {
        let p = i as f64 / 100.0;
        let mut inv_total = 0;
        let mut rand_inv_total = 0;
        for _ in 0..10000 {
            let (inv, rand_inv) = sample_inversions(rng.borrow_mut().deref_mut(), p);
            inv_total += inv;
            rand_inv_total += rand_inv;
        }
        writeln!(f, "{} {} {}", p, inv_total, rand_inv_total)?;
        f.sync_data()?;
    }
    Ok(())
}
