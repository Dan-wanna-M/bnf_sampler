use crate::simplified_grammar::SimplifiedGrammar;
use crate::stack::Stack;
use crate::stack::StackArena;
use crate::utils::NonterminalID;
use crate::utils::SliceU8Wrapper;
use crate::utils::VecU8Wrapper;
use bnf::Term;
use qp_trie::Trie;
use rustc_hash::FxHashSet;
use std::time::Instant;
use std::vec;

#[derive(PartialEq, Clone, Debug, Copy)]
pub enum StackItem<'a> {
    Nonterminal(NonterminalID),
    Terminal(&'a [u8]),
}
#[derive(Clone, Debug)]
pub struct PushDownAutomata<'a> {
    pub stacks: Vec<Vec<StackItem<'a>>>,
    grammar: &'a SimplifiedGrammar,
    tokens_tree: Trie<VecU8Wrapper, u32>,
    stack_arena: StackArena<StackItem<'a>>,
    token_ids: FxHashSet<u32>,
    current_token: VecU8Wrapper,
    current_prefix: VecU8Wrapper,
}

enum BytesMatchResult<'b> {
    AllMatched,
    PartiallyMatched(&'b [u8]),
    Failed(usize),
}

impl<'a> PushDownAutomata<'a> {
    /// Create a new PushDownAutomata with simplified grammar
    pub fn new(
        grammar: &'a SimplifiedGrammar,
        start_term: &str,
        tokens_tree: Trie<VecU8Wrapper, u32>,
        stack_arena_capacity: usize,
    ) -> PushDownAutomata<'a> {
        let stacks = vec![vec![StackItem::Nonterminal(
            grammar.nonterminal_to_terminal_id[start_term],
        )]];
        let token_ids: FxHashSet<u32> = FxHashSet::default();
        let current_token = VecU8Wrapper(Vec::with_capacity(128));
        let current_prefix = VecU8Wrapper(Vec::with_capacity(128));
        PushDownAutomata {
            stacks,
            grammar,
            tokens_tree,
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
        let now = Instant::now();
        if !self.accept_tokens(previous_tokens) {
            return None;
        }
        // println!("Time used for accepting tokens: {:?}", now.elapsed());
        self.token_ids.clear();
        for stack in self.stacks.iter() {
            for i in stack.iter().rev() {
                match i {
                    StackItem::Terminal(value) => self.current_prefix.0.extend(*value),
                    _ => break,
                }
            }
            let now = Instant::now();
            let mut failed_prefixs: Trie<SliceU8Wrapper, ()> = Trie::new();
            for (VecU8Wrapper(token), &token_id) in self
                .tokens_tree
                .iter_prefix(self.tokens_tree.longest_common_prefix(&self.current_prefix))
            {
                self.current_token.0.extend(token);
                if self.token_ids.contains(&token_id)
                    || failed_prefixs.contains_key(
                        failed_prefixs.longest_common_prefix(self.current_token.0.as_slice()),
                    )
                {
                    continue;
                }
                let arena_ptr = &mut self.stack_arena as *mut StackArena<StackItem<'a>>;
                let mut temp_stack = self.stack_arena.allocate_a_stack(stack.len());
                temp_stack.copy_from_slice(stack);
                let result = Self::find_stacks_matching_bytes(
                    unsafe { &mut *arena_ptr },
                    &mut temp_stack,
                    self.grammar,
                    Some(token),
                    0,
                    false,
                    &mut |_| {},
                    &mut |bytes, index| {
                        failed_prefixs.insert(SliceU8Wrapper(&bytes[..index + 1]), ());
                    },
                );
                if result {
                    self.token_ids.insert(token_id);
                }
                self.stack_arena.clear();
                self.current_token.0.clear();
                self.current_prefix.0.clear();
            }
            //println!("Time used for one stack: {:?}", now.elapsed());
        }
        Some(&self.token_ids)
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
                            0,
                            true,
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
        // println!("{:?}", self.stacks);
    }

    fn match_stack_to_bytes<'b>(
        stack: &mut Stack<StackItem<'a>>,
        bytes: Option<&'b [u8]>,
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
                    },
                    None => return BytesMatchResult::Failed(i),
                }
            }
        }
        return BytesMatchResult::AllMatched;
    }

    fn find_stacks_matching_bytes<'b, F1, F2>(
        arena: &mut StackArena<StackItem<'a>>,
        stack: &mut Stack<StackItem<'a>>,
        grammar: &'a SimplifiedGrammar,
        bytes: Option<&'b [u8]>,
        layer: i8,
        find_all: bool,
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
                StackItem::Terminal(_) => {
                    stack.push(value);
                    match Self::match_stack_to_bytes(stack, bytes) {
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
        for expression in grammar.nonterminal_id_to_expression[&top].iter() {
            let arena_ptr = arena as *mut StackArena<StackItem<'a>>;
            let temp_stack = &mut arena.allocate_a_stack(stack.len() + expression.len());
            temp_stack.copy_from(&stack);
            for term in expression.iter().rev() {
                temp_stack.push(match term {
                    Term::Terminal(value) => StackItem::Terminal(value.as_bytes()),
                    Term::Nonterminal(value) => {
                        StackItem::Nonterminal(grammar.nonterminal_to_terminal_id[value])
                    }
                });
            }
            // println!("{layer}start=>{:?}", stack);

            let temp = Self::find_stacks_matching_bytes(
                unsafe { &mut *arena_ptr },
                temp_stack,
                grammar,
                bytes,
                layer + 1,
                find_all,
                after_finding_stack,
                after_match_failed,
            );
            //println!("{layer}end=>{:?}", stack);
            found |= temp;
            if !find_all && found {
                return found;
            }
        }
        found
    }
}
