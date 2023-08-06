use crate::utils::NonterminalID;
/// an immutable trie tree that stores terminals as bytes
pub struct TerminalsTrie
{
    root: Option<TrieNodeID>,
    nonterminal_id:NonterminalID,
    arena:Vec<TrieNode>,
}
pub struct TrieNodeID(u32);
pub struct TrieNode
{
    current:u8,
    trie_nonterminal_id: NonterminalID,
    childrens:Vec<TrieNodeID>
}

impl TerminalsTrie {
    pub fn new(nonterminal_id:NonterminalID)->Self
    {
        TerminalsTrie
        {
            root: None,
            nonterminal_id,
            arena:Vec::new()
        }
    }

    pub fn with_capacity(nonterminal_id:NonterminalID, capacity:usize)->Self
    {
        TerminalsTrie
        {
            root: None,
            nonterminal_id,
            arena:Vec::with_capacity(capacity)
        }
    }
}