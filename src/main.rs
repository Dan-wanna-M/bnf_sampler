use std::{collections::*, ops::Range, f32::consts, time::Instant};
use bnf::{Expression, Grammar, Production, Term};
use indextree::*;

pub mod Utils
{
    use std::collections::{HashMap, HashSet};
    use bnf::{Expression, Grammar, Term};

    pub fn SimplifyGrammarTree(grammar: &Grammar) -> HashMap<&str, HashSet<Vec<&Term>>> {
        let mut simplified_grammar: HashMap<&str, HashSet<Vec<&Term>>> = HashMap::new();
        for i in grammar.productions_iter() {
            let key = match &i.lhs {
                Term::Terminal(x)=>x,
                Term::Nonterminal(x)=>x
            };
            simplified_grammar
                .entry(key)
                .or_insert(HashSet::new())
                .extend(i.rhs_iter().map(|x|x.terms_iter().collect()));
        }
        simplified_grammar
    }
}
#[derive(PartialEq)]
#[derive(Clone)]
#[derive(Debug)]
pub enum StackItem<'a>
{
    Nonterminal(&'a str),
    Terminal(&'a str),
    Byte(u8)
}

pub struct PushDownAutomata<'a> {
    stacks: Vec<Vec<StackItem<'a>>>,
    grammar: HashMap<&'a str, HashSet<Vec<&'a Term>>>,
}



impl<'a> PushDownAutomata<'a> {
    /// Create a new PushDownAutomata with simplified grammar
    pub fn new(grammar: &'a Grammar, start_term: &'a Term) -> PushDownAutomata<'a> {
        let start_nonterminal = match start_term {
            Term::Nonterminal(x)=>x,
            _=>panic!("Start term should be nonterminal")
        };
        let mut stacks = Vec::new();
        stacks.push(vec![StackItem::Nonterminal(start_nonterminal)]);
        PushDownAutomata {
            stacks,
            grammar: Utils::SimplifyGrammarTree(grammar),
        }
    }

    pub fn all_possible_next_string_iter(&mut self)->HashSet<&str>
    {
        let mut result: HashSet<&str> = HashSet::new();
        let initial_len = self.stacks.len();
        let mut indices_for_removal: Vec<usize> = vec![];
        for i in 0..initial_len
        {
            match self.stacks[i].last() {
                Some(value)=>
                {
                    match value {
                        StackItem::Nonterminal(_value)=>
                        {
                            let mut stack = self.stacks[i].clone();
                            Self::expand_nonterminal_to_all_possible_terminals(&mut self.stacks,&mut stack, &self.grammar, 0);
                            indices_for_removal.push(i);
                        },
                        _ => continue
                    }
                },
                None=>
                {
                    indices_for_removal.push(i);
                    continue;
                }
            };
        }
        for i in indices_for_removal.into_iter()
        {
            self.stacks.swap_remove(i);
        }
        for i in self.stacks.iter()
        {
            result.insert(match i.last().unwrap() {
                StackItem::Terminal(value)=>&value,
                _=>continue
            } );
        }
        result
    }

    fn expand_nonterminal_to_all_possible_terminals(stacks: &mut Vec<Vec<StackItem<'a>>>, stack:& mut Vec<StackItem<'a>>,
     grammar: &HashMap<&'a str, HashSet<Vec<&'a Term>>>, layer: i8)
    {
        let top;
        {
            top = match stack.pop() {
                Some(value)=>
                {
                    match value {
                        StackItem::Nonterminal(value2)=>
                        {
                            value2
                        },
                        _ => 
                        {
                            stack.push(value);
                            // println!("{:?}", stack);
                            stacks.push(stack.clone());
                            return
                        }
                    }
                },
                None=>
                {
                    return
                }
            };
        }
        for expression in grammar[top].iter()
        {
            let count = expression.len();
            // let mut temp_stack = stack.clone();
            for term in expression.iter().rev()
            {
                stack.push(match term {
                    Term::Terminal(value)=>StackItem::Terminal(&value),
                    Term::Nonterminal(value)=>StackItem::Nonterminal(&value)
                });
            }
            // println!("{layer}start=>{:?}", stack);
            Self::expand_nonterminal_to_all_possible_terminals(stacks,stack, grammar, layer+1);
            for _i in 0..count
            {
                stack.pop();
            }
            // println!("{layer}end=>{:?}", stack);
        }
        stack.push(StackItem::Nonterminal(top));
    }
    pub fn accept_a_terminal(&mut self, terminal:&str)
    {
        let mut i =0;
        loop {
            if i==self.stacks.len()
            {
                break;
            }
            let top = self.stacks[i].pop().expect("No stack should be empty.");
            match top {
                StackItem::Terminal(value)=>{
                    if value != terminal
                    {
                        // println!("{value}, {:#?}", self.stacks[i]);
                        self.stacks.swap_remove(i);
                    }
                    else {
                        i+=1;
                    }
                },
                _=>panic!("The top element in every stack should be terminal.")
            }
        }
    }
}

fn main() {
    let input = "<dna> ::= <sequence><digit>
    <sequence> ::= <base>|<base><sequence>
    <digit> ::= '1'|'2'|'3'|'4'
    <base> ::= 'A'|'C'|'G'|'T'";
    let grammar: Grammar = input.parse().unwrap();
    // println!("{:#?}", Utils::SimplifyGrammarTree(&grammar));
    let binding = Term::Nonterminal("dna".to_string());
    let mut machine = PushDownAutomata::new(&grammar, &binding);
    let mut result = machine.all_possible_next_string_iter();
    let mut times: Vec<f64> = vec![];
    loop {
        // println!("{:?}", machine.stacks);
        println!("Input a terminal: ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).expect("Input should exist");
        let now = Instant::now();
        machine.accept_a_terminal(input.trim());
        // println!("{:#?}", machine.stacks);
        result = machine.all_possible_next_string_iter();
        let end = now.elapsed();
        times.push(end.as_secs_f64());
        println!("Time used: {:?}", end);
        println!("{:#?}", result);
        if result.is_empty()
        {
            break;
        }
    }
    println!("{}", times.iter().sum::<f64>()/times.len() as f64);
}
