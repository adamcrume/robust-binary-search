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

use lazy_static::lazy_static;
use log::info;
use rand;
use rand::rngs::ThreadRng;
use rand::Rng;
use regex::Regex;
use simplelog::Config;
use simplelog::LevelFilter;
use simplelog::TermLogger;
use simplelog::TerminalMode;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process;
use std::process::Command;

fn run<F>(name: &str, mut configure: F) -> Result<String, String>
where
    F: FnMut(&mut Command) -> &mut Command,
{
    let mut command = Command::new(name);
    let configured = configure(&mut command);
    info!("Executing {:?}", configured);
    let out = configured.output().unwrap();
    if !out.status.success() {
        let msg = format!("failed to execute {:?}", configured);
        info!(
            "{}: {}{}",
            msg,
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap()
        );
        return Err(msg);
    }
    info!("Command {:?} finished successfully", configured);
    Ok(String::from_utf8(out.stdout).unwrap())
}

lazy_static! {
    // `git bisect` output looks like:
    // Bisecting: 30215 revisions left to test after this (roughly 15 steps)
    // [53284de77712b2234c739afa3aa5f024fc89fc83] Second half of the fifth batch for 1.8.0
    static ref BISECT_COMMIT_RE: Regex = Regex::new("^\\[([a-z0-9]+)\\].*").unwrap();
    static ref FIRST_BAD_COMMIT_RE: Regex = Regex::new("^([a-z0-9]+) is the first bad commit.*").unwrap();
    static ref ROBUST_BISECT_RE: Regex = Regex::new("Most likely commit is ([a-z0-9]+) .* after ([0-9]+) iterations").unwrap();
}

enum BisectResult {
    Nothing,
    Next(String),
    Final(String),
}

fn git_bisect<P: AsRef<Path>>(dir: P, heads: bool, commit: &str) -> Result<BisectResult, String> {
    let dir = &dir;
    let output = run("git", |cmd| {
        cmd.current_dir(dir)
            .arg("bisect")
            .arg(if heads { "bad" } else { "good" })
            .arg(commit)
    })?;
    for line in output.lines() {
        if let Some(captures) = BISECT_COMMIT_RE.captures(line) {
            return Ok(BisectResult::Next(
                captures.get(1).unwrap().as_str().to_string(),
            ));
        }
        if let Some(captures) = FIRST_BAD_COMMIT_RE.captures(line) {
            return Ok(BisectResult::Final(
                captures.get(1).unwrap().as_str().to_string(),
            ));
        }
    }
    Ok(BisectResult::Nothing)
}

fn git_is_ancestor<P: AsRef<Path>>(dir: P, r1: &str, r2: &str) -> bool {
    let dir = &dir;
    run("git", |cmd| {
        cmd.current_dir(dir)
            .arg("merge-base")
            .arg("--is-ancestor")
            .arg(r1)
            .arg(r2)
    })
    .is_ok()
}

struct CommitTester {
    target_commit: String,
    rng: ThreadRng,
    flakiness: f64,
}

impl CommitTester {
    fn is_bad<P: AsRef<Path>>(&mut self, dir: P, commit: &str) -> Result<bool, String> {
        if self.rng.gen::<f64>() < self.flakiness {
            Ok(self.rng.gen())
        } else {
            Ok(git_is_ancestor(&dir, &self.target_commit, commit))
        }
    }
}

