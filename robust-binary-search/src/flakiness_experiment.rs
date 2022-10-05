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

fn count_steps<R: Rng>(rng: &mut R, p_before: f64, p_after: f64) -> usize {
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
            return i;
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

fn count_steps_split<R: Rng>(rng: &mut R, p_before: f64, p_after: f64) -> usize {
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
            return i;
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
        // TODO: wire in prior
        let estimated_flakiness = tracker.split_flakiness(test_index, /*prior=*/ 1.0);
        searcher.report(
            test_index,
            heads,
            0.25 * if heads {
                estimated_flakiness.1
            } else {
                estimated_flakiness.0
            },
        );
    }
}

fn count_steps_split2<R: Rng>(rng: &mut R, p_before: f64, p_after: f64) -> usize {
    let size = 1 << 20;
    let mut tracker = FlakinessTracker::default();
    let mut searcher = Searcher::new(size);
    let mut i = 0;
    let index = (rng.gen::<f64>() * size as f64) as usize;
    let max_steps = 10000;
    //    let calc = ChebyshevStiffnessCalculator::new(vec![2.910747138962504, -0.5572787377654687, -0.03444287884747687, -0.057673370071682176, -0.2827996467349072, 0.3048301316856115]);
    // let prior = 2.50144255610522;
    // let calc = ChebyshevStiffnessCalculator::new(vec![2.284947801255808, -0.07845103471077425, -0.46362033270837044, -0.03499750224605142, 0.456962254374073, 0.03814038195887797]);
    // let prior = 2.159243534762829;
    let calc = ChebyshevStiffnessCalculator::new(vec![
        2.0176422822909013,
        -0.4786093869542331,
        -0.6766987361375544,
        0.4131402297415648,
        0.4384968585051378,
        0.5777164791765699,
    ]);
    let prior = 1.29722745915973;
    loop {
        i += 1;
        let test_index = searcher.next_index();
        if test_index == index || i == max_steps {
            return i;
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

        // 4.451945171590554, -2.7333515561945605, -10.0, -8.492264559262455, -3.992843324117235, 0.1197982795003523
        // -0.8965408631202667, 10.0, -9.380237198031194, 6.650249768655522, -2.7881267992830616, 8.985829570990507
        // let prior = 0.5923277868951454;

        // let a = 4.0149195959497925;
        // let b = -1.3080370156586596;
        // let c = -0.01533033933852389;
        // let d = 0.11019071067031458;
        // let e = -0.31472692189970497;
        // let prior = 1.259169397634381;
        // let a = 3.140519794853536;
        // let b = -1.1618988693559598;
        // let c = 0.3461331340018353;
        // let d = -0.004719726734068967;
        // let e = 0.22377430562730266;
        // let prior = 1.9443324395516166;
        // let a = -3.5781052226066077;
        // let b = 9.179652407049218;
        // let c = 2.3007472219050777;
        // let d = -6.430105351228759;
        // let e = 4.5124466300970765;
        // let prior = 10.0;

        // up to 0.8 flakiness
        // let a = 1.5129638864836987;
        // let b = 4.064979196778419;
        // let c = 10.0;
        // let d = -8.314806785250196;
        // let e = -4.845251767772728;
        // let prior = 0.5029170212537349;
        // // up to 0.8 flakiness
        // let a = 2.1575271984130233;
        // let b = -2.123933920274482;
        // let c = 10.0;
        // let d = 0.8249281230361246;
        // let e = -6.759307360944897;
        // let prior = 0.704491879344536;
        // // Up to 0.5 flakiness
        // let a = 3.99098501447317;
        // let b = 0.6697507609026504;
        // let c = 10.0;
        // let d = -1.2254658142060089;
        // let e = -9.656189741559054;
        // let prior = 0.6293224766847301;
        // let estimated_flakiness = tracker.split_flakiness(test_index, prior);
        let estimated_flakiness = tracker.split_flakiness2(test_index, prior);
        // let estimated_flakiness = tracker.split_flakiness2(test_index, 0.1);
        let flakiness = if heads {
            estimated_flakiness.1
        } else {
            estimated_flakiness.0
        };
        //let stiffness = (2.2460557048585152 + 2.96709010514848*flakiness + 0.41822045649913*flakiness*flakiness).max(0.01);
        // let a = 3.469284207115773;
        // let b = 6.582382867358567;
        // let c = -4.155782439999453;
        // let d = 5.563893799708781;
        // let e = 7.611636318293071;
        // let a = 3.095454438735819;
        // let b = 9.732188250252278;
        // let c = -3.119963574088521;
        // let d = 1.5227901994829178;
        // let e = -0.5413158592184788;
        let x = flakiness;
        //let stiffness = (a + b * x + c * x * x + d * x * x * x + e * x * x * x * x).max(0.01);
        // let x = 2.0 * flakiness - 1.0; // was [0, 1], is now [-1, 1]
        // let x2 = x * x;
        // let x3 = x2 * x;
        // let x4 = x3 * x;
        // let t0 = 1.0;
        // let t1 = x;
        // let t2 = 2.0 * x2 - 1.0;
        // let t3 = 4.0 * x3 - 3.0 * x;
        // let t4 = 8.0 * x4 - 8.0 * x2 + 1.0;
        // let stiffness = (a * t0 + b * t1 + c * t2 + d * t3 + e * t4).max(0.01);
        // let stiffness = {
        //     let params = [5.943730581744327f64, 3.9954159898710166, 3.3917961192012953, 3.1086151574109917, 2.2470511185632587, 2.1724729140887145, 1.784411942661323, 1.4552231278842758, 1.4221489536564318];
        //     let n = params.len() - 1;
        //     let mut start = params[n - 1];
        //     let mut end = params[n];
        //     let mut diff = 1.0 / n as f64;
        //     let mut found = false;
        //     for i in 0..n {
        //         if x < (i + 1) as f64 / n as f64 {
        //             start = params[i];
        //             end = params[i + 1];
        //             diff = x - i as f64 / n as f64;
        //             found = true;
        //             break;
        //         }
        //     }
        //     let alpha = diff * n as f64;
        //     (start.exp() * (1.0 - alpha) + end.exp() * alpha).max(0.01)
        // };
        let stiffness = calc.stiffness(flakiness);
        searcher.report_with_stiffness(test_index, heads, stiffness);
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
            let mut total_unsplit = 0;
            //let mut total_split = 0;
            let mut total_split2 = 0;
            let n = 100;
            for _ in 0..n {
                total_unsplit += count_steps(rng.borrow_mut().deref_mut(), p_before, p_after);
                //total_split += count_steps_split(rng.borrow_mut().deref_mut(), p_before, p_after);
                total_split2 += count_steps_split2(rng.borrow_mut().deref_mut(), p_before, p_after);
            }
            //writeln!(f, "{} {} {} {} {}", p_before, p_after, total_unsplit as f64 / n as f64, total_split as f64 / n as f64, total_split2 as f64 / n as f64);
            writeln!(
                f,
                "{} {} {} {}",
                p_before,
                p_after,
                total_unsplit as f64 / n as f64,
                total_split2 as f64 / n as f64
            );
            f.sync_data()?;
        }
    }
    Ok(())
}
