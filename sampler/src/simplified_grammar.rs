use crate::trie::TerminalsTrie;
use crate::trie::TrieNodeID;
use crate::utils;
use crate::utils::NonterminalID;
use crate::utils::VecU8Wrapper;
use bnf::{Grammar, Term};
use qp_trie::Trie;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
#[derive(Clone, Debug)]
pub struct SimplifiedGrammar {
    pub(crate) nonterminal_id_to_expression: FxHashMap<NonterminalID, SimplifiedExpressions>,
    pub(crate) nonterminal_to_terminal_id: FxHashMap<String, NonterminalID>,
    pub(crate) terminals_trie: TerminalsTrie,
}
#[derive(Clone, Debug)]
pub(crate) enum SimplifiedExpressions {
    Expressions(FxHashSet<Vec<Term>>),
    Terminals(TrieNodeID),
}
impl SimplifiedGrammar {
    pub fn new(input: &str, tokens_tree: &Trie<VecU8Wrapper, u32>) -> Self {
        let input = format!("<{}>::=' '", utils::ANY_NONTERMINAL_NAME)+input+"\n";
        let grammar: Grammar = input.parse().unwrap();
        let mut simplified_grammar: FxHashMap<String, FxHashSet<Vec<Term>>> = FxHashMap::default();
        for i in grammar.productions_iter() {
            let key = match &i.lhs {
                Term::Terminal(x) => x,
                Term::Nonterminal(x) => x,
            };
            simplified_grammar
                .entry(key.clone())
                .or_insert(FxHashSet::default())
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
        let nonterminal_to_terminal_id: FxHashMap<String, NonterminalID> = simplified_grammar
            .iter()
            .enumerate()
            .map(|(i, (key, _))| (key.clone(), NonterminalID(i)))
            .collect();
        simplified_grammar.remove(utils::ANY_NONTERMINAL_NAME);
        simplified_grammar.insert(utils::ANY_NONTERMINAL_NAME.to_string(), tokens_tree.keys().map(|k|vec![Term::Terminal(String::from_utf8(k.0.clone()).unwrap())]).collect());
        let mut terminals_arena = TerminalsTrie::new();
        
        for (key, _) in tokens_tree.iter() {
            terminals_arena.add(
                key.0.as_slice(),
                nonterminal_to_terminal_id[utils::ANY_NONTERMINAL_NAME],
            )
        }
        
        let mut new_simplified_grammar: FxHashMap<String, SimplifiedExpressions> =
            simplified_grammar
                .into_iter()
                .map(|(k, v)| {
                    if v.iter().all(|terms| {
                        terms.len() == 1
                            && match terms.last().unwrap() {
                                Term::Terminal(_) => true,
                                Term::Nonterminal(_) => false,
                            }
                    }) {
                        for i in v.into_iter() {
                            let value = match i.last().unwrap() {
                                Term::Terminal(value) => value,
                                _ => panic!("There should only be terminals."),
                            };
                            terminals_arena.add(value.as_bytes(), nonterminal_to_terminal_id[&k]);
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
        
        new_simplified_grammar.insert(
            utils::ANY_NONTERMINAL_NAME.to_string(),
            SimplifiedExpressions::Terminals(
                terminals_arena.roots[&nonterminal_to_terminal_id[utils::ANY_NONTERMINAL_NAME]],
            ),
        );
        
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
