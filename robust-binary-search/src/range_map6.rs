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

#![allow(dead_code)] // TODO: remove
#![allow(unreachable_code)] // TODO: remove
#![allow(unused_assignments)] // TODO: remove
#![allow(unused_imports)] // TODO: remove
#![allow(unused_variables)] // TODO: remove

use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::ptr;

/// A single entry in a RangeMap, which corresponds to a range of individual values.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RangeMapEntry<T> {
    /// Beginning index of the range within the conceptual vector of individual values.
    offset: usize,
    /// Number of indices captured by the range.
    len: usize,
    /// Value of all individual values within the range.
    value: T,
}

impl<T> RangeMapEntry<T> {
    #[allow(dead_code)]
    /// Returns the index of the first individual value in the range.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the length of the range.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns offset() + len().
    pub fn end(&self) -> usize {
        self.offset + self.len
    }

    /// Returns the value of the range.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Returns the value of the range.
    pub fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
enum Color {
    Red,
    Black,
}

#[derive(Clone, Debug)]
struct Node<T> {
    values: Vec<RangeMapEntry<T>>,
    left: Option<Box<Node<T>>>,
    right: Option<Box<Node<T>>>,
    color: Color,
}

impl<T: Clone> Node<T> {
    fn offset(&self) -> usize {
        self.values[0].offset
    }

    fn end(&self) -> usize {
        self.values.last().unwrap().end()
    }

