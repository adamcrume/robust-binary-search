# Robust Binary Search

Robust Binary Search provides a binary search implementation which is robust against errors during
the search. In other words, if the comparison function sometimes returns an incorrect result, the
search in this project will still converge on the correct solution.

This is adapted from the multiplicative weights algorithm in ["Noisy binary search and its
applications" by Karp and Kleinberg](https://www.cs.cornell.edu/~rdk/papers/karpr2.pdf), with
adjustments to make it deterministic and then extended to support directed acyclic graphs.

## Usage

See `AutoSearcher` for binary search over a linear range and `AutoCompressedDAGSearcher` for binary
search over a graph.

If you're looking for a git bisect replacement, see the `robust-git-bisect` crate which uses this
library.

## Performance

This code is optimized to minimize the number of tests executed (i.e. number of iterations) and not
necessrily the CPU time of the search algorithm itself, so this will be slower than a plain binary
search if the test is deterministic.

The linear algorithm (`Searcher` and `AutoSearcher`) takes approximately `O(log N)` time per
iteration. The graph algorithm (`CompressedDAGSearcher` and `AutoCompressedDAGSearcher`) takes
approximately `O(segments)` time per iteration.
