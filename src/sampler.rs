use bnf::{Grammar, Term};
use qp_trie::Trie;
use std::borrow::Borrow;
use std::vec;
use std::{collections::*, time::Instant};
#[derive(Clone, Debug)]
pub enum SimplifiedTerms {
    Terms(Vec<String>),
    SimplifiedTerminals(Trie<Vec<u8>, ()>)
}

#[derive(PartialEq, Clone, Debug, Copy)]
pub enum StackItem<'a> {
    Nonterminal(&'a str),
    Terminal(&'a str),
    Byte(u8),
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
        <&'a [u8]>::from(<&'a [u8]>::default())
    }

    #[inline]
    fn find_break(&self, loc: usize) -> &[u8] {
        From::from(&self.0[..loc])
    }
}
#[derive(Clone, Debug)]
pub struct PushDownAutomata<'a> {
    stacks: Vec<Vec<StackItem<'a>>>,
    grammar: &'a SimplifiedGrammar,
    tokens_tree: Trie<VecU8Wrapper, u32>,
}
#[derive(Clone, Debug)]
pub struct SimplifiedGrammar(HashMap<String, HashSet<Vec<Term>>>);

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
        SimplifiedGrammar(simplified_grammar)
    }
}

enum BytesMatchResult<'b> {
    AllMatched,
    PartiallyMatched(&'b [u8]),
    Failed,
}