    fn ranges<'a>(this: &'a Box<Self>) -> RangesIter<'a, T> {
        let mut stack = Vec::new();
        let mut n = this;
        loop {
            stack.push(n);
            if let Some(left) = &n.left {
                n = left;
            } else {
                break;
            }
        }
        RangesIter { stack, index: 0 }
    }

    fn ranges_mut<'a>(_this: &'a Box<Self>) -> RangesMutIter<'a, T> {
        todo!()
    }

    fn node_for_index(&self, index: usize) -> (&Node<T>, usize) {
        let mut node = self;
        loop {
            if index < node.values[0].offset {
                node = node.left.as_ref().unwrap().borrow();
            } else if index >= node.values.last().unwrap().end() {
                node = node.right.as_ref().unwrap().borrow();
            } else {
                return match node.values.binary_search_by_key(&index, |e| e.offset) {
                    Ok(i) => (node, i),
                    Err(i) => (node, i - 1),
                };
            }
        }
    }

    fn entry_index(&self, index: usize) -> usize {
        match self.values.binary_search_by_key(&index, |e| e.offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    fn node_for_index_mut(&mut self, index: usize) -> (&mut Node<T>, usize) {
        let mut stack = Vec::new();
        let mut node = self;
        loop {
            stack.push(node as *mut Node<T>);
            if index < node.values[0].offset {
                node = node.left.as_mut().unwrap().borrow_mut();
            } else if index >= node.values.last().unwrap().end() {
                node = node.right.as_mut().unwrap().borrow_mut();
            } else {
                return match node.values.binary_search_by_key(&index, |e| e.offset) {
                    Ok(i) => (node, i),
                    Err(i) => (node, i - 1),
                };
            }
        }
    }

    fn range_for_index(&self, index: usize) -> &RangeMapEntry<T> {
        let (node, i) = self.node_for_index(index);
        &node.values[i]
    }

    fn rotate_left(&mut self) {
        let mut node = self.right.take().unwrap();
        std::mem::swap(self, &mut node);
        node.right = self.left.take();
        self.left = Some(node);
    }

    fn rotate_right(&mut self) {
        let mut node = self.left.take().unwrap();
        std::mem::swap(self, &mut node);
        node.left = self.right.take();
        self.right = Some(node);
    }

    fn insert(self: &mut Box<Node<T>>, values: Vec<RangeMapEntry<T>>) {
        unsafe {
            // Invariants:
            // - Each entry is non-null
            // - The first entry is the root
            // - Each entry is a child of the previous entry
            // - Only the last entry may be modified structurally
            let mut stack: Vec<*mut Node<T>> = Vec::new();
            // TODO: remove
            let validate = |s: &Vec<*mut Node<T>>| {
                // let mut parent_p: *mut Node<T> = ptr::null_mut();
                // for p in s {
                //     assert!(*p as usize > 0x1000, "p = {:?}", *p);
                //     assert!(parent_p.is_null() ||
                //             opt_box_ptr(&(*parent_p).left) == *p ||
                //             opt_box_ptr(&(*parent_p).right) == *p);
                //     parent_p = *p;
                // }
                true
            };
            {
                let new_node = Node {
                    values,
                    left: None,
                    right: None,
                    color: Color::Red,
                };
                let mut parent: &mut Node<T> = self.borrow_mut();
                loop {
                    stack.push(parent as *mut Node<T>);
                    validate(&stack);
                    if new_node.offset() < parent.offset() {
                        match &mut parent.left {
                            Some(left) => parent = left,
                            left @ None => {
                                *left = Some(Box::new(new_node));
                                stack.push(left.as_mut().unwrap().borrow_mut() as *mut Node<T>);
                                break;
                            }
                        }
                    } else {
                        match &mut parent.right {
                            Some(right) => parent = right,
                            right @ None => {
                                *right = Some(Box::new(new_node));
                                stack.push(right.as_mut().unwrap().borrow_mut() as *mut Node<T>);
                                validate(&stack);
                                break;
                            }
                        }
                    }
                }
            }
            validate(&stack);
            while validate(&stack)
                && stack.len() > 1
                && (*stack[stack.len() - 2]).color == Color::Red
            {
                let mut node_p: *mut Node<T> = stack[stack.len() - 1];
                let mut parent_p: *mut Node<T> = stack[stack.len() - 2];
                let mut grandparent_p: *mut Node<T> = stack[stack.len() - 3];
                if parent_p as *const _ == opt_box_ptr(&(*grandparent_p).left) {
                    let uncle_p: *mut Option<Box<Node<T>>> = &mut (*grandparent_p).right as *mut _;
                    if node_color(&*uncle_p) == Color::Red {
                        (*parent_p).color = Color::Black;
                        (*uncle_p).as_mut().unwrap().color = Color::Black;
                        (*grandparent_p).color = Color::Red;
                        stack.pop();
                        stack.pop();
                        validate(&stack);
                    } else {
                        if node_p as *const _ == opt_box_ptr(&mut (*parent_p).right) {
                            stack.pop();
                            node_p = parent_p;
                            // parent_p = grandparent_p;
                            // grandparent_p = stack[stack.len() - 3];
                            validate(&stack);
                            (*node_p).rotate_left();
                            stack.push((*node_p).left.as_mut().unwrap().borrow_mut());
                            node_p = stack[stack.len() - 1];
                            parent_p = stack[stack.len() - 2];
                            grandparent_p = stack[stack.len() - 3];
                            validate(&stack);
                        }
                        (*parent_p).color = Color::Black;
                        (*grandparent_p).color = Color::Red;
                        validate(&stack);
                        stack.pop(); // node
                        stack.pop(); // parent
                                     //     G
                                     //    /
                                     //   P
                                     //  /
                                     // X
                        (*grandparent_p).rotate_right();
                        //   P
                        //  / \
                        // X   G
                        stack.push(node_p);
                        validate(&stack);
                    }
                    validate(&stack);
                } else {
                    let uncle_p: *mut Option<Box<Node<T>>> = &mut (*grandparent_p).left as *mut _;
                    if node_color(&*uncle_p) == Color::Red {
                        (*parent_p).color = Color::Black;
                        (*uncle_p).as_mut().unwrap().color = Color::Black;
                        (*grandparent_p).color = Color::Red;
                        stack.pop();
                        stack.pop();
                        validate(&stack);
                    } else {
                        if node_p as *const _ == opt_box_ptr(&mut (*parent_p).left) {
                            stack.pop();
                            node_p = parent_p;
                            // parent_p = grandparent_p;
                            // grandparent_p = stack[stack.len() - 3];
                            validate(&stack);
                            (*node_p).rotate_right();
                            stack.push((*node_p).right.as_mut().unwrap().borrow_mut());
                            node_p = stack[stack.len() - 1];
                            parent_p = stack[stack.len() - 2];
                            grandparent_p = stack[stack.len() - 3];
                            validate(&stack);
                        }
                        (*parent_p).color = Color::Black;
                        (*grandparent_p).color = Color::Red;
                        validate(&stack);
                        stack.pop(); // node
                        stack.pop(); // parent
                                     // G
                                     //  \
                                     //   P
                                     //    \
                                     //     X
                        (*grandparent_p).rotate_left();
                        //   P
                        //  / \
                        // G   X
                        stack.push(node_p);
                        validate(&stack);
                    }
                    validate(&stack);
                }
                validate(&stack);
            }
            (*stack[0]).color = Color::Black;
        }
    }
}

fn box_ptr<T>(b: &Box<T>) -> *const T {
    Box::borrow(b) as *const T
}

fn opt_box_ptr<T>(p: &Option<Box<T>>) -> *const T {
    if let Some(b) = p.as_ref() {
        Box::borrow(b) as *const T
    } else {
        std::ptr::null()
    }
}

fn node_color<T>(node: &Option<Box<Node<T>>>) -> Color {
    if let Some(n) = node.as_ref() {
        n.color
    } else {
        Color::Black
    }
}

struct RangesIter<'a, T> {
    stack: Vec<&'a Box<Node<T>>>,
    index: usize,
}

