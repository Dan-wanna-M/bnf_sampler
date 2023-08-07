use rustc_hash::FxHashMap;

use crate::utils::NonterminalID;
#[derive(Clone, Debug)]
pub(crate) struct TerminalsTrie {
    pub roots: FxHashMap<NonterminalID, TrieNodeID>,
    arena: Vec<TrieNode>,
}
#[derive(Clone, Debug)]
pub(crate) struct TerminalsTrieIter<'a> {
    initial_index: usize,
    pub stack: Vec<std::collections::hash_map::Iter<'a, u8, TrieNodeID>>,
    trie: &'a TerminalsTrie,
}

impl<'a> Iterator for TerminalsTrieIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.stack.last_mut() {
                None => {
                    return None;
                }
                Some(x) => match x.next() {
                    None => {
                        self.stack.pop();
                    }
                    Some((_, v)) => {
                        self.stack.push(self.trie.get(*v).children.iter());
                        if let Some(value) = &self.trie.get(*v).value {
                            return Some(&value[self.initial_index..]);
                        }
                    }
                },
            }
        }
    }
}

impl TerminalsTrie {
    pub fn new() -> Self {
        let arena = Vec::new();
        TerminalsTrie {
            roots: FxHashMap::default(),
            arena,
        }
    }

    fn new_node(arena: &mut Vec<TrieNode>, node: TrieNode) -> TrieNodeID {
        arena.push(node);
        TrieNodeID {
            id: arena.len() - 1,
        }
    }

    pub fn get(&self, node_id: TrieNodeID) -> &TrieNode {
        &self.arena[node_id.id]
    }

    fn get_mut(&mut self, node_id: TrieNodeID) -> &mut TrieNode {
        &mut self.arena[node_id.id]
    }

    pub fn add(&mut self, terminal: &[u8], nonterminal_id: NonterminalID) {
        let mut current_node_id = *self
            .roots
            .entry(nonterminal_id)
            .or_insert(Self::new_node(
                &mut self.arena,
                TrieNode {
                    index: 0,
                    value: None,
                    children: FxHashMap::default(),
                },
            ));
        for i in terminal {
            let matched_child_node = self.get(current_node_id).children.get(i);
            match matched_child_node {
                None => {
                    let index = self.get(current_node_id).index + 1;
                    let new_node_id = Self::new_node(
                        &mut self.arena,
                        TrieNode {
                            index,
                            value: None,
                            children: FxHashMap::default(),
                        },
                    );
                    self.get_mut(current_node_id).append(*i, new_node_id);
                    current_node_id = new_node_id;
                }
                Some(id) => {
                    current_node_id = *id;
                }
            }
        }
        let mut temp = Vec::with_capacity(terminal.len());
        temp.extend_from_slice(terminal);
        self.get_mut(current_node_id).value = Some(temp);
    }

    pub fn iter(&self, start_node_id: TrieNodeID) -> TerminalsTrieIter {
        let stack = vec![self.get(start_node_id).children.iter()];
        return TerminalsTrieIter {
            trie: self,
            initial_index: self.get(start_node_id).index,
            stack,
        };
    }
}
#[derive(PartialEq, Clone, Debug, Copy, Eq, Hash)]
pub(crate) struct TrieNodeID {
    pub id: usize,
}
#[derive(Clone, Debug)]
pub(crate) struct TrieNode {
    pub index: usize,
    pub value: Option<Vec<u8>>,
    pub children: FxHashMap<u8, TrieNodeID>,
}

impl TrieNode {
    pub fn append(&mut self, byte: u8, node_id: TrieNodeID) {
        self.children.insert(byte, node_id);
    }
}
