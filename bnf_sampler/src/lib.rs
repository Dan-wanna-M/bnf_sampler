pub mod grammar;
pub mod sampler;
pub(crate) mod stack;
pub(crate) mod trie;
pub mod utils;
pub mod vocabulary;
use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
