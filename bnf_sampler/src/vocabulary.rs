use qp_trie::Trie;
use rustc_hash::FxHashMap;

use crate::utils::VecU8Wrapper;
#[derive(Debug, Clone)]
pub struct Vocabulary {
    pub token_to_id: Trie<VecU8Wrapper, u32>,
    pub id_to_token: FxHashMap<u32, String>,
}
