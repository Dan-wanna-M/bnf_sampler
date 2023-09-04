use crate::grammar::Grammar;
use crate::grammar::SimplifiedExpressions;
use crate::grammar::U8Term;
use crate::stack::BufferArena;
use crate::stack::FixedBuffer;
use crate::trie::TerminalsTrie;
use crate::trie::TerminalsTrieIter;
use crate::trie::TrieNodeID;
use crate::utils::NonterminalID;
use crate::utils::SliceU8Wrapper;
use crate::utils::VecU8Wrapper;
use bit_set::BitSet;
use qp_trie::Trie;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::ptr::NonNull;
use std::vec;

const INVALID_INDEX: i32 = -1;

#[derive(PartialEq, Clone, Debug, Copy, Eq, Hash)]
enum StackItem<'a> {
    Nonterminal(NonterminalID),
    Terminal(&'a [u8]),
    Terminals(TrieNodeID),
}
#[derive(Clone, Debug)]
pub struct Sampler<'a> {
    stacks: Vec<Vec<StackItem<'a>>>,
    grammar: &'a Grammar,
    tokens_buffer: Vec<(VecU8Wrapper, u32)>,
    tokens_tree: &'a Trie<VecU8Wrapper, u32>,
    stack_arena: BufferArena<StackItem<'a>>,
    stacks_to_token_ids: FxHashMap<Vec<Vec<StackItem<'a>>>, BitSet<u32>>,
    start_nonterminal: String,
    token_ids: BitSet<u32>,
    stack_to_bytes_cache_enabled: bool,
}
#[derive(Debug, PartialEq, Clone, Copy)]
enum AcceptTokensResult {
    Continue,
    End,
    Failed,
}
#[derive(Debug, PartialEq, Clone)]
pub enum PossibleTokensResult<'a> {
    /// contains all possible token ids
    Continue(&'a BitSet<u32>),
    End,
    InputTokensRejected,
}

#[derive(Debug)]
enum BytesMatchResults<'a> {
    Failed(usize),
    Matches(Vec<BytesMatchResult<'a>>),
}
#[derive(Debug)]
struct BytesMatchResult<'a> {
    remaining_bytes_start: i32,
    stack_offset: u32,
    modified_item_at_offset: Option<StackItem<'a>>,
}

enum TokensIterType<'a> {
    Flat(std::slice::Iter<'a, (VecU8Wrapper, u32)>),
    SinglePrefix(qp_trie::Iter<'a, VecU8Wrapper, u32>),
    MultiplePrefixs(
        (
            TerminalsTrieIter<'a>,
            Option<qp_trie::Iter<'a, VecU8Wrapper, u32>>,
        ),
    ),
}

struct BufferOrTreeIter<'a> {
    tokens_buffer_iter: TokensIterType<'a>,
    tokens_tree: &'a Trie<VecU8Wrapper, u32>,
}

