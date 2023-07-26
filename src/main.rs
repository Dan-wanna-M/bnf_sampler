use bnf::{Grammar, Term};
use qp_trie::Trie;
use std::borrow::Borrow;
use std::{collections::*, time::Instant};
use std::{fs, vec};

pub mod utils {
    use crate::VecU8Wrapper;
    use bnf::{Grammar, Term};
    use qp_trie::Trie;
    use std::collections::{HashMap, HashSet};
    use std::fs::File;
    use std::io::{prelude::*, BufReader};

    pub fn simplify_grammar_tree(grammar: &Grammar) -> HashMap<&str, HashSet<Vec<&Term>>> {
        let mut simplified_grammar: HashMap<&str, HashSet<Vec<&Term>>> = HashMap::new();
        for i in grammar.productions_iter() {
            let key = match &i.lhs {
                Term::Terminal(x) => x,
                Term::Nonterminal(x) => x,
            };
            simplified_grammar
                .entry(key)
                .or_insert(HashSet::new())
                .extend(i.rhs_iter().map(|x| x.terms_iter().collect()));
        }
        simplified_grammar
    }

    pub fn read_world_vocab(file_name: &str) -> (Trie<VecU8Wrapper, u32>, HashMap<u32, String>) {
        let file = File::open(file_name).expect(format!("{file_name} does not exist.").as_str());
        let reader = BufReader::new(file);
        let mut map: HashMap<u32, String> = HashMap::new();
        let mut tree = Trie::<VecU8Wrapper, u32>::new();
        for line in reader.lines() {
            let line = line.unwrap();
            let mut start = line.find(' ').expect(
                format!(
                    "Invalid format. Ensure this vocab file{file_name} belongs to RWKV world model."
                )
                .as_str(),
            );
            let mut end = line.rfind(' ').expect(
                format!(
                    "Invalid format. Ensure this vocab file{file_name} belongs to RWKV world model."
                )
                .as_str(),
            );
            let token_id = line[..start]
                .parse::<u32>()
                .expect(format!("{line} cannot be parsed.").as_str());
            start += 1;
            end -= 1;
            if line.chars().nth(start).unwrap() == 'b' {
                start += 2;
            } else {
                start += 1;
            }
            // println!("token: {}",&line[start..end]);
            let token = fix_utf8_escape(&line[start..end]);
            tree.insert(VecU8Wrapper(token.clone()), token_id);
            // println!("{:?}", String::from_utf8(token.clone()));
            map.insert(token_id, String::from_utf8(token).unwrap());
        }
        (tree, map)
    }

    pub fn fix_utf8_escape(token: &str) -> Vec<u8> {
        /*
            translated from https://github.com/npk48/rwkv_cuda/blob/main/tokenizer.hpp#L166
            sequence need to be unescaped
            [
                "\\symbol", ["\\", "symbol"]
                "\\",       ["\\"]
                "\\t",      ["\\", "t"]
                "\\n",      ["\\", "n"]
                "\\r",      ["\\", "r"]
                "\\x12",    ["\\", "x", "1", "2"]
                "\\u1234",  ["\\", "u", "1", "2", "3", "4"]
            ]
        */

        let mut result: Vec<u8> = Vec::new();
        result.reserve(token.as_bytes().len());
        let mut token = token;
        let convert_to_utf8 = |c: char, buffer: &mut Vec<u8>| {
            let mut temp = [0, 0, 0, 0];
            buffer.extend(c.encode_utf8(&mut temp).as_bytes());
        };
        let process_hex_digits = |hex_digit_len: usize, token: &str, buffer: &mut Vec<u8>| {
            let hex_digits: String = token.chars().skip(2).take(hex_digit_len).collect();
            convert_to_utf8(
                char::from_u32(u32::from_str_radix(&hex_digits, 16).unwrap()).unwrap(),
                buffer,
            );
        };
        while !token.is_empty() {
            let c = token.chars().next().unwrap();
            if c == '\\' {
                let next_c = token.chars().nth(1).unwrap();
                if next_c == 't' {
                    result.push(b'\t');
                    token = &token[2..];
                } else if next_c == 'n' {
                    result.push(b'\n');
                    token = &token[2..];
                } else if next_c == 'r' {
                    result.push(b'\r');
                    token = &token[2..];
                } else if next_c == 'x' {
                    process_hex_digits(2, token, &mut result);
                    token = &token[4..];
                } else if next_c == 'u' {
                    process_hex_digits(4, token, &mut result);
                    token = &token[6..];
                } else {
                    result.push(next_c as u8);
                    token = &token[2..];
                }
            } else {
                convert_to_utf8(c, &mut result);
                token = &token[c.len_utf8()..];
            }
        }
        result
    }
}
#[derive(PartialEq, Clone, Debug, Copy)]
pub enum StackItem<'a> {
    Nonterminal(&'a str),
    Terminal(&'a str),
    Byte(u8),
}
pub struct VecU8Wrapper(Vec<u8>);

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
    fn find_break(&self, mut loc: usize) -> &[u8] {
        From::from(&self.0[..loc])
    }
}
pub struct PushDownAutomata<'a> {
    stacks: Vec<Vec<StackItem<'a>>>,
    grammar: HashMap<&'a str, HashSet<Vec<&'a Term>>>,
    tokens_tree: &'a Trie<VecU8Wrapper, u32>,
}

