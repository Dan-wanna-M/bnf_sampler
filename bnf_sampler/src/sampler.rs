use crate::grammar::Grammar;
use crate::grammar::SimplifiedExpressions;
use crate::grammar::U8Term;
use crate::stack::BufferArena;
use crate::stack::FixedBuffer;
use crate::trie::TerminalsTrie;
use crate::trie::TerminalsTrieIter;
use crate::trie::TrieNodeID;
use crate::utils::NonterminalID;
use crate::utils::U8ArrayWrapper;
use crate::vocabulary::Vocabulary;
use anyhow::anyhow;
use anyhow::Error;
use anyhow::Ok;
use bit_set::BitSet;
use qp_trie::Trie;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::collections::hash_map::Entry;
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Instant;
use std::vec;

const INVALID_INDEX: i32 = -1;

unsafe impl Send for Sampler {}

unsafe impl Sync for Sampler {}

#[derive(PartialEq, Clone, Debug, Copy, Eq, Hash)]
enum StackItem {
    Nonterminal(NonterminalID),
    Terminal(*const [u8]),
    Terminals(TrieNodeID),
}
#[derive(Clone, Debug)]
pub struct Sampler {
    stacks: Vec<Vec<StackItem>>,
    grammar: Arc<Grammar>,
    tokens_buffer: Vec<(U8ArrayWrapper, u32)>,
    vocabulary: Arc<Vocabulary>,
    stack_arena: BufferArena<StackItem>,
    stacks_to_token_ids: FxHashMap<Vec<Vec<StackItem>>, BitSet<u32>>,
    start_nonterminal: String,
    token_ids: BitSet<u32>,
    stack_to_bytes_cache_enabled: bool,
}
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AcceptTokenResult {
    Continue,
    End,
    Failed,
}
#[derive(Debug, PartialEq, Clone, Eq)]
pub enum PossibleTokensResult<'a> {
    /// contains all possible token ids
    Continue(&'a BitSet<u32>),
    /// the sampler successfully terminates
    End,
    InputTokenRejected,
}

#[derive(Debug)]
enum BytesMatchResults {
    Failed,
    Matches(Vec<BytesMatchResult>),
}
#[derive(Debug)]
struct BytesMatchResult {
    remaining_bytes_start: i32,
    stack_offset: u32,
    modified_item_at_offset: Option<StackItem>,
}

enum TokensIterType<'a> {
    Flat(std::slice::Iter<'a, (U8ArrayWrapper, u32)>),
    SinglePrefix(qp_trie::Iter<'a, U8ArrayWrapper, u32>),
    MultiplePrefixs(
        (
            TerminalsTrieIter<'a>,
            Option<qp_trie::Iter<'a, U8ArrayWrapper, u32>>,
        ),
    ),
}

struct BufferOrTreeIter<'a> {
    tokens_buffer_iter: TokensIterType<'a>,
    tokens_tree: &'a Trie<U8ArrayWrapper, u32>,
}

impl<'a> BufferOrTreeIter<'a> {
    pub fn new(
        tokens_buffer: &'a [(U8ArrayWrapper, u32)],
        tokens_tree: &'a Trie<U8ArrayWrapper, u32>,
        trie: &'a TerminalsTrie,
        current_top: StackItem,
    ) -> Self {
        let tokens_buffer_iter = match current_top {
            StackItem::Terminal(terminal) => TokensIterType::SinglePrefix(
                tokens_tree.iter_prefix(tokens_tree.longest_common_prefix(unsafe { &*terminal })),
            ),
            StackItem::Terminals(node_id) => {
                let node = trie.get(node_id);
                if node.children.len() > (u8::MAX / 2).into() {
                    TokensIterType::Flat(tokens_buffer.iter())
                } else {
                    TokensIterType::MultiplePrefixs((trie.iter(node_id), None))
                }
            }
            StackItem::Nonterminal(_) => panic!("No nonterminals should be here."),
        };
        BufferOrTreeIter {
            tokens_buffer_iter,
            tokens_tree,
        }
    }
}

impl<'a> Iterator for BufferOrTreeIter<'a> {
    type Item = (&'a U8ArrayWrapper, &'a u32);