impl<'a, 'b> BufferOrTreeIter<'a> {
    pub fn new(
        tokens_buffer: &'a [(VecU8Wrapper, u32)],
        tokens_tree: &'a Trie<VecU8Wrapper, u32>,
        trie: &'a TerminalsTrie,
        current_top: StackItem<'b>,
    ) -> Self {
        let tokens_buffer_iter = match current_top {
            StackItem::Terminal(terminal) => TokensIterType::SinglePrefix(
                tokens_tree.iter_prefix(tokens_tree.longest_common_prefix(terminal)),
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
    type Item = (&'a VecU8Wrapper, &'a u32);

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

impl<'a> std::fmt::Display for Sampler<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The `f` value implements the `Write` trait, which is what the
        // write! macro is expecting. Note that this formatting ignores the
        // various flags provided to format strings.
        write!(f, "stacks: {:?}", self.stacks)
    }
}

impl<'a> Sampler<'a> {
    pub fn new(
        grammar: &'a Grammar,
        start_nonterminal: String,
        tokens_tree: &'a Trie<VecU8Wrapper, u32>,
        stack_arena_capacity: usize,
        stack_to_bytes_cache_enabled: bool,
    ) -> Sampler<'a> {
        let stacks = vec![vec![StackItem::Nonterminal(
            grammar.nonterminal_to_terminal_id[&start_nonterminal],
        )]];
        let token_ids: BitSet<u32> = BitSet::with_capacity(u16::MAX.into());
        let stacks_to_token_ids = FxHashMap::default();
        let tokens_buffer = Vec::from_iter(tokens_tree.iter().map(|(k, v)| (k.clone(), *v)));
        Sampler {
            stacks,
            grammar,
            tokens_tree,
            tokens_buffer,
            stacks_to_token_ids,
            token_ids,
            stack_arena: BufferArena::with_capacity(stack_arena_capacity),
            stack_to_bytes_cache_enabled,
            start_nonterminal,
        }
    }

    pub fn reset(&mut self) {
        self.stacks = vec![vec![StackItem::Nonterminal(
            self.grammar.nonterminal_to_terminal_id[&self.start_nonterminal],
        )]];
    }

    pub fn all_possible_next_tokens(
        &mut self,
        previous_tokens: Option<&[u8]>,
    ) -> PossibleTokensResult {
        // let now = Instant::now();
        self.token_ids.clear();
        match self.accept_tokens(previous_tokens) {
            AcceptTokensResult::End => PossibleTokensResult::End,
            AcceptTokensResult::Failed => PossibleTokensResult::InputTokensRejected,
            AcceptTokensResult::Continue => {
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
                PossibleTokensResult::Continue(
                    self.stacks_to_token_ids
                        .entry(self.stacks.clone())
                        .or_insert_with(|| {
                            let mut stack_to_bytes_cache: FxHashMap<
                                (FixedBuffer<StackItem<'a>>, Box<[u8]>),
                                bool,
                            > = FxHashMap::default();
                            for stack in self.stacks.iter() {
                                // let now = Instant::now();
                                let mut failed_prefixs: Trie<SliceU8Wrapper, ()> = Trie::new();
                                let iter = BufferOrTreeIter::new(
                                    &self.tokens_buffer,
                                    self.tokens_tree,
                                    &self.grammar.terminals_trie,
                                    *stack.last().unwrap(),
                                );
                                for (token, token_id) in iter {
                                    if self.token_ids.contains(*token_id as usize)
                                        || failed_prefixs.contains_key(
                                            failed_prefixs
                                                .longest_common_prefix(token.0.as_slice()),
                                        )
                                    {
                                        continue;
                                    }
                                    let arena = unsafe {
                                        NonNull::new_unchecked(
                                            &mut self.stack_arena
                                                as *mut BufferArena<StackItem<'a>>,
                                        )
                                    };
                                    let mut temp_stack =
                                        self.stack_arena.allocate_a_stack(stack.len());
                                    temp_stack.copy_from_slice(stack.as_slice());
                                    let mut cache;
                                    if self.stack_to_bytes_cache_enabled {
                                        cache = Some(&mut stack_to_bytes_cache);
                                    } else {
                                        cache = None;
                                    }
                                    let result = Self::find_stacks_matching_bytes(
                                        arena,
                                        &mut temp_stack,
                                        self.grammar,
                                        Some(token.0.as_slice()),
                                        false,
                                        &mut cache,
                                        &mut |_, _| {},
                                        &mut |bytes, index| {
                                            if index < usize::MAX {
                                                failed_prefixs.insert(
                                                    SliceU8Wrapper(&bytes[..index + 1]),
                                                    (),
                                                );
                                            }
                                        },
                                    );
                                    if result {
                                        self.token_ids.insert(*token_id as usize);
                                    }
                                    self.stack_arena.clear();
                                }
                                // println!("stack: {:?}, {:?}", stack, now.elapsed());
                            }
                            self.token_ids.clone()
                        }),
                )
            }
        }
    }
    #[must_use]
    fn accept_tokens(&mut self, bytes: Option<&[u8]>) -> AcceptTokensResult {
        let mut find_stacks_matching_bytes = |bytes| {
            let len = self.stacks.len();
            let mut accepted = false;
            for i in 0..len {
                let arena = unsafe {
                    NonNull::new_unchecked(&mut self.stack_arena as *mut BufferArena<StackItem<'a>>)
                };
                let mut stack = self.stack_arena.allocate_a_stack(self.stacks[i].len());
                stack.copy_from_slice(&self.stacks[i]);
                let stack_to_bytes_cache: &mut FxHashMap<
                    (FixedBuffer<StackItem<'a>>, Box<[u8]>),
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
                            self.grammar,
                            bytes,
                            true,
                            &mut cache,
                            &mut |temp_stack: &[Option<StackItem<'_>>], top: Option<StackItem>| {
                                let mut new_vec = Vec::with_capacity(temp_stack.len() + 1);
                                new_vec.extend(temp_stack.iter().map(|x| x.unwrap()));
                                if let Some(top) = top {
                                    new_vec.push(top);
                                }
                                if !self.stacks[len..].iter().any(|x| *x == new_vec) {
                                    self.stacks.push(new_vec);
                                }
                            },
                            &mut |_, _| {},
                        );
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
                if self.stacks.is_empty() || self.stacks.iter().all(|x| x.is_empty()) {
                    return AcceptTokensResult::End;
                }
                AcceptTokensResult::Continue
            } else {
                AcceptTokensResult::Failed
            }
        };
        let result;
        if bytes.is_some() {
            result = find_stacks_matching_bytes(bytes);
            if result == AcceptTokensResult::Failed || result == AcceptTokensResult::End {
                return result;
            }
        }
        find_stacks_matching_bytes(None)
    }
    fn match_stack_to_bytes<'b>(
        stack: &FixedBuffer<StackItem<'a>>,
        bytes: Option<&'b [u8]>,
        trie: &TerminalsTrie,
        find_all: bool,
    ) -> BytesMatchResults<'a> {
        #[allow(clippy::too_many_arguments)]
        fn _match_stack_to_bytes<'a>(
            stack: &FixedBuffer<StackItem<'a>>,
            bytes: &[u8],
            bytes_index: usize,
            trie: &TerminalsTrie,
            stack_offset: usize,
            find_all: bool,
            found: &mut bool,
            shortest_failed_index: &mut usize,
            result: &mut Vec<BytesMatchResult<'a>>,
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
                            *shortest_failed_index =
                                std::cmp::min(i + bytes_index, *shortest_failed_index);
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
                            shortest_failed_index,
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
                                    *shortest_failed_index =
                                        std::cmp::min(i, *shortest_failed_index);
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
                                shortest_failed_index,
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
            None => return BytesMatchResults::Matches(result),
            Some(bytes) => {
                result.reserve(bytes.len());
                let stack_offset = stack.len() - 1;
                let mut shortest_failed_index = usize::MAX;
                let mut found = false;
                _match_stack_to_bytes(
                    stack,
                    bytes,
                    0,
                    trie,
                    stack_offset,
                    find_all,
                    &mut found,
                    &mut shortest_failed_index,
                    &mut result,
                );
                if result.is_empty() {
                    return BytesMatchResults::Failed(shortest_failed_index);
                } else {
                    return BytesMatchResults::Matches(result);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::type_complexity)]
    fn find_stacks_matching_bytes<'b, F1, F2>(
        mut arena: NonNull<BufferArena<StackItem<'a>>>,
        stack: &mut FixedBuffer<StackItem<'a>>,
        grammar: &'a Grammar,
        bytes: Option<&'b [u8]>,
        find_all: bool,
        stack_to_bytes_cache: &mut Option<
            &mut FxHashMap<(FixedBuffer<StackItem<'a>>, Box<[u8]>), bool>,
        >,
        after_finding_stack: &mut F1,
        after_match_failed: &mut F2,
    ) -> bool
    where
        F1: FnMut(&[Option<StackItem<'a>>], Option<StackItem<'a>>),
        F2: FnMut(&'b [u8], usize),
    {
        let trie = &grammar.terminals_trie;
        let mut _find_stacks_matching_bytes =
            |mut arena: NonNull<BufferArena<StackItem<'a>>>,
             top: NonterminalID,
             stack: &[Option<StackItem<'a>>],
             bytes: Option<&'b [u8]>,
             stack_to_bytes_cache: &mut Option<
                &mut FxHashMap<(FixedBuffer<StackItem<'a>>, Box<[u8]>), bool>,
            >,
             after_finding_stack: &mut F1| {
                let mut found = false;
                match &grammar.nonterminal_id_to_expression[&top] {
                    SimplifiedExpressions::Expressions(expressions) => {
                        for expression in expressions.iter() {
                            let temp_stack = &mut unsafe { arena.as_mut() }
                                .allocate_a_stack(stack.len() + expression.len());
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
                                find_all,
                                stack_to_bytes_cache,
                                after_finding_stack,
                                after_match_failed,
                            );
                            found |= temp;
                            if !find_all && found {
                                return found;
                            }
                        }
                    }
                    SimplifiedExpressions::Terminals(node_id) => {
                        let temp_stack =
                            &mut unsafe { arena.as_mut() }.allocate_a_stack(stack.len() + 1);
                        temp_stack.copy_from_raw_slice(stack);
                        temp_stack.push(StackItem::Terminals(*node_id));
                        found |= Self::find_stacks_matching_bytes(
                            arena,
                            temp_stack,
                            grammar,
                            bytes,
                            find_all,
                            stack_to_bytes_cache,
                            after_finding_stack,
                            after_match_failed,
                        );
                        if !find_all && found {
                            return found;
                        }
                    }
                }
                found
            };
        match stack.pop() {
            Some(value) => match value {
                StackItem::Nonterminal(top) => {
                    return _find_stacks_matching_bytes(
                        arena,
                        top,
                        stack.as_raw_slice(),
                        bytes,
                        stack_to_bytes_cache,
                        after_finding_stack,
                    )
                }
                StackItem::Terminal(_) | StackItem::Terminals(_) => {
                    stack.push(value);
                    match Self::match_stack_to_bytes(stack, bytes, trie, find_all) {
                        BytesMatchResults::Failed(index) => {
                            after_match_failed(bytes.unwrap(), index);
                            false
                        }
                        BytesMatchResults::Matches(possible_results) => {
                            if possible_results.is_empty() {
                                after_finding_stack(stack.as_raw_slice(), None);
                                return true;
                            }
                            let mut flag = false;
                            for result in possible_results.iter() {
                                if result.remaining_bytes_start == INVALID_INDEX {
                                    after_finding_stack(
                                        &stack[..result.stack_offset as usize],
                                        result.modified_item_at_offset,
                                    );
                                    flag |= true;
                                    if !find_all {
                                        return true;
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
                                        .allocate_a_stack((result.stack_offset + 1) as usize);
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
                                                Some(
                                                    &(bytes.unwrap()
                                                        [result.remaining_bytes_start as usize..]),
                                                ),
                                                &mut Some(stack_to_bytes_cache),
                                                after_finding_stack,
                                            );
                                            stack_to_bytes_cache.insert(k, temp);
                                        }
                                    } else {
                                        temp = _find_stacks_matching_bytes(
                                            arena,
                                            top,
                                            &stack[..result.stack_offset as usize],
                                            Some(
                                                &(bytes.unwrap()
                                                    [result.remaining_bytes_start as usize..]),
                                            ),
                                            &mut None,
                                            after_finding_stack,
                                        );
                                    }
                                    flag |= temp;
                                    if !find_all && flag {
                                        return true;
                                    }
                                }
                            }
                            flag
                        }
                    }
                }
            },
            None => false,
        }
    }
}
