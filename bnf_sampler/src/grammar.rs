use std::sync::Arc;

use crate::sampler::PossibleTokensResult;
use crate::sampler::Sampler;
use crate::trie::TerminalsTrie;
use crate::trie::TrieNodeID;
use crate::utils;
use crate::utils::NonterminalID;
use crate::utils::VecU8Wrapper;
use crate::vocabulary::Vocabulary;
use bit_set::BitSet;
use bnf::Production;
use bnf::Term;
use itertools::Itertools;
use memchr::memmem;
use regex::Regex;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) enum U8Term {
    Terminal(Vec<u8>),
    Nonterminal(String),
}

#[derive(Clone, Debug)]
/// The struct represents the BNF schema.
pub struct Grammar {
    pub(crate) nonterminal_id_to_expression: FxHashMap<NonterminalID, SimplifiedExpressions>,
    pub(crate) nonterminal_to_terminal_id: FxHashMap<String, NonterminalID>,
    pub(crate) terminals_trie: TerminalsTrie,
    pub(crate) nonterminal_to_token_ids: FxHashMap<NonterminalID, BitSet<u32>>,
}
#[derive(Clone, Debug)]
pub(crate) enum SimplifiedExpressions {
    Expressions(FxHashSet<Vec<U8Term>>),
    Terminals(TrieNodeID),
}
impl Grammar {
    /// Create a new grammar.
    ///
    /// # Arguments
    ///
    /// * `input` - the BNF schema in text format
    /// * `vocabulary` - vocabulary is used to generate terminals for <any!> and <except!(excepted_literals)>
    /// * `stack_arena_capacity` - stack_arena_capacity is the temporary stack arena created when generating <except!(excepted_literals)>
    pub fn new(input: &str, vocabulary: Arc<Vocabulary>, stack_arena_capacity: usize) -> Arc<Self> {
        let except_present = utils::EXCEPTS_REGEX.is_match(input);
        let any_present = input.contains(&format!("<{}>", utils::ANY_NONTERMINAL_NAME));
        let mut grammar: bnf::Grammar = input.parse().unwrap();
        if any_present {
            let mut any_prod = Production::new();
            any_prod.lhs = Term::Nonterminal(utils::ANY_NONTERMINAL_NAME.to_string());
            grammar.add_production(any_prod);
        }
        let mut nonterminal_to_token_ids: FxHashMap<NonterminalID, BitSet<u32>> =
            FxHashMap::default();
        let mut excepts: FxHashSet<String> = FxHashSet::default();
        if except_present {
            for i in utils::EXCEPT_LITERAL_REGEX.find_iter(input) {
                let temp = i.as_str().to_string();
                let mut any_prod = Production::new();
                excepts.insert(temp.clone());
                any_prod.lhs = Term::Nonterminal(temp);
                grammar.add_production(any_prod);
            }
            for i in utils::EXCEPT_NONTERMINAL_REGEX.find_iter(input) {
                let temp = i.as_str().to_string();
                excepts.insert(temp);
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
                        temp_vec.push(U8Term::Terminal(utils::fix_utf8_escape(&value)));
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
        let add_tokens = |simplified_grammar: &mut FxHashMap<String, FxHashSet<Vec<U8Term>>>,
                          terminals_arena: &mut TerminalsTrie,
                          nonterminal_to_terminal_id: &FxHashMap<String, NonterminalID>,
                          nonterminal_to_token_ids: &mut FxHashMap<NonterminalID, BitSet>,
                          nonterminal: &str,
                          excepted_literal: Option<&Vec<&[u8]>>| {
            simplified_grammar.remove(nonterminal);
            let predicate = |haystack: &&VecU8Wrapper| {
                excepted_literal.is_none()
                    || excepted_literal.is_some_and(|x| {
                        x.iter().all(|x| {
                            return haystack.0 != *x
                                && memmem::find(haystack.0.as_slice(), x).is_none();
                        })
                    })
            };
            match excepted_literal {
                Some(_) => {
                    simplified_grammar.insert(
                        nonterminal.to_string(),
                        vocabulary
                            .token_to_id
                            .keys()
                            .filter(|x| predicate(x))
                            .map(|k| vec![U8Term::Terminal(k.0.clone())])
                            .collect(),
                    );
                    for (key, _) in vocabulary.token_to_id.iter() {
                        terminals_arena.add(
                            key.0.as_slice(),
                            nonterminal_to_terminal_id[nonterminal],
                            false,
                        )
                    }
                    let mut bit_set = BitSet::new();
                    bit_set.extend(vocabulary.token_to_id.iter().filter_map(|(k, token_id)| {
                        if predicate(&k) {
                            Some(*(token_id) as usize)
                        } else {
                            None
                        }
                    }));

                    nonterminal_to_token_ids
                        .insert(nonterminal_to_terminal_id[nonterminal], bit_set);
                }
                None => {
                    simplified_grammar.insert(
                        nonterminal.to_string(),
                        vocabulary
                            .token_to_id
                            .keys()
                            .map(|k| vec![U8Term::Terminal(k.0.clone())])
                            .collect(),
                    );
                    let mut bit_set = BitSet::new();
                    for (key, token_id) in vocabulary.token_to_id.iter() {
                        bit_set.insert((*token_id) as usize);
                        terminals_arena.add(
                            key.0.as_slice(),
                            nonterminal_to_terminal_id[nonterminal],
                            false,
                        )
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
                &nonterminal_to_terminal_id,
                &mut nonterminal_to_token_ids,
                utils::ANY_NONTERMINAL_NAME,
                None,
            );
        }
        fn process_valid_excepts<F: FnOnce(&str)>(regex: &Regex, nonterminal: &str, process: F) {
            let extracted = utils::extract_excepted(regex, nonterminal);
            if let Some(extracted) = extracted {
                if extracted.is_empty() {
                    panic!("{nonterminal} is invalid except!() nonterminal because the brackets contain nothing.");
                }
                process(extracted);
            }
        }
        if except_present {
            for nonterminal in excepts.iter() {
                process_valid_excepts(&utils::EXCEPT_LITERAL_REGEX, nonterminal, |extracted| {
                    let bytes = utils::fix_utf8_escape(extracted);
                    println!("{:?}", bytes);
                    add_tokens(
                        &mut simplified_grammar,
                        &mut terminals_arena,
                        &nonterminal_to_terminal_id,
                        &mut nonterminal_to_token_ids,
                        nonterminal,
                        Some(&vec![&bytes]),
                    );
                    terminals_arena.except_literal(&bytes, nonterminal_to_terminal_id[nonterminal]);
                });
            }
        }
        fn convert_u8terms_to_simplified_expressions(
            k: &str,
            v: FxHashSet<Vec<U8Term>>,
            terminals_arena: &mut TerminalsTrie,
            nonterminal_to_terminal_id: &FxHashMap<String, NonterminalID>,
        ) -> (String, SimplifiedExpressions) {
            for i in v.into_iter() {
                let value = match i.last().unwrap() {
                    U8Term::Terminal(value) => value,
                    _ => panic!("There should only be terminals."),
                };
                terminals_arena.add(value, nonterminal_to_terminal_id[k], true);
            }
            let v = SimplifiedExpressions::Terminals(
                terminals_arena.roots[&nonterminal_to_terminal_id[k]],
            );
            (k.to_string(), v)
        }
        let mut new_simplified_grammar: FxHashMap<String, SimplifiedExpressions> =
            simplified_grammar
                .iter()
                .map(|(k, v)| {
                    if v.iter().all(|terms| {
                        terms.len() == 1
                            && match terms.last().unwrap() {
                                U8Term::Terminal(_) => true,
                                U8Term::Nonterminal(_) => false,
                            }
                    }) {
                        convert_u8terms_to_simplified_expressions(
                            k,
                            v.clone(),
                            &mut terminals_arena,
                            &nonterminal_to_terminal_id,
                        )
                    } else {
                        (k.clone(), SimplifiedExpressions::Expressions(v.clone()))
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
                if utils::EXCEPT_LITERAL_REGEX.is_match(nonterminal) {
                    new_simplified_grammar.insert(
                        nonterminal.to_string(),
                        SimplifiedExpressions::Terminals(
                            terminals_arena.roots[&nonterminal_to_terminal_id[nonterminal]],
                        ),
                    );
                }
            }
        }
        let nonterminal_id_to_expression: FxHashMap<NonterminalID, SimplifiedExpressions> =
            new_simplified_grammar
                .iter()
                .map(|(key, value)| (nonterminal_to_terminal_id[key], value.clone()))
                .collect();
        let grammar = Arc::new(Grammar {
            nonterminal_to_terminal_id,
            nonterminal_id_to_expression,
            terminals_trie: terminals_arena,
            nonterminal_to_token_ids,
        });
        let mut_grammar = unsafe { &mut *(Arc::as_ptr(&grammar) as *mut Grammar) };
        if except_present {
            for nonterminal in excepts.iter() {
                process_valid_excepts(&utils::EXCEPT_NONTERMINAL_REGEX, nonterminal, |extracted| {
                    assert!(
                        mut_grammar
                            .nonterminal_to_terminal_id
                            .contains_key(extracted),
                        "{extracted} is not a valid nonterminal."
                    );
                    // println!("{nonterminal}");
                    mut_grammar.nonterminal_to_terminal_id.insert(
                        nonterminal.to_string(),
                        NonterminalID(grammar.nonterminal_id_to_expression.len()),
                    );
                    let mut temp_machine = Sampler::new(
                        grammar.clone(),
                        extracted.to_string(),
                        vocabulary.clone(),
                        stack_arena_capacity,
                        false,
                    );
                    let mut simplified_grammar: FxHashMap<String, FxHashSet<Vec<U8Term>>> =
                        FxHashMap::default();
                    match temp_machine.all_possible_next_tokens(None) {
                        PossibleTokensResult::Continue(tokens) => {
                            let iter = utils::get_tokens_from_token_ids(
                                tokens,
                                &vocabulary.id_to_token_string,
                            )
                            .map(|x| x.to_string())
                            .collect_vec();
                            add_tokens(
                                &mut simplified_grammar,
                                &mut mut_grammar.terminals_trie,
                                &mut_grammar.nonterminal_to_terminal_id,
                                &mut mut_grammar.nonterminal_to_token_ids,
                                nonterminal,
                                Some(&(iter.iter().map(|x| x.as_bytes()).collect_vec())),
                            );
                            for token in iter {
                                mut_grammar.terminals_trie.except_literal(
                                    token.as_bytes(),
                                    mut_grammar.nonterminal_to_terminal_id[nonterminal],
                                );
                            }
                            let (new_k, new_v) = {
                                let (new_k, new_v) = convert_u8terms_to_simplified_expressions(
                                    nonterminal,
                                    simplified_grammar[nonterminal].clone(),
                                    &mut mut_grammar.terminals_trie,
                                    &grammar.nonterminal_to_terminal_id,
                                );
                                (grammar.nonterminal_to_terminal_id[&new_k], new_v)
                            };
                            mut_grammar
                                .nonterminal_id_to_expression
                                .insert(new_k, new_v);
                            simplified_grammar.clear();
                        }
                        _ => panic!("{extracted} does not produce valid terminals."),
                    }
                });
            }
        }
        grammar
    }
}
