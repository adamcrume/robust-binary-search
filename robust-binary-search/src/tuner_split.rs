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

use bayesian_optimization::Optimizer;
use bayesian_optimization::Param;
use bayesian_optimization::Value;
use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use rand::RngCore;
use rand_distr::Distribution;
use rand_distr::Normal;
use rayon::prelude::*;
use robust_binary_search::flakiness_tracker::*;
use robust_binary_search::optimizer::Function;
use robust_binary_search::optimizer::Optimizer2;
use robust_binary_search::*;
use std::borrow::Borrow;
use std::cmp;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::process;
use std::rc::Rc;

fn steps_required<R: Rng>(
    rng: &mut R,
    flakiness_before: f64,
    flakiness_after: f64,
    index: usize,
    calc: &StiffnessCalculator,
    prior: f64,
    max_steps: usize,
) -> f64 {
    let size = 1 << 20;
    let mut max = 0;
    let mut sum = 0;
    //let count = 100;
    //let count = 10;
    let count = 1;
    for _ in 0..count {
        let steps = {
            let mut searcher = Searcher::new(size);
            let mut tracker = FlakinessTracker::default();
            let mut i = 0;
            let index = (rng.gen::<f64>() * size as f64) as usize;
            loop {
                i += 1;
                let test_index = searcher.next_index();
                if test_index == index || i == max_steps {
                    break i;
                }
                let heads = if test_index >= index {
                    if rng.gen::<f64>() < flakiness_after {
                        rng.gen::<f32>() < 0.5
                    } else {
                        true
                    }
                } else {
                    if rng.gen::<f64>() < flakiness_before {
                        rng.gen::<f32>() < 0.5
                    } else {
                        false
                    }
                };
                tracker.report(test_index, heads);
                // let estimated_flakiness = tracker.split_flakiness(test_index, prior);
                let estimated_flakiness = tracker.split_flakiness2(test_index, prior);
                let flakiness = if heads {
                    estimated_flakiness.1
                } else {
                    estimated_flakiness.0
                };
                searcher.report_with_stiffness(test_index, heads, calc.stiffness(flakiness));
            }
        };
        max = cmp::max(max, steps);
        sum += steps;
    }
    //    max as f64
    sum as f64 / count as f64
}

fn steps_required2<R: Rng>(
    rng: &mut R,
    calc: &StiffnessCalculator,
    rounds: usize,
    index: usize,
    prior: f64,
    max_steps: usize,
) -> f64 {
    let max_flakiness = 0.8;
    let mut steps = 0.0;
    for i in 0..rounds {
        let flakiness_before = (i as f64 / rounds as f64) * max_flakiness;
        for j in 0..rounds {
            let flakiness_after = (j as f64 / rounds as f64) * max_flakiness;
            steps += steps_required(
                rng,
                flakiness_before,
                flakiness_after,
                index,
                calc,
                prior,
                max_steps,
            )
            .ln();
        }
    }
    steps / (rounds * rounds) as f64
}

fn log_interpolate(index: usize, buckets: usize, min: f64, max: f64) -> f64 {
    (min.ln() + index as f64 / buckets as f64 * (max / min).ln()).exp()
}

trait Individual: Default + Clone + Send + Sync {
    fn mate<R: Rng>(&self, other: &Self, rng: &mut R) -> Self;

    fn mutate<R: Rng>(&self, amount: f64, rng: &mut R) -> Self;

    fn calculator(&self) -> Box<StiffnessCalculator>;

    fn prior(&self) -> f64;
}

#[derive(Clone, Debug)]
struct ChebyshevIndividual {
    params: Vec<f64>,
}

impl Default for ChebyshevIndividual {
    fn default() -> Self {
        Self {
            params: vec![0.0; 7],
        }
    }
}

impl Individual for ChebyshevIndividual {
    fn mate<R: Rng>(&self, other: &Self, rng: &mut R) -> Self {
        Self {
            params: self
                .params
                .iter()
                .zip(other.params.iter())
                .map(|(p1, p2)| (p1 + p2) * 0.5)
                .collect(),
        }
    }

    fn mutate<R: Rng>(&self, amount: f64, rng: &mut R) -> Self {
        let dist = Normal::new(0.0, amount * amount).unwrap();
        let mut params = self.params.clone();
        for i in 0..params.len() {
            params[i] += dist.sample(rng);
        }
        Self { params }
    }

    fn calculator(&self) -> Box<StiffnessCalculator> {
        Box::new(ChebyshevStiffnessCalculator {
            params: self.params[0..self.params.len() - 1].into(),
        })
    }

