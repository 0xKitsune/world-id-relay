use std::str::FromStr;
pub use std::time::Duration;

pub use ethers::abi::{AbiEncode, Address};
pub use ethers::core::abi::Abi;
pub use ethers::core::k256::ecdsa::SigningKey;
pub use ethers::core::rand;
pub use ethers::prelude::{
    ContractFactory, Http, LocalWallet, NonceManagerMiddleware, Provider, Signer, SignerMiddleware,
    Wallet,
};
pub use ethers::providers::{Middleware, StreamExt};
pub use ethers::types::{Bytes, H256, U256};
pub use ethers::utils::{Anvil, AnvilInstance};
pub use serde::{Deserialize, Serialize};
pub use serde_json::json;
use state_bridge_relay::abi::IBridgedWorldID;
use state_bridge_relay::state_bridge::StateBridge;
use state_bridge_relay::state_bridge::StateBridgeService;
use test_utils::abi::RootAddedFilter;
use test_utils::chain_mock::{spawn_mock_chain, MockChain};
pub use tokio::spawn;
pub use tokio::task::JoinHandle;
pub use tracing::{error, info, instrument};

// test that spawns a mock anvil chain, deploys world id contracts, instantiates a `StateBridgeService`
// and propagates a root in order to see if the `StateBridgeService` works as intended.
#[tokio::test]
pub async fn test_relay_root() -> eyre::Result<()> {
    // we need anvil to be in scope in order for the middleware provider to not be dropped
    #[allow(unused_variables)]
    let MockChain {
        mock_state_bridge,
        mock_bridged_world_id,
        mock_world_id,
        middleware,
        anvil,
        wallet,
    } = spawn_mock_chain().await?;

    let relaying_period = std::time::Duration::from_secs(5);

    mock_state_bridge.propagate_root().send().await?.await?;

    let state_bridge_address = mock_state_bridge.address();

    let bridged_world_id_address = mock_bridged_world_id.address();

    let mut state_bridge_service =
        StateBridgeService::new(mock_world_id.address(), middleware.clone())
            .await
            .expect("couldn't create StateBridgeService");

    let bridged_world_id = IBridgedWorldID::new(bridged_world_id_address, middleware.clone());

    let state_bridge = StateBridge::new(
        state_bridge_address,
        wallet,
        middleware,
        bridged_world_id,
        relaying_period,
    )
    .unwrap();

    state_bridge_service.add_state_bridge(state_bridge);

    state_bridge_service
        .spawn()
        .expect("failed to spawn a state bridge service");

    let latest_root = U256::from_str("0x12312321321").expect("couldn't parse hexstring");

    mock_world_id.insert_root(latest_root).send().await?.await?;

    let await_matching_root = tokio::spawn(async move {
        let filter = mock_bridged_world_id.event::<RootAddedFilter>();

        let mut event_stream = filter.stream().await?.with_meta();

        // Listen to a stream of events, when a new event is received, update the root and block number
        while let Some(Ok((event, _))) = event_stream.next().await {
            let latest_bridged_root = event.root;

            if latest_bridged_root == latest_root {
                return Ok(latest_bridged_root);
            }
        }

        Err(eyre::eyre!(
            "The root in the event stream did not match `latest_root`"
        ))
    });

    let bridged_world_id_root = await_matching_root.await??;

    assert_eq!(latest_root, bridged_world_id_root);

    Ok(())
}