    fn next(&mut self) -> Option<Self::Item> {
        let result;
        match &mut self.tokens_buffer_iter {
            TokensIterType::Flat(buffer_iter) => {
                result = buffer_iter.next().map(|(k, v)| (k, v));
            }
            TokensIterType::SinglePrefix(trie_iter) => {
                result = trie_iter.next();
            }
            TokensIterType::MultiplePrefixs((keys, trie_iter)) => match trie_iter {
                None => {
                    result = keys.next().and_then(|key| {
                        let mut iter = self
                            .tokens_tree
                            .iter_prefix(self.tokens_tree.longest_common_prefix(key));
                        let temp = iter.next();
                        *trie_iter = Some(iter);
                        temp
                    });
                }
                Some(trie_iter) => {
                    result = trie_iter.next().or_else(|| {
                        keys.next().and_then(|key| {
                            *trie_iter = self
                                .tokens_tree
                                .iter_prefix(self.tokens_tree.longest_common_prefix(key));
                            trie_iter.next()
                        })
                    });
                }
            },
        };
        result
    }
}

impl std::fmt::Display for Sampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The `f` value implements the `Write` trait, which is what the
        // write! macro is expecting. Note that this formatting ignores the
        // various flags provided to format strings.
        write!(f, "stacks: {:?}", self.stacks)
    }
}

impl Sampler {
    /// Create a new grammar.
    ///
    /// # Arguments
    ///
    /// * `grammar` - the grammar for this sampler
    /// * `start_nonterminal` - the starting point of the BNF schema
    /// * `vocabulary` - the vocabulary for this sampler
    /// * `stack_arena_capacity` - the arena capacity. This value depends on how long and complex the BNF schema is, and the maximum token length in bytes.
    /// * `stack_to_bytes_cache_enabled` - a cache that speeds up certain types of except!(excepted_literals) when the BNF schema is not very long.
    pub fn new(
        grammar: Arc<Grammar>,
        start_nonterminal: String,
        vocabulary: Arc<Vocabulary>,
        stack_arena_capacity: usize,
        stack_to_bytes_cache_enabled: bool,
    ) -> Result<Self, Error> {
        let stacks = vec![vec![StackItem::Nonterminal(
            *grammar
                .nonterminal_to_terminal_id
                .get(&start_nonterminal)
                .ok_or(anyhow!(
                    "Start_nonterminal {start_nonterminal} is not defined in the BNF schema."
                ))?,
        )]];
        let token_ids: BitSet<u32> = BitSet::with_capacity(u16::MAX.into());
        let stacks_to_token_ids = FxHashMap::default();
        let tokens_buffer =
            Vec::from_iter(vocabulary.token_to_id.iter().map(|(k, v)| (k.clone(), *v)));
        Ok(Sampler {
            stacks,
            grammar,
            vocabulary,
            tokens_buffer,
            stacks_to_token_ids,
            token_ids,
            stack_arena: BufferArena::with_capacity(stack_arena_capacity),
            stack_to_bytes_cache_enabled,
            start_nonterminal,
        })
    }

    pub fn reset(&mut self) {
        self.stacks = vec![vec![StackItem::Nonterminal(
            self.grammar.nonterminal_to_terminal_id[&self.start_nonterminal],
        )]];
    }