    fn prior(&self) -> f64 {
        self.params[self.params.len() - 1].exp()
    }
}

#[derive(Debug, Clone, Default)]
struct InterpolatingIndividual {
    params: Vec<f64>,
}

impl Individual for InterpolatingIndividual {
    fn mate<R: Rng>(&self, other: &Self, rng: &mut R) -> Self {
        let mut params: Vec<f64> = self
            .params
            .iter()
            .zip(other.params.iter())
            .map(|(p1, p2)| (p1 + p2) * 0.5)
            .collect();
        // TODO: Do we need this?
        // Sort decreasing; i.e. higher flakiness requires lower stiffness.
        params.sort_by(|p1, p2| p2.partial_cmp(&p1).unwrap());
        Self { params }
    }

    fn mutate<R: Rng>(&self, amount: f64, rng: &mut R) -> Self {
        let dist = Normal::new(0.0, amount * amount).unwrap();
        let mut params = self.params.clone();
        for i in 0..params.len() {
            params[i] += dist.sample(rng);
        }
        // TODO: Do we need this?
        // Sort decreasing; i.e. higher flakiness requires lower stiffness.
        params.sort_by(|p1, p2| p2.partial_cmp(&p1).unwrap());
        Self { params }
    }

    fn calculator(&self) -> Box<StiffnessCalculator> {
        Box::new(InterpolatingStiffnessCalculator {
            params: self.params[0..self.params.len() - 1].into(),
        })
    }

    fn prior(&self) -> f64 {
        self.params[self.params.len() - 1].exp()
    }
}

#[derive(Debug, Default)]
struct Problem {}

impl Function for Problem {
    fn evaluate(&self, params: &[f64]) -> f64 {
        let mut rng = rand::thread_rng();
        let rounds = 100;
        let calc = ChebyshevStiffnessCalculator {
            params: params[0..params.len() - 1].into(),
        };
        let result = steps_required2(
            &mut rng,
            &calc,
            rounds,
            0,
            params[params.len() - 1].exp(),
            100,
        );
        println!("evaluate returning {}", result); // TODO: remove
        result
    }

    fn modify(&self, params: &mut [f64], extent: f64) {
        let mut rng = rand::thread_rng();
        let dist = Normal::new(0.0, extent / 1.0).unwrap();
        // *params.choose_mut(&mut rng).unwrap() += dist.sample(&mut rng);
        for i in 0..params.len() {
            params[i] += dist.sample(&mut rng);
        }
        // TODO: Do we need this?
        // Sort decreasing; i.e. higher flakiness requires lower stiffness.
        //params.sort_by(|p1, p2| p2.partial_cmp(&p1).unwrap());
        // for &mut (mut p) in params {
        //     p += dist.sample(&mut rng);
        // }
    }
}

#[derive(Default, Debug)]
struct Toy {}

impl Function for Toy {
    fn evaluate(&self, params: &[f64]) -> f64 {
        let d = params[0] - 3.14159265358979;
        d.abs()
    }
    fn modify(&self, params: &mut [f64], extent: f64) {
        unimplemented!("not supported");
    }
}

fn dist(p1: &[f64], p2: &[f64]) -> f64 {
    p1.iter()
        .zip(p2.iter())
        .map(|(v1, v2)| (v1 - v2) * (v1 - v2))
        .sum::<f64>()
        .sqrt()
}

struct GA<I: Individual> {
    population: Vec<I>,
}

impl<I: Individual> GA<I> {
    fn new() -> Self {
        let mut rng = rand::thread_rng();
        let mut population = Vec::new();
        for _ in 0..100 {
            population.push(I::default().mutate(1.0, &mut rng));
        }
        Self { population }
    }

