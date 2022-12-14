#![allow(unused)] // temporary

///----------------------------------------------------------------------------------------------------
/// The author disclaims copyright to this source code. In place of a legal notice, here is a blessing:
///     May you do good and not evil.
///     May you find forgiveness for yourself and forgive others.
///     May you share freely, never taking more than you give.
///----------------------------------------------------------------------------------------------------
/// This file implements Node objects which represent logical nodes in a B-Link Tree. It implements a
/// thread-safe API for modifying, splitting, and traversing nodes.
///----------------------------------------------------------------------------------------------------
use crate::sync::{BinarySemaphore, LatchType, RwLatch as _, RwSynchronized};

/// The internal structure of a node is as follows:
///     - `min_ord` contains the minimum order of the node. It is a tree parameter which determines
///        the lower bound of the number of keys present in a node.
///     - `root` is a boolean value representing whether this node is the tree's root
///     - `keys` is a vector of generic keys (must implement PartialOrd + Ord + PartialEq + Eq)
///     - `children` is a vector of RwSynchronized Nodes. This is type-aliased to Node<T> below.
///        It is a vector of smart pointers, not actual objects
///     - `right_link` is a optional value representing the link of a node to its immediate right
///        sibling  
///     - `out_link` is an optional value representing the link of a deleted node to a node where
///        a thread may resume its search in case it had strayed from the path.

pub struct NodeInner<T> {
    min_ord: usize,
    root: bool,
    keys: Vec<T>,
    children: Vec<RwSynchronized<NodeInner<T>>>,
    right_link: Option<RwSynchronized<NodeInner<T>>>,
    out_link: Option<RwSynchronized<NodeInner<T>>>,
}

impl<T> NodeInner<T> {
    fn new(min_ord: usize) -> Self {
        Self {
            min_ord,
            root: false,
            keys: Vec::new(),
            children: Vec::new(),
            right_link: None,
            out_link: None,
        }
    }
}

pub type Node<T> = RwSynchronized<NodeInner<T>>;

/// Methods for generic BTreeNodes
pub trait BTreeNode<T> {
    fn create(min_ord: usize) -> Node<T>;
    fn move_right(&self, key: &T, latch_type: LatchType) -> Node<T>;
    fn has_key(&self, key: &T) -> bool;
    fn is_root(&self) -> bool;
    fn set_root(&mut self, root: bool);
    fn set_keys(&self, keys: Vec<T>);
    fn set_children(&self, children: Vec<Node<T>>);
    fn would_overflow(&self) -> bool;
    fn would_underflow(&self) -> bool;
    fn set_right_link(&mut self, new_right_link: Option<Node<T>>);
    fn set_out_link(&mut self, new_out_link: Option<Node<T>>);
}

impl<T> BTreeNode<T> for Node<T>
where
    T: Ord + PartialOrd,
{
    fn create(min_ord: usize) -> Node<T> {
        RwSynchronized::init(NodeInner::new(min_ord))
    }

    /// Check whether the given key is in the node. Must have a latch or RAII guard on the node for safety.
    fn has_key(&self, key: &T) -> bool {
        let inner = unsafe { &(*self.data_ptr()) };
        inner.keys.binary_search(&key).is_err()
    }

    /// Check whether a node is the root. Must have a latch or RAII guard on the node for safety
    fn is_root(&self) -> bool {
        let inner = unsafe { &(*self.data_ptr()) };
        inner.root
    }

    fn set_root(&mut self, root: bool) {
        let inner = unsafe { &mut (*self.data_ptr()) };
        inner.root = root;
    }

    /// Move right until we are at the node at which they key would exist if it exists
    fn move_right(&self, key: &T, latch_type: LatchType) -> Node<T> {
        let inner = unsafe { &(*self.data_ptr()) };
        Node::create(0)
    }

    /// Set the children of a node to a vector of Node<T>
    fn set_children(&self, children: Vec<Node<T>>) {
        let inner = unsafe { &mut (*self.data_ptr()) };
        inner.children = children;
    }

    /// Set the keys of a node to a vector of T
    fn set_keys(&self, keys: Vec<T>) {
        let inner = unsafe { &mut (*self.data_ptr()) };
        inner.keys = keys;
    }

    /// Return true if the node is in danger of overflowing
    fn would_overflow(&self) -> bool {
        let inner = unsafe { &mut (*self.data_ptr()) };
        inner.keys.len() == inner.min_ord
    }

    /// Return true if the node is in danger of underflowing
    fn would_underflow(&self) -> bool {
        let inner = unsafe { &mut (*self.data_ptr()) };
        inner.keys.len() == 2 * inner.min_ord
    }

    /// Set the right link of the node
    fn set_right_link(&mut self, new_right_link: Option<Node<T>>) {
        let inner = unsafe { &mut (*self.data_ptr()) };
        inner.right_link = new_right_link;
    }

    /// Set the out link of the node
    fn set_out_link(&mut self, new_out_link: Option<Node<T>>) {
        let inner = unsafe { &mut (*self.data_ptr()) };
        inner.out_link = new_out_link;
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused)]
    use rayon::ThreadPoolBuilder;

    use super::{BTreeNode, Node};

    #[test]
    fn test_create() {
        // Testing creation
        let mut node: Node<usize> = Node::create(2);
        let inner = unsafe { &mut (*node.data_ptr()) };
        assert!(inner.root == false);
        assert!(inner.right_link.is_none());
        assert!(inner.out_link.is_none());
        assert!(inner.children.len() == 0);
        assert!(inner.keys.len() == 0);
        assert!(inner.min_ord == 2);

        // Testing setters

        assert!(!node.is_root());
        node.set_root(true);
        assert!(node.is_root());

        let new_keys: Vec<usize> = vec![1, 2, 3, 4];
        node.set_keys(new_keys);
        assert!(inner.keys.len() == 4);

        for i in 0..4 {
            assert!(inner.keys[i] == i + 1);
        }

        let child_one: Node<usize> = Node::create(2);
        let child_two: Node<usize> = Node::create(2);
        let child_three: Node<usize> = Node::create(2);
        let child_four: Node<usize> = Node::create(2);
        let child_five: Node<usize> = Node::create(2);

        let new_children: Vec<Node<usize>> =
            vec![child_one, child_two, child_three, child_four, child_five];

        node.set_children(new_children);

        assert!(inner.children.len() == 5);

        let sib: Node<usize> = Node::create(2);
        let out: Node<usize> = Node::create(2);

        node.set_right_link(Some(sib));
        assert!(inner.right_link.is_some());

        node.set_out_link(Some(out));
        assert!(inner.out_link.is_some());

        node.set_right_link(None);
        node.set_out_link(None);

        assert!(inner.right_link.is_none());
        assert!(inner.out_link.is_none());
    }

    #[test]
    fn test_concurrent_update() {
        let pool = ThreadPoolBuilder::new().num_threads(20).build().unwrap();
        let node: Node<usize> = Node::create(2);
    }
}
