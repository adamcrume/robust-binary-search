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

use clap::App;
use clap::Arg;
use log::info;
use log::trace;
use robust_binary_search::AutoCompressedDAGSearcher;
use robust_binary_search::CompressedDAG;
use robust_binary_search::CompressedDAGSegment;
use simplelog::Config;
use simplelog::LevelFilter;
use simplelog::TermLogger;
use simplelog::TerminalMode;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;
use union_find::QuickFindUf;
use union_find::Union;
use union_find::UnionFind;
use union_find::UnionResult;

#[derive(Clone, Debug)]
struct StringUnion(String);

impl Union for StringUnion {
    fn union(lval: Self, _rval: Self) -> UnionResult<Self> {
        UnionResult::Left(lval)
    }
}

#[derive(Debug, Default)]
struct GitSegmentUf {
    parents: Vec<usize>,
    commits: Vec<String>,
}

#[derive(Debug, Default)]
struct GitSegment {
    parents: Vec<usize>,
    commits: Vec<String>,
}

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
        info!("{}", msg);
        return Err(msg);
    }
    info!("Command {:?} finished successfully", configured);
    Ok(String::from_utf8(out.stdout).unwrap())
}

fn sort_segments(segments: &HashMap<usize, GitSegmentUf>) -> Vec<usize> {
    let mut parents = HashMap::<usize, HashSet<usize>>::new();
    let mut children = HashMap::<usize, HashSet<usize>>::new();
    let mut initial_segments = Vec::new();
    for (id, segment) in segments {
        parents.insert(*id, segment.parents.iter().copied().collect());
        for parent in &segment.parents {
            children
                .entry(*parent)
                .or_insert_with(HashSet::new)
                .insert(*id);
        }
        if segment.parents.is_empty() {
            initial_segments.push(*id);
        }
    }
    let mut sorted = Vec::new();
    while let Some(id) = initial_segments.pop() {
        sorted.push(id);
        if let Some(children_to_update) = children.get(&id) {
            for child in children_to_update {
                let p = parents.get_mut(child).unwrap();
                p.remove(&id);
                if p.is_empty() {
                    parents.remove(child);
                    initial_segments.push(*child);
                }
            }
        }
        children.remove(&id);
    }
    sorted
}

fn run_bisect<P: AsRef<Path>>(
    dir: P,
    segments: &[GitSegment],
    test_cmd: &str,
    min_likelihood: f64,
) -> HashMap<String, Duration> {
    let start = Instant::now();
    let mut graph = CompressedDAG::new();
    for (i, segment) in segments.iter().enumerate() {
        if i % 100 == 0 {
            trace!("Processing segment {} of {}", i, segments.len());
        }
        graph.add_node(
            CompressedDAGSegment::new(segment.commits.len()),
            segment.parents.clone(),
        );
    }
    let mut metrics = HashMap::new();
    metrics.insert("graph-built".to_string(), start.elapsed());
    trace!(
        "CompressedDAG built in {} seconds",
        start.elapsed().as_secs_f64()
    );
    let mut searcher = AutoCompressedDAGSearcher::new(Rc::new(graph));
    let mut iterations = 0;
    loop {
        iterations += 1;
        let node = searcher.next_node();
        let commit = &segments[node.segment].commits[node.index];
        run("git", |cmd| {
            cmd.current_dir(&dir).arg("checkout").arg(commit)
        })
        .unwrap();
        let heads = run("sh", |cmd| cmd.current_dir(&dir).arg("-c").arg(test_cmd)).is_err();
        println!(
            "Reporting {} as {}",
            commit,
            if heads { "bad" } else { "good" }
        );
        searcher.report(node, heads);
        let best = searcher.best_node();
        let best_commit = segments[best.segment].commits[best.index].clone();
        println!("Most likely commit is {} with likelihood {} after {} iterations.  Estimated flakiness is {}.",
                 best_commit, searcher.likelihood(best), iterations, searcher.flakiness());
        if searcher.likelihood(best) > min_likelihood {
            break;
        }
    }
    metrics
}