    pub fn all_possible_next_tokens(
        &mut self,
        input_token_id: Option<u32>,
    ) -> Result<PossibleTokensResult, Error> {
        // let now = Instant::now();
        self.token_ids.clear();
        match self.accept_a_token(input_token_id)? {
            AcceptTokenResult::End => Ok(PossibleTokensResult::End),
            AcceptTokenResult::Failed => Ok(PossibleTokensResult::InputTokenRejected),
            AcceptTokenResult::Continue => {
                let mut cached_node_id = FxHashSet::default();
                for stack in self.stacks.iter() {
                    if let StackItem::Terminals(node_id) =
                        stack.last().expect("The stack should not be empty.")
                    {
                        if cached_node_id.contains(node_id) {
                            continue;
                        }
                        if let Some((k, _)) = self
                            .grammar
                            .terminals_trie
                            .roots
                            .iter()
                            .find(|(_, v)| **v == *node_id)
                        {
                            if let Some(x) = self.grammar.nonterminal_to_token_ids.get(k) {
                                self.token_ids.extend(x.iter());
                                // println!("{} tokens are skipped.", self.token_ids.len());
                                cached_node_id.insert(*node_id);
                            }
                        }
                    }
                }
                let entry = self.stacks_to_token_ids.entry(self.stacks.clone());
                match entry {
                    Entry::Occupied(value) => Ok(PossibleTokensResult::Continue(value.into_mut())),
                    Entry::Vacant(entry) => {
                        let mut stack_to_bytes_cache: FxHashMap<
                            (FixedBuffer<StackItem>, Box<[u8]>),
                            bool,
                        > = FxHashMap::default();
                        for stack in self.stacks.iter() {
                            let _now = Instant::now();
                            let iter = BufferOrTreeIter::new(
                                &self.tokens_buffer,
                                &self.vocabulary.token_to_id,
                                &self.grammar.terminals_trie,
                                *stack.last().unwrap(),
                            );

                            for (token, token_id) in iter {
                                if self.token_ids.contains(*token_id as usize) {
                                    continue;
                                }
                                let arena = unsafe {
                                    NonNull::new_unchecked(
                                        &mut self.stack_arena as *mut BufferArena<StackItem>,
                                    )
                                };
                                let mut temp_stack =
                                    self.stack_arena.allocate_a_stack(stack.len())?;
                                temp_stack.copy_from_slice(stack.as_slice());
                                let mut cache;
                                if self.stack_to_bytes_cache_enabled {
                                    cache = Some(&mut stack_to_bytes_cache);
                                } else {
                                    cache = None;
                                }
                                let result = Self::find_stacks_matching_bytes::<
                                    fn(&[Option<StackItem>], Option<StackItem>),
                                >(
                                    arena,
                                    &mut temp_stack,
                                    &self.grammar,
                                    Some(&token.0[..]),
                                    0,
                                    false,
                                    &mut cache,
                                    &mut None,
                                )?;
                                if result {
                                    self.token_ids.insert(*token_id as usize);
                                }
                                self.stack_arena.clear();
                                // println!("failed: {:?}",failed_prefixs);
                            }
                            // println!("stack: {:?}, {:?}", stack, now.elapsed());
                            // println!("{:?}",accepted_prefixs);
                        }
                        entry.insert(self.token_ids.clone());
                        Ok(PossibleTokensResult::Continue(&self.token_ids))
                    }
                }
            }
        }
    }
    pub fn accept_a_token(&mut self, token_id: Option<u32>) -> Result<AcceptTokenResult, Error> {
        let mut find_stacks_matching_bytes = |bytes| {
            let len = self.stacks.len();
            let mut accepted = false;
            for i in 0..len {
                let arena = unsafe {
                    NonNull::new_unchecked(&mut self.stack_arena as *mut BufferArena<StackItem>)
                };
                let mut stack = self.stack_arena.allocate_a_stack(self.stacks[i].len())?;
                stack.copy_from_slice(&self.stacks[i]);
                let stack_to_bytes_cache: &mut FxHashMap<
                    (FixedBuffer<StackItem>, Box<[u8]>),
                    bool,
                > = &mut FxHashMap::default();
                match stack.last() {
                    Some(_) => {
                        let mut cache;
                        if self.stack_to_bytes_cache_enabled {
                            cache = Some(stack_to_bytes_cache);
                        } else {
                            cache = None;
                        }
                        accepted |= Self::find_stacks_matching_bytes(
                            arena,
                            &mut stack,
                            &self.grammar,
                            bytes,
                            0,
                            true,
                            &mut cache,
                            &mut Some(
                                |temp_stack: &[Option<StackItem>], top: Option<StackItem>| {
                                    let mut new_vec = Vec::with_capacity(temp_stack.len() + 1);
                                    new_vec.extend(temp_stack.iter().map(|x| x.unwrap()));
                                    if let Some(top) = top {
                                        new_vec.push(top);
                                    }
                                    if !self.stacks[len..].iter().any(|x| *x == new_vec) {
                                        self.stacks.push(new_vec);
                                    }
                                },
                            ),
                        )?;
                    }
                    None => {
                        continue;
                    }
                };
                self.stack_arena.clear();
            }
            for i in (0..len).rev() {
                self.stacks.swap_remove(i);
            }
            if accepted {
                if self.stacks.is_empty() || self.stacks.iter().any(|x| x.is_empty()) {
                    return Ok(AcceptTokenResult::End);
                }
                Ok(AcceptTokenResult::Continue)
            } else {
                Ok(AcceptTokenResult::Failed)
            }
        };
        let result;
        let bytes = token_id.map(|id| self.vocabulary.id_to_token[&id].as_slice());
        if bytes.is_some() {
            result = find_stacks_matching_bytes(bytes)?;
            if result == AcceptTokenResult::Failed || result == AcceptTokenResult::End {
                return Ok(result);
            }
        }
        find_stacks_matching_bytes(None)
    }
    fn match_stack_to_bytes(
        stack: &FixedBuffer<StackItem>,
        bytes: Option<&[u8]>,
        remaining_byte_start: usize,
        trie: &TerminalsTrie,
        find_all: bool,
    ) -> BytesMatchResults {
        #[allow(clippy::too_many_arguments)]
        fn _match_stack_to_bytes(
            stack: &FixedBuffer<StackItem>,
            bytes: &[u8],
            bytes_index: usize,
            trie: &TerminalsTrie,
            stack_offset: usize,
            find_all: bool,
            found: &mut bool,
            result: &mut Vec<BytesMatchResult>,
        ) {
            if bytes.is_empty() || (!find_all && *found) {
                return;
            }
            match stack[stack_offset] {
                StackItem::Nonterminal(_) => {
                    if bytes_index != 0 {
                        result.push(BytesMatchResult {
                            remaining_bytes_start: bytes_index as i32,
                            stack_offset: stack_offset as u32,
                            modified_item_at_offset: None,
                        });
                    }
                }
                StackItem::Terminal(terminal) => {
                    let terminal = unsafe { &*terminal };
                    for i in 0..terminal.len() {
                        if bytes.len() == i + bytes_index {
                            *found = true;
                            result.push(BytesMatchResult {
                                remaining_bytes_start: INVALID_INDEX,
                                stack_offset: stack_offset as u32,
                                modified_item_at_offset: Some(StackItem::Terminal(&terminal[i..])),
                            });
                            return;
                        }
                        if bytes[i + bytes_index] != terminal[i] {
                            return;
                        }
                    }
                    if bytes.len() - bytes_index == terminal.len() {
                        *found = true;
                        result.push(BytesMatchResult {
                            remaining_bytes_start: INVALID_INDEX,
                            stack_offset: stack_offset as u32,
                            modified_item_at_offset: None,
                        });
                        return;
                    }
                    if stack_offset > 0 {
                        _match_stack_to_bytes(
                            stack,
                            bytes,
                            terminal.len() + bytes_index,
                            trie,
                            stack_offset - 1,
                            find_all,
                            found,
                            result,
                        )
                    }
                }
                StackItem::Terminals(current_node_id) => {
                    let mut nodes = Vec::with_capacity(bytes.len() - bytes_index);
                    let mut flag = true;
                    {
                        let mut current_node = trie.get(current_node_id);
                        for (i, byte) in bytes.iter().enumerate().skip(bytes_index) {
                            match current_node.children.get(byte) {
                                Some(new_node_id) => {
                                    let new_node = trie.get(*new_node_id);
                                    nodes.push(*new_node_id);
                                    if let Some(index) = &new_node.negative_bytes_index {
                                        nodes.truncate(i + 1 - bytes_index - *index as usize);
                                        flag = false;
                                        break;
                                    }
                                    current_node = new_node;
                                }
                                None => {
                                    flag = false;
                                    break;
                                }
                            }
                        }
                    }
                    for (i, node_id) in nodes.iter().enumerate() {
                        let new_node = trie.get(*node_id);
                        if new_node.value.is_some()
                            && stack_offset > 0
                            && i + bytes_index < bytes.len() - 1
                        {
                            _match_stack_to_bytes(
                                stack,
                                bytes,
                                bytes_index + i + 1,
                                trie,
                                stack_offset - 1,
                                find_all,
                                found,
                                result,
                            );
                            if !find_all && *found {
                                return;
                            }
                        }
                    }
                    if let Some(last_node_id) = nodes.last() {
                        if flag {
                            let last_node = trie.get(*last_node_id);
                            if !last_node.children.is_empty() && last_node.can_stop {
                                *found = true;
                                result.push(BytesMatchResult {
                                    remaining_bytes_start: INVALID_INDEX,
                                    stack_offset: stack_offset as u32,
                                    modified_item_at_offset: Some(StackItem::Terminals(
                                        *last_node_id,
                                    )),
                                });
                            }
                            if last_node.value.is_some() {
                                *found = true;
                                result.push(BytesMatchResult {
                                    remaining_bytes_start: INVALID_INDEX,
                                    stack_offset: stack_offset as u32,
                                    modified_item_at_offset: None,
                                });
                            }
                        }
                    }
                }
            }
        }
        let mut result: Vec<BytesMatchResult> = vec![];
        match bytes {
            None => BytesMatchResults::Matches(result),
            Some(bytes) => {
                result.reserve(bytes.len());
                let stack_offset = stack.len() - 1;
                let mut found = false;
                _match_stack_to_bytes(
                    stack,
                    bytes,
                    remaining_byte_start,
                    trie,
                    stack_offset,
                    find_all,
                    &mut found,
                    &mut result,
                );

                if result.is_empty() {
                    BytesMatchResults::Failed
                } else {
                    BytesMatchResults::Matches(result)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::type_complexity)]
    fn find_stacks_matching_bytes<'b, F1>(
        mut arena: NonNull<BufferArena<StackItem>>,
        stack: &mut FixedBuffer<StackItem>,
        grammar: &Grammar,
        bytes: Option<&'b [u8]>,
        remaining_byte_start: usize,
        find_all: bool,
        stack_to_bytes_cache: &mut Option<
            &mut FxHashMap<(FixedBuffer<StackItem>, Box<[u8]>), bool>,
        >,
        after_finding_stack: &mut Option<F1>,
    ) -> Result<bool, Error>
    where
        F1: FnMut(&[Option<StackItem>], Option<StackItem>),
    {
        let trie = &grammar.terminals_trie;
        let mut _find_stacks_matching_bytes =
            |mut arena: NonNull<BufferArena<StackItem>>,
             top: NonterminalID,
             stack: &[Option<StackItem>],
             bytes: Option<&'b [u8]>,
             remaining_byte_start: usize,
             stack_to_bytes_cache: &mut Option<
                &mut FxHashMap<(FixedBuffer<StackItem>, Box<[u8]>), bool>,
            >,
             after_finding_stack: &mut Option<F1>| {
                let mut found = false;
                match &grammar.nonterminal_id_to_expression[&top] {
                    SimplifiedExpressions::Expressions(expressions) => {
                        for expression in expressions.iter() {
                            let temp_stack = &mut unsafe { arena.as_mut() }
                                .allocate_a_stack(stack.len() + expression.len())?;
                            temp_stack.copy_from_raw_slice(stack);
                            for term in expression.iter().rev() {
                                temp_stack.push(match term {
                                    U8Term::Terminal(value) => {
                                        StackItem::Terminal(value.as_slice())
                                    }
                                    U8Term::Nonterminal(value) => StackItem::Nonterminal(
                                        grammar.nonterminal_to_terminal_id[value],
                                    ),
                                });
                            }
                            let temp = Self::find_stacks_matching_bytes(
                                arena,
                                temp_stack,
                                grammar,
                                bytes,
                                remaining_byte_start,
                                find_all,
                                stack_to_bytes_cache,
                                after_finding_stack,
                            )?;
                            found |= temp;
                            if !find_all && found {
                                return Ok(found);
                            }
                        }
                    }
                    SimplifiedExpressions::Terminals(node_id) => {
                        let temp_stack =
                            &mut unsafe { arena.as_mut() }.allocate_a_stack(stack.len() + 1)?;
                        temp_stack.copy_from_raw_slice(stack);
                        temp_stack.push(StackItem::Terminals(*node_id));
                        found |= Self::find_stacks_matching_bytes(
                            arena,
                            temp_stack,
                            grammar,
                            bytes,
                            remaining_byte_start,
                            find_all,
                            stack_to_bytes_cache,
                            after_finding_stack,
                        )?;
                        if !find_all && found {
                            return Ok(found);
                        }
                    }
                }
                Ok(found)
            };
        match stack.pop() {
            Some(value) => match value {
                StackItem::Nonterminal(top) => {
                    return _find_stacks_matching_bytes(
                        arena,
                        top,
                        stack.as_raw_slice(),
                        bytes,
                        remaining_byte_start,
                        stack_to_bytes_cache,
                        after_finding_stack,
                    )
                }
                StackItem::Terminal(_) | StackItem::Terminals(_) => {
                    stack.push(value);
                    match Self::match_stack_to_bytes(
                        stack,
                        bytes,
                        remaining_byte_start,
                        trie,
                        find_all,
                    ) {
                        BytesMatchResults::Failed => Ok(false),
                        BytesMatchResults::Matches(possible_results) => {
                            if possible_results.is_empty() {
                                if let Some(f) = after_finding_stack.as_mut() {
                                    f(stack.as_raw_slice(), None)
                                }
                                return Ok(true);
                            }
                            let mut flag = false;
                            for result in possible_results.iter() {
                                if result.remaining_bytes_start == INVALID_INDEX {
                                    if let Some(f) = after_finding_stack.as_mut() {
                                        f(
                                            &stack[..result.stack_offset as usize],
                                            result.modified_item_at_offset,
                                        )
                                    }
                                    flag |= true;
                                    if !find_all {
                                        return Ok(true);
                                    }
                                }
                            }
                            for result in possible_results.iter().rev() {
                                if result.remaining_bytes_start != INVALID_INDEX {
                                    let top = match result
                                        .modified_item_at_offset
                                        .unwrap_or(stack[result.stack_offset as usize])
                                    {
                                        StackItem::Nonterminal(id) => id,
                                        _ => panic!(
                                            "{:?} should only be nonterminal.",
                                            &stack[..(result.stack_offset + 1) as usize]
                                        ),
                                    };
                                    let mut temp_stack = unsafe { arena.as_mut() }
                                        .allocate_a_stack((result.stack_offset + 1) as usize)?;
                                    temp_stack.copy_from_raw_slice(
                                        &stack[..result.stack_offset as usize],
                                    );
                                    if let Some(value) = result.modified_item_at_offset {
                                        temp_stack.push(value);
                                    }
                                    let k = (
                                        temp_stack,
                                        (&bytes.unwrap()[result.remaining_bytes_start as usize..])
                                            .into(),
                                    );
                                    let temp;
                                    if let Some(stack_to_bytes_cache) = stack_to_bytes_cache {
                                        if let Some(value) = stack_to_bytes_cache.get(&k) {
                                            temp = *value;
                                        } else {
                                            temp = _find_stacks_matching_bytes(
                                                arena,
                                                top,
                                                &stack[..result.stack_offset as usize],
                                                bytes,
                                                result.remaining_bytes_start as usize,
                                                &mut Some(stack_to_bytes_cache),
                                                after_finding_stack,
                                            )?;
                                            stack_to_bytes_cache.insert(k, temp);
                                        }
                                    } else {
                                        temp = _find_stacks_matching_bytes(
                                            arena,
                                            top,
                                            &stack[..result.stack_offset as usize],
                                            bytes,
                                            result.remaining_bytes_start as usize,
                                            &mut None,
                                            after_finding_stack,
                                        )?;
                                    }
                                    flag |= temp;
                                    if !find_all && flag {
                                        return Ok(true);
                                    }
                                }
                            }
                            Ok(flag)
                        }
                    }
                }
            },
            None => Ok(false),
        }
    }
}
