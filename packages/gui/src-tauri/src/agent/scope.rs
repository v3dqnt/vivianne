//! Scoped registration: each agent or group gets its own set of contributions.
//!
//! A scope inherits everything its ancestors registered, and where both define
//! the same key the nearest one wins. An ancestor can look down at what its
//! descendants registered; a descendant's contributions never reach up into its
//! ancestors' own view. Disposing a scope removes everything it owns.
//!
//! Dependency-free, so anything that needs per-thread or per-group isolation can
//! use it without depending on the agent loop.

use std::sync::{Arc, Mutex, Weak};

struct Node<T> {
    parent: Option<Arc<Node<T>>>,
    items: Mutex<Vec<(String, T)>>,
    children: Mutex<Vec<Weak<Node<T>>>>,
}

pub struct Scope<T> {
    node: Arc<Node<T>>,
}

impl<T> Clone for Scope<T> {
    fn clone(&self) -> Self {
        Self { node: self.node.clone() }
    }
}

impl<T: Clone> Scope<T> {
    pub fn root() -> Self {
        Self { node: Arc::new(Node { parent: None, items: Mutex::new(vec![]), children: Mutex::new(vec![]) }) }
    }

    /// A scope under this one.
    pub fn child(&self) -> Self {
        let node = Arc::new(Node { parent: Some(self.node.clone()), items: Mutex::new(vec![]), children: Mutex::new(vec![]) });
        let mut kids = self.node.children.lock().unwrap_or_else(|e| e.into_inner());
        kids.retain(|w| w.strong_count() > 0);
        kids.push(Arc::downgrade(&node));
        Self { node }
    }

    /// Register `value` under `key` in this scope, replacing this scope's own earlier entry for the key.
    pub fn register(&self, key: &str, value: T) {
        let mut items = self.node.items.lock().unwrap_or_else(|e| e.into_inner());
        match items.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => items.push((key.to_string(), value)),
        }
    }

    /// Remove this scope's own entry for `key`. Returns whether there was one.
    pub fn unregister(&self, key: &str) -> bool {
        let mut items = self.node.items.lock().unwrap_or_else(|e| e.into_inner());
        let before = items.len();
        items.retain(|(k, _)| k != key);
        items.len() != before
    }

    fn own(&self, key: &str) -> Option<T> {
        self.node.items.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// The nearest definition of `key`: this scope's, else the closest ancestor's.
    pub fn get(&self, key: &str) -> Option<T> {
        let mut at = Some(&self.node);
        while let Some(node) = at {
            if let Some(v) = node.items.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()) {
                return Some(v);
            }
            at = node.parent.as_ref();
        }
        None
    }

    /// Everything visible from here: ancestors' entries first, then nearer ones, with a
    /// nearer definition replacing a farther one of the same key.
    pub fn visible(&self) -> Vec<(String, T)> {
        let mut chain = vec![];
        let mut at = Some(&self.node);
        while let Some(node) = at {
            chain.push(node.clone());
            at = node.parent.as_ref();
        }
        let mut out: Vec<(String, T)> = vec![];
        for node in chain.iter().rev() {
            for (k, v) in node.items.lock().unwrap_or_else(|e| e.into_inner()).iter() {
                match out.iter_mut().find(|(ok, _)| ok == k) {
                    Some(slot) => slot.1 = v.clone(),
                    None => out.push((k.clone(), v.clone())),
                }
            }
        }
        out
    }

    /// Everything registered under `key` in this scope and in every live descendant.
    /// This is how an ancestor sees what its descendants contributed.
    pub fn observe(&self, key: &str) -> Vec<T> {
        let mut found: Vec<T> = self.own(key).into_iter().collect();
        let kids: Vec<Arc<Node<T>>> = self.node.children.lock().unwrap_or_else(|e| e.into_inner()).iter().filter_map(Weak::upgrade).collect();
        for kid in kids {
            found.extend(Scope { node: kid }.observe(key));
        }
        found
    }

    /// Remove everything this scope owns, and everything its descendants own.
    pub fn dispose(&self) {
        let kids: Vec<Arc<Node<T>>> = self.node.children.lock().unwrap_or_else(|e| e.into_inner()).drain(..).filter_map(|w| w.upgrade()).collect();
        for kid in kids {
            Scope { node: kid }.dispose();
        }
        self.node.items.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_inherits_and_the_nearest_definition_wins() {
        let root = Scope::root();
        root.register("a", 1);
        root.register("b", 2);
        let child = root.child();
        child.register("b", 20);
        child.register("c", 30);
        assert_eq!((child.get("a"), child.get("b"), child.get("c"), child.get("z")), (Some(1), Some(20), Some(30), None));
        assert_eq!(root.get("b"), Some(2), "the ancestor's own view is unchanged");
        assert_eq!(root.get("c"), None, "nothing reaches up");
    }

    #[test]
    fn what_is_visible_is_ancestors_first_with_nearer_entries_replacing_in_place() {
        let root = Scope::root();
        root.register("a", 1);
        root.register("b", 2);
        let child = root.child();
        child.register("b", 20);
        child.register("c", 30);
        assert_eq!(child.visible(), [("a".into(), 1), ("b".into(), 20), ("c".into(), 30)]);
        assert_eq!(root.visible(), [("a".into(), 1), ("b".into(), 2)]);
    }

    #[test]
    fn an_ancestor_can_observe_every_descendant_but_not_the_reverse() {
        let root = Scope::root();
        root.register("k", "root");
        let a = root.child();
        let b = a.child();
        a.register("k", "a");
        b.register("k", "b");
        let mut seen = root.observe("k");
        seen.sort();
        assert_eq!(seen, ["a", "b", "root"]);
        assert_eq!(b.observe("k"), ["b"], "a descendant sees only itself and below");
    }

    #[test]
    fn disposing_a_scope_removes_what_it_and_its_descendants_own_and_nothing_else() {
        let root = Scope::root();
        root.register("keep", 1);
        let a = root.child();
        let b = a.child();
        a.register("x", 2);
        b.register("y", 3);
        a.dispose();
        assert_eq!((a.get("x"), b.get("y"), a.get("keep")), (None, None, Some(1)));
        assert_eq!(root.observe("x"), Vec::<i32>::new());
    }

    #[test]
    fn registering_again_replaces_and_unregistering_removes_only_that_scopes_entry() {
        let root = Scope::root();
        root.register("k", 1);
        let child = root.child();
        child.register("k", 2);
        child.register("k", 3);
        assert_eq!(child.get("k"), Some(3));
        assert!(child.unregister("k") && !child.unregister("k"));
        assert_eq!(child.get("k"), Some(1), "the inherited one shows through again");
    }

    #[test]
    fn dropped_children_are_forgotten() {
        let root = Scope::root();
        {
            let c = root.child();
            c.register("k", 1);
        }
        assert!(root.observe("k").is_empty());
    }
}
