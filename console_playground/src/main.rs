use bnf_sampler::sampler::{PossibleTokensResult, Sampler};
use bnf_sampler::{grammar, utils};
use clap::Parser;
use std::time::Instant;
use std::{fs, vec};
/// Command line arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// to display stacks in the sampler.
    #[arg(short, long, default_value_t = false)]
    stacks_display: bool,
    /// to display all possible tokens. WARNING: it can be slow when there are a lot of possible tokens.
    #[arg(short, long, default_value_t = true)]
    possible_tokens_display: bool,
    /// to display input in bytes.
    #[arg(short, long, default_value_t = false)]
    input_display: bool,
    /// set the arena capacity.
    #[arg(short, long, default_value_t = 1024*1024)]
    arena_capacity: usize,
    /// set the temp arena capacity used to expand each except!([nonterminal]).
    #[arg(short, long, default_value_t = 1024)]
    temp_arena_capacity: usize,
    /// enable stack to bytes cache. When a nonterminal directly expands to a lot of nonterminals and terminals, it may be slow.
    #[arg(short, long, default_value_t = true)]
    bytes_cache: bool,
    /// set the initial nonterminal.
    #[arg(short = 'n', long, default_value = "start")]
    start_nonterminal: String,
}

fn main() {
    let args = Args::parse();
    println!("{:?}", args);
    let input =
        fs::read_to_string("./assets/grammar.bnf").expect("./assets/grammar.bnf should exist.");
    let (tree, map) = utils::read_rwkv_world_vocab("./assets/vocab.txt");
    let grammar =
        grammar::Grammar::new(&input, &tree, &map, args.temp_arena_capacity);
    let mut machine = Sampler::new(
        &grammar,
        &args.start_nonterminal,
        &tree,
        args.arena_capacity,
        args.bytes_cache,
    );
    if args.stacks_display
    {
        println!("Stacks: {}", machine);
    }

    if let PossibleTokensResult::Continue(result) = machine.all_possible_next_tokens(None) {
        let result: Vec<&str> = utils::get_tokens_from_token_ids(result, &map).collect();
        if args.possible_tokens_display
        {
            println!("Possible tokens: {:?}", result);
        }
    }
    else {
        panic!("An internal eror happens.")
    }

    let mut times: Vec<f64> = vec![];
    loop {
        println!("Input a token: ");
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .expect("Input should exist");
        let input = utils::fix_utf8_escape(input.trim());
        if args.input_display
        {
            println!("Input: {:?}", input);
        }
        let now = Instant::now();
        let result = machine.all_possible_next_tokens(Some(&input));
        let end = now.elapsed();
        times.push(end.as_secs_f64());
        println!("Time used: {:?}", end);
        let result: Vec<&str> = match result {
            PossibleTokensResult::Continue(result) => {
                utils::get_tokens_from_token_ids(result, &map).collect()
            }
            PossibleTokensResult::InputTokensRejected => {
                println!("Invalid input.");
                break;
            }
            PossibleTokensResult::End => {
                println!("One termination path is reached.");
                break;
            }
        };
        if args.possible_tokens_display
        {
            println!("Possible tokens: {:?}", result);
        }
        if args.stacks_display
        {
            println!("Stacks: {}", machine);
        }

    }
    println!(
        "Average time taken for each token: {}",
        times.iter().sum::<f64>() / times.len() as f64
    );
}
