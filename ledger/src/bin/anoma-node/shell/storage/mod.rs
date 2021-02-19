//! The storage module handles both the current state in-memory and the stored
//! state in DB.

// TODO add storage error type
// TODO make derive macro for H256 https://doc.rust-lang.org/book/ch19-06-macros.html#how-to-write-a-custom-derive-macro

mod db;

use blake2b_rs::{Blake2b, Blake2bBuilder};
use sparse_merkle_tree::{
    blake2b::Blake2bHasher, default_store::DefaultStore, SparseMerkleTree, H256,
};
use std::{collections::HashMap, convert::TryFrom, hash::Hash};

// TODO adjust once chain ID scheme is chosen
const CHAIN_ID_LENGTH: usize = 20;
const BLOCK_HASH_LENGTH: usize = 32;

#[derive(Debug)]
pub struct Storage {
    chain_id: String,
    block: BlockStorage,
}

#[derive(Debug)]
pub struct BlockStorage {
    tree: MerkleTree,
    hash: BlockHash,
    height: u64,
    balances: HashMap<Address, Balance>,
}

pub struct BlockHash([u8; 32]);

struct MerkleTree(SparseMerkleTree<Blake2bHasher, H256, DefaultStore<H256>>);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Address {
    Validator(ValidatorAddress),
    Basic(BasicAddress),
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BasicAddress(String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ValidatorAddress(String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Balance(u64);

impl Default for Storage {
    fn default() -> Self {
        let tree = MerkleTree::default();
        let balances = HashMap::new();
        let block = BlockStorage {
            tree,
            hash: BlockHash::default(),
            height: 0,
            balances,
        };
        Self {
            chain_id: String::with_capacity(CHAIN_ID_LENGTH),
            block,
        }
    }
}

impl Storage {
    /// # Storage reads
    pub fn merkle_root(&self) -> &H256 {
        self.block.tree.0.root()
    }

    // TODO this doesn't belong here, but just for convenience...
    pub fn has_balance_gte(
        &self,
        addr: &Address,
        amount: u64,
    ) -> Result<(), String> {
        match self.block.balances.get(&addr) {
            None => return Err("Source not found".to_owned()),
            Some(&Balance(src_balance)) => {
                if src_balance < amount {
                    return Err("Source balance is too low".to_owned());
                };
            }
        }
        Ok(())
    }

    /// # Storage writes
    // TODO Enforce or check invariant (it should catch newly added storage
    // fields too) that every function that changes storage, except for data
    // from Tendermint's block header should call this function to update the
    // Merkle tree.
    fn update_tree(&mut self, key: H256, value: H256) -> Result<(), String> {
        self.block
            .tree
            .0
            .update(key, value)
            .map_err(|err| format!("SMT error {}", err))?;
        Ok(())
    }

    pub fn update_balance(
        &mut self,
        addr: &Address,
        balance: Balance,
    ) -> Result<(), String> {
        let key = addr.hash256();
        let value = balance.hash256();
        self.update_tree(key, value)?;
        self.block.balances.insert(addr.clone(), balance);
        Ok(())
    }

    // TODO this doesn't belong here, but just for convenience...
    pub fn transfer(
        &mut self,
        src: &Address,
        dest: &Address,
        amount: u64,
    ) -> Result<(), String> {
        match self.block.balances.get(&src) {
            None => return Err("Source not found".to_owned()),
            Some(&Balance(src_balance)) => {
                if src_balance < amount {
                    return Err("Source balance is too low".to_owned());
                };
                self.update_balance(src, Balance::new(src_balance - amount))?;
                match self.block.balances.get(&dest) {
                    None => self.update_balance(dest, Balance::new(amount))?,
                    Some(&Balance(dest_balance)) => self.update_balance(
                        dest,
                        Balance::new(dest_balance + amount),
                    )?,
                }
                Ok(())
            }
        }
    }

    /// # Block header data
    /// Chain ID is not in the Merkle tree as it's tracked by Tendermint in the
    /// block header. Hence, we don't update the tree when this is set.
    pub fn set_chain_id(&mut self, chain_id: &str) -> Result<(), String> {
        self.chain_id = chain_id.to_owned();
        Ok(())
    }

    /// Block data is in the Merkle tree as it's tracked by Tendermint in the
    /// block header. Hence, we don't update the tree when this is set.
    pub fn begin_block(
        &mut self,
        hash: BlockHash,
        height: u64,
    ) -> Result<(), String> {
        self.block.hash = hash;
        self.block.height = height;
        Ok(())
    }
}

impl Default for BlockHash {
    fn default() -> Self {
        Self([0; 32])
    }
}
impl Hash256 for BlockHash {
    fn hash256(&self) -> H256 {
        self.0.hash256()
    }
}

impl TryFrom<&[u8]> for BlockHash {
    type Error = String;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != BLOCK_HASH_LENGTH {
            return Err(format!(
                "Unexpected block hash length {}, expected {}",
                value.len(),
                BLOCK_HASH_LENGTH
            ));
        }
        let mut hash = [0; 32];
        hash.copy_from_slice(value);
        Ok(BlockHash(hash))
    }
}

impl core::fmt::Debug for BlockHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hash = format!("{:x}", ByteBuf(&self.0));
        f.debug_tuple("BlockHash").field(&hash).finish()
    }
}