fn main() -> Result<(), Box<dyn Error>> {
    let start = Instant::now();
    let matches = App::new("git-bisect")
        .version("1.0")
        .author("Adam Crume <acrume@google.com>")
        .about("Robust git bisect which works in the face of noise.")
        .arg(
            Arg::with_name("dir")
                .long("dir")
                .help("Git repo directory")
                .default_value("."),
        )
        .arg(
            Arg::with_name("min-likelihood")
                .long("min-likelihood")
                .help("Minimum likelihood required to stop iterating.")
                .default_value("0.99"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("More verbose output")
                .multiple(true),
        )
        .arg(
            Arg::with_name("start-commit")
                .help("Good/start commit")
                .required(true),
        )
        .arg(
            Arg::with_name("end-commit")
                .help("Bad/end commit")
                .required(true),
        )
        .arg(
            Arg::with_name("test-cmd")
                .help("Command to run which succeeds for good commits and fails for bad commits")
                .required(true),
        )
        .get_matches();
    let level_filter = match matches.occurrences_of("verbose") {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };
    TermLogger::init(level_filter, Config::default(), TerminalMode::Mixed).unwrap();
    let dir = matches.value_of("dir").unwrap();
    let min_likelihood = matches
        .value_of("min-likelihood")
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let start_commit = matches.value_of("start-commit").unwrap();
    let end_commit = matches.value_of("end-commit").unwrap();
    let test_cmd = matches.value_of("test-cmd").unwrap();
    let commit_log = run("git", |command| {
        // TODO: Do we need --ancestry-path?
        command
            .current_dir(dir)
            .arg("log")
            .arg(format!("{}..{}", start_commit, end_commit))
            .arg("--format=format:%H %P")
    })
    .unwrap();
    let mut parents = HashMap::<String, Vec<String>>::new();
    let mut children = HashMap::<String, Vec<String>>::new();
    for line in commit_log.lines() {
        let mut hashes = line.split(' ').map(|s| s.to_string()).collect::<Vec<_>>();
        let commit = hashes.swap_remove(0);
        for parent in hashes.into_iter() {
            children
                .entry(parent.clone())
                .or_insert_with(Vec::new)
                .push(commit.clone());
            parents
                .entry(commit.clone())
                .or_insert_with(Vec::new)
                .push(parent);
        }
    }

    let mut unify = [].iter().cloned().collect::<QuickFindUf<StringUnion>>();
    let mut uf_keys = HashMap::<String, usize>::new();
    for (key, value) in &parents {
        let uf_key1: usize = *uf_keys
            .entry(key.clone())
            .or_insert_with(|| unify.insert(StringUnion(key.clone())));
        if value.len() == 1 {
            if let Some(child_hashes) = children.get(&value[0]) {
                if child_hashes.len() == 1 {
                    let uf_key2: usize = *uf_keys
                        .entry(value[0].clone())
                        .or_insert_with(|| unify.insert(StringUnion(value[0].clone())));
                    unify.union(uf_key1, uf_key2);
                }
            }
        }
    }

    let mut segments = HashMap::<usize, GitSegmentUf>::new();
    for (key, value) in &parents {
        let uf_key: usize = *uf_keys.get(key).unwrap();
        let segment: usize = unify.find(uf_key);
        let git_segment = segments
            .entry(segment)
            .or_insert_with(GitSegmentUf::default);
        git_segment.commits.push(key.clone());
        for parent in value {
            if let Some(parent_uf_key) = uf_keys.get(parent) {
                let parent_segment: usize = unify.find(*parent_uf_key);
                if parent_segment != segment {
                    git_segment.parents.push(parent_segment);
                }
            }
        }
    }

    for value in segments.values_mut() {
        let commit_set = value.commits.iter().cloned().collect::<HashSet<String>>();
        let first_commits = value
            .commits
            .iter()
            .filter(|commit: &&String| {
                let commit_parents = parents.get(*commit).unwrap();
                commit_parents.len() != 1 || !commit_set.contains(&commit_parents[0])
            })
            .cloned()
            .collect::<Vec<String>>();
        assert_eq!(first_commits.len(), 1);
        let mut commit = first_commits[0].clone();
        let mut sorted_commits = vec![commit.clone()];
        while let Some(child_commits) = children.get(&commit) {
            if child_commits.len() != 1 {
                break;
            }
            let child_commit = child_commits[0].clone();
            if !commit_set.contains(&child_commit) {
                break;
            }
            sorted_commits.push(child_commit);
            commit = child_commits[0].clone();
        }
        assert_eq!(
            sorted_commits.iter().cloned().collect::<HashSet<_>>(),
            commit_set
        );
        value.commits = sorted_commits;
    }

    let sorted_segments = sort_segments(&segments);
    let segment_index_by_id = sorted_segments
        .iter()
        .enumerate()
        .map(|(k, v)| (*v, k))
        .collect::<HashMap<usize, usize>>();
    let git_segments = sorted_segments
        .iter()
        .map(|segment_id| {
            let segment = segments.get(segment_id).unwrap();
            let parents = segment
                .parents
                .iter()
                .map(|id| segment_index_by_id.get(id).unwrap())
                .copied()
                .collect::<Vec<usize>>();
            GitSegment {
                parents,
                commits: segment.commits.clone(),
            }
        })
        .collect::<Vec<_>>();

    info!("Running bisection");
    let metrics = run_bisect(dir, &git_segments, test_cmd, min_likelihood);
    for (k, v) in metrics {
        info!("{}: {}", k, v.as_secs_f64());
    }
    info!("Elapsed time: {} seconds", start.elapsed().as_secs_f64());
    Ok(())
}