enum BytesMatchResult<'b> {
    AllMatched,
    PartiallyMatched(&'b [u8]),
    Failed,
}

impl<'a> PushDownAutomata<'a> {
    /// Create a new PushDownAutomata with simplified grammar
    pub fn new(
        grammar: &'a Grammar,
        start_term: &'a Term,
        tokens_tree: &'a Trie<VecU8Wrapper, u32>,
    ) -> PushDownAutomata<'a> {
        let start_nonterminal = match start_term {
            Term::Nonterminal(x) => x,
            _ => panic!("Start term should be nonterminal"),
        };
        let mut stacks = Vec::new();
        stacks.push(vec![StackItem::Nonterminal(start_nonterminal)]);
        PushDownAutomata {
            stacks,
            grammar: utils::simplify_grammar_tree(grammar),
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
            let mut stack: Vec<StackItem> = Vec::new();
            let mut accepted = false;
            stack.reserve(self.stacks.iter().map(|x| x.len()).max().unwrap());
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
        match bytes {
            Some(bytes) => {
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
                return BytesMatchResult::AllMatched;
            }
            None => BytesMatchResult::AllMatched,
        }
    }

    fn find_stacks_matching_bytes<'b, F>(
        stack: &mut Vec<StackItem<'a>>,
        grammar: &HashMap<&'a str, HashSet<Vec<&'a Term>>>,
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
        let count = stack.len();
        let mut found = false;
        // let debug = stack.clone();
        // println!("{layer}start=>{:?}", stack);
        for expression in grammar[top].iter() {
            // let mut temp_stack = stack.clone();
            for term in expression.iter().rev() {
                stack.push(match term {
                    Term::Terminal(value) => StackItem::Terminal(&value),
                    Term::Nonterminal(value) => StackItem::Nonterminal(&value),
                });
            }

            let temp = Self::find_stacks_matching_bytes(
                stack,
                grammar,
                bytes,
                layer + 1,
                find_all,
                after_finding_stack,
            );
            found |= temp;
            if !find_all && found {
                return found;
            }
            assert!(stack.len() >= count);
            stack.truncate(count);
        }
        // println!("{layer}end=>{:?}", stack);
        stack.push(StackItem::Nonterminal(top));
        found
    }
}

fn main() {
    let input = fs::read_to_string("./grammar.bnf").expect("grammar.bnf should exist.");
    let input = String::from_utf8(utils::fix_utf8_escape(&input)).unwrap();
    let grammar: Grammar = input.parse().unwrap();
    let (tree, map) = utils::read_world_vocab("vocab.txt");
    // println!("{:#?}", Utils::SimplifyGrammarTree(&grammar));
    let binding = Term::Nonterminal("dna".to_string());
    let mut machine = PushDownAutomata::new(&grammar, &binding, &tree);
    let result: Vec<&str> = machine
        .all_possible_next_tokens(None)
        .unwrap()
        .into_iter()
        .map(|x| map[&x].as_str())
        .collect();
    println!("{:?}", result);
    let mut times: Vec<f64> = vec![];
    // println!("{:?}", machine.stacks);
    loop {
        // println!("{:?}", machine.stacks);
        println!("Input a terminal: ");
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .expect("Input should exist");
        let now = Instant::now();
        // println!("{:?}", machine.stacks);
        let input = utils::fix_utf8_escape(input.trim());
        let result: Vec<&str> = match machine.all_possible_next_tokens(Some(&input)) {
            Some(result) => result.into_iter().map(|x| map[&x].as_str()).collect(),
            None => {
                println!("Invalid input.");
                break;
            }
        };
        println!("{:?}", machine.stacks);
        let end = now.elapsed();
        times.push(end.as_secs_f64());
        println!("Time used: {:?}", end);
        println!("{:?}", result);
        if result.is_empty() {
            break;
        }
    }
    println!("{}", times.iter().sum::<f64>() / times.len() as f64);
}