impl<'a, T: 'a> Iterator for RangesIter<'a, T> {
    type Item = &'a RangeMapEntry<T>;

    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        if let Some(last) = self.stack.last() {
            if self.index < last.values.len() {
                let i = self.index;
                self.index += 1;
                return Some(&last.values[i]);
            }
        } else {
            return None;
        }
        loop {
            let old = self.stack.pop().unwrap();
            if let Some(last) = self.stack.last() {
                if box_ptr(old) == opt_box_ptr(&last.left) {
                    if let Some(right) = &last.right {
                        self.stack.push(right);
                        self.index = 0;
                        return Some(&right.values[0]);
                    } else {
                        return None;
                    }
                }
                assert_eq!(box_ptr(old), opt_box_ptr(&last.right));
            } else {
                return None;
            }
        }
    }
}

impl<'a, T: 'a> DoubleEndedIterator for RangesIter<'a, T> {
    fn next_back(&mut self) -> Option<<Self as Iterator>::Item> {
        todo!()
    }
}

#[derive(Debug)]
struct RangesMutIter<'a, T> {
    left_stack: Vec<*mut Node<T>>,
    left_index: usize,
    right_stack: Vec<*mut Node<T>>,
    right_index: usize,
    phantom: PhantomData<&'a mut Node<T>>,
}

// TODO: Remove Debug constraint
impl<'a, T: 'a + Debug> Iterator for RangesMutIter<'a, T> {
    type Item = &'a mut RangeMapEntry<T>;

    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        // TODO: shrink unsafe
        unsafe {
            let left_last = self.left_stack.last();
            let right_last = self.right_stack.last();
            if left_last.is_none()
                || right_last.is_none()
                || (left_last == right_last && self.left_index == self.right_index)
            {
                None
            } else {
                let mut node: *mut Node<T> = *self.left_stack.last().unwrap();
                let item: &mut RangeMapEntry<T> = &mut (*node).values[self.left_index];
                self.left_index += 1;
                if self.left_index >= (*node).values.len() {
                    if let Some(right) = &mut (*node).right {
                        self.left_stack.push(right.borrow_mut() as *mut _);
                        node = right.borrow_mut() as *mut _;
                        while let Some(left) = &mut (*node).left {
                            self.left_stack.push(left.borrow_mut() as *mut _);
                            node = left.borrow_mut() as *mut _;
                        }
                        self.left_index = 0;
                    } else {
                        if self.left_stack.len() > 1
                            && node as *const _
                                == opt_box_ptr(&(*self.left_stack[self.left_stack.len() - 2]).left)
                        {
                            self.left_stack.pop();
                            self.left_index = 0;
                        } else {
                            // We've hit the end.
                        }
                    }
                }
                Some(item)
            }
        }
    }
}

impl<'a, T: 'a + Debug> DoubleEndedIterator for RangesMutIter<'a, T> {
    fn next_back(&mut self) -> Option<<Self as Iterator>::Item> {
        todo!()
    }
}

/// A RangeMap is essentially a fixed-length vector optimized for long stretches of equal values.
/// The RangeMap is partitioned into contiguous RangeMapEntries. For example, a map with the
/// entries:
///
/// ```text
/// RangeMapEntry {
///   offset: 0,
///   len: 1,
///   value: 'a'
/// }
/// RangeMapEntry {
///   offset: 1,
///   len: 2,
///   value: 'b'
/// }
/// RangeMapEntry {
///   offset: 3,
///   len: 3,
///   value: 'c'
/// }
/// RangeMapEntry {
///   offset: 6,
///   len: 4,
///   value: 'd'
/// }
/// ```
///
/// represents the data:
///
/// ```text
/// ['a', 'b', 'b', 'c', 'c', 'c', 'd', 'd', 'd', 'd']
/// ```
///
/// Note that neighboring entries may contain the same value.
#[derive(Clone, Debug)]
pub struct RangeMap<T> {
    root: Box<Node<T>>,
    len: usize,
    branching: usize,
}

// TODO: remove Debug constraint
impl<T: Clone + Debug> RangeMap<T> {
    /// Creates a new RangeMap with the given size and initial value. It contains a single entry
    /// spanning the entire range.
    pub fn new(size: usize, value: T, branching: usize) -> Self {
        RangeMap {
            root: Box::new(Node {
                values: vec![RangeMapEntry {
                    offset: 0,
                    len: size,
                    value,
                }],
                left: None,
                right: None,
                color: Color::Black,
            }),
            len: size,
            branching,
        }
    }

    /// Returns the length of the entire range.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns an iterator over entries.
    pub fn ranges(&self) -> impl DoubleEndedIterator<Item = &RangeMapEntry<T>> {
        Node::ranges(&self.root)
    }

    /// Returns an iterator over mutable entries.
    pub fn ranges_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>> {
        Node::ranges_mut(&self.root)
    }