    fn run(&mut self) {
        let mut rng = rand::thread_rng();
        let mut i = 0;
        let mut max_steps = 100;
        let max_max_steps = 10000;
        let mut max_steps_maxed_at = 0;
        loop {
            println!("max_steps = {}", max_steps);
            let mut evaluated: Vec<(I, f64)> = self
                .population
                .par_iter()
                .map(|individual| {
                    let mut rng = rand::thread_rng();
                    let calc = individual.calculator();
                    let prior = individual.prior();
                    let rounds = 100;
                    let cost =
                        steps_required2(&mut rng, calc.borrow(), rounds, 0, prior, max_steps);
                    println!("cost is {}", cost); // TODO: remove
                    (individual.clone(), cost)
                })
                .collect();
            evaluated.sort_by(|e1, e2| e1.1.partial_cmp(&e2.1).unwrap());
            let parents: Vec<I> = evaluated[0..evaluated.len() / 4]
                .iter()
                .map(|e| e.0.clone())
                .collect();
            let mut next_gen: Vec<_> = parents.iter().cloned().collect();
            //let reduction = if max_steps_maxed_at == 0 {1.0} else {1.0 / (1.0 + (i - max_steps_maxed_at) as f64)};
            let reduction = 1.0 / (1.0 + (i - max_steps_maxed_at) as f64);
            let step_size = 1.0 * reduction;
            while next_gen.len() < evaluated.len() {
                let parent1 = parents.choose(&mut rng).unwrap();
                let parent2 = parents.choose(&mut rng).unwrap();
                let child = parent1.mate(parent2, &mut rng).mutate(step_size, &mut rng);
                next_gen.push(child);
            }
            let params = &evaluated[0].0;
            let calc = params.calculator();
            let prior = params.prior();
            println!("{} {:?} {} {}", evaluated[0].1, calc, prior, step_size);
            self.population = next_gen;
            // TODO: do we need this?
            let mut new_max_steps = (max_steps as f64 * 1.1) as usize;
            if new_max_steps >= max_max_steps {
                if max_steps < max_max_steps {
                    max_steps_maxed_at = i;
                }
                new_max_steps = max_max_steps;
            }
            max_steps = new_max_steps;
            i += 1;
        }
    }
}

fn optimize<R: Rng, F: FnMut(&[Value]) -> f64>(
    optimizer: &mut Optimizer,
    mut function: F,
    mut rng: R,
    max_iterations: usize,
) -> Vec<Value> {
    let mut iterations = 0;
    loop {
        println!("--------------");
        println!("Iteration {} of {}", iterations, max_iterations);
        let sample = optimizer.choose_sample(&mut rng);
        let value = function(sample.values());
        optimizer.report_pending_sample(sample, value).unwrap();
        let best = optimizer.best().unwrap();
        println!("best = {}, {:?}", best.1, best.0);
        let expected_best = optimizer.expected_best().unwrap();
        println!("expected best = {}, {:?}", expected_best.1, expected_best.0);
        iterations += 1;
        if iterations >= max_iterations {
            return Vec::from(best.0);
        }
    }
}

