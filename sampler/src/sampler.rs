use crate::simplified_grammar::SimplifiedExpressions;
use crate::simplified_grammar::SimplifiedGrammar;
use crate::stack::Stack;
use crate::stack::StackArena;
use crate::trie::TerminalsTrie;
use crate::trie::TerminalsTrieIter;
use crate::trie::TrieNodeID;
use crate::utils::NonterminalID;
use crate::utils::SliceU8Wrapper;
use crate::utils::VecU8Wrapper;
use bnf::Term;
use itertools::Itertools;
use qp_trie::Trie;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::ptr::NonNull;
use std::vec;

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
    tokens_tree: Trie<VecU8Wrapper, u32>,
    stack_arena: StackArena<StackItem<'a>>,
    stacks_to_token_ids: FxHashMap<Vec<Vec<StackItem<'a>>>, FxHashSet<u32>>,
    token_ids: FxHashSet<u32>,
    current_token: VecU8Wrapper,
    current_prefix: VecU8Wrapper,
}
#[derive(Debug)]
enum BytesMatchResults<'a, 'b> {
    Failed(usize),
    Matches(Vec<BytesMatchResult<'a, 'b>>),
}
#[derive(Debug)]
struct BytesMatchResult<'a, 'b> {
    remaining_bytes: Option<&'b [u8]>,
    stack_offset: usize,
    modified_item_at_offset: Option<StackItem<'a>>,
}

