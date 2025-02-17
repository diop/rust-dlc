//! Data structure and functions to create, insert, lookup and iterate a trie
//! of trie.

use crate::{LookupResult, Node};
use combination_iterator::CombinationIterator;
use digit_trie::{DigitTrie, DigitTrieDump, DigitTrieIter};
use dlc::Error;
use multi_oracle::compute_outcome_combinations;

#[derive(Clone, Debug)]
/// Information stored in a node.
pub struct TrieNodeInfo {
    /// The index of the sub-trie.
    pub trie_index: usize,
    /// The index of the node in the trie store.
    pub store_index: usize,
}

type MultiTrieNode<T> = Node<DigitTrie<T>, DigitTrie<Vec<TrieNodeInfo>>>;
type NodeStackElement<'a> = Vec<((usize, Vec<usize>), DigitTrieIter<'a, Vec<TrieNodeInfo>>)>;

impl<T> MultiTrieNode<T> {
    fn new_node(base: usize) -> MultiTrieNode<T> {
        let m_trie = DigitTrie::<Vec<TrieNodeInfo>>::new(base);
        MultiTrieNode::Node(m_trie)
    }
    fn new_leaf(base: usize) -> MultiTrieNode<T> {
        let d_trie = DigitTrie::<T>::new(base);
        MultiTrieNode::Leaf(d_trie)
    }
}

/// Struct for iterating over the values of a MultiTrie.
pub struct MultiTrieIterator<'a, T> {
    trie: &'a MultiTrie<T>,
    node_stack: NodeStackElement<'a>,
    trie_info_iter: Vec<(
        Vec<usize>,
        std::iter::Enumerate<std::slice::Iter<'a, TrieNodeInfo>>,
    )>,
    leaf_iter: Vec<(usize, DigitTrieIter<'a, T>)>,
    cur_path: Vec<(usize, Vec<usize>)>,
}

fn create_node_iterator<T>(node: &MultiTrieNode<T>) -> DigitTrieIter<Vec<TrieNodeInfo>> {
    match node {
        Node::Node(d_trie) => DigitTrieIter::new(d_trie),
        _ => unreachable!(),
    }
}

fn create_leaf_iterator<T>(node: &MultiTrieNode<T>) -> DigitTrieIter<T> {
    match node {
        Node::Leaf(d_trie) => DigitTrieIter::new(d_trie),
        _ => unreachable!(),
    }
}

impl<'a, T> MultiTrieIterator<'a, T> {
    /// Create a new MultiTrie iterator.
    pub fn new(trie: &'a MultiTrie<T>) -> MultiTrieIterator<'a, T> {
        let mut node_stack = Vec::with_capacity(trie.nb_required);
        let nb_roots = trie.nb_tries - trie.nb_required + 1;
        let mut leaf_iter = Vec::new();
        for i in (0..nb_roots).rev() {
            if trie.nb_required > 1 {
                node_stack.push((
                    (i, Vec::<usize>::new()),
                    create_node_iterator(&trie.store[i]),
                ));
            } else {
                leaf_iter.push((i, create_leaf_iterator(&trie.store[i])));
            }
        }
        MultiTrieIterator {
            trie,
            node_stack,
            trie_info_iter: Vec::new(),
            leaf_iter,
            cur_path: Vec::new(),
        }
    }
}

/// Implements the Iterator trait for MultiTrieIterator.
impl<'a, T> Iterator for MultiTrieIterator<'a, T> {
    type Item = LookupResult<'a, T, (usize, Vec<usize>)>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut leaf_iter = self.leaf_iter.last_mut();
        if let Some(ref mut iter) = &mut leaf_iter {
            match iter.1.next() {
                Some(res) => {
                    let mut path = self.cur_path.clone();
                    path.push((iter.0, res.path));
                    return Some(LookupResult {
                        value: res.value,
                        path,
                    });
                }
                None => {
                    self.leaf_iter.pop();
                    return self.next();
                }
            }
        };

        let mut trie_info_iter = self.trie_info_iter.last_mut();

