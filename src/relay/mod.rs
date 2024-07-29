use alloy::rpc::types::TransactionReceipt;
use ethers::types::{transaction::eip2718::TypedTransaction, H256};

pub mod tx_sitter;
pub mod tx_sitter_aws;
pub mod wallet;

pub trait TransactionRelay {
    type Error;

    async fn send_transaction(&self, tx: TypedTransaction) -> Result<H256, Self::Error>;

    async fn get_tx_receipt(&self, tx_hash: H256) -> Result<TransactionReceipt, Self::Error>;
}