#[derive(Debug)]
enum ByteOrTerminals {
    Byte(u8),
    Terminals(TrieNodeID),
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
        let token_ids: FxHashSet<u32> = FxHashSet::default();
        let current_token = VecU8Wrapper(Vec::with_capacity(128));
        let current_prefix = VecU8Wrapper(Vec::with_capacity(128));
        let stacks_to_token_ids = FxHashMap::default();
        Sampler {
            stacks,
            grammar,
            tokens_tree,
            stacks_to_token_ids,
            token_ids,
            current_token,
            current_prefix,
            stack_arena: StackArena::with_capacity(stack_arena_capacity),
        }
    }

    pub fn all_possible_next_tokens(
        &mut self,
        previous_tokens: Option<&[u8]>,
    ) -> Option<&FxHashSet<u32>> {
        if !self.accept_tokens(previous_tokens) {
            return None;
        }
        self.token_ids.clear();
        Some(
            self.stacks_to_token_ids
                .entry(self.stacks.clone())
                .or_insert_with(|| {
                    for stack in self.stacks.iter() {
                        let mut current_prefix_buffer: Vec<ByteOrTerminals> =
                            Vec::with_capacity(20);
                        let mut iters: Vec<TerminalsTrieIter> = Vec::with_capacity(20);
                        for i in stack.iter().rev() {
                            match i {
                                StackItem::Terminal(value) => current_prefix_buffer
                                    .extend(value.iter().map(|x| ByteOrTerminals::Byte(*x))),
                                StackItem::Terminals(value) => {
                                    current_prefix_buffer.push(ByteOrTerminals::Terminals(*value));
                                    iters.push(self.grammar.terminals_trie.iter(*value).clone())
                                }
                                _ => break,
                            }
                        }
                        let mut products = iters.into_iter().multi_cartesian_product();
                        let mut flag = false;
                        loop {
                            let one_product = match products.next() {
                                Some(x) => {
                                    flag = true;
                                    x
                                }
                                None => {
                                    if flag {
                                        break;
                                    }
                                    flag = true;
                                    vec![]
                                }
                            };
                            let mut counter = 0;
                            for i in current_prefix_buffer.iter() {
                                match i {
                                    ByteOrTerminals::Byte(x) => {
                                        self.current_prefix.0.push(*x);
                                    }
                                    ByteOrTerminals::Terminals(_) => {
                                        self.current_prefix
                                            .0
                                            .extend_from_slice(one_product[counter]);
                                        counter += 1;
                                    }
                                }
                            }
                            let mut failed_prefixs: Trie<SliceU8Wrapper, ()> = Trie::new();
                            for (VecU8Wrapper(token), &token_id) in self.tokens_tree.iter_prefix(
                                self.tokens_tree.longest_common_prefix(&self.current_prefix),
                            ) {
                                self.current_token.0.extend(token);
                                if self.token_ids.contains(&token_id)
                                    || failed_prefixs.contains_key(
                                        failed_prefixs
                                            .longest_common_prefix(self.current_token.0.as_slice()),
                                    )
                                {
                                    continue;
                                }
                                let arena = unsafe {
                                    NonNull::new_unchecked(
                                        &mut self.stack_arena as *mut StackArena<StackItem<'a>>,
                                    )
                                };
                                let mut temp_stack = self.stack_arena.allocate_a_stack(stack.len());
                                temp_stack.copy_from_slice(stack);
                                let result = Self::find_stacks_matching_bytes(
                                    arena,
                                    &mut temp_stack,
                                    self.grammar,
                                    Some(token),
                                    false,
                                    &self.grammar.terminals_trie,
                                    &mut |_, _| {},
                                    &mut |bytes, index| {
                                        failed_prefixs
                                            .insert(SliceU8Wrapper(&bytes[..index + 1]), ());
                                    },
                                );
                                if result {
                                    self.token_ids.insert(token_id);
                                }
                                self.stack_arena.clear();
                                self.current_token.0.clear();
                            }
                            self.current_prefix.0.clear();
                        }
                        //println!("Time used for one stack: {:?}", now.elapsed());
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
                    NonNull::new_unchecked(&mut self.stack_arena as *mut StackArena<StackItem<'a>>)
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
                            &self.grammar.terminals_trie,
                            &mut |temp_stack: &[Option<StackItem<'_>>], top: Option<StackItem>| {
                                let mut new_vec = Vec::with_capacity(temp_stack.len() + 1);
                                new_vec.extend(temp_stack.iter().map(|x| x.unwrap()));
                                if let Some(top) = top {
                                    new_vec.push(top);
                                }
                                self.stacks.push(new_vec);
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
            self.stacks = Vec::from_iter(self.stacks.iter().unique().cloned());
            accepted
        };
        let result = find_stacks_matching_bytes(bytes);
        if bytes.is_some() && !result {
            return false;
        }
        result | find_stacks_matching_bytes(None)
    }

    fn match_stack_to_bytes<'b>(
        stack: &Stack<StackItem<'a>>,
        bytes: Option<&'b [u8]>,
        trie: &TerminalsTrie,
    ) -> BytesMatchResults<'a, 'b> {
        fn _match_stack_to_bytes<'a, 'b>(
            stack: &Stack<StackItem<'a>>,
            bytes: &'b [u8],
            trie: &TerminalsTrie,
            stack_offset: usize,
            shortest_failed_index: &mut usize,
            result: &mut Vec<BytesMatchResult<'a, 'b>>,
        ) {
            if bytes.is_empty() {
                return;
            }
            match stack[stack_offset] {
                StackItem::Nonterminal(_) => {
                    result.push(BytesMatchResult {
                        remaining_bytes: Some(bytes),
                        stack_offset,
                        modified_item_at_offset: None,
                    });
                }
                StackItem::Terminal(terminal) => {
                    for i in 0..terminal.len() {
                        if bytes.len() == i {
                            result.push(BytesMatchResult {
                                remaining_bytes: None,
                                stack_offset,
                                modified_item_at_offset: Some(StackItem::Terminal(&terminal[i..])),
                            })
                        }
                        if bytes[i] != terminal[i] {
                            *shortest_failed_index = std::cmp::min(i, *shortest_failed_index);
                            return;
                        }
                    }
                    if stack_offset > 0 {
                        _match_stack_to_bytes(
                            stack,
                            &bytes[terminal.len()..],
                            trie,
                            stack_offset - 1,
                            shortest_failed_index,
                            result,
                        )
                    }
                }
                StackItem::Terminals(mut current_node_ID) => {
                    let mut current_node = trie.get(current_node_ID);
                    for i in 0..bytes.len() {
                        match current_node.children.get(&bytes[i]) {
                            Some(new_node_id) => {
                                let new_node = trie.get(*new_node_id);
                                if new_node.value.is_some() && stack_offset > 0 {
                                    _match_stack_to_bytes(
                                        stack,
                                        &bytes[i + 1..],
                                        trie,
                                        stack_offset - 1,
                                        shortest_failed_index,
                                        result,
                                    );
                                }
                                current_node_ID = *new_node_id;
                                current_node = new_node;
                            }
                            None => {
                                *shortest_failed_index = std::cmp::min(i, *shortest_failed_index);
                                return;
                            }
                        }
                    }
                    let mut modified_item_at_offset = None;
                    if !current_node.children.is_empty() {
                        modified_item_at_offset = Some(StackItem::Terminals(current_node_ID));
                    }
                    result.push(BytesMatchResult {
                        remaining_bytes: None,
                        stack_offset,
                        modified_item_at_offset,
                    });
                    if current_node.value.is_some()
                    {
                        result.push(BytesMatchResult {
                            remaining_bytes: None,
                            stack_offset,
                            modified_item_at_offset:None,
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
                let mut shortest_failed_index = 0;
                _match_stack_to_bytes(
                    stack,
                    bytes,
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
        arena: NonNull<StackArena<StackItem<'a>>>,
        stack: &mut Stack<StackItem<'a>>,
        grammar: &'a SimplifiedGrammar,
        bytes: Option<&'b [u8]>,
        find_all: bool,
        trie: &TerminalsTrie,
        after_finding_stack: &mut F1,
        after_match_failed: &mut F2,
    ) -> bool
    where
        F1: FnMut(&[Option<StackItem<'a>>], Option<StackItem<'a>>),
        F2: FnMut(&'b [u8], usize),
    {
        let mut _find_stacks_matching_bytes =
            |mut arena: NonNull<StackArena<StackItem<'a>>>,
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
                                trie,
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
                            trie,
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
                StackItem::Nonterminal(top) => return _find_stacks_matching_bytes(
                    arena,
                    top,
                    stack.as_raw_slice(),
                    bytes,
                    after_finding_stack,
                ),
                StackItem::Terminal(_) | StackItem::Terminals(_) => {
                    stack.push(value);
                    match Self::match_stack_to_bytes(stack, bytes, trie) {
                        BytesMatchResults::Failed(index) => {
                            after_match_failed(bytes.unwrap(), index);
                            return false;
                        }
                        BytesMatchResults::Matches(possible_results) => {
                            // println!("results: {:?}, {:?}", possible_results, stack);
                            if possible_results.is_empty() {
                                after_finding_stack(stack.as_raw_slice(), None);
                                return true;
                            }
                            let mut flag = false;
                            for result in possible_results {
                                match result.remaining_bytes {
                                    None => {
                                        after_finding_stack(
                                            &stack[..result.stack_offset],
                                            result.modified_item_at_offset,
                                        );
                                        flag |= true;
                                        if !find_all {
                                            return true;
                                        }
                                    }
                                    Some(_) => {
                                        let top = match result
                                            .modified_item_at_offset
                                            .unwrap_or(stack[result.stack_offset])
                                        {
                                            StackItem::Nonterminal(id) => id,
                                            _ => panic!(
                                                "{:?} should only be nonterminal.",
                                                &stack[..result.stack_offset + 1]
                                            ),
                                        };
                                        flag |= _find_stacks_matching_bytes(
                                            arena,
                                            top,
                                            &stack[..result.stack_offset],
                                            result.remaining_bytes,
                                            after_finding_stack,
                                        );
                                        if !find_all {
                                            return true;
                                        }
                                    }
                                }
                            }
                            return flag;
                        }
                    }
                }
            },
            None => return false,
        };
    }
}