        if let Some(ref mut iter) = &mut trie_info_iter {
            match iter.1.next() {
                None => {
                    self.trie_info_iter.pop();
                    self.cur_path.pop();
                }
                Some((i, info)) => {
                    if i == 0 {
                        self.cur_path
                            .push((self.node_stack.last().unwrap().0 .0, iter.0.clone()));
                    }
                    match &self.trie.store[info.store_index] {
                        Node::None => unreachable!(),
                        Node::Node(d_trie) => {
                            self.node_stack.push((
                                (info.trie_index, iter.0.clone()),
                                DigitTrieIter::new(d_trie),
                            ));
                        }
                        Node::Leaf(d_trie) => {
                            self.leaf_iter
                                .push((info.trie_index, DigitTrieIter::new(d_trie)));
                            return self.next();
                        }
                    }
                }
            }
        }

        let res = self.node_stack.pop();

        let ((cur_trie_index, parent_path), mut cur_iter) = match res {
            None => return None,
            Some(cur) => cur,
        };

        match cur_iter.next() {
            None => self.next(),
            Some(res) => {
                // Put back the node on the stack
                self.node_stack
                    .push(((cur_trie_index, parent_path), cur_iter));

                // Push an iterator to the child on the trie info stack
                self.trie_info_iter
                    .push((res.path, res.value.iter().enumerate()));

                self.next()
            }
        }
    }
}

/// Struct used to store DLC outcome information for multi oracle cases.  
#[derive(Clone)]
pub struct MultiTrie<T> {
    store: Vec<MultiTrieNode<T>>,
    base: usize,
    nb_tries: usize,
    nb_required: usize,
    min_support_exp: usize,
    max_error_exp: usize,
    nb_digits: usize,
    maximize_coverage: bool,
}

impl<T> MultiTrie<T> {
    /// Create a new MultiTrie. Panics if `nb_required` is less or equal to
    /// zero, or if `nb_tries` is less than `nb_required`.
    pub fn new(
        nb_tries: usize,
        nb_required: usize,
        base: usize,
        min_support_exp: usize,
        max_error_exp: usize,
        nb_digits: usize,
        maximize_coverage: bool,
    ) -> MultiTrie<T> {
        assert!(nb_required > 0 && nb_tries >= nb_required);
        let nb_roots = nb_tries - nb_required + 1;
        let mut store = Vec::new();

        if nb_required > 1 {
            store.resize_with(nb_roots, || MultiTrieNode::new_node(base));
        } else {
            store.resize_with(nb_roots, || MultiTrieNode::new_leaf(base));
        }

        MultiTrie {
            store,
            base,
            nb_tries,
            nb_required,
            min_support_exp,
            max_error_exp,
            nb_digits,
            maximize_coverage,
        }
    }

    fn swap_remove(&mut self, index: usize) -> MultiTrieNode<T> {
        self.store.push(MultiTrieNode::None);
        self.store.swap_remove(index)
    }

    /// Insert the value returned by `get_value` at the position specified by `path`.
    pub fn insert<F>(&mut self, path: &[usize], get_value: &mut F) -> Result<(), Error>
    where
        F: FnMut(&[Vec<usize>], &[usize]) -> Result<T, Error>,
    {
        let combinations = if self.nb_required > 1 {
            compute_outcome_combinations(
                self.nb_digits,
                path,
                self.max_error_exp,
                self.min_support_exp,
                self.maximize_coverage,
                self.nb_required,
            )
        } else {
            vec![vec![path.to_vec()]]
        };

        for combination in combinations {
            let combination_iter = CombinationIterator::new(self.nb_tries, self.nb_required);

            for selector in combination_iter {
                self.insert_internal(selector[0], &combination, 0, &selector, get_value)?;
            }
        }

        Ok(())
    }

    fn insert_new(&mut self, is_leaf: bool) {
        let m_trie = if is_leaf {
            let d_trie = DigitTrie::<T>::new(self.base);
            MultiTrieNode::Leaf(d_trie)
        } else {
            let d_trie = DigitTrie::<Vec<TrieNodeInfo>>::new(self.base);
            MultiTrieNode::Node(d_trie)
        };
        self.store.push(m_trie);
    }

