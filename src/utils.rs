use bnf::{Grammar, Term};
use qp_trie::Trie;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{prelude::*, BufReader};
use crate::sampler::VecU8Wrapper;
pub enum SimplifiedTerm {
    Nonterminal(String),
    Terminals(Trie<Vec<u8>, ()>),
}
pub fn simplify_grammar_tree(grammar: &Grammar) -> HashMap<&str, HashSet<Vec<Term>>> {
    let mut simplified_grammar: HashMap<&str, HashSet<Vec<Term>>> = HashMap::new();
    for i in grammar.productions_iter() {
        let key = match &i.lhs {
            Term::Terminal(x) => x,
            Term::Nonterminal(x) => x,
        };
        simplified_grammar
            .entry(key)
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
                        Term::Nonterminal(x) => {
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
    let simplified_grammar: HashMap<&str, HashSet<Vec<Term>>> = HashMap::new();
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
