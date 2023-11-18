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

use im_rc::OrdSet;
use std::collections::HashSet;

/// A node in a DAG.
#[derive(Clone, Debug)]
pub struct DAGNode<T> {
    value: T,
    inputs: Vec<usize>,
    ancestors: OrdSet<usize>,
    remainder_ancestors: Vec<usize>,
}

impl<T> DAGNode<T> {
    /// Returns the value in the node.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Returns indices within the DAG of the node's input nodes.
    pub fn inputs(&self) -> &[usize] {
        &self.inputs
    }

    /// Returns indices within the DAG of the transitive closure of the node's inputs. Includes the
    /// inputs but excludes the node itself.
    pub fn ancestors(&self) -> &OrdSet<usize> {
        &self.ancestors
    }

    /// Returns indices within the DAG of ancestors which are not the first input or its ancestors.
    /// In other words, the sets `remainder_ancestors()`, `{inputs()[0]}` (assuming there is at
    /// least one input), and `inputs()[0].ancestors()` (assuming there is at least one input) are
    /// disjoint, and their union equals `ancestors()`.
    ///
    /// This can be used to compute certain properties over a graph more efficiently. For example,
    /// computing the sum of ancestors' values incrementally for every node in the graph can be done
    /// by starting with the sum for `inputs()[0]` then adding the values of nodes in
    /// `remainder_ancestors()`. This can reduce the complexity from `O(n^2)` to roughly `O(n)` for
    /// deep and narrow graphs.
    pub fn remainder_ancestors(&self) -> &[usize] {
        &self.remainder_ancestors
    }
}

/// A Directed Acyclic Graph with the nodes sorted topologically.
#[derive(Clone, Debug)]
pub struct DAG<T> {
    nodes: Vec<DAGNode<T>>,
}

impl<T> Default for DAG<T> {
    fn default() -> Self {
        Self { nodes: vec![] }
    }
}

impl<T> DAG<T> {
    /// Creates an empty DAG.
    pub fn new() -> Self {
        DAG { nodes: vec![] }
    }

    /// Returns the nodes in the DAG.
    pub fn nodes(&self) -> &[DAGNode<T>] {
        &self.nodes
    }

    /// Convenience method for nodes()[index].
    ///
    /// # Panics
    ///
    /// Panics if index is greater than or equal to nodes().len().
    pub fn node(&self, index: usize) -> &DAGNode<T> {
        &self.nodes[index]
    }

    /// Adds a node to the DAG. Each input must all be less than the index of the new node itself,
    /// i.e. must be less than the number of nodes currently in the DAG. The first input is treated
    /// specially by DAGNode::remainder_ancestors.
    ///
    /// # Panics
    ///
    /// Panics if any value in inputs is greater than or equal to nodes().len().
    pub fn add_node(&mut self, value: T, inputs: Vec<usize>) {
        for input in &inputs {
            assert!(*input < self.nodes.len());
        }

        let (ancestors, remainder_ancestors) = if inputs.is_empty() {
            (OrdSet::new(), vec![])
        } else {
            let mut ancestors = self.nodes[inputs[0]].ancestors.clone();
            let mut remainder_ancestors = HashSet::new();
            ancestors.insert(inputs[0]);
            let mut queue = Vec::new();
            for input in &inputs[1..] {
                queue.push(*input);
            }
            while let Some(ancestor) = queue.pop() {
                if ancestors.insert(ancestor).is_none() {
                    remainder_ancestors.insert(ancestor);
                    for ancestor_input in &self.nodes[ancestor].inputs {
                        queue.push(*ancestor_input);
                    }
                }
            }
            let mut sorted_remainder_ancestors =
                remainder_ancestors.into_iter().collect::<Vec<usize>>();
            sorted_remainder_ancestors.sort();
            (ancestors, sorted_remainder_ancestors)
        };
        self.nodes.push(DAGNode {
            value,
            ancestors,
            remainder_ancestors,
            inputs,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! hash_set {
        ($($arg:expr),*) => {
            vec![$($arg),*].into_iter().map(|x: i32| x as usize).collect::<OrdSet<_>>()
        }
    }

    #[test]
    fn ancestor_segments() {
        let mut graph = DAG::default();
        graph.add_node((), vec![]);
        graph.add_node((), vec![0]);
        graph.add_node((), vec![1]);
        graph.add_node((), vec![2]);
        assert_eq!(graph.node(0).ancestors(), &hash_set![]);
        assert_eq!(graph.node(1).ancestors(), &hash_set![0]);
        assert_eq!(graph.node(2).ancestors(), &hash_set![0, 1]);
        assert_eq!(graph.node(3).ancestors(), &hash_set![0, 1, 2]);
    }

    #[test]
    fn remainder_ancestors() {
        // 0---1---2
        //  \       \
        //   3---4---x
        let mut graph = DAG::default();
        graph.add_node((), vec![]);
        graph.add_node((), vec![0]);
        graph.add_node((), vec![1]);
        graph.add_node((), vec![0]);
        graph.add_node((), vec![3]);
        graph.add_node((), vec![2, 4]);
        assert_eq!(graph.node(5).remainder_ancestors(), &[3, 4]);
    }
}
