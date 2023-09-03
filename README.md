# bnf_sampler
[![crates.io](https://img.shields.io/crates/v/bnf_sampler)](https://crates.io/crates/bnf_sampler)

This is a language model sampler that uses recursive descent algorithm to ensure tokens produced by a large language model follow a schema based on [Backus Naur Form(BNF)](https://en.wikipedia.org/wiki/Backus%E2%80%93Naur_form).
## How to try it?
1. [Install Rust](https://rustup.rs/).
2. Run `cargo run --release` to run the console_playground program. Your console input is considered as tokens. Try `cargo run --release -- --help` to check all possible command line configurations. Modify assets/grammar.bnf to change schema. (see [Grammar schema](#grammar-schema))


Or you can download the pre-compiled binaries from the release page and run.

## Use in Your Project
To use in your own rust project, simply add `bnf_sampler = "0.1.2"` as a dependency in your `Cargo.toml`.

## Grammar schema {#grammar-schema}
In this project, a slightly modified version of BNF is used. The key differences are:
- Left recursion is not supported. (plan to support in the future.)
- `<any!>` is added as a special nonterminal which matches any token in the given vocabulary.
- `<except!(excepted_literals)>` is added as a special nonterminal which:
    - matches any token in the given vocabulary that does not contain any of the `excepted_literals`.
    - matches the slice `token[:the beginning of the first appearing excepted literal]` if the token contains any of the `excepted_literals` and the slice equals any token in the given vocabulary.

    `<except!(excepted_literals)>` has two forms:
    - `<except!('excepted_literal')>` or `<except!("excepted_literal")>` which specifies one and only one `excepted_literal`. 
    e.g. `<except!('?')>` specifies `?` as the excepted_literal.
    - `<except!([nonterminal])>` which specifies any token accepted by the nonterminal belongs to excepted_literals. 
    e.g.  given `<abc> ::= 'a'|'b'|'c'`, `<sequence>::= <abc>|<abc><sequence>` `<except!([sequence])>` specifies all tokens which only contains `a`,`b` and `c` as excepted_literals.
    Warning: the nonterminal itself and all the nonterminals expanded from the nonterminal should not be `<except!([nonterminal])>`, or the program may panic.
-   In terminals and `excepted_literals`, escape sequences like `\t`, `\r`, `\n`, `\u1234` are recognized and converted to corresponding UTF-8 bytes. `\x<hex><hex>`, like `\x00`, are converted to raw bytes however.