fn run_git_bisect<P: AsRef<Path>>(
    dir: P,
    good_commit: &str,
    bad_commit: &str,
    target_commit: &str,
    commit_tester: &mut CommitTester,
    inner_min_best: usize,
    outer_min_best: usize,
) -> Result<(usize, bool), String> {
    let mut iterations = 0;
    let mut final_commits = HashMap::new();
    loop {
        run("git", |cmd| {
            cmd.current_dir(&dir).arg("bisect").arg("reset")
        })
        .unwrap();
        run("git", |cmd| {
            cmd.current_dir(&dir).arg("bisect").arg("start")
        })
        .unwrap();
        git_bisect(&dir, false, good_commit)?;
        let mut final_commit: Option<String> = None;
        let mut next = match git_bisect(&dir, true, bad_commit)? {
            BisectResult::Nothing => None,
            BisectResult::Next(commit) => Some(commit),
            BisectResult::Final(commit) => {
                final_commit = Some(commit);
                None
            }
        };
        while let Some(next_commit) = next {
            let mut heads = 0;
            let mut tails = 0;
            while heads < inner_min_best && tails < inner_min_best {
                iterations += 1;
                if commit_tester.is_bad(&dir, &next_commit)? {
                    heads += 1;
                } else {
                    tails += 1;
                }
            }
            match git_bisect(&dir, heads > tails, &next_commit)? {
                BisectResult::Nothing => next = None,
                BisectResult::Next(commit) => next = Some(commit),
                BisectResult::Final(commit) => {
                    next = None;
                    final_commit = Some(commit);
                }
            }
        }
        let count = {
            let count = final_commits
                .entry(final_commit.clone().unwrap())
                .or_insert(0);
            *count += 1;
            *count
        };
        if count >= outer_min_best {
            return Ok((iterations, final_commit == Some(target_commit.to_string())));
        }
    }
}

fn run_robust_bisect<P: AsRef<Path>>(
    bisect: &str,
    dir: P,
    good_commit: &str,
    bad_commit: &str,
    test_commit: &str,
    flakiness: f64,
    target_commit: &str,
    min_likelihood: f64,
) -> Result<(usize, bool), Box<dyn Error>> {
    let output = run(bisect, |cmd| {
        cmd.current_dir(&dir)
            .arg(good_commit)
            .arg(bad_commit)
            .arg(format!(
                "'{}' {} {}",
                test_commit,
                (flakiness * 100.0) as usize,
                target_commit
            ))
            .arg("-vv")
            .arg(format!("--min-likelihood={}", min_likelihood))
    })
    .unwrap();
    let mut best_commit = None;
    let mut iterations = 0;
    for line in output.lines() {
        if let Some(captures) = ROBUST_BISECT_RE.captures(line) {
            best_commit = Some(captures.get(1).unwrap().as_str().to_string());
            iterations = captures.get(2).unwrap().as_str().parse()?;
        }
    }
    Ok((iterations, best_commit == Some(target_commit.to_string())))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        println!("Usage: main <dir> <output_file> <bisect> <test_commit_script>");
        process::exit(1);
    }
    TermLogger::init(LevelFilter::Info, Config::default(), TerminalMode::Mixed).unwrap();
    let dir = &args[1];
    let good_commit = "e83c516331";
    let bad_commit = "54e85e7af1";
    let target_commit = "9c3592cf3cf9a9d49ad9a69b76d2be130a21d499";
    let mut f = File::create(&args[2])?;
    let bisect = &args[3];
    let test_commit = &args[4];
    println!("test_commit = {}", test_commit);
    let flakiness = 0.1;
    let mut commit_tester = CommitTester {
        target_commit: target_commit.to_string(),
        rng: rand::thread_rng(),
        flakiness,
    };
    loop {
        let (iterations0, correct0) = run_robust_bisect(
            bisect,
            dir,
            good_commit,
            bad_commit,
            test_commit,
            flakiness,
            target_commit,
            0.99,
        )?;
        let (iterations1, correct1) = run_robust_bisect(
            bisect,
            dir,
            good_commit,
            bad_commit,
            test_commit,
            flakiness,
            target_commit,
            0.9,
        )?;
        let (iterations2, correct2) = run_git_bisect(
            dir,
            good_commit,
            bad_commit,
            target_commit,
            &mut commit_tester,
            1,
            1,
        )?;
        let (iterations3, correct3) = run_git_bisect(
            dir,
            good_commit,
            bad_commit,
            target_commit,
            &mut commit_tester,
            2,
            1,
        )?;
        let (iterations4, correct4) = run_git_bisect(
            dir,
            good_commit,
            bad_commit,
            target_commit,
            &mut commit_tester,
            1,
            2,
        )?;
        let correct = |b| if b { "correct" } else { "incorrect" };
        writeln!(
            f,
            "{} {} {} {} {} {} {} {} {} {} {}",
            flakiness,
            iterations0,
            correct(correct0),
            iterations1,
            correct(correct1),
            iterations2,
            correct(correct2),
            iterations3,
            correct(correct3),
            iterations4,
            correct(correct4)
        )?;
        f.sync_data()?;
    }
}