    fn insert_internal<F>(
        &mut self,
        cur_node_index: usize,
        paths: &[Vec<usize>],
        path_index: usize,
        trie_indexes: &[usize],
        get_value: &mut F,
    ) -> Result<(), Error>
    where
        F: FnMut(&[Vec<usize>], &[usize]) -> Result<T, Error>,
    {
        assert!(path_index < paths.len());
        let cur_node = self.swap_remove(cur_node_index);
        match cur_node {
            MultiTrieNode::None => unreachable!(),
            MultiTrieNode::Leaf(mut digit_trie) => {
                assert_eq!(path_index, paths.len() - 1);
                let mut get_data = |_| get_value(paths, trie_indexes);
                digit_trie.insert(&paths[path_index], &mut get_data)?;
                self.store[cur_node_index] = MultiTrieNode::Leaf(digit_trie);
            }
            MultiTrieNode::Node(mut node) => {
                assert!(path_index < paths.len() - 1);
                let mut store_index = 0;
                let mut callback =
                    |cur_data_res: Option<Vec<TrieNodeInfo>>| -> Result<Vec<TrieNodeInfo>, Error> {
                        let mut cur_data = match cur_data_res {
                            Some(cur_data) => {
                                if let Some(cur_store_index) =
                                    find_store_index(&cur_data, trie_indexes[path_index + 1])
                                {
                                    store_index = cur_store_index;
                                    return Ok(cur_data);
                                }
                                cur_data
                            }
                            _ => vec![],
                        };
                        self.insert_new(paths.len() - 1 == path_index + 1);
                        store_index = self.store.len() - 1;
                        let trie_index = trie_indexes[path_index + 1];
                        let trie_node_info = TrieNodeInfo {
                            trie_index,
                            store_index,
                        };
                        cur_data.push(trie_node_info);
                        Ok(cur_data)
                    };
                node.insert(&paths[path_index], &mut callback)?;
                self.store[cur_node_index] = MultiTrieNode::Node(node);
                self.insert_internal(store_index, paths, path_index + 1, trie_indexes, get_value)?;
            }
        }
        Ok(())
    }

    /// Lookup in the trie for a value that matches with `paths`.
    pub fn look_up<'a>(
        &'a self,
        paths: &[(usize, Vec<usize>)],
    ) -> Option<LookupResult<'a, T, (usize, Vec<usize>)>> {
        if paths.len() < self.nb_required {
            return None;
        }

        let store = &self.store;

        let combination_iter = CombinationIterator::new(paths.len(), self.nb_required);

        let nb_roots = self.nb_tries - self.nb_required + 1;

        for selector in combination_iter {
            let first_index = paths[selector[0]].0;
            if first_index >= nb_roots {
                continue;
            }

            let res = self.look_up_internal(
                &store[first_index],
                &paths
                    .iter()
                    .enumerate()
                    .filter_map(|(i, x)| {
                        if selector.contains(&i) {
                            return Some(x);
                        }
                        None
                    })
                    .collect::<Vec<_>>(),
                0,
            );
            if let Some(mut l_res) = res {
                l_res.path.reverse();
                return Some(l_res);
            }
        }

        None
    }

    fn look_up_internal<'a>(
        &'a self,
        cur_node: &'a MultiTrieNode<T>,
        paths: &[&(usize, Vec<usize>)],
        path_index: usize,
    ) -> Option<LookupResult<'a, T, (usize, Vec<usize>)>> {
        assert!(path_index < paths.len());
        let trie_index = paths[path_index].0;

        match cur_node {
            MultiTrieNode::None => unreachable!(),
            MultiTrieNode::Leaf(d_trie) => {
                let res = d_trie.look_up(&paths[path_index].1)?;
                Some(LookupResult {
                    value: res[0].value,
                    path: vec![(trie_index, res[0].path.clone())],
                })
            }
            MultiTrieNode::Node(d_trie) => {
                assert!(path_index < paths.len() - 1);
                let results = d_trie.look_up(&paths[path_index].1)?;

                for l_res in results {
                    if let Some(index) = find_store_index(l_res.value, paths[path_index + 1].0) {
                        let next_node = &self.store[index];
                        if let Some(mut child_l_res) =
                            self.look_up_internal(next_node, paths, path_index + 1)
                        {
                            child_l_res.path.push((trie_index, l_res.path));
                            return Some(child_l_res);
                        }
                    }
                }

                None
            }
        }
    }
}

