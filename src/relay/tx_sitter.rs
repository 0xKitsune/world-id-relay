use std::time::Duration;

use alloy::{
    consensus::{Transaction, TypedTransaction},
    primitives::{Address, Bytes, B256},
    providers::{Provider, ProviderBuilder, RootProvider},
    rpc::types::TransactionReceipt,
    transports::{
        http::{Client, Http},
        TransportErrorKind,
    },
};
use reqwest::StatusCode;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::TransactionRelay;

pub struct TxSitterClient {
    pub relay_endpoint: String,
    pub relay_client: reqwest::Client,
    pub rpc_provider: RootProvider<Http<Client>>,
}

impl TxSitterClient {
    pub fn new(relay_endpoint: String, rpc_endpoint: String) -> Result<Self, TxSitterError> {
        let relay_client = reqwest::Client::new();

        let rpc_provider = ProviderBuilder::new().on_http(rpc_endpoint.parse()?);
        Ok(Self {
            relay_endpoint: format!("{}/tx", relay_endpoint),
            relay_client,
            rpc_provider,
        })
    }

    pub async fn wait_for_tx_hash(&self, tx_id: String) -> Result<B256, TxSitterError> {
        let client = reqwest::Client::new();
        let url = format!("{}{}", self.relay_endpoint, tx_id);

        loop {
            let response = client.get(url.clone()).send().await?;

            if response.status().is_success() {
                let get_tx_response = response.json::<GetTxResponse>().await?;

                if let Some(tx_status) = get_tx_response.status {
                    if tx_status == BlockTxStatus::Pending
                        || tx_status == BlockTxStatus::Mined
                        || tx_status == BlockTxStatus::Finalized
                    {
                        if let Some(tx_hash) = get_tx_response.tx_hash {
                            tracing::info!(?tx_hash, "Tx sent through relay");
                            return Ok(tx_hash);
                        }
                    } else {
                        tracing::info!(?tx_status, "Tx not sent, waiting for tx hash")
                    }
                }

                tokio::time::sleep(Duration::from_secs(3)).await;
            } else {
                return Err(TxSitterError::TransactionError(
                    TransactionError::GetTransactionStatusError(response.status()),
                ));
            }
        }
    }
}

impl TransactionRelay for TxSitterClient {
    type Error = TxSitterError;

    async fn send_transaction(&self, tx: TypedTransaction) -> Result<B256, Self::Error> {
        let payload = &SendTxRequest::try_from(tx).map_err(TxSitterError::TransactionError)?;

        let response = self
            .relay_client
            .post(&self.relay_endpoint)
            .json(payload)
            .send()
            .await?;

        if response.status().is_success() {
            let send_tx_response = response.json::<SendTxResponse>().await?;
            tracing::info!(?send_tx_response.tx_id, "Sent transaction to tx-sitter");

            let tx_hash = self.wait_for_tx_hash(send_tx_response.tx_id).await?;

            Ok(tx_hash)
        } else {
            Err(TxSitterError::TransactionError(
                TransactionError::SendTransactionError(response.status()),
            ))
        }
    }

    async fn get_transaction_receipt(
        &self,
        tx_hash: B256,
    ) -> Result<Option<TransactionReceipt>, Self::Error> {
        let transaction_receipt = self.rpc_provider.get_transaction_receipt(tx_hash).await?;

        Ok(transaction_receipt)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "camelCase")]
pub enum TransactionPriority {
    // 5th percentile
    Slowest = 0,
    // 25th percentile
    Slow = 1,
    // 50th percentile
    #[default]
    Regular = 2,
    // 75th percentile
    Fast = 3,
    // 95th percentile
    Fastest = 4,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTxRequest {
    pub to: Address,
    pub value: U256,
    #[serde(default)]
    pub data: Option<Bytes>,
    pub gas_limit: U256,
    #[serde(default)]
    pub priority: TransactionPriority,
    #[serde(default)]
    pub tx_id: Option<String>,
}

#[derive(Error, Debug)]
pub enum TxSitterError {
    #[error(transparent)]
    TransactionError(TransactionError),
    #[error(transparent)]
    UrlParseError(#[from] url::ParseError),
    #[error(transparent)]
    RpcError(#[from] alloy::transports::RpcError<TransportErrorKind>),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
}

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("To address not found.")]
    ToAddressNotFound,
    #[error("Data not found.")]
    DataNotFound,
    #[error("Value not found.")]
    ValueNotFound,
    #[error("Gas limit not found.")]
    GasLimitNotFound,
    #[error("Send transaction error")]
    SendTransactionError(StatusCode),
    #[error("Get transaction status error")]
    GetTransactionStatusError(StatusCode),
}

impl TryFrom<TypedTransaction> for SendTxRequest {
    type Error = TransactionError;
    fn try_from(tx: TypedTransaction) -> Result<Self, Self::Error> {
        let tx_kind = tx.to();

        let to = tx_kind.to().ok_or(TransactionError::ToAddressNotFound)?;

        let data = Bytes::from(tx.input().to_owned());

        Ok(SendTxRequest {
            to: to.clone(),
            value: tx.value(),
            data: Some(data),
            gas_limit: U256::from(tx.gas_limit()),
            priority: TransactionPriority::Regular,
            tx_id: None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTxResponse {
    pub tx_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTxResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<BlockTxStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<B256>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BlockTxStatus {
    Pending = 0,
    Mined = 1,
    Finalized = 2,
    Unsent = 3,
}
