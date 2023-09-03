pub mod sampler;
pub mod grammar;
pub(crate) mod stack;
pub(crate) mod trie;
pub mod utils;
use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;