use ::bnf_sampler;
use bit_set::BitSet;
use bnf_sampler::{
    grammar,
    sampler::{self, AcceptTokenResult, PossibleTokensResult},
    utils::U8ArrayWrapper,
    vocabulary,
};
use pyo3::prelude::*;
use rustc_hash::FxHashMap;
use std::{borrow::Cow, collections::HashSet, sync::Arc};

/// The struct represents a language model's vocabulary.
///
/// Constructor function signature: Vocabulary(token_to_id: Dict[bytes|Sequence[int]|bytearray, int],
///                                              id_to_token: Dict[int, bytes|Sequence[int]|bytearray],
///                                              id_to_token_string: Dict[int, str]) -> Vocabulary
#[pyclass]
#[derive(Clone)]
pub struct Vocabulary {
    data: Arc<vocabulary::Vocabulary>,
}

#[pymethods]
impl Vocabulary {
    #[new]
    pub fn new(
        token_to_id: FxHashMap<Vec<u8>, u32>,
        id_to_token: FxHashMap<u32, Vec<u8>>,
        id_to_token_string: FxHashMap<u32, String>,
    ) -> Vocabulary {
        Vocabulary {
            data: Arc::new(vocabulary::Vocabulary {
                token_to_id: token_to_id
                    .into_iter()
                    .map(|(k, v)| (U8ArrayWrapper(k.into_boxed_slice()), v))
                    .collect(),
                id_to_token,
                id_to_token_string,
            }),
        }
    }
    /// Obtain tokens in UTF-8 String representation from token ids.
    ///
    /// Function signature: get_token_strings_from_token_ids(self, token_ids: Sequence[int]) -> Sequence[str]
    pub fn get_token_strings_from_token_ids(&self, token_ids: Vec<usize>) -> Vec<String> {
        self.data
            .get_token_strings_from_token_ids(&BitSet::from_iter(token_ids.into_iter()))
            .map(|x| x.to_string())
            .collect()
    }

    /// Obtain tokens in UTF-8 String representation from token ids.
    ///
    /// Function signature: get_token_strings_from_token_ids(self, token_ids: Sequence[int]) -> Sequence[bytes]
    pub fn get_token_from_token_ids(&self, token_ids: Vec<usize>) -> Vec<Cow<[u8]>> {
        self.data
            .get_token_from_token_ids(&BitSet::from_iter(token_ids.into_iter()))
            .map(|x| Cow::Owned(x.to_vec()))
            .collect()
    }
    /// Function signature: deepcopy(self)
    pub fn deepcopy(&self) -> Vocabulary {
        Vocabulary {
            data: Arc::new((*self.data).clone()),
        }
    }

    pub fn __repr__(&self) -> String {
        format!("{:?}", self.data)
    }
}
/// Read the vocabulary from RWKV-world model series vocabulary file.
///
/// Function signature: read_rwkv_world_vocab(file_name: str) -> Vocabulary
#[pyfunction]
#[pyo3(signature = (file_name))]
pub fn read_rwkv_world_vocab(file_name: &str) -> anyhow::Result<Vocabulary> {
    let result = Vocabulary {
        data: bnf_sampler::utils::read_rwkv_world_vocab(file_name)?,
    };
    Ok(result)
}
/// The class represents the BNF schema.
///
/// Constructor function signature: Grammar(schema: str, vocabulary: Vocabulary, grammar_arena_capacity: int)-> Grammar
#[pyclass]
#[derive(Clone)]
pub struct Grammar {
    data: Arc<grammar::Grammar>,
}

#[pymethods]
impl Grammar {
    #[new]
    pub fn new(
        schema: &str,
        vocabulary: Vocabulary,
        grammar_arena_capacity: usize,
    ) -> anyhow::Result<Grammar> {
        Ok(Grammar {
            data: grammar::Grammar::new(schema, vocabulary.data, grammar_arena_capacity)?,
        })
    }
    /// Function signature: deepcopy(self)
    pub fn deepcopy(&self) -> Grammar {
        Grammar {
            data: Arc::new((*self.data).clone()),
        }
    }

    pub fn __repr__(&self) -> String {
        format!("{:?}", self.data)
    }
}
/// Constructor function signature: Sampler(grammar: Grammar, start_nonterminal: str, vocabulary: Vocabulary, arena_capacity: int,enable_bytes_cache: bool)->Sampler
#[pyclass]
#[derive(Clone)]
pub struct Sampler {
    data: sampler::Sampler,
}

#[pymethods]
impl Sampler {
    #[new]
    pub fn new(
        grammar: Grammar,
        start_nonterminal: String,
        vocabulary: Vocabulary,
        arena_capacity: usize,
        enable_bytes_cache: bool,
    ) -> anyhow::Result<Sampler> {
        Ok(Sampler {
            data: sampler::Sampler::new(
                grammar.data,
                start_nonterminal,
                vocabulary.data,
                arena_capacity,
                enable_bytes_cache,
            )?,
        })
    }
    /// Function signature: accept_a_token(self, token_id: int) -> "Continue"|"End"|"Failed"
    pub fn accept_a_token(&mut self, token_id: Option<u32>) -> anyhow::Result<&'static str> {
        Ok(match self.data.accept_a_token(token_id)? {
            AcceptTokenResult::Continue => "Continue",
            AcceptTokenResult::End => "End",
            AcceptTokenResult::Failed => "Failed",
        })
    }
    /// Function signature: all_possible_tokens(self, token_id: int|None) -> ("Continue", Set[int])|("End", None)|("InputTokenRejected", None)
    pub fn all_possible_next_tokens(
        &mut self,
        token_id: Option<u32>,
    ) -> anyhow::Result<(&'static str, Option<HashSet<usize>>)> {
        Ok(match self.data.all_possible_next_tokens(token_id)? {
            PossibleTokensResult::Continue(set) => {
                ("Continue", Some(HashSet::from_iter(set.iter())))
            }
            PossibleTokensResult::End => ("End", None),
            PossibleTokensResult::InputTokenRejected => ("InputTokenRejected", None),
        })
    }
    /// Function signature: reset(self)
    pub fn reset(&mut self) {
        self.data.reset()
    }
    /// Function signature: copy(self)
    pub fn copy(&self) -> Self {
        self.clone()
    }

    pub fn __repr__(&self) -> String {
        format!("{:?}", self.data)
    }
}

#[pymodule]
#[pyo3(name = "bnf_sampler_py")]
pub fn bnf_sampler_py(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<Vocabulary>()?;
    m.add_function(wrap_pyfunction!(read_rwkv_world_vocab, m)?)?;
    m.add_class::<Grammar>()?;
    m.add_class::<Sampler>()?;
    Ok(())
}
