use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use crate::utils::NonterminalID;
use bnf::{Grammar, Term};
#[derive(Clone, Debug)]
pub struct SimplifiedGrammar {
    pub nonterminal_id_to_expression: FxHashMap<NonterminalID, FxHashSet<Vec<Term>>>,
    pub nonterminal_to_terminal_id: FxHashMap<String, NonterminalID>,
}

impl SimplifiedGrammar {
    pub fn new(input: &str) -> Self {
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
        let nonterminal_id_to_expression: FxHashMap<NonterminalID, FxHashSet<Vec<Term>>> = simplified_grammar
            .iter()
            .map(|(key, value)| (nonterminal_to_terminal_id[key], value.clone()))
            .collect();
        SimplifiedGrammar {
            nonterminal_to_terminal_id,
            nonterminal_id_to_expression,
        }
    }
}