# bnf_sampler
[![crates.io](https://img.shields.io/crates/v/bnf_sampler)](https://crates.io/crates/bnf_sampler)
This is a language model sampler that uses recursive descent algorithm to ensure tokens produced by a large language model follow a Backus Naur Form schema.
## How to try it?
1. [Install Rust](https://rustup.rs/).
2. Run `cargo run --release` to run the console_playground program. Your console input is considered as tokens. 

Or you can download the pre-compiled binaries from the release page and run

## Use in Your Project
To use in your own rust project, simply add `bnf_sampler = "0.1.2"` as a dependency in your `Cargo.toml`.