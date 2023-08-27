use crate::trie::TerminalsTrie;
use crate::trie::TrieNodeID;
use crate::utils;
use crate::utils::NonterminalID;
use crate::utils::VecU8Wrapper;
use bit_set::BitSet;
use bnf::Production;
use bnf::{Grammar, Term};
use memchr::memmem;
use memchr::memmem::Finder;
use qp_trie::Trie;
use regex::Regex;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) enum U8Term
{
    Terminal(Vec<u8>),
    Nonterminal(String)
}

#[derive(Clone, Debug)]
pub struct SimplifiedGrammar {
    pub(crate) nonterminal_id_to_expression: FxHashMap<NonterminalID, SimplifiedExpressions>,
    pub(crate) nonterminal_to_terminal_id: FxHashMap<String, NonterminalID>,
    pub(crate) terminals_trie: TerminalsTrie,
    pub(crate) nonterminal_to_token_ids: FxHashMap<NonterminalID, BitSet<u32>>,
    pub(crate) nonterminal_to_excluded_token_ids: FxHashMap<NonterminalID, BitSet<u32>>
}
#[derive(Clone, Debug)]
pub(crate) enum SimplifiedExpressions {
    Expressions(FxHashSet<Vec<U8Term>>),
    Terminals(TrieNodeID),
}
impl SimplifiedGrammar {
    pub fn new(input: &str, tokens_tree: &Trie<VecU8Wrapper, u32>) -> Self {
        let except_present = utils::EXCEPTS_REGEX.is_match(input);
        let any_present = input.contains(&format!("<{}>", utils::ANY_NONTERMINAL_NAME));
        let mut grammar: Grammar = input.parse().unwrap();
        if any_present {
            let mut any_prod = Production::new();
            any_prod.lhs = Term::Nonterminal(utils::ANY_NONTERMINAL_NAME.to_string());
            grammar.add_production(any_prod);
        }
        let mut nonterminal_to_token_ids: FxHashMap<NonterminalID, BitSet<u32>> =
            FxHashMap::default();
        let mut nonterminal_to_excluded_token_ids: FxHashMap<NonterminalID, BitSet<u32>> =
        FxHashMap::default();
        let mut excepts: FxHashSet<String> = FxHashSet::default();
        if except_present {
            for i in utils::EXCEPTS_REGEX.find_iter(input) {
                let temp = i.as_str().to_string();
                let mut any_prod = Production::new();
                excepts.insert(temp.clone());
                any_prod.lhs = Term::Nonterminal(temp);
                grammar.add_production(any_prod);
            }
        }
        let mut simplified_grammar: FxHashMap<String, FxHashSet<Vec<U8Term>>> =
            FxHashMap::default();
        for i in grammar.productions_iter() {
            let key = match &i.lhs {
                Term::Terminal(x) => x,
                Term::Nonterminal(x) => x,
            };
            simplified_grammar
                .entry(key.clone())
                .or_insert(FxHashSet::default())
                .extend(i.rhs_iter().map(|x| {
                    let mut temp_vec: Vec<U8Term> = vec![];
                    let mut temp_string: Option<String> = None;
                    for i in x.terms_iter() {
                        match i {
                            Term::Terminal(x) => match temp_string {
                                Some(value) => temp_string = Some(value + x),
                                None => temp_string = Some(x.clone()),
                            },
                            Term::Nonterminal(nonterminal) => {
                                if let Some(value) = temp_string {
                                    temp_vec.push(U8Term::Terminal(utils::fix_utf8_escape(&value)));
                                    temp_string = None;
                                }
                                temp_vec.push(U8Term::Nonterminal(nonterminal.clone()));
                            }
                        }
                    }
                    if let Some(value) = temp_string {
                        temp_vec.push(U8Term::Terminal(value.as_bytes().to_vec()));
                    }
                    temp_vec
                }));
        }
        let nonterminal_to_terminal_id: FxHashMap<String, NonterminalID> = simplified_grammar
            .iter()
            .enumerate()
            .map(|(i, (key, _))| (key.clone(), NonterminalID(i)))
            .collect();
        let mut terminals_arena = TerminalsTrie::new();
        let mut add_tokens =
            |simplified_grammar: &mut FxHashMap<String, FxHashSet<Vec<U8Term>>>,
             terminals_arena: &mut TerminalsTrie,
             nonterminal: &str,
             excepted_literal: Option<&str>| {
                simplified_grammar.remove(nonterminal);
                let finder = excepted_literal.map(|x| memmem::Finder::new(x.as_bytes()));
                let predicate = |x: &&VecU8Wrapper, finder: &Finder| {
                    excepted_literal.is_some_and(|excepted_literal| {
                        let excepted_bytes = excepted_literal.as_bytes();
                        return x.0 != excepted_bytes
                            && (x.0.ends_with(excepted_bytes)
                                || finder.find(x.0.as_slice()).is_none());
                    })
                };
                match finder {
                    Some(finder) => {
                        simplified_grammar.insert(
                            nonterminal.to_string(),
                            tokens_tree
                                .keys()
                                .filter(|x| predicate(x, &finder))
                                .map(|k| vec![U8Term::Terminal(k.0.clone())])
                                .collect(),
                        );
                        for (key, _) in tokens_tree.iter().filter(|(x, _)| predicate(x, &finder)) {
                            terminals_arena
                                .add(key.0.as_slice(), nonterminal_to_terminal_id[nonterminal])
                        }
                        let mut bit_set = BitSet::new();
                        bit_set.extend(tokens_tree.iter().filter_map(|(x, token_id)| {
                            excepted_literal.and_then(|excepted_literal| {
                                let excepted_bytes = excepted_literal.as_bytes();
                                if x.0 != excepted_bytes && finder.find(x.0.as_slice()).is_none() {
                                    Some((*token_id) as usize)
                                }
                                else {

                                    None
                                }
                            })
                        }));
                        nonterminal_to_token_ids
                            .insert(nonterminal_to_terminal_id[nonterminal], bit_set);
                        let mut bit_set = BitSet::new();
                        bit_set.extend(tokens_tree.iter().filter_map(|(k,token_id)|{
                            if !predicate(&k, &finder)
                            {
                                println!("{:?}", String::from_utf8(k.0.clone()));
                                Some(*(token_id) as usize)
                            }
                            else {
                                None
                            }
                        }));
                        nonterminal_to_excluded_token_ids.insert(nonterminal_to_terminal_id[nonterminal], bit_set);
                    }
                    None => {
                        simplified_grammar.insert(
                            nonterminal.to_string(),
                            tokens_tree
                                .keys()
                                .map(|k| vec![U8Term::Terminal(k.0.clone())])
                                .collect(),
                        );
                        let mut bit_set = BitSet::new();
                        for (key, token_id) in tokens_tree.iter() {
                            bit_set.insert((*token_id) as usize);
                            terminals_arena
                                .add(key.0.as_slice(), nonterminal_to_terminal_id[nonterminal])
                        }
                        nonterminal_to_token_ids
                            .insert(nonterminal_to_terminal_id[nonterminal], bit_set);
                    }
                }
            };
        if any_present {
            add_tokens(
                &mut simplified_grammar,
                &mut terminals_arena,
                utils::ANY_NONTERMINAL_NAME,
                None,
            );
        }
        if except_present {
            for nonterminal in excepts.iter() {
                fn process_valid_result<F: FnOnce(&str)>(
                    regex: &Regex,
                    nonterminal: &str,
                    process: F,
                ) {
                    let extracted = utils::extract_excepted(regex, nonterminal);
                    if let Some(extracted) = extracted {
                        if extracted.is_empty() {
                            panic!("{nonterminal} is invalid except!() nonterminal because the brackets contain nothing.");
                        }
                        process(extracted);
                    }
                }
                process_valid_result(&utils::EXCEPT_LITERAL_REGEX, nonterminal, |extracted| {
                    println!("extracted: {}", extracted);
                    add_tokens(
                        &mut simplified_grammar,
                        &mut terminals_arena,
                        nonterminal,
                        Some(extracted),
                    );
                    terminals_arena.except_terminal(
                        extracted.as_bytes(),
                        nonterminal_to_terminal_id[nonterminal],
                    );
                });
            }
        }
        let mut new_simplified_grammar: FxHashMap<String, SimplifiedExpressions> =
            simplified_grammar
                .into_iter()
                .map(|(k, v)| {
                    if v.iter().all(|terms| {
                        terms.len() == 1
                            && match terms.last().unwrap() {
                                U8Term::Terminal(_) => true,
                                U8Term::Nonterminal(_) => false,
                            }
                    }) {
                        for i in v.into_iter() {
                            let value = match i.last().unwrap() {
                                U8Term::Terminal(value) => value,
                                _ => panic!("There should only be terminals."),
                            };
                            terminals_arena.add(value, nonterminal_to_terminal_id[&k]);
                        }
                        let v = SimplifiedExpressions::Terminals(
                            terminals_arena.roots[&nonterminal_to_terminal_id[&k]],
                        );
                        (k, v)
                    } else {
                        (k, SimplifiedExpressions::Expressions(v))
                    }
                })
                .collect();
        if any_present {
            new_simplified_grammar.insert(
                utils::ANY_NONTERMINAL_NAME.to_string(),
                SimplifiedExpressions::Terminals(
                    terminals_arena.roots[&nonterminal_to_terminal_id[utils::ANY_NONTERMINAL_NAME]],
                ),
            );
        }
        if except_present {
            for nonterminal in excepts.iter() {
                new_simplified_grammar.insert(
                    nonterminal.to_string(),
                    SimplifiedExpressions::Terminals(
                        terminals_arena.roots[&nonterminal_to_terminal_id[nonterminal]],
                    ),
                );
            }
        }
        let nonterminal_id_to_expression: FxHashMap<NonterminalID, SimplifiedExpressions> =
            new_simplified_grammar
                .iter()
                .map(|(key, value)| (nonterminal_to_terminal_id[key], value.clone()))
                .collect();
        SimplifiedGrammar {
            nonterminal_to_terminal_id,
            nonterminal_id_to_expression,
            terminals_trie: terminals_arena,
            nonterminal_to_token_ids,
            nonterminal_to_excluded_token_ids
        }
    }
}