    /// Returns the entry containing the given index.
    pub fn range_for_index(&self, index: usize) -> &RangeMapEntry<T> {
        self.root.range_for_index(index)
    }

    fn split_iters(
        &mut self,
        index: usize,
    ) -> (
        impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>>,
        impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>>,
    ) {
        let (mid_stack, mid_index) = {
            let mut stack: Vec<*mut Node<T>> = Vec::new();
            let mut node = &mut self.root;
            loop {
                stack.push(node.borrow_mut() as *mut _);
                if index < node.offset() {
                    if let Some(left) = &mut node.left {
                        node = left;
                    } else {
                        break;
                    }
                } else if index >= node.end() {
                    if let Some(right) = &mut node.right {
                        node = right;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let entry_index = unsafe { (**stack.last().unwrap()).entry_index(index) };
            (stack, entry_index)
        };
        let (left_stack, left_index) = {
            let mut stack = Vec::new();
            let mut node = &mut self.root;
            loop {
                stack.push(node.borrow_mut() as *mut _);
                if let Some(left) = &mut node.left {
                    node = left;
                } else {
                    break;
                }
            }
            (stack, 0)
        };
        let (right_stack, right_index) = {
            let mut stack = Vec::new();
            let mut node = &mut self.root;
            loop {
                stack.push(node.borrow_mut() as *mut _);
                if let Some(right) = &mut node.right {
                    node = right;
                } else {
                    break;
                }
            }
            (stack, node.values.len())
        };
        let left = RangesMutIter {
            left_stack,
            left_index,
            right_stack: mid_stack.clone(),
            right_index: mid_index,
            phantom: PhantomData,
        };
        let right = RangesMutIter {
            left_stack: mid_stack,
            left_index: mid_index,
            right_stack,
            right_index,
            phantom: PhantomData,
        };
        (left, right)
    }

    /// Ensures that `index-1` and `index` are in different RangeMapEntrys.
    /// Returns iterators for the left and right side of the split.
    pub fn split(
        &mut self,
        index: usize,
        // ) -> (
        //     impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>>,
        //     impl DoubleEndedIterator<Item = &mut RangeMapEntry<T>>,
    ) {
        let (node, i) = self.root.node_for_index_mut(index);
        if node.values[i].offset == index {
            // return self.split_iters(index);
            return;
        }
        let old_entry = &mut node.values[i];
        let new_entry = RangeMapEntry {
            offset: index,
            len: old_entry.end() - index,
            value: old_entry.value.clone(),
        };
        old_entry.len = index - old_entry.offset;
        node.values.insert(i + 1, new_entry);
        if node.values.len() >= self.branching {
            let half = node.values.len() / 2;
            let split_values = node.values.drain(half..).collect::<Vec<_>>();
            self.root.insert(split_values);
        }
        //        self.split_iters(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_for_index_empty() {
        let m = RangeMap::new(10, 0.0, 2);
        assert_eq!(
            m.range_for_index(0),
            &RangeMapEntry {
                offset: 0,
                len: 10,
                value: 0.0
            }
        );
        assert_eq!(
            m.range_for_index(9),
            &RangeMapEntry {
                offset: 0,
                len: 10,
                value: 0.0
            }
        );
    }

    // TODO: uncomment
    // #[test]
    // fn split() {
    //     let mut m = RangeMap::new(10, 0.0, 2);
    //     assert_eq!(
    //         m.ranges().collect::<Vec<_>>(),
    //         vec![&RangeMapEntry {
    //             offset: 0,
    //             len: 10,
    //             value: 0.0
    //         }]
    //     );
    //     let (left, right) = m.split(5);
    //     assert_eq!(
    //         left.collect::<Vec<_>>(),
    //         vec![&RangeMapEntry {
    //             offset: 0,
    //             len: 5,
    //             value: 0.0
    //         }]
    //     );
    //     assert_eq!(
    //         right.collect::<Vec<_>>(),
    //         vec![&RangeMapEntry {
    //             offset: 5,
    //             len: 5,
    //             value: 0.0
    //         }]
    //     );
    //     assert_eq!(
    //         m.range_for_index(0),
    //         &RangeMapEntry {
    //             offset: 0,
    //             len: 5,
    //             value: 0.0
    //         }
    //     );
    //     assert_eq!(
    //         m.range_for_index(4),
    //         &RangeMapEntry {
    //             offset: 0,
    //             len: 5,
    //             value: 0.0
    //         }
    //     );
    //     assert_eq!(
    //         m.range_for_index(5),
    //         &RangeMapEntry {
    //             offset: 5,
    //             len: 5,
    //             value: 0.0
    //         }
    //     );
    //     assert_eq!(
    //         m.range_for_index(9),
    //         &RangeMapEntry {
    //             offset: 5,
    //             len: 5,
    //             value: 0.0
    //         }
    //     );
    // }
}
