# Robust Binary Search

Robust Binary Search provides a binary search implementation which is robust against errors during
the search. In other words, if the comparison function sometimes returns an incorrect result, the
search in this project will still converge on the correct solution.

This is adapted from the multiplicative weights algorithm in ["Noisy binary search and its
applications" by Karp and Kleinberg](https://www.cs.cornell.edu/~rdk/papers/karpr2.pdf), with
adjustments to make it deterministic and then extended to support directed acyclic graphs.

## Usage

To use the git bisect replacement, install with `cargo install robust-git-bisect`, and then
`~/.cargo/bin/robust-git-bisect $start_commit $end_commit $command_to_test_commit`

For library usage, see `AutoSearcher` for binary search over a linear range and
`AutoCompressedDAGSearcher` for binary search over a graph.

## Performance

This code is optimized to minimize the number of tests executed (i.e. number of iterations) and not
necessarily the CPU time of the search algorithm itself, so this will be slower than a plain binary
search if the test is deterministic.

The linear algorithm (`Searcher` and `AutoSearcher`) takes approximately `O(log N)` time per
iteration. The graph algorithm (`CompressedDAGSearcher` and `AutoCompressedDAGSearcher`) takes
approximately `O(segments)` time per iteration.

robust-git-bisect shows improved performance compared with git bisect (higher accuracy with fewer
iterations):

Method                             | Iterations | Accuracy
---------------------------------- | ---------- | --------
robust-git-bisect with 0.99 target | 29.6558    | 99.5392%
robust-git-bisect with 0.9 target  | 26.1828    | 98.8950%
git bisect                         | 16.1907    | 31.7972%
git bisect with tests repeated     | 35.0465    | 86.6359%
git bisect repeated                | 72.3674    | 86.1751%

This test is run over the `git` git repo from e83c516331 to 54e85e7af1, simulating 9c3592cf3c as the
bad commit, with a test that returns an incorrect result 5% of the time. See benchmark.rs for
details.
