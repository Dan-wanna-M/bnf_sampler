use bnf::{Grammar, Term};
use qp_trie::Trie;
use std::borrow::Borrow;
use std::{collections::*, time::Instant};
use std::{fs, vec};

use crate::sampler::SimplifiedGrammar;
mod utils;
mod sampler;

fn main() {
    let input = fs::read_to_string("./grammar.bnf").expect("grammar.bnf should exist.");
    let input = String::from_utf8(utils::fix_utf8_escape(&input)).unwrap();
    let (tree, map) = utils::read_world_vocab("vocab.txt");
    let grammar = SimplifiedGrammar::new(&input);
    let mut machine = sampler::PushDownAutomata::new(&grammar, "dna", tree);
    let result: Vec<&str> = machine
        .all_possible_next_tokens(None)
        .unwrap()
        .into_iter()
        .map(|x| map[&x].as_str())
        .collect();
    println!("{:?}", result);
    let mut times: Vec<f64> = vec![];
    // println!("{:?}", machine.stacks);
    let now = Instant::now();
    machine.all_possible_next_tokens(Some("statistics".as_bytes()));
    let end = now.elapsed();
    println!("Time used: {:?}", end);
    return;
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
        // println!("{:?}", machine);
        let end = now.elapsed();
        times.push(end.as_secs_f64());
        println!("Time used: {:?}", end);
        // println!("{:?}", result);
        if result.is_empty() {
            break;
        }
    }
    println!("{}", times.iter().sum::<f64>() / times.len() as f64);
}
