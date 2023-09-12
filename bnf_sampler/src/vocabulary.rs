use bit_set::BitSet;
use qp_trie::Trie;
use rustc_hash::FxHashMap;

use crate::utils::VecU8Wrapper;
#[derive(Debug, Clone)]
/// The struct represents a language model's vocabulary.
pub struct Vocabulary {
    pub token_to_id: Trie<VecU8Wrapper, u32>,
    /// This field represents a map from token id to the token in bytes.
    pub id_to_token: FxHashMap<u32, Vec<u8>>,
    /// This field represents a map from token id to the token in UTF-8 String representation.
    pub id_to_token_string: FxHashMap<u32, String>,
}

impl Vocabulary {
    pub fn get_token_strings_from_token_ids<'a>(
        &'a self,
        token_ids: &'a BitSet,
    ) -> impl Iterator<Item = &'a str> {
        token_ids
            .iter()
            .map(|x| self.id_to_token_string[&(x as u32)].as_str())
    }

    pub fn get_token_from_token_ids<'a>(
        &'a self,
        token_ids: &'a BitSet,
    ) -> impl Iterator<Item = &'a [u8]> {
        token_ids
            .iter()
            .map(|x| self.id_to_token[&(x as u32)].as_slice())
    }
}