fn find_store_index(children: &[TrieNodeInfo], trie_index: usize) -> Option<usize> {
    for info in children {
        if trie_index == info.trie_index {
            return Some(info.store_index);
        }
    }

    None
}

/// Container for a dump of a MultiTrie used for serialization purpose.
pub struct MultiTrieDump<T>
where
    T: Clone,
{
    /// The node data.
    pub node_data: Vec<MultiTrieNodeData<T>>,
    /// The base for which the trie was built.
    pub base: usize,
    /// The total number of tries.
    pub nb_tries: usize,
    /// The number of trie per path.
    pub nb_required: usize,
    /// The guaranteed support as a power of 2.
    pub min_support_exp: usize,
    /// The maximum support as a power of 2.
    pub max_error_exp: usize,
    /// The maximum number of digits for a single trie path.
    pub nb_digits: usize,
    /// Whether this trie maximizes outcome coverage.
    pub maximize_coverage: bool,
}

impl<T> MultiTrie<T>
where
    T: Clone,
{
    /// Dump the content of the trie for the purpose of serialization.
    pub fn dump(&self) -> MultiTrieDump<T> {
        let node_data = self.store.iter().map(|x| x.get_data()).collect();
        MultiTrieDump {
            node_data,
            base: self.base,
            nb_tries: self.nb_tries,
            nb_required: self.nb_required,
            min_support_exp: self.min_support_exp,
            max_error_exp: self.max_error_exp,
            nb_digits: self.nb_digits,
            maximize_coverage: self.maximize_coverage,
        }
    }

    /// Restore a trie from a dump.
    pub fn from_dump(dump: MultiTrieDump<T>) -> MultiTrie<T> {
        let MultiTrieDump {
            node_data,
            base,
            nb_tries,
            nb_required,
            min_support_exp,
            max_error_exp,
            nb_digits,
            maximize_coverage,
        } = dump;

        let store = node_data
            .into_iter()
            .map(|x| MultiTrieNode::from_data(x))
            .collect();

        MultiTrie {
            store,
            base,
            nb_tries,
            nb_required,
            min_support_exp,
            max_error_exp,
            nb_digits,
            maximize_coverage,
        }
    }
}

/// Holds the data of a multi trie node. Used for serialization purpose.
pub enum MultiTrieNodeData<T>
where
    T: Clone,
{
    /// A leaf in the trie.
    Leaf(DigitTrieDump<T>),
    /// A node in the trie.
    Node(DigitTrieDump<Vec<TrieNodeInfo>>),
}

