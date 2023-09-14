use lazy_static::lazy_static;
use qp_trie::Trie;
use regex::Regex;
use rustc_hash::FxHashMap;
use std::borrow::Borrow;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::sync::Arc;

use crate::vocabulary::Vocabulary;

pub(crate) static ANY_NONTERMINAL_NAME: &str = "any!";
lazy_static! {
    pub(crate) static ref EXCEPT_LITERAL_REGEX: Regex =
        Regex::new("except!\\(['\"](.+?)['\"]\\)").unwrap();
}
lazy_static! {
    pub(crate) static ref EXCEPT_NONTERMINAL_REGEX: Regex =
        Regex::new("except!\\(\\[(.+?)\\]\\)").unwrap();
}
lazy_static! {
    pub(crate) static ref EXCEPTS_REGEX: Regex =
        Regex::new("except!\\(['\"](.+?)['\"]\\)|except!\\(\\[(.+?)\\]\\)").unwrap();
}
pub(crate) fn extract_excepted<'a>(regex: &Regex, except_nonterminal: &'a str) -> Option<&'a str> {
    Some(regex.captures(except_nonterminal)?.extract::<1>().1[0])
}
#[derive(PartialEq, Clone, Debug, Copy, Eq)]
pub(crate) struct NonterminalID(pub usize);

impl std::hash::Hash for NonterminalID {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        hasher.write_usize(self.0)
    }
}
impl nohash_hasher::IsEnabled for NonterminalID {}

#[derive(PartialEq, Clone, Debug, Eq, Hash)]
pub struct U8ArrayWrapper(pub Box<[u8]>);

impl Borrow<[u8]> for U8ArrayWrapper {
    #[inline]
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl qp_trie::Break for U8ArrayWrapper {
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

#[derive(PartialEq, Clone, Debug)]
pub(crate) struct SliceU8Wrapper<'a>(pub &'a [u8]);

impl<'a> Borrow<[u8]> for SliceU8Wrapper<'a> {
    #[inline]
    fn borrow(&self) -> &[u8] {
        self.0
    }
}

impl<'a> qp_trie::Break for SliceU8Wrapper<'a> {
    type Split = [u8];

    #[inline]
    fn empty<'b>() -> &'b [u8] {
        <&'b [u8]>::default()
    }

    #[inline]
    fn find_break(&self, loc: usize) -> &[u8] {
        &self.0[..loc]
    }
}

/// Read the vocabulary from RWKV-world model series vocabulary file
pub fn read_rwkv_world_vocab(file_name: &str) -> Arc<Vocabulary> {
    let file = File::open(file_name).unwrap();
    let reader = BufReader::new(file);
    let mut id_to_token: FxHashMap<u32, Vec<u8>> = FxHashMap::default();
    let mut id_to_token_string: FxHashMap<u32, String> = FxHashMap::default();
    let mut token_to_id = Trie::<U8ArrayWrapper, u32>::new();
    for line in reader.lines() {
        let line = line.unwrap();
        let mut start = line.find(' ').unwrap_or_else(|| {
            panic!("Invalid format. Ensure this vocab file{file_name} belongs to RWKV world model.")
        });
        let mut end = line.rfind(' ').unwrap_or_else(|| {
            panic!("Invalid format. Ensure this vocab file{file_name} belongs to RWKV world model.")
        });
        let token_id = line[..start]
            .parse::<u32>()
            .unwrap_or_else(|x| panic!("{line} cannot be parsed due to {x}."));
        start += 1;
        end -= 1;
        if line.chars().nth(start).unwrap() == 'b' {
            start += 2;
        } else {
            start += 1;
        }
        // println!("token: {}",&line[start..end]);
        let token = fix_utf8_escape(&line[start..end]);
        id_to_token.insert(token_id, token.clone());
        token_to_id.insert(U8ArrayWrapper(token.into()), token_id);
        // println!("{:?}", String::from_utf8(token.clone()));
        id_to_token_string.insert(token_id, line[start..end].to_string());
    }
    Arc::new(Vocabulary {
        token_to_id,
        id_to_token_string,
        id_to_token,
    })
}

/// translated from <https://github.com/npk48/rwkv_cuda/blob/main/tokenizer.hpp#L166>
///
/// sequence need to be unescaped:
///
///     "\\symbol", ["\\", "symbol"]
///
///     "\\",       ["\\"]
///
///     "\\t",      ["\\", "t"]
///
///     "\\n",      ["\\", "n"]
///
///     "\\r",      ["\\", "r"]
///
///     "\\x12",    ["\\", "x", "1", "2"]
///
///     "\\u1234",  ["\\", "u", "1", "2", "3", "4"]
pub fn fix_utf8_escape(token: &str) -> Vec<u8> {
    let mut result: Vec<u8> = Vec::new();
    result.reserve(token.as_bytes().len());
    let mut token = token;
    let convert_to_utf8 = |c: char, buffer: &mut Vec<u8>| {
        let mut temp = [0, 0, 0, 0];
        buffer.extend(c.encode_utf8(&mut temp).as_bytes());
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
                let hex_digits: String = token.chars().skip(2).take(2).collect();
                result.push(u8::from_str_radix(&hex_digits, 16).unwrap());
                token = &token[4..];
            } else if next_c == 'u' {
                let hex_digits: String = token.chars().skip(2).take(4).collect();
                convert_to_utf8(
                    char::from_u32(u32::from_str_radix(&hex_digits, 16).unwrap()).unwrap(),
                    &mut result,
                );
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
