use std::time::Duration;

use alloy::{
    providers::{Provider, ProviderBuilder, RootProvider},
    rpc::types::TransactionReceipt,
    transports::http::{Client, Http},
};
use ethers::types::{transaction::eip2718::TypedTransaction, Address, Bytes, H256, U256};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::TransactionRelay;

pub struct TxSitterCient {
    pub relay_endpoint: String,
    pub relay_client: reqwest::Client,
    pub rpc_provider: RootProvider<Http<Client>>,
}

impl TxSitterCient {
    pub fn new(relay_endpoint: String, rpc_endpoint: String) -> Result<Self, TxSitterError> {
        let relay_client = reqwest::Client::new();

        let rpc_provider = ProviderBuilder::new().on_http(rpc_endpoint.parse()?);
        Ok(Self {
            relay_endpoint: format!("{}/tx", relay_endpoint),
            relay_client,
            rpc_provider,
        })
    }

    pub async fn wait_for_tx_hash(&self, tx_id: String) -> Result<H256, TxSitterTransactionError> {
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
                return Err(TxSitterTransactionError::GetTransactionStatusError(
                    response.status(),
                ));
            }
        }
    }
}

impl TransactionRelay for TxSitterCient {
    type Error = TxSitterTransactionError;

    async fn send_transaction(&self, tx: TypedTransaction) -> Result<H256, Self::Error> {
        let payload = &SendTxRequest::try_from(tx)?;

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
            Err(TxSitterTransactionError::SendTransactionError(
                response.status(),
            ))
        }
    }

    async fn get_tx_receipt(&self, _tx_hash: H256) -> Result<TransactionReceipt, Self::Error> {
        unimplemented!()
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
    #[serde(with = "crate::relay::tx_sitter")]
    pub value: U256,
    #[serde(default)]
    pub data: Option<Bytes>,
    #[serde(with = "crate::relay::tx_sitter")]
    pub gas_limit: U256,
    #[serde(default)]
    pub priority: TransactionPriority,
    #[serde(default)]
    pub tx_id: Option<String>,
}

#[derive(Error, Debug)]
pub enum TxSitterError {
    #[error(transparent)]
    TxSitterTransactionError(TxSitterTransactionError),
    #[error(transparent)]
    UrlParseError(#[from] url::ParseError),
}

#[derive(Error, Debug)]
pub enum TxSitterTransactionError {
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
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
}

impl TryFrom<TypedTransaction> for SendTxRequest {
    type Error = TxSitterTransactionError;
    fn try_from(tx: TypedTransaction) -> Result<Self, Self::Error> {
        let to = *tx
            .to_addr()
            .ok_or(TxSitterTransactionError::ToAddressNotFound)?;

        let data = tx
            .data()
            .ok_or(TxSitterTransactionError::ToAddressNotFound)?;

        let gas_limit = *tx
            .gas()
            .ok_or(TxSitterTransactionError::ToAddressNotFound)?;

        Ok(SendTxRequest {
            to,
            value: U256::zero(),
            data: Some(data.to_owned()),
            gas_limit,
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
    pub tx_hash: Option<H256>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BlockTxStatus {
    Pending = 0,
    Mined = 1,
    Finalized = 2,
    Unsent = 3,
}

pub fn serialize<S>(u256: &U256, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let s = u256.to_string();
    serializer.serialize_str(&s)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: &str = serde::Deserialize::deserialize(deserializer)?;
    let u256 = U256::from_dec_str(s).map_err(serde::de::Error::custom)?;
    Ok(u256)
}
