use bnf::{Grammar, Term};
use qp_trie::Trie;
use std::borrow::Borrow;
use std::vec;
use std::{collections::*, time::Instant};

#[derive(PartialEq, Clone, Debug, Copy)]
pub enum StackItem<'a> {
    Nonterminal(usize),
    Terminal(&'a [u8]),
}

#[derive(PartialEq, Clone, Debug)]
pub struct VecU8Wrapper(pub Vec<u8>);

impl Borrow<[u8]> for VecU8Wrapper {
    #[inline]
    fn borrow(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl qp_trie::Break for VecU8Wrapper {
    type Split = [u8];

    #[inline]
    fn empty<'a>() -> &'a [u8] {
        <&'a [u8]>::default()
    }

    #[inline]
    fn find_break(&self, loc: usize) -> &[u8] {
        &self.0[..loc]
    }
}
#[derive(Clone, Debug)]
pub struct PushDownAutomata<'a> {
    pub stacks: Vec<Vec<StackItem<'a>>>,
    grammar: &'a SimplifiedGrammar,
    tokens_tree: Trie<VecU8Wrapper, u32>,
    token_ids: HashSet<u32>,
    current_stack: Vec<StackItem<'a>>,
    current_token: VecU8Wrapper,
    current_prefix: VecU8Wrapper,
}

#[derive(Clone, Debug)]
pub struct SimplifiedGrammar {
    nonterminal_id_to_expression: HashMap<usize, HashSet<Vec<Term>>>,
    pub nonterminal_to_terminal_id: HashMap<String, usize>,
}

impl SimplifiedGrammar {
    pub fn new(input: &str) -> Self {
        let grammar: Grammar = input.parse().unwrap();
        let mut simplified_grammar: HashMap<String, HashSet<Vec<Term>>> = HashMap::new();
        for i in grammar.productions_iter() {
            let key = match &i.lhs {
                Term::Terminal(x) => x,
                Term::Nonterminal(x) => x,
            };
            simplified_grammar
                .entry(key.clone())
                .or_insert(HashSet::new())
                .extend(i.rhs_iter().map(|x| {
                    let mut temp_vec: Vec<Term> = vec![];
                    let mut temp_string: Option<String> = None;
                    for i in x.terms_iter() {
                        match i {
                            Term::Terminal(x) => match temp_string {
                                Some(value) => temp_string = Some(value + x),
                                None => temp_string = Some(x.clone()),
                            },
                            Term::Nonterminal(_) => {
                                if let Some(value) = temp_string {
                                    temp_vec.push(Term::Terminal(value));
                                    temp_string = None;
                                }
                                temp_vec.push(i.clone());
                            }
                        }
                    }
                    if let Some(value) = temp_string {
                        temp_vec.push(Term::Terminal(value));
                    }
                    temp_vec
                }));
        }
        let nonterminal_to_terminal_id: HashMap<String, usize> = simplified_grammar
            .iter()
            .enumerate()
            .map(|(i, (key, _))| (key.clone(), i))
            .collect();
        let nonterminal_id_to_expression: HashMap<usize, HashSet<Vec<Term>>> = simplified_grammar
            .iter()
            .map(|(key, value)| (nonterminal_to_terminal_id[key], value.clone()))
            .collect();
        SimplifiedGrammar {
            nonterminal_to_terminal_id,
            nonterminal_id_to_expression,
        }
    }
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
    ) -> PushDownAutomata<'a> {
        let stacks = vec![vec![StackItem::Nonterminal(
            grammar.nonterminal_to_terminal_id[start_term],
        )]];
        let token_ids: HashSet<u32> = HashSet::with_capacity(64);
        let current_stack: Vec<StackItem> = Vec::with_capacity(20);
        let current_token = VecU8Wrapper(Vec::with_capacity(20));
        let current_prefix = VecU8Wrapper(Vec::with_capacity(20));
        PushDownAutomata {
            stacks,
            grammar,
            tokens_tree,
            token_ids,
            current_stack,
            current_token,
            current_prefix,
        }
    }

    pub fn all_possible_next_tokens(
        &mut self,
        previous_tokens: Option<&[u8]>,
    ) -> Option<&HashSet<u32>> {
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
            let mut failed_prefixs: Trie<VecU8Wrapper, ()> = Trie::new();
            for (VecU8Wrapper(token), &token_id) in self
                .tokens_tree
                .iter_prefix(self.tokens_tree.longest_common_prefix(&self.current_prefix))
            {
                self.current_token.0.extend(token);
                if self.token_ids.contains(&token_id)
                    || failed_prefixs
                        .contains_key(failed_prefixs.longest_common_prefix(&self.current_token))
                {
                    continue;
                }
                self.current_stack.extend(stack);
                let result = Self::find_stacks_matching_bytes(
                    &mut self.current_stack,
                    self.grammar,
                    Some(token),
                    0,
                    false,
                    &mut |_| {},
                    &mut |bytes, index| {
                        failed_prefixs.insert(VecU8Wrapper(bytes[..index + 1].to_vec()), ());
                    },
                );
                if result {
                    self.token_ids.insert(token_id);
                }
                self.current_stack.clear();
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
            let mut stack: Vec<StackItem> = Vec::with_capacity(
                self.stacks
                    .iter()
                    .map(|x| x.len())
                    .max()
                    .unwrap_or_default(),
            );
            let mut accepted = false;
            for i in 0..len {
                stack.extend(&self.stacks[i]);
                match stack.last() {
                    Some(_) => {
                        accepted |= Self::find_stacks_matching_bytes(
                            &mut stack,
                            self.grammar,
                            bytes,
                            0,
                            true,
                            &mut |temp_stack: &Vec<StackItem<'_>>| {
                                self.stacks.push(temp_stack.clone());
                            },
                            &mut |_, _| {},
                        );
                    }
                    None => {
                        continue;
                    }
                };
                stack.clear();
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
        result|find_stacks_matching_bytes(None)
        // println!("{:?}", self.stacks);
    }

    fn match_stack_to_bytes<'b>(
        stack: &mut Vec<StackItem<'a>>,
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
        stack: &mut Vec<StackItem<'a>>,
        grammar: &'a SimplifiedGrammar,
        bytes: Option<&'b [u8]>,
        layer: i8,
        find_all: bool,
        after_finding_stack: &mut F1,
        after_match_failed: &mut F2,
    ) -> bool
    where
        F1: FnMut(&Vec<StackItem<'a>>),
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
        let initial_stack_len = stack.len();
        for expression in grammar.nonterminal_id_to_expression[&top].iter() {
            let temp_stack = &mut stack.clone();
            for term in expression.iter().rev() {
                temp_stack.push(match term {
                    Term::Terminal(value) => StackItem::Terminal(value.as_bytes()),
                    Term::Nonterminal(value) => StackItem::Nonterminal(grammar.nonterminal_to_terminal_id[value]),
                });
            }
            // println!("{layer}start=>{:?}", stack);
            let temp = Self::find_stacks_matching_bytes(
                temp_stack,
                grammar,
                bytes,
                layer + 1,
                find_all,
                after_finding_stack,
                after_match_failed
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
