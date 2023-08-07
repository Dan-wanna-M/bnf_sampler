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
use std::vec;

#[derive(PartialEq, Clone, Debug, Copy, Eq, Hash)]
pub(crate) enum StackItem<'a> {
    Nonterminal(NonterminalID),
    Terminal(&'a [u8]),
    Terminals(TrieNodeID),
}
#[derive(Clone, Debug)]
pub struct Sampler<'a> {
    pub(crate) stacks: Vec<Vec<StackItem<'a>>>,
    grammar: &'a SimplifiedGrammar,
    tokens_tree: Trie<VecU8Wrapper, u32>,
    stack_arena: StackArena<StackItem<'a>>,
    stacks_to_token_ids: FxHashMap<Vec<Vec<StackItem<'a>>>, FxHashSet<u32>>,
    token_ids: FxHashSet<u32>,
    current_token: VecU8Wrapper,
    current_prefix: VecU8Wrapper,
}

enum BytesMatchResult<'b> {
    AllMatched,
    PartiallyMatched(&'b [u8]),
    Failed(usize),
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
                        for one_product in iters.into_iter().multi_cartesian_product() {
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
                                let arena_ptr =
                                    &mut self.stack_arena as *mut StackArena<StackItem<'a>>;
                                let mut temp_stack = self.stack_arena.allocate_a_stack(stack.len());
                                temp_stack.copy_from_slice(stack);
                                let result = Self::find_stacks_matching_bytes(
                                    unsafe { &mut *arena_ptr },
                                    &mut temp_stack,
                                    self.grammar,
                                    Some(token),
                                    false,
                                    &self.grammar.terminals_trie,
                                    &mut |_| {},
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
                let arena_ptr = &mut self.stack_arena as *mut StackArena<StackItem<'a>>;
                let mut stack = self.stack_arena.allocate_a_stack(self.stacks[i].len());
                stack.copy_from_slice(&self.stacks[i]);
                match stack.last() {
                    Some(_) => {
                        accepted |= Self::find_stacks_matching_bytes(
                            unsafe { &mut *arena_ptr },
                            &mut stack,
                            self.grammar,
                            bytes,
                            true,
                            &self.grammar.terminals_trie,
                            &mut |temp_stack: &Stack<StackItem<'_>>| {
                                self.stacks.push(temp_stack.to_vec());
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
        let result = find_stacks_matching_bytes(bytes);
        if bytes.is_some() && !result {
            return false;
        }
        result | find_stacks_matching_bytes(None)
    }

    fn match_stack_to_bytes<'b>(
        stack: &mut Stack<StackItem<'a>>,
        bytes: Option<&'b [u8]>,
        trie: &TerminalsTrie,
    ) -> BytesMatchResult<'b> {
        let mut i = 0;
        if let Some(bytes) = bytes {
            while i < bytes.len() {
                match stack.pop() {
                    Some(value) => match value {
                        StackItem::Nonterminal(_) => {
                            stack.push(value);
                            return BytesMatchResult::PartiallyMatched(&bytes[i..]);
                        }
                        StackItem::Terminal(terminal) => {
                            for j in 0..terminal.len() {
                                if terminal[j] == bytes[i] {
                                    i += 1;
                                    if i == bytes.len() {
                                        if j != terminal.len() - 1 {
                                            stack.push(StackItem::Terminal(&terminal[j + 1..]))
                                        }
                                        return BytesMatchResult::AllMatched;
                                    }
                                } else {
                                    return BytesMatchResult::Failed(i);
                                }
                            }
                        }
                        StackItem::Terminals(mut node_id) => {
                            if !trie.get(node_id).children.contains_key(&bytes[i]) {
                                return BytesMatchResult::Failed(i);
                            }
                            loop {
                                if trie.get(node_id).children.contains_key(&bytes[i]) {
                                    node_id = trie.get(node_id).children[&bytes[i]];
                                    i += 1;
                                    if i == bytes.len() {
                                        if !trie.get(node_id).children.is_empty() {
                                            stack.push(StackItem::Terminals(node_id));
                                        }
                                        return BytesMatchResult::AllMatched;
                                    }
                                } else if trie.get(node_id).value.is_some() {
                                    break;
                                } else {
                                    return BytesMatchResult::Failed(i);
                                }
                            }
                        }
                    },
                    None => return BytesMatchResult::Failed(i),
                }
            }
        }
        BytesMatchResult::AllMatched
    }
    #[allow(clippy::too_many_arguments)]
    fn find_stacks_matching_bytes<'b, F1, F2>(
        arena: &mut StackArena<StackItem<'a>>,
        stack: &mut Stack<StackItem<'a>>,
        grammar: &'a SimplifiedGrammar,
        bytes: Option<&'b [u8]>,
        find_all: bool,
        trie: &TerminalsTrie,
        after_finding_stack: &mut F1,
        after_match_failed: &mut F2,
    ) -> bool
    where
        F1: FnMut(&Stack<StackItem<'a>>),
        F2: FnMut(&'b [u8], usize),
    {
        let mut bytes = bytes;
        let top = match stack.pop() {
            Some(value) => match value {
                StackItem::Nonterminal(value2) => value2,
                StackItem::Terminal(_) | StackItem::Terminals(_) => {
                    stack.push(value);
                    match Self::match_stack_to_bytes(stack, bytes, trie) {
                        BytesMatchResult::Failed(i) => {
                            after_match_failed(bytes.unwrap(), i);
                            return false;
                        }
                        BytesMatchResult::AllMatched => {
                            after_finding_stack(stack);
                            return true;
                        }
                        BytesMatchResult::PartiallyMatched(new_bytes) => {
                            bytes = Some(new_bytes);
                            match stack.pop().expect("The stack should not be empty.") {
                                StackItem::Nonterminal(x) => x,
                                _ => panic!("The top item should be nonterminal."),
                            }
                        }
                    }
                }
            },
            None => return false,
        };
        let mut found = false;
        let arena_ptr = arena as *mut StackArena<StackItem<'a>>;
        match &grammar.nonterminal_id_to_expression[&top] {
            SimplifiedExpressions::Expressions(expressions) => {
                for expression in expressions.iter() {
                    let temp_stack = &mut arena.allocate_a_stack(stack.len() + expression.len());
                    temp_stack.copy_from(stack);
                    for term in expression.iter().rev() {
                        temp_stack.push(match term {
                            Term::Terminal(value) => StackItem::Terminal(value.as_bytes()),
                            Term::Nonterminal(value) => {
                                StackItem::Nonterminal(grammar.nonterminal_to_terminal_id[value])
                            }
                        });
                    }
                    let temp = Self::find_stacks_matching_bytes(
                        unsafe { &mut *arena_ptr },
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
                let temp_stack = &mut arena.allocate_a_stack(stack.len() + 1);
                temp_stack.copy_from(stack);
                temp_stack.push(StackItem::Terminals(*node_id));
                let temp = Self::find_stacks_matching_bytes(
                    unsafe { &mut *arena_ptr },
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
    }
}