impl<T> MultiTrieNode<T>
where
    T: Clone,
{
    fn get_data(&self) -> MultiTrieNodeData<T> {
        match self {
            Node::Leaf(l) => MultiTrieNodeData::Leaf(l.dump()),
            Node::Node(n) => MultiTrieNodeData::Node(n.dump()),
            Node::None => unreachable!(),
        }
    }

    fn from_data(data: MultiTrieNodeData<T>) -> MultiTrieNode<T> {
        match data {
            MultiTrieNodeData::Leaf(l) => Node::Leaf(DigitTrie::from_dump(l)),
            MultiTrieNodeData::Node(n) => Node::Node(DigitTrie::from_dump(n)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tests_common(
        mut m_trie: MultiTrie<usize>,
        path: Vec<usize>,
        good_paths: Vec<Vec<(usize, Vec<usize>)>>,
        bad_paths: Vec<Vec<(usize, Vec<usize>)>>,
        expected_iter: Option<Vec<Vec<(usize, Vec<usize>)>>>,
    ) {
        let mut get_value = |_: &[Vec<usize>], _: &[usize]| -> Result<usize, Error> { Ok(2) };

        m_trie.insert(&path, &mut get_value).unwrap();

        for good_path in good_paths {
            assert!(m_trie.look_up(&good_path).is_some());
        }

        for bad_path in bad_paths {
            assert!(m_trie.look_up(&bad_path).is_none());
        }

        if let Some(expected) = expected_iter {
            let iter = MultiTrieIterator::new(&m_trie);

            for (i, res) in iter.enumerate() {
                assert_eq!(expected[i], res.path);
            }
        }
    }

    #[test]
    fn multi_trie_1_of_1_test() {
        let m_trie = MultiTrie::<usize>::new(1, 1, 2, 2, 3, 5, true);

        let path = vec![0, 1, 1, 1];

        let good_paths = vec![
            vec![(0, vec![0, 1, 1, 1, 1])],
            vec![(0, vec![0, 1, 1, 1, 0])],
        ];

        let bad_paths = vec![
            vec![(0, vec![1, 1, 1, 1, 1])],
            vec![(0, vec![0, 1, 1, 0, 1])],
            vec![(0, vec![0, 1, 0, 1, 0])],
        ];

        let expected_iter: Vec<Vec<(usize, Vec<usize>)>> = vec![vec![(0, vec![0, 1, 1, 1])]];

        tests_common(m_trie, path, good_paths, bad_paths, Some(expected_iter));
    }

    #[test]
    fn multi_trie_1_of_2_test() {
        let m_trie = MultiTrie::<usize>::new(2, 1, 2, 2, 3, 5, true);

        let path = vec![0, 1, 1, 1];

        let good_paths = vec![
            vec![(0, vec![0, 1, 1, 1, 1])],
            vec![(1, vec![0, 1, 1, 1, 1])],
            vec![(0, vec![0, 1, 1, 1, 0])],
            vec![(1, vec![0, 1, 1, 1, 0])],
        ];

        let bad_paths = vec![
            vec![(0, vec![1, 1, 1, 1, 1])],
            vec![(1, vec![0, 1, 1, 0, 1])],
            vec![(0, vec![0, 1, 0, 1, 0])],
        ];

        let expected_iter: Vec<Vec<(usize, Vec<usize>)>> =
            vec![vec![(0, vec![0, 1, 1, 1])], vec![(1, vec![0, 1, 1, 1])]];

        tests_common(m_trie, path, good_paths, bad_paths, Some(expected_iter));
    }

    #[test]
    fn multi_trie_2_of_2_test() {
        let m_trie = MultiTrie::<usize>::new(2, 2, 2, 2, 3, 5, true);

        let path = vec![0, 1, 1, 1];

        let good_paths = vec![
            vec![(0, vec![0, 1, 1, 1, 1]), (1, vec![0, 1, 1, 1, 1])],
            vec![(0, vec![0, 1, 1, 1, 1]), (1, vec![1, 0, 0, 1, 1])],
            vec![(0, vec![0, 1, 1, 1, 1]), (1, vec![0, 1, 1, 0, 0])],
        ];

        let bad_paths = vec![
            vec![(0, vec![1, 1, 1, 1, 1]), (1, vec![0, 1, 1, 1, 1])],
            vec![(0, vec![0, 1, 1, 1, 1]), (1, vec![1, 1, 0, 1, 1])],
            vec![(0, vec![0, 1, 0, 1, 1]), (1, vec![0, 1, 1, 0, 0])],
        ];

        let expected_iter: Vec<Vec<(usize, Vec<usize>)>> = vec![
            vec![(0, vec![0, 1, 1, 1]), (1, vec![0, 1])],
            vec![(0, vec![0, 1, 1, 1]), (1, vec![1, 0, 0])],
        ];

        tests_common(m_trie, path, good_paths, bad_paths, Some(expected_iter));
    }

    #[test]
    fn multi_trie_2_of_3_test() {
        let m_trie = MultiTrie::<usize>::new(3, 2, 2, 2, 3, 5, true);

        let path = vec![0, 1, 1, 1];

        let good_paths = vec![
            vec![(0, vec![0, 1, 1, 1, 1]), (1, vec![0, 1, 1, 1, 1])],
            vec![(1, vec![0, 1, 1, 1, 1]), (2, vec![0, 1, 1, 1, 1])],
            vec![(0, vec![0, 1, 1, 1, 1]), (2, vec![0, 1, 1, 1, 1])],
            vec![(0, vec![0, 1, 1, 1, 1]), (2, vec![1, 0, 0, 1, 1])],
            vec![(1, vec![0, 1, 1, 1, 1]), (2, vec![1, 0, 0, 1, 1])],
        ];

        let bad_paths = vec![
            vec![(0, vec![1, 1, 1, 1, 1]), (1, vec![0, 1, 1, 1, 1])],
            vec![(2, vec![0, 1, 1, 1, 1]), (1, vec![0, 1, 1, 1, 1])],
            vec![(0, vec![0, 1, 1, 1, 1]), (2, vec![1, 1, 1, 1, 1])],
            vec![(1, vec![0, 1, 1, 1, 1]), (2, vec![1, 1, 1, 1, 1])],
        ];

        tests_common(m_trie, path, good_paths, bad_paths, None);
    }

    #[test]
    fn multi_trie_5_of_5_test() {
        let m_trie = MultiTrie::<usize>::new(5, 5, 2, 1, 2, 3, true);

        let path = vec![0, 0, 0];

        let good_paths = vec![vec![
            (0, vec![0, 0, 0]),
            (1, vec![0]),
            (2, vec![0]),
            (3, vec![0]),
            (4, vec![0]),
        ]];

        tests_common(m_trie, path, good_paths.clone(), vec![], Some(good_paths));
    }

    #[test]
    fn multi_3_of_3_test_lexicographic_order() {
        let mut m_trie = MultiTrie::<usize>::new(3, 3, 2, 1, 2, 3, true);

        let inputs = vec![
            vec![0, 0],
            vec![0, 0, 1],
            vec![0, 1, 0],
            vec![0, 1, 1],
            vec![1, 0, 0],
            vec![1, 0, 1],
        ];

        let mut counter = 0;

        let mut get_value = |_: &[std::vec::Vec<usize>], _: &[usize]| -> Result<usize, Error> {
            counter += 1;
            Ok(counter - 1)
        };

        for input in inputs {
            m_trie
                .insert(&input, &mut get_value)
                .expect("Error inserting in trie");
        }

        let iter = MultiTrieIterator::new(&m_trie);

        for (i, res) in iter.enumerate() {
            assert_eq!(i, *res.value);
        }
    }

    fn multi_enumerate_equal_lookup_common(mut m_trie: MultiTrie<usize>) {
        let inputs = vec![
            // vec![0, 0],
            vec![0, 1, 0],
            // vec![0, 1, 1],
            // vec![1, 0, 0],
            // vec![1, 0, 1],
        ];

        let mut counter = 0;

        let mut get_value = |_: &[Vec<usize>], _: &[usize]| -> Result<usize, Error> {
            counter += 1;
            Ok(counter - 1)
        };

        for input in inputs {
            m_trie
                .insert(&input, &mut get_value)
                .expect("Error inserting in trie");
        }

        let iter = MultiTrieIterator::new(&m_trie);

        for res in iter {
            assert_eq!(
                m_trie.look_up(&res.path).expect("Path not found").value,
                res.value
            );
        }
    }

    #[test]
    fn multi_3_of_5_test_enumerate_equal_lookup() {
        let m_trie = MultiTrie::<usize>::new(5, 3, 2, 1, 2, 3, true);
        multi_enumerate_equal_lookup_common(m_trie);
    }

    #[test]
    fn multi_5_of_5_test_enumerate_equal_lookup() {
        let m_trie = MultiTrie::<usize>::new(5, 5, 2, 1, 2, 3, true);
        multi_enumerate_equal_lookup_common(m_trie);
    }
}
