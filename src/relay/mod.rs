use alloy::{consensus::TypedTransaction, primitives::B256, rpc::types::TransactionReceipt};

pub mod tx_sitter;

// TODO: probably update name
pub mod wallet;

pub trait TransactionRelay {
    type Error;

    async fn send_transaction(&self, tx: TypedTransaction) -> Result<B256, Self::Error>;

    async fn get_transaction_receipt(
        &self,
        tx_hash: B256,
    ) -> Result<Option<TransactionReceipt>, Self::Error>;
}