impl<'a> PushDownAutomata<'a> {
    /// Create a new PushDownAutomata with simplified grammar
    pub fn new(
        grammar: &'a SimplifiedGrammar,
        start_term: &'a Term,
        tokens_tree: Trie<VecU8Wrapper, u32>,
    ) -> PushDownAutomata<'a> {
        let start_nonterminal = match start_term {
            Term::Nonterminal(x) => x,
            _ => panic!("Start term should be nonterminal"),
        };
        let mut stacks = Vec::new();
        stacks.push(vec![StackItem::Nonterminal(start_nonterminal)]);
        PushDownAutomata {
            stacks,
            grammar,
            tokens_tree,
        }
    }

    pub fn all_possible_next_tokens<'b>(
        &mut self,
        previous_tokens: Option<&'b [u8]>,
    ) -> Option<HashSet<u32>> {
        let now = Instant::now();
        if !self.accept_tokens(previous_tokens) {
            return None;
        }
        println!("Time used for accepting tokens: {:?}", now.elapsed());
        let mut token_ids: HashSet<u32> = HashSet::new();
        let mut current_stack: Vec<StackItem> = vec![];
        for (prefix, stack) in self.stacks.iter().map(|x| {
            let mut index = 0;
            let mut temp = vec![];
            for i in (0..x.len()).rev() {
                match x[i] {
                    StackItem::Byte(_) => index = i,
                    _ => break,
                }
            }
            for i in (&x[index..x.len()]).into_iter().rev() {
                match i {
                    StackItem::Byte(value) => temp.push(*value),
                    _ => panic!("Only bytes here."),
                }
            }
            (temp, x)
        }) {
            let now = Instant::now();
            for (VecU8Wrapper(token), &token_id) in self.tokens_tree.iter_prefix(
                self.tokens_tree
                    .longest_common_prefix(&VecU8Wrapper(prefix)),
            ) {
                if token_ids.contains(&token_id) {
                    continue;
                }
                current_stack.extend(stack);
                let result = Self::find_stacks_matching_bytes(
                    &mut current_stack,
                    &self.grammar,
                    Some(token),
                    0,
                    false,
                    &mut |_| {},
                );
                if result {
                    token_ids.insert(token_id);
                }
                current_stack.clear();
            }
            println!("Time used for one stack: {:?}", now.elapsed());
        }
        Some(token_ids)
    }
    #[must_use]
    fn accept_tokens<'b>(&mut self, bytes: Option<&'b [u8]>) -> bool {
        let len = self.stacks.len();
        let mut find_stacks_matching_bytes = |bytes| {
            let mut stack: Vec<StackItem> =
                Vec::with_capacity(self.stacks.iter().map(|x| x.len()).max().unwrap());
            let mut accepted = false;
            for i in 0..len {
                stack.extend(&self.stacks[i]);
                match stack.last() {
                    Some(_) => {
                        accepted |= Self::find_stacks_matching_bytes(
                            &mut stack,
                            &self.grammar,
                            bytes,
                            0,
                            true,
                            &mut |temp_stack: &Vec<StackItem<'_>>| {
                                self.stacks.push(temp_stack.clone());
                            },
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
        if bytes.is_some() {
            if !find_stacks_matching_bytes(bytes) {
                return false;
            }
        }
        find_stacks_matching_bytes(None)
        // println!("{:?}", self.stacks);
    }

    fn convert_terminal_to_bytes(stack: &mut Vec<StackItem<'a>>, popped_terminal: &str) {
        for j in popped_terminal.as_bytes().into_iter().rev() {
            stack.push(StackItem::Byte(*j));
        }
    }

    fn match_stack_to_bytes<'b>(
        stack: &mut Vec<StackItem<'a>>,
        bytes: Option<&'b [u8]>,
    ) -> BytesMatchResult<'b> {
        let mut i = 0;
        if let Some(bytes) = bytes {
            while i < bytes.len() {
                let byte1 = bytes[i];
                match stack.pop() {
                    Some(value) => match value {
                        StackItem::Byte(byte) => {
                            if byte != byte1 {
                                return BytesMatchResult::Failed;
                            }
                            i += 1;
                        }
                        StackItem::Nonterminal(_) => {
                            stack.push(value);
                            return BytesMatchResult::PartiallyMatched(&bytes[i..]);
                        }
                        StackItem::Terminal(terminal) => {
                            Self::convert_terminal_to_bytes(stack, terminal);
                        }
                    },
                    None => return BytesMatchResult::Failed,
                }
            }
        }
        return BytesMatchResult::AllMatched;
    }

    fn find_stacks_matching_bytes<'b, F>(
        stack: &mut Vec<StackItem<'a>>,
        grammar: &'a SimplifiedGrammar,
        bytes: Option<&'b [u8]>,
        layer: i8,
        find_all: bool,
        after_finding_stack: &mut F,
    ) -> bool
    where
        F: FnMut(&Vec<StackItem<'a>>),
    {
        let mut bytes = bytes;
        let top = match stack.pop() {
            Some(value) => {
                let mut result = None;
                let mut flag = false;
                match value {
                    StackItem::Nonterminal(value2) => result = Some(value2),
                    StackItem::Terminal(value2) => {
                        flag = true;
                        Self::convert_terminal_to_bytes(stack, value2);
                    }
                    StackItem::Byte(_) => {
                        stack.push(value);
                        flag = true
                    }
                };
                if flag {
                    match Self::match_stack_to_bytes(stack, bytes) {
                        BytesMatchResult::Failed => {
                            return false;
                        }
                        BytesMatchResult::AllMatched => {
                            after_finding_stack(&stack);
                            return true;
                        }
                        BytesMatchResult::PartiallyMatched(new_bytes) => {
                            bytes = Some(new_bytes);
                            match stack.pop().expect("The stack should not be empty.") {
                                StackItem::Nonterminal(x) => result = Some(x),
                                _ => panic!("The top item should be nonterminal."),
                            }
                        }
                    };
                }
                result.unwrap()
            }
            None => return false,
        };
        let mut found = false;
        let SimplifiedGrammar(actual_grammar) = grammar;
        for expression in actual_grammar[top].iter() {
            let temp_stack = &mut stack.clone();
            for term in expression.iter().rev() {
                temp_stack.push(match term {
                    Term::Terminal(value) => StackItem::Terminal(&value),
                    Term::Nonterminal(value) => StackItem::Nonterminal(&value),
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
            );
            // println!("{layer}end=>{:?}", stack);
            found |= temp;
            if !find_all && found {
                return found;
            }
        }
        found
    }
}
