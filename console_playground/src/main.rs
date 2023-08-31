use bnf_sampler::sampler::{PossibleTokensResult, Sampler};
use bnf_sampler::{simplified_grammar, utils};
use std::time::Instant;
use std::{fs, vec};
fn main() {
    let input = fs::read_to_string("./grammar.bnf").expect("grammar.bnf should exist.");
    let input = String::from_utf8(utils::fix_utf8_escape(&input)).unwrap();
    let (tree, map) = utils::read_world_vocab("vocab.txt");
    let grammar = simplified_grammar::SimplifiedGrammar::new(&input, &tree, &map, 1024);
    let mut machine = Sampler::new(&grammar, "start", &tree, 1024 * 1024, true);
    // println!("{:?}", machine.stacks);
    if let PossibleTokensResult::Continue(result) = machine.all_possible_next_tokens(None) {
        let result: Vec<&str> = result.iter().map(|x| map[&(x as u32)].as_str()).collect();
        // println!("{:?}", result);
    }

    let mut times: Vec<f64> = vec![];
    // machine.all_possible_next_tokens(Some("我是土豆".as_bytes()));
    // println!("{:?}", machine.stacks);
    let now = Instant::now();
    
    machine.all_possible_next_tokens(Some("我热爱土豆".as_bytes()));
    machine.all_possible_next_tokens(Some("我爱你".as_bytes()));
    machine.all_possible_next_tokens(Some("你是一个一个".as_bytes()));
    
    let end = now.elapsed();
    println!("Time used: {:?}", end / 3);
    // return;
    loop {
        // println!("{:?}",grammar.nonterminal_to_terminal_id);
        println!("Input a token: ");
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .expect("Input should exist");
        let input = utils::fix_utf8_escape(input.trim());
        println!("{:?}", input);
        let now = Instant::now();
        let result = machine.all_possible_next_tokens(Some(&input));
        // println!("{:?}", machine);
        let end = now.elapsed();
        times.push(end.as_secs_f64());
        println!("Time used: {:?}", end);
        let result: Vec<&str> = match result {
            PossibleTokensResult::Continue(result) => {
                result.iter().map(|x| map[&(x as u32)].as_str()).collect()
            }
            PossibleTokensResult::Failed => {
                println!("Invalid input.");
                break;
            }
            PossibleTokensResult::End => {
                println!("One termination path is reached.");
                break;
            }
        };
        // println!("{:?}", result);
        println!("{:?}", machine.stacks.clone());
    }
    println!("{}", times.iter().sum::<f64>() / times.len() as f64);
}
