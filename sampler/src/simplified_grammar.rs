use crate::trie::TerminalsTrie;
use crate::trie::TrieNodeID;
use crate::utils;
use crate::utils::NonterminalID;
use crate::utils::VecU8Wrapper;
use bnf::Production;
use bnf::{Grammar, Term};
use qp_trie::Trie;
use regex::Regex;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) enum U8Term {
    Terminal(Vec<u8>),
    Nonterminal(String),
}

#[derive(Clone, Debug)]
pub struct SimplifiedGrammar {
    pub(crate) nonterminal_id_to_expression: FxHashMap<NonterminalID, SimplifiedExpressions>,
    pub(crate) nonterminal_to_terminal_id: FxHashMap<String, NonterminalID>,
    pub(crate) terminals_trie: TerminalsTrie,
}
#[derive(Clone, Debug)]
pub(crate) enum SimplifiedExpressions {
    Expressions(FxHashSet<Vec<U8Term>>),
    Terminals(TrieNodeID),
}
impl SimplifiedGrammar {
    pub fn new(input: &str, tokens_tree: &Trie<VecU8Wrapper, u32>) -> Self {
        let except_present = utils::EXCEPT_NONTERMINAL_REGEX.is_match(input);
        let any_present = input.contains(&format!("<{}>", utils::ANY_NONTERMINAL_NAME))||except_present;
        let mut grammar: Grammar = input.parse().unwrap();
        if any_present {
            let mut any_prod = Production::new();
            any_prod.lhs = Term::Nonterminal(utils::ANY_NONTERMINAL_NAME.to_string());
            grammar.add_production(any_prod);
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
        if any_present{
            simplified_grammar.remove(utils::ANY_NONTERMINAL_NAME);
            simplified_grammar.insert(
                utils::ANY_NONTERMINAL_NAME.to_string(),
                tokens_tree
                    .keys()
                    .map(|k| vec![U8Term::Terminal(k.0.clone())])
                    .collect(),
            );
            for (key, _) in tokens_tree.iter() {
                terminals_arena.add(
                    key.0.as_slice(),
                    nonterminal_to_terminal_id[utils::ANY_NONTERMINAL_NAME],
                )
            }
        }
        if except_present
        {
            let temp: Vec<(String, FxHashSet<Vec<U8Term>>)> = vec![];
            for (_, expression) in simplified_grammar.iter()
            {
                for terms in expression.iter()
                {
                    for term in terms.iter()
                    {
                        if let U8Term::Nonterminal(nonterminal)=term
                        {
                            fn process_valid_result<F: FnOnce(&str)>(regex: &Regex,nonterminal: &str, process: F)
                            {
                                let extracted = utils::extract_excepted(regex, &nonterminal);
                                if let Some(extracted) = extracted
                                {
                                    if extracted.is_empty()
                                    {
                                        panic!("{nonterminal} is invalid except!() nonterminal because the brackets contain nothing.");
                                    }
                                    process(extracted);
                                }
                            }
                            process_valid_result(&utils::EXCEPT_LITERAL_REGEX, nonterminal, |extracted|{
                                
                            });
                        }
                    }
                }
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
                            if value.contains(&240) {
                                println!("{:?}", value);
                            }
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

        let nonterminal_id_to_expression: FxHashMap<NonterminalID, SimplifiedExpressions> =
            new_simplified_grammar
                .iter()
                .map(|(key, value)| (nonterminal_to_terminal_id[key], value.clone()))
                .collect();
        SimplifiedGrammar {
            nonterminal_to_terminal_id,
            nonterminal_id_to_expression,
            terminals_trie: terminals_arena,
        }
    }
}
