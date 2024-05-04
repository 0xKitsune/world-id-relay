use std::path::Path;

use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use url::Url;

pub const CONFIG_PREFIX: &str = "WLD";

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    pub canonical_tree: TreeConfig,
    #[serde(with = "map_vec", default)]
    pub state_bridge: Vec<StateBridgeConfig>,
    #[serde(default)]
    pub telemetry: Option<TelemetryConfig>,
    #[serde(default)]
    pub max_gas_price: Option<U256>,
}

impl ServiceConfig {
    pub fn load(config_path: Option<&Path>) -> eyre::Result<Self> {
        let mut settings = config::Config::builder();

        if let Some(path) = config_path {
            settings = settings.add_source(config::File::from(path).required(true));
        }

        let settings = settings
            .add_source(
                config::Environment::with_prefix(CONFIG_PREFIX)
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        let config = settings.try_deserialize::<Self>()?;

        Ok(config)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TreeConfig {
    pub address: Address,
    #[serde(default = "default::window_size")]
    pub window_size: u64,
    #[serde(default)]
    pub creation_block: u64,
    pub provider: ProviderConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateBridgeConfig {
    pub state_bridge: Address,
    pub bridged_world_id: Address,
    #[serde(default = "default::window_size")]
    pub window_size: u64,
    pub provider: ProviderConfig,
    #[serde(default)]
    pub relaying_period_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    /// Ethereum RPC endpoint
    #[serde(with = "crate::utils::url")]
    pub rpc_endpoint: Url,
    #[serde(default = "default::provider_throttle")]
    pub throttle: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    // Service name - used for logging, metrics and tracing
    pub service_name: String,
    // Traces
    pub traces_endpoint: Option<String>,
    // Metrics
    pub metrics: Option<MetricsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub host: String,
    pub port: u16,
    pub queue_size: usize,
    pub buffer_size: usize,
    pub prefix: String,
}

mod default {

    pub fn window_size() -> u64 {
        5000
    }

    pub fn provider_throttle() -> u32 {
        150
    }
}

// Utility functions to convert map to vec
mod map_vec {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        let v: BTreeMap<String, T> = Deserialize::deserialize(deserializer)?;

        Ok(v.into_values().collect())
    }
}