impl Default for MerkleTree {
    fn default() -> Self {
        MerkleTree(SparseMerkleTree::default())
    }
}

impl core::fmt::Debug for MerkleTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let root_hash = format!("{:x}", ByteBuf(self.0.root().as_slice()));
        f.debug_struct("MerkleTree")
            .field("root_hash", &root_hash)
            .finish()
    }
}

trait Hash256 {
    fn hash256(&self) -> H256;
}

impl Hash256 for Address {
    fn hash256(&self) -> H256 {
        match self {
            Address::Basic(addr) => addr.hash256(),
            Address::Validator(addr) => addr.hash256(),
        }
    }
}

impl BasicAddress {
    pub fn new_address(addr: String) -> Address {
        Address::Basic(Self(addr))
    }
}
impl Hash256 for BasicAddress {
    fn hash256(&self) -> H256 {
        self.0.hash256()
    }
}

impl ValidatorAddress {
    pub fn new_address(addr: String) -> Address {
        Address::Validator(Self(addr))
    }
}
impl Hash256 for ValidatorAddress {
    fn hash256(&self) -> H256 {
        self.0.hash256()
    }
}

impl Balance {
    pub fn new(balance: u64) -> Self {
        Self(balance)
    }
}
impl Hash256 for Balance {
    fn hash256(&self) -> H256 {
        if self.0 == 0 {
            return H256::zero();
        }
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&self.0.to_le_bytes());
        hasher.finalize(&mut buf);
        buf.into()
    }
}

impl Hash256 for &str {
    fn hash256(&self) -> H256 {
        if self.is_empty() {
            return H256::zero();
        }
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(self.as_bytes());
        hasher.finalize(&mut buf);
        buf.into()
    }
}

impl Hash256 for String {
    fn hash256(&self) -> H256 {
        if self.is_empty() {
            return H256::zero();
        }
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(self.as_bytes());
        hasher.finalize(&mut buf);
        buf.into()
    }
}

impl Hash256 for [u8; 32] {
    fn hash256(&self) -> H256 {
        if self.is_empty() {
            return H256::zero();
        }
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(self);
        hasher.finalize(&mut buf);
        buf.into()
    }
}

impl Hash256 for u64 {
    fn hash256(&self) -> H256 {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&self.to_le_bytes());
        hasher.finalize(&mut buf);
        buf.into()
    }
}

fn new_blake2b() -> Blake2b {
    Blake2bBuilder::new(32).personal(b"anoma storage").build()
}

/// A helper to show bytes in hex
struct ByteBuf<'a>(&'a [u8]);
impl<'a> std::fmt::LowerHex for ByteBuf<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        for byte in self.0 {
            f.write_fmt(format_args!("{:02x}", byte))?;
        }
        Ok(())
    }
}
