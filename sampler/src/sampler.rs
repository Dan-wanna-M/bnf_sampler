use crate::simplified_grammar::SimplifiedExpressions;
use crate::simplified_grammar::SimplifiedGrammar;
use crate::stack::BufferArena;
use crate::stack::FixedBuffer;
use crate::trie::TerminalsTrie;
use crate::trie::TrieNodeID;
use crate::utils;
use crate::utils::NonterminalID;
use crate::utils::SliceU8Wrapper;
use crate::utils::VecU8Wrapper;
use bit_set::BitSet;
use bnf::Term;
use qp_trie::Trie;
use rustc_hash::FxHashMap;
use std::ptr::NonNull;
use std::time::Instant;
use std::vec;

const INVALID_INDEX: i32 = -1;

#[derive(PartialEq, Clone, Debug, Copy, Eq, Hash)]
pub enum StackItem<'a> {
    Nonterminal(NonterminalID),
    Terminal(&'a [u8]),
    Terminals(TrieNodeID),
}
#[derive(Clone, Debug)]
pub struct Sampler<'a> {
    pub stacks: Vec<Vec<StackItem<'a>>>,
    grammar: &'a SimplifiedGrammar,
    tokens_buffer: Vec<(VecU8Wrapper, u32)>,
    tokens_tree: Trie<VecU8Wrapper, u32>,
    stack_arena: BufferArena<StackItem<'a>>,
    stacks_to_token_ids: FxHashMap<Vec<Vec<StackItem<'a>>>, BitSet<u32>>,
    token_ids: BitSet<u32>,
}
#[derive(Debug)]
enum BytesMatchResults<'a> {
    Failed(usize),
    Matches(Vec<BytesMatchResult<'a>>),
}
#[derive(Debug)]
struct BytesMatchResult<'a> {
    remaining_bytes_start: i32,
    stack_offset: u32,
    modified_item_at_offset: Option<StackItem<'a>>,
}

enum TokensIterType<'a> {
    Flat(std::slice::Iter<'a, (VecU8Wrapper, u32)>),
    SinglePrefix(qp_trie::Iter<'a, VecU8Wrapper, u32>),
    MultiplePrefixs(
        (
            std::collections::hash_map::Keys<'a, u8, TrieNodeID>,
            Option<qp_trie::Iter<'a, VecU8Wrapper, u32>>,
        ),
    ),
}

struct BufferOrTreeIter<'a> {
    tokens_buffer_iter: TokensIterType<'a>,
    tokens_tree: &'a Trie<VecU8Wrapper, u32>,
    current_prefixs: VecU8Wrapper,
}

impl<'a, 'b> BufferOrTreeIter<'a> {
    pub fn new(
        tokens_buffer: &'a [(VecU8Wrapper, u32)],
        tokens_tree: &'a Trie<VecU8Wrapper, u32>,
        trie: &'a TerminalsTrie,
        current_top: StackItem<'b>,
    ) -> Self {
        let tokens_buffer_iter = match current_top {
            StackItem::Terminal(terminal) => TokensIterType::SinglePrefix(
                tokens_tree.iter_prefix(tokens_tree.longest_common_prefix(terminal)),
            ),
            StackItem::Terminals(node_id) => {
                let node = trie.get(node_id);
                if node.children.len() > (u8::MAX / 2).into() {
                    TokensIterType::Flat(tokens_buffer.iter())
                } else {
                    TokensIterType::MultiplePrefixs((node.children.keys(), None))
                }
            }
            StackItem::Nonterminal(_) => panic!("No nonterminals should be here."),
        };
        BufferOrTreeIter {
            tokens_buffer_iter,
            tokens_tree,
            current_prefixs: VecU8Wrapper(Vec::with_capacity(8)),
        }
    }
}

impl<'a> Iterator for BufferOrTreeIter<'a> {
    type Item = (&'a VecU8Wrapper, &'a u32);

    fn next(&mut self) -> Option<Self::Item> {
        let result;
        match &mut self.tokens_buffer_iter {
            TokensIterType::Flat(buffer_iter) => {
                result = buffer_iter.next().map(|(k, v)| (k, v));
            }
            TokensIterType::SinglePrefix(trie_iter) => {
                result = trie_iter.next();
            }
            TokensIterType::MultiplePrefixs((keys, trie_iter)) => match trie_iter {
                None => {
                    let mut trie_iter = None;
                    result = keys.next().and_then(|key| {
                        self.current_prefixs.0.push(*key);
                        let mut iter = self.tokens_tree.iter_prefix(&self.current_prefixs);
                        self.current_prefixs.0.clear();
                        let temp = iter.next();
                        trie_iter = Some(iter);
                        temp
                    });
                }
                Some(trie_iter) => {
                    result = trie_iter.next().or_else(|| {
                        keys.next().and_then(|key| {
                            self.current_prefixs.0.push(*key);
                            *trie_iter = self.tokens_tree.iter_prefix(&self.current_prefixs);
                            self.current_prefixs.0.clear();
                            trie_iter.next()
                        })
                    });
                }
            },
        };
        result
    }
}

impl<'a> Sampler<'a> {
    /// Create a new Sampler with simplified grammar
    pub fn new(
        grammar: &'a SimplifiedGrammar,
        start_term: &str,
        tokens_tree: Trie<VecU8Wrapper, u32>,
        stack_arena_capacity: usize,
    ) -> Sampler<'a> {
        let stacks = vec![vec![StackItem::Nonterminal(
            grammar.nonterminal_to_terminal_id[start_term],
        )]];
        let token_ids: BitSet<u32> = BitSet::with_capacity(u16::MAX.into());
        let stacks_to_token_ids = FxHashMap::default();
        let tokens_buffer = Vec::from_iter(tokens_tree.iter().map(|(k, v)| (k.clone(), *v)));
        Sampler {
            stacks,
            grammar,
            tokens_tree,
            tokens_buffer,
            stacks_to_token_ids,
            token_ids,
            stack_arena: BufferArena::with_capacity(stack_arena_capacity),
        }
    }

    pub fn all_possible_next_tokens(
        &mut self,
        previous_tokens: Option<&[u8]>,
    ) -> Option<&BitSet<u32>> {
        let now = Instant::now();
        if !self.accept_tokens(previous_tokens) {
            return None;
        }
        // println!("accepting tokens: {:?}", now.elapsed());
        self.token_ids.clear();
        for stack in self.stacks.iter() {
            if let StackItem::Terminals(node_id) =
                stack.last().expect("The stack should not be empty.")
            {
                if let Some((k, _)) = self
                    .grammar
                    .terminals_trie
                    .roots
                    .iter()
                    .find(|(_, v)| **v == *node_id)
                {
                    if self
                        .grammar
                        .nonterminal_to_terminal_id
                        .get(utils::ANY_NONTERMINAL_NAME)
                        .is_some_and(|x| *k == *x)
                    {
                        self.token_ids
                            .extend(self.tokens_tree.iter().map(|(_, v)| (*v) as usize));
                        break;
                    }
                }
            }
        }
        Some(
            self.stacks_to_token_ids
                .entry(self.stacks.clone())
                .or_insert_with(|| {
                    for stack in self.stacks.iter() {
                        // let now = Instant::now();
                        let mut failed_prefixs: Trie<SliceU8Wrapper, ()> = Trie::new();
                        let iter = BufferOrTreeIter::new(
                            &self.tokens_buffer,
                            &self.tokens_tree,
                            &self.grammar.terminals_trie,
                            *stack.last().unwrap(),
                        );
                        for (token, token_id) in iter {
                            if self.token_ids.contains(*token_id as usize)
                                || failed_prefixs.contains_key(
                                    failed_prefixs.longest_common_prefix(token.0.as_slice()),
                                )
                            {
                                continue;
                            }
                            // println!("{:?}", String::from_utf8(token.0.clone()).unwrap());
                            let arena = unsafe {
                                NonNull::new_unchecked(
                                    &mut self.stack_arena as *mut BufferArena<StackItem<'a>>,
                                )
                            };
                            let mut temp_stack = self.stack_arena.allocate_a_stack(stack.len());
                            temp_stack.copy_from_slice(stack.as_slice());
                            let result = Self::find_stacks_matching_bytes(
                                arena,
                                &mut temp_stack,
                                self.grammar,
                                Some(token.0.as_slice()),
                                false,
                                &mut |_, _| {},
                                &mut |bytes, index| {
                                    failed_prefixs.insert(SliceU8Wrapper(&bytes[..index + 1]), ());
                                },
                            );
                            if result {
                                self.token_ids.insert(*token_id as usize);
                            }
                            self.stack_arena.clear();
                        }
                        // println!("stack: {:?}, {:?}", stack, now.elapsed());
                    }
                    self.token_ids.clone()
                }),
        )
    }
    #[must_use]
    fn accept_tokens(&mut self, bytes: Option<&[u8]>) -> bool {
        let mut find_stacks_matching_bytes = |bytes| {
            let len = self.stacks.len();
            let mut accepted = false;
            for i in 0..len {
                let arena = unsafe {
                    NonNull::new_unchecked(&mut self.stack_arena as *mut BufferArena<StackItem<'a>>)
                };
                let mut stack = self.stack_arena.allocate_a_stack(self.stacks[i].len());
                stack.copy_from_slice(&self.stacks[i]);
                match stack.last() {
                    Some(_) => {
                        accepted |= Self::find_stacks_matching_bytes(
                            arena,
                            &mut stack,
                            self.grammar,
                            bytes,
                            true,
                            &mut |temp_stack: &[Option<StackItem<'_>>], top: Option<StackItem>| {
                                let mut new_vec = Vec::with_capacity(temp_stack.len() + 1);
                                new_vec.extend(temp_stack.iter().map(|x| x.unwrap()));
                                if let Some(top) = top {
                                    new_vec.push(top);
                                }
                                if !self.stacks[i + 1..].iter().any(|x| *x == new_vec) {
                                    self.stacks.push(new_vec);
                                }
                            },
                            &mut |_, _| {},
                        );
                    }
                    None => {
                        continue;
                    }
                };
                self.stack_arena.clear();
            }
            for i in (0..len).rev() {
                self.stacks.swap_remove(i);
            }
            accepted
        };
        let mut result = true;
        if bytes.is_some() {
            result = find_stacks_matching_bytes(bytes);
            if !result {
                return false;
            }
        }
        result | find_stacks_matching_bytes(None)
    }

    fn match_stack_to_bytes<'b>(
        stack: &FixedBuffer<StackItem<'a>>,
        bytes: Option<&'b [u8]>,
        trie: &TerminalsTrie,
    ) -> BytesMatchResults<'a> {
        fn _match_stack_to_bytes<'a>(
            stack: &FixedBuffer<StackItem<'a>>,
            bytes: &[u8],
            bytes_index: usize,
            trie: &TerminalsTrie,
            stack_offset: usize,
            shortest_failed_index: &mut usize,
            result: &mut Vec<BytesMatchResult<'a>>,
        ) {
            if bytes.is_empty() {
                return;
            }
            match stack[stack_offset] {
                StackItem::Nonterminal(_) => {
                    result.push(BytesMatchResult {
                        remaining_bytes_start: bytes_index as i32,
                        stack_offset: stack_offset as u32,
                        modified_item_at_offset: None,
                    });
                }
                StackItem::Terminal(terminal) => {
                    for i in 0..terminal.len() {
                        if bytes.len() == i + bytes_index {
                            result.push(BytesMatchResult {
                                remaining_bytes_start: INVALID_INDEX,
                                stack_offset: stack_offset as u32,
                                modified_item_at_offset: Some(StackItem::Terminal(&terminal[i..])),
                            });
                            return;
                        }
                        if bytes[i + bytes_index] != terminal[i] {
                            *shortest_failed_index =
                                std::cmp::min(i + bytes_index, *shortest_failed_index);
                            return;
                        }
                    }
                    if bytes.len() == terminal.len() {
                        result.push(BytesMatchResult {
                            remaining_bytes_start: INVALID_INDEX,
                            stack_offset: stack_offset as u32,
                            modified_item_at_offset: None,
                        });
                        return;
                    }
                    if stack_offset > 0 {
                        _match_stack_to_bytes(
                            stack,
                            bytes,
                            terminal.len() + bytes_index,
                            trie,
                            stack_offset - 1,
                            shortest_failed_index,
                            result,
                        )
                    }
                }
                StackItem::Terminals(mut current_node_id) => {
                    let mut current_node = trie.get(current_node_id);
                    for i in bytes_index..bytes.len() {
                        match current_node.children.get(&bytes[i]) {
                            Some(new_node_id) => {
                                let new_node = trie.get(*new_node_id);
                                if new_node.value.is_some()
                                    && stack_offset > 0
                                    && i < bytes.len() - 1
                                {
                                    _match_stack_to_bytes(
                                        stack,
                                        bytes,
                                        i + 1,
                                        trie,
                                        stack_offset - 1,
                                        shortest_failed_index,
                                        result,
                                    );
                                }
                                current_node_id = *new_node_id;
                                current_node = new_node;
                            }
                            None => {
                                *shortest_failed_index = std::cmp::min(i, *shortest_failed_index);
                                return;
                            }
                        }
                    }

                    if !current_node.children.is_empty() {
                        result.push(BytesMatchResult {
                            remaining_bytes_start: INVALID_INDEX,
                            stack_offset: stack_offset as u32,
                            modified_item_at_offset: Some(StackItem::Terminals(current_node_id)),
                        });
                    }
                    if current_node.value.is_some() {
                        result.push(BytesMatchResult {
                            remaining_bytes_start: INVALID_INDEX,
                            stack_offset: stack_offset as u32,
                            modified_item_at_offset: None,
                        });
                    }
                }
            }
        }
        let mut result: Vec<BytesMatchResult> = vec![];
        match bytes {
            None => return BytesMatchResults::Matches(result),
            Some(bytes) => {
                result.reserve(bytes.len());
                let stack_offset = stack.len() - 1;
                let mut shortest_failed_index = usize::MAX;
                _match_stack_to_bytes(
                    stack,
                    bytes,
                    0,
                    trie,
                    stack_offset,
                    &mut shortest_failed_index,
                    &mut result,
                );
                if result.is_empty() {
                    return BytesMatchResults::Failed(shortest_failed_index);
                } else {
                    return BytesMatchResults::Matches(result);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn find_stacks_matching_bytes<'b, F1, F2>(
        arena: NonNull<BufferArena<StackItem<'a>>>,
        stack: &mut FixedBuffer<StackItem<'a>>,
        grammar: &'a SimplifiedGrammar,
        bytes: Option<&'b [u8]>,
        find_all: bool,
        after_finding_stack: &mut F1,
        after_match_failed: &mut F2,
    ) -> bool
    where
        F1: FnMut(&[Option<StackItem<'a>>], Option<StackItem<'a>>),
        F2: FnMut(&'b [u8], usize),
    {
        let trie = &grammar.terminals_trie;
        let mut _find_stacks_matching_bytes =
            |mut arena: NonNull<BufferArena<StackItem<'a>>>,
             top: NonterminalID,
             stack: &[Option<StackItem<'a>>],
             bytes: Option<&'b [u8]>,
             after_finding_stack: &mut F1| {
                let mut found = false;
                match &grammar.nonterminal_id_to_expression[&top] {
                    SimplifiedExpressions::Expressions(expressions) => {
                        for expression in expressions.iter() {
                            let temp_stack = &mut unsafe { arena.as_mut() }
                                .allocate_a_stack(stack.len() + expression.len());
                            temp_stack.copy_from_raw_slice(stack);
                            for term in expression.iter().rev() {
                                temp_stack.push(match term {
                                    Term::Terminal(value) => StackItem::Terminal(value.as_bytes()),
                                    Term::Nonterminal(value) => StackItem::Nonterminal(
                                        grammar.nonterminal_to_terminal_id[value],
                                    ),
                                });
                            }
                            let temp = Self::find_stacks_matching_bytes(
                                arena,
                                temp_stack,
                                grammar,
                                bytes,
                                find_all,
                                after_finding_stack,
                                after_match_failed,
                            );
                            found |= temp;
                            if !find_all && found {
                                return found;
                            }
                        }
                    }
                    SimplifiedExpressions::Terminals(node_id) => {
                        let temp_stack =
                            &mut unsafe { arena.as_mut() }.allocate_a_stack(stack.len() + 1);
                        temp_stack.copy_from_raw_slice(stack);
                        temp_stack.push(StackItem::Terminals(*node_id));
                        let temp = Self::find_stacks_matching_bytes(
                            arena,
                            temp_stack,
                            grammar,
                            bytes,
                            find_all,
                            after_finding_stack,
                            after_match_failed,
                        );
                        found |= temp;
                        if !find_all && found {
                            return found;
                        }
                    }
                }
                found
            };
        match stack.pop() {
            Some(value) => match value {
                StackItem::Nonterminal(top) => {
                    return _find_stacks_matching_bytes(
                        arena,
                        top,
                        stack.as_raw_slice(),
                        bytes,
                        after_finding_stack,
                    )
                }
                StackItem::Terminal(_) | StackItem::Terminals(_) => {
                    stack.push(value);
                    match Self::match_stack_to_bytes(stack, bytes, trie) {
                        BytesMatchResults::Failed(index) => {
                            after_match_failed(bytes.unwrap(), index);
                            false
                        }
                        BytesMatchResults::Matches(possible_results) => {
                            if possible_results.is_empty() {
                                after_finding_stack(stack.as_raw_slice(), None);
                                return true;
                            }
                            let mut flag = false;
                            for result in possible_results {
                                match result.remaining_bytes_start {
                                    INVALID_INDEX => {
                                        after_finding_stack(
                                            &stack[..result.stack_offset as usize],
                                            result.modified_item_at_offset,
                                        );
                                        flag |= true;
                                        if !find_all {
                                            return true;
                                        }
                                    }
                                    value => {
                                        let top = match result
                                            .modified_item_at_offset
                                            .unwrap_or(stack[result.stack_offset as usize])
                                        {
                                            StackItem::Nonterminal(id) => id,
                                            _ => panic!(
                                                "{:?} should only be nonterminal.",
                                                &stack[..(result.stack_offset + 1) as usize]
                                            ),
                                        };
                                        flag |= _find_stacks_matching_bytes(
                                            arena,
                                            top,
                                            &stack[..result.stack_offset as usize],
                                            Some(&(bytes.unwrap()[value as usize..])),
                                            after_finding_stack,
                                        );
                                        if !find_all {
                                            return true;
                                        }
                                    }
                                }
                            }
                            flag
                        }
                    }
                }
            },
            None => false,
        }
    }
}
