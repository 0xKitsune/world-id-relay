use clap::Parser;
use ethers::middleware::gas_escalator::{Frequency, GasEscalatorMiddleware, GeometricGasPrice};
use ethers::prelude::{Http, LocalWallet, NonceManagerMiddleware, Provider, Signer, H160};
use ethers::providers::Middleware;
use ethers::types::U256;
use ethers_throttle::ThrottledJsonRpcClient;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use governor::Jitter;
use opentelemetry::global::shutdown_tracer_provider;
use state_bridge_relay::config::ServiceConfig;
use state_bridge_relay::state_bridge::{StateBridge, StateBridgeService};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use telemetry_batteries::metrics::statsd::StatsdBattery;
use telemetry_batteries::tracing::datadog::DatadogBattery;
use telemetry_batteries::tracing::TracingShutdownHandle;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use url::Url;

#[derive(Parser, Debug)]
#[clap(
    name = "State Bridge Service",
    about = "The state bridge service listens to root changes from the `WorldIdIdentityManager` and propagates them to each of the corresponding Layer 2s specified in the configuration file."
)]
struct Opts {
    /// Path to the configuration file
    #[clap(short, long)]
    config: Option<PathBuf>,
    #[clap(
        short,
        long,
        help = "Private key for account used to send `propagateRoot()` txs"
    )]
    private_key: String,
    #[clap(short, long, help = "Enable datadog backend for instrumentation")]
    datadog: bool,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let opts = Opts::parse();

    let config = ServiceConfig::load(opts.config.as_deref())?;

    let _tracing_shutdown_handle = if let Some(telemetry) = &config.telemetry {
        let tracing_shutdown_handle = DatadogBattery::init(
            telemetry.traces_endpoint.as_deref(),
            &telemetry.service_name,
            None,
            true,
        );

        if let Some(metrics_config) = &telemetry.metrics {
            StatsdBattery::init(
                &metrics_config.host,
                metrics_config.port,
                metrics_config.queue_size,
                metrics_config.buffer_size,
                Some(&metrics_config.prefix),
            )?;
        }

        tracing_shutdown_handle
    } else {
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().pretty().compact())
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .init();

        TracingShutdownHandle
    };

    let mut wallet = opts.private_key.parse::<LocalWallet>()?;
    let l1_middleware = initialize_l1_middleware(
        &config.canonical_tree.provider.rpc_endpoint,
        config.canonical_tree.provider.throttle,
        wallet.address(),
        config.max_gas_price,
    )
    .await?;

    let chain_id = l1_middleware.get_chainid().await?.as_u64();
    wallet = wallet.with_chain_id(chain_id);

    let mut state_bridge_service =
        StateBridgeService::new(config.canonical_tree.address, l1_middleware.clone()).await?;

    for bridge_config in config.state_bridge.iter() {
        let l2_middleware = initialize_l2_middleware(
            &bridge_config.provider.rpc_endpoint,
            bridge_config.provider.throttle,
        )
        .await?;

        let relaying_period_seconds = Duration::from_secs(bridge_config.relaying_period_seconds);
        let state_bridge = StateBridge::new_from_parts(
            bridge_config.state_bridge,
            wallet.clone(),
            l1_middleware.clone(),
            bridge_config.bridged_world_id,
            l2_middleware,
            relaying_period_seconds,
        )?;

        state_bridge_service.add_state_bridge(state_bridge);
    }

    tracing::info!("Spawning state bridge service");
    let handles = state_bridge_service.spawn()?;

    let mut handles = handles.into_iter().collect::<FuturesUnordered<_>>();
    while let Some(result) = handles.next().await {
        tracing::error!("StateBridgeError: {:?}", result);

        result??;
    }

    shutdown_tracer_provider();

    Ok(())
}

pub async fn initialize_l1_middleware(
    rpc_endpoint: &Url,
    throttle: u32,
    wallet_address: H160,
    max_gas_price: Option<U256>,
) -> eyre::Result<
    Arc<GasEscalatorMiddleware<NonceManagerMiddleware<Provider<ThrottledJsonRpcClient<Http>>>>>,
> {
    let provider = initialize_throttled_provider(rpc_endpoint, throttle)?;
    let nonce_manager_middleware = NonceManagerMiddleware::new(provider, wallet_address);

    let geometric_escalator = GeometricGasPrice::new(1.125, 60_u64, max_gas_price);
    let gas_escalator_middleware = GasEscalatorMiddleware::new(
        nonce_manager_middleware,
        geometric_escalator,
        Frequency::PerBlock,
    );

    Ok(Arc::new(gas_escalator_middleware))
}

pub async fn initialize_l2_middleware(
    l2_rpc_endpoint: &Url,
    throttle: u32,
) -> eyre::Result<Arc<Provider<ThrottledJsonRpcClient<Http>>>> {
    Ok(Arc::new(initialize_throttled_provider(
        l2_rpc_endpoint,
        throttle,
    )?))
}

pub fn initialize_throttled_provider(
    rpc_endpoint: &Url,
    throttle: u32,
) -> eyre::Result<Provider<ThrottledJsonRpcClient<Http>>> {
    let http_provider = Http::new(rpc_endpoint.clone());
    let throttled_http_provider = ThrottledJsonRpcClient::new(
        http_provider,
        throttle,
        Some(Jitter::new(
            Duration::from_millis(10),
            Duration::from_millis(100),
        )),
    );

    Ok(Provider::new(throttled_http_provider))
}