fn run_bayesopt() -> Result<(), Box<dyn Error>> {
    let mut optimizer = Optimizer::builder()
        .set_population_size(100)
        // .set_maximize(true)
        .add_param(Param::Linear(-16.0, 16.0))?
        .add_param(Param::Linear(-16.0, 16.0))?
        .add_param(Param::Linear(-16.0, 16.0))?
        .add_param(Param::Linear(-16.0, 16.0))?
        .add_param(Param::Linear(-16.0, 16.0))?
        .add_param(Param::Linear(-16.0, 16.0))?
        .add_param(Param::Logarithmic(0.1, 1000.0))?
        .build();
    let mut evals = 0;
    let mut variance_sum = 0.0;
    let max_steps = 200;
    optimize(
        &mut optimizer,
        |values| {
            let mut rng = rand::thread_rng();
            let individual = ChebyshevIndividual {
                params: values.iter().map(|v| v.unwrap_f64()).collect(),
            };
            let calc = individual.calculator();
            let prior = individual.prior();
            let rounds = 100;
            let cost = steps_required2(&mut rng, calc.borrow(), rounds, 0, prior, max_steps);
            println!("cost is {}", cost); // TODO: remove
            cost
        },
        &mut thread_rng(),
        1000000,
    );
    //    optimize(&mut optimizer, |values| hp_func(data.clone(), &mut evals, &mut variance_sum, values), &mut thread_rng(), 1000000);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    // let mut ga = GA::<ChebyshevIndividual>::new();
    // ga.run();

    run_bayesopt();

    // let mut rng = rand::thread_rng();
    // let mut calc = ChebyshevStiffnessCalculator::new(vec![1.0, 0.0, 0.0, 0.0]);
    // let mut rounds = 30;
    // // {
    // //     let f = Box::new(Toy::default());
    // //     let mut optimizer = Optimizer::new(f, vec![-10.0], vec![10.0], vec![0.0]);
    // //     optimizer.optimize();
    // // }
    // // let f = Box::new(Problem::new());
    // //let mut optimizer = Optimizer::new(f, vec![-10.0, -10.0, -10.0, -10.0, -10.0], vec![10.0, 10.0, 10.0, 10.0, 10.0], vec![1.0, 0.0, 0.0, 0.0, 0.0]);

    // // let f = Rc::new(Problem::new());
    // // let mut optimizer = Optimizer2::new(f, vec![-10.0, -10.0, -10.0, -10.0, -10.0, -10.0], vec![10.0, 10.0, 20.0, 10.0, 10.0, 10.0], vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    // // optimizer.optimize()?;

    // let mut problem = Problem::default();
    // // //let mut params = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    // // let mut params = vec![0.0; 10];
    // // // let mut step_size = 1.0;
    // // let mut prev_best = 0.0;
    // // let mut i = 0;
    // // loop {
    // //     let index = (rng.gen::<f64>() * (1<<20) as f64) as usize;
    // //     let mut steps1 = problem.evaluate(&params);
    // //     // let mut best_step_size = 0.0;
    // //     // let mut steps1 = steps_required2(&mut rng, &calc, rounds, index, 0.1);
    // //     let reduction = 1.0 / (1.0 + i as f64);
    // //     let step_size = 1.0 * reduction;
    // //     for _ in 0..10 {
    // //         let mut params2 = params.clone();
    // //         problem.modify(&mut params2, step_size * step_size);
    // //         let steps2 = problem.evaluate(&params2);
    // //         // let calc2 = calc.mutate(&mut rng);
    // //         // let steps2 = steps_required2(&mut rng, &calc2, rounds, index, 0.1);
    // //         if steps2 < steps1 {
    // //             //step_size = 0.75 * step_size + 0.25 * dist(&params, &params2);
    // //             params = params2;
    // //             // calc = calc2;
    // //             steps1 = steps2;
    // //             // best_step_size = dist(&params, &params2);
    // //         }
    // //     }
    // //     // step_size = 0.5 * step_size + 0.5 * best_step_size;
    // //     let control_points = params[0..params.len()-1].iter().map(|x| x.exp()).collect::<Vec<_>>();
    // //     let prior = params[params.len()-1].exp();
    // //     println!("{} {:?} {} {}", steps1, control_points, prior, step_size);
    // //     //        println!("{} {:?} {}", steps1, calc, rounds);
    // //     // step_size *= if steps1 < prev_best {
    // //     //     1.1
    // //     // } else {
    // //     //     1.0/1.1 * 0.99
    // //     // };
    // //     prev_best = steps1;
    // //     i += 1;
    // // }
    // let mut population = Vec::new();
    // for _ in 0..20 {
    //     population.push(ChebyshevIndividual::default().mutate(&mut rng));
    //     // let mut params = vec![0.0; 10];
    //     // problem.modify(&mut params, 1.0);
    //     // population.push(params);
    // }
    // let mut rng = rand::thread_rng();
    // let mut i = 0;
    // loop {
    //     let mut evaluated = Vec::<(ChebyshevIndividual, f64)>::new();
    //     for individual in &population {
    //         let calc = individual.calculator();
    //         let prior = individual.prior();
    //         let rounds = 100;
    //         let cost = steps_required2(&mut rng, calc.borrow(), rounds, 0, prior);
    //         evaluated.push((individual.clone(), cost));
    //     }
    //     evaluated.sort_by(|e1, e2| e1.1.partial_cmp(&e2.1).unwrap());
    //     let parents: Vec<ChebyshevIndividual> = evaluated[0..evaluated.len() / 4].iter().map(|e| e.0.clone()).collect();
    //     let mut next_gen: Vec<_> = parents.iter().cloned().collect();
    //     let reduction = 1.0 / (1.0 + i as f64);
    //     let step_size = 1.0 * reduction;
    //     let dist = Normal::new(0.0, step_size * step_size);
    //     while next_gen.len() < evaluated.len() {
    //         let parent1 = parents.choose(&mut rng).unwrap();
    //         let parent2 = parents.choose(&mut rng).unwrap();
    //         let child = parent1.mate(parent2, &mut rng).mutate(&mut rng);
    //         // let mut child: Vec<_> = parent1.iter().zip(parent2.iter()).map(|(p1, p2)| (p1 + p2) * 0.5 + dist.sample(&mut rng)).collect();
    //         // // // TODO: Do we need this?
    //         // // // Sort decreasing; i.e. higher flakiness requires lower stiffness.
    //         // // child.sort_by(|p1, p2| p2.partial_cmp(&p1).unwrap());
    //         next_gen.push(child);
    //     }
    //     let params = &evaluated[0].0;
    //     // let control_points = params[0..params.len()-1].iter().map(|x| x.exp()).collect::<Vec<_>>();
    //     let calc = params.calculator();
    //     let prior = params.prior();
    //     // let control_points = &params[0..params.len()-1];
    //     // let prior = params[params.len()-1].exp();
    //     println!("{} {:?} {} {}", evaluated[0].1, calc, prior, step_size);
    //     population = next_gen;
    //     i += 1;
    // }
    Ok(())
}
