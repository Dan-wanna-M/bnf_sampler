# bnf_sampler

[![crates.io](https://img.shields.io/crates/v/bnf_sampler)](https://crates.io/crates/bnf_sampler)[![docs.rs](https://docs.rs/web-rwkv/badge.svg)](https://docs.rs/bnf_sampler)

**This crate is deprecated. Use [kbnf](https://crates.io/crates/kbnf) instead.**

This is a language model sampler that uses recursive descent algorithm to ensure tokens produced by a large language model follow a schema based on [Backus Naur Form(BNF)](https://en.wikipedia.org/wiki/Backus%E2%80%93Naur_form).

Tokens must be encoded in UTF-8.

## Features

1. Very fast.
2. Compatible with any type of vocabulary of language model.
3. Easy to use.

## How to try it?

1. [Install Rust](https://rustup.rs/).
2. Run `cargo run --release` to run the console_playground program. Your console input is considered as tokens. Try `cargo run --release -- --help` to check all possible command line configurations. Modify `assets/grammar.bnf`` to change schema. (see Grammar schema section and Listing possible tokens section)

Or you can download the pre-compiled binaries from the release page and run.

## Use in Your Project

To use in your own rust project, simply add `bnf_sampler = "0.3.1"` as a dependency in your `Cargo.toml`.

## Examples

Copy paste one of these examples into `assets/grammar.bnf` to try by yourself.

### DNA Sequence

```BNF
<start>::=<sequence>
<sequence>::=<base>|<base><sequence>
<base>::="A"|"C"|"G"|"T"
```

### Integer

```BNF
<start>::=<integer>
<integer>::=<positive_digit><digits>|'0'
<digits>::=<digit>|<digit><digits>
<digit>::='0'|<positive_digit>
<positive_digit>::="1"|"2"|"3"|"4"|"5"|"6"|"7"|"8"|"9"
```

### String

```BNF
<start>::=<string>
<string>::='"'<content>'"'
<content>::=<except!([escaped_literals])>|<except!([escaped_literals])><content>|'\\"'<content>|'\\"'
<escaped_literals>::='\t'|'\n'|'\r'|'"'
```

## Grammar schema

In this project, a slightly modified version of BNF is used. The key differences are:

- Left recursion is not supported. (plan to support in the future.)
- Consecutive terminals are merged into one terminal. e.g. `'b''o''y'` becomes `'boy'`.
- `<any!>` is added as a special nonterminal which matches any token in the given vocabulary.
- `<except!(excepted_literals)>` is added as a special nonterminal which:
  - matches any token in the given vocabulary that does not contain any of the `excepted_literals`.
  - matches the slice `token[:the beginning of the first appearing excepted literal]` if the token contains any of the `excepted_literals` and at least one possible prefix of the slice equals any token in the given vocabulary.

  - `<except!(excepted_literals)>` has two forms:
    - `<except!('excepted_literal')>` or `<except!("excepted_literal")>` which specifies one and only one `excepted_literal`.
      - e.g. `<except!('ar')>` specifies `ar` as the excepted_literal. It will match `c` in `card`(given `c` is one valid token), and pass `ard` to next term in grammar.
    - `<except!([nonterminal])>` which specifies any token accepted by the nonterminal belongs to excepted_literals.
      - **WARNING**: the nonterminal itself and all the nonterminals expanded from the nonterminal should not be `<except!([nonterminal])>`, or the program may panic.
      - e.g.  given `<abc> ::= 'a'|'b'|'c'`, `<sequence>::= <abc>|<abc><sequence>` `<except!([sequence])>` specifies all tokens which only contains `a`,`b` and `c` as excepted_literals.

- In terminals and `excepted_literals`, escape sequences like `\t`, `\r`, `\n`, `\u1234` are recognized and converted to corresponding UTF-8 bytes. `\x<hex><hex>`, like `\x00`, are converted to raw bytes however.

## Listing possible tokens

The possible tokens listed are the tokens that can be accepted by the sampler in its current state.
The following rule defines whether a token is listed in the return value of `Sampler::all_possible_tokens` with a given BNF:

- The sampler has not terminated or gets into an invalid state. In other words,  the current input token can be accepted by the sampler, and no path exists such that all the terminals and nonterminals are consumed in the path.

  - e.g. With `<start>::=<A><B><C>, <A>::='boy', <B>::='next', <C>::='door'`,`<start>` will create a sampler that terminates after `boy`,`next`,`door` are inputed in this exact sequence, and goes into an invalid state otherwise.
  - e.g. `<sequence>::=<any!>|<any!><sequence>` will create a sampler that terminates after any input token because of the path where `<sequence>` become `<any!>`. In other words, `<any!>` is the only nonterminal in the path and is consumed.

- For a given terminal, only the longest possible token is listed.

  - e.g. terminal `'apple'` will only list token `apple` given that token exists. Tokens like `a`,`ap`,`app` will not be listed.

- A terminal can be partially matched and consumed.
  - e.g. terminal `apple66666` will only list token `apple`(see rule 1), given `apple66666` is not a valid token. After `apple` is inputed, the terminal becomes `66666` because the prefix `apple` is matched.
- A token can be matched by multiple terminals on byte level.
  - e.g. Given `<byte> ::= '\xf0'|'\xa0'|'xb0'`, `<sequence>::= <byte>|<byte><sequence>`,`<sequence>` will list any token whose UTF-8 encoding only contains byte value `240`,`160` and `176`.

## Roadmap

1. Add more examples and ready-to-use BNF schema.
2. Add more tests.
3. Test more advanced parser algorithms(like Earley) and see whether the speed can be improved.
4. Python binding.
5. Huggingface transformers integration.
