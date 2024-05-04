use crate::abi::{self, IBridgedWorldID, IWorldIDIdentityManager, TreeChangedFilter};
use crate::error::StateBridgeError;
use crate::transaction;
use ethers::providers::Middleware;
use ethers::signers::{LocalWallet, Signer};
use ethers::types::H160;
use futures::StreamExt;
use ruint::Uint;
use semaphore::merkle_tree::Hasher;
use semaphore::poseidon_tree::PoseidonHash;
use std::ops::Sub;
use std::sync::Arc;
use tokio::select;
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};
use tracing::instrument;

pub type Hash = <PoseidonHash as Hasher>::Hash;

/// The number of block confirmations before a `propagateRoot()` transaction on L1 is considered finalized
pub const BLOCK_CONFIRMATIONS: usize = 2;

/// The `StateBridge` is responsible for monitoring root changes from the `WorldTree` on Layer 1, propagating the root to the target chain.
pub struct StateBridge<L1M: Middleware + 'static, L2M: Middleware + 'static> {
    // Address for the state bridge contract on layer 1
    l1_state_bridge: H160,
    // Wallet responsible for sending `propagateRoot` transactions
    wallet: LocalWallet,
    // Middleware to interact with layer 1
    l1_middleware: Arc<L1M>,
    /// Interface for the `BridgedWorldID` contract on the target chain
    pub l2_world_id: IBridgedWorldID<L2M>,
    /// Minimum time between `propagateRoot()` transactions
    pub relaying_period: Duration,
}

impl<L1M: Middleware + 'static, L2M: Middleware + 'static> StateBridge<L1M, L2M> {
    /// # Arguments
    ///
    /// * l1_state_bridge - Address for the state bridge contract on layer 1.
    /// * wallet - Wallet responsible for sending `propagateRoot` transactions.
    /// * l1_middleware - Middleware to interact with layer 1.
    /// * l2_world_id - Interface to the BridgedWorldID smart contract.
    /// * relaying_period - Duration between successive propagateRoot() invocations.
    /// * block_confirmations - Number of block confirmations required to consider a propagateRoot() transaction as finalized.
    pub fn new(
        l1_state_bridge: H160,
        wallet: LocalWallet,
        l1_middleware: Arc<L1M>,
        l2_world_id: IBridgedWorldID<L2M>,
        relaying_period: Duration,
    ) -> Result<Self, StateBridgeError<L1M, L2M>> {
        Ok(Self {
            l1_state_bridge,
            wallet,
            l1_middleware,
            l2_world_id,
            relaying_period,
        })
    }

    /// # Arguments
    ///
    /// * l1_state_bridge - Address for the state bridge contract on layer 1.
    /// * l1_middleware - Middleware to interact with layer 1.
    /// * `l2_world_id` - Address of the BridgedWorldID contract.
    /// * `l2_middleware` - Middleware to interact with layer 2.
    /// * `relaying_period` - Duration between `propagateRoot()` transactions.
    /// * `block_confirmations` - Number of block confirmations before a`propagateRoot()` transaction is considered finalized.
    pub fn new_from_parts(
        l1_state_bridge: H160,
        wallet: LocalWallet,
        l1_middleware: Arc<L1M>,
        //TODO: make this whole thing generic so that you could implement evm compatible or non evm compatible.
        l2_world_id: H160,
        l2_middleware: Arc<L2M>,
        relaying_period: Duration,
    ) -> Result<Self, StateBridgeError<L1M, L2M>> {
        let l2_world_id = IBridgedWorldID::new(l2_world_id, l2_middleware);

        Ok(Self {
            l1_state_bridge,
            wallet,
            l1_middleware,
            l2_world_id,
            relaying_period,
        })
    }

    /// Spawns a `StateBridge` task to listen for `TreeChanged` events from `WorldRoot` and propagate new roots.
    ///
    /// # Arguments
    ///
    /// * `root_rx` - Receiver channel for roots from `WorldRoot`.
    #[instrument(skip(self, root_rx))]
    pub fn spawn(
        &self,
        mut root_rx: tokio::sync::broadcast::Receiver<Hash>,
    ) -> JoinHandle<Result<(), StateBridgeError<L1M, L2M>>> {
        let l2_world_id = self.l2_world_id.clone();
        let l1_state_bridge = self.l1_state_bridge;
        let relaying_period = self.relaying_period;
        let wallet = self.wallet.clone();
        let l2_world_id_address = l2_world_id.address();
        let l1_middleware = self.l1_middleware.clone();

        tracing::info!(
            ?l2_world_id_address,
            ?l1_state_bridge,
            ?relaying_period,
            "Spawning bridge"
        );

        // Spawn a task to listen for new roots from the `WorldTree` and propagate them to the target chain
        tokio::spawn(async move {
            // Initialize the latest root and last propagation time
            let mut latest_root = Uint::from_limbs(
                l2_world_id
                    .latest_root()
                    .call()
                    .await
                    .map_err(StateBridgeError::L2ContractError)?
                    .0,
            );
            let mut last_propagation = Instant::now().sub(relaying_period);

            tracing::info!(?l1_state_bridge, ?latest_root, "Latest root");

            loop {
                // Wait for a new root or for the relaying period to elapse
                select! {
                    root = root_rx.recv() => {
                        tracing::info!(?l1_state_bridge, ?root, "Root received from rx");
                        latest_root = root?;
                    }

                    _ = tokio::time::sleep(relaying_period) => {
                        tracing::debug!(?l1_state_bridge, "Sleep time elapsed");
                    }
                }

                let time_since_last_propagation = Instant::now() - last_propagation;
                tracing::info!(
                    ?time_since_last_propagation,
                    ?relaying_period,
                    "Time since last propagation"
                );

                // If the relaying period has elapsed since the last propagation, proceed with root propagation
                if time_since_last_propagation >= relaying_period {
                    tracing::info!(?l1_state_bridge, "Relaying period elapsed");

                    let latest_bridged_root = Uint::from_limbs(
                        l2_world_id
                            .latest_root()
                            .call()
                            .await
                            .map_err(StateBridgeError::L2ContractError)?
                            .0,
                    );

                    tracing::info!(
                        ?l1_state_bridge,
                        ?latest_root,
                        ?latest_bridged_root,
                        "Latest root"
                    );

                    // If the latest root on the target chain is different from the latest root on the source chain, propagate the root
                    if latest_root != latest_bridged_root {
                        tracing::info!(
                            ?l1_state_bridge,
                            ?latest_root,
                            ?latest_bridged_root,
                            "Latest root != bridged root, propagating root"
                        );

                        Self::propagate_root(l1_state_bridge, &wallet, l1_middleware.clone())
                            .await?;

                        last_propagation = Instant::now();
                    } else {
                        tracing::info!(?l1_state_bridge, "Latest root already propagated");
                    }
                }
            }
        })
    }

    #[instrument(skip(wallet, l1_middleware))]
    pub async fn propagate_root(
        l1_state_bridge: H160,
        wallet: &LocalWallet,
        l1_middleware: Arc<L1M>,
    ) -> Result<(), StateBridgeError<L1M, L2M>> {
        let calldata = abi::ISTATEBRIDGE_ABI
            .function("propagateRoot")?
            .encode_input(&[])?;

        let tx = transaction::fill_and_simulate_eip1559_transaction(
            calldata.into(),
            l1_state_bridge,
            wallet.address(),
            wallet.chain_id(),
            l1_middleware.clone(),
        )
        .await?;

        transaction::sign_and_send_transaction(tx, wallet, BLOCK_CONFIRMATIONS, l1_middleware)
            .await?;

        Ok(())
    }
}

/// Monitors the world tree root for changes and propagates new roots to target Layer 2s
pub struct StateBridgeService<L1M: Middleware + 'static, L2M: Middleware + 'static> {
    /// Monitors `TreeChanged` events from `WorldIDIdentityManager` and broadcasts new roots to through the `root_tx`.
    pub world_id_identity_manager: IWorldIDIdentityManager<L1M>,
    /// Vec of `StateBridge`, responsible for root propagation to target Layer 2s.
    pub state_bridges: Vec<StateBridge<L1M, L2M>>,
}

impl<L1M, L2M> StateBridgeService<L1M, L2M>
where
    L1M: Middleware,
    L2M: Middleware,
{
    pub async fn new(
        world_tree_address: H160,
        middleware: Arc<L1M>,
    ) -> Result<Self, StateBridgeError<L1M, L2M>> {
        let world_id_identity_manager =
            IWorldIDIdentityManager::new(world_tree_address, middleware.clone());
        Ok(Self {
            world_id_identity_manager,
            state_bridges: vec![],
        })
    }

    /// Adds a `StateBridge` to orchestrate root propagation to a target Layer 2.
    pub fn add_state_bridge(&mut self, state_bridge: StateBridge<L1M, L2M>) {
        self.state_bridges.push(state_bridge);
    }

    /// Spawns the `WorldTreeRoot` task which will listen to changes to the `WorldIDIdentityManager`
    /// [merkle tree root](https://github.com/worldcoin/world-id-contracts/blob/852790da8f348d6a2dbb58d1e29123a644f4aece/src/WorldIDIdentityManagerImplV1.sol#L63).
    #[instrument(skip(self))]
    pub fn listen_for_new_roots(
        &self,
        root_tx: tokio::sync::broadcast::Sender<Hash>,
    ) -> JoinHandle<Result<(), StateBridgeError<L1M, L2M>>> {
        let world_id_identity_manager = self.world_id_identity_manager.clone();

        let world_id_identity_manager_address = world_id_identity_manager.address();
        tracing::info!(?world_id_identity_manager_address, "Spawning root");

        let root_tx_clone = root_tx.clone();
        tokio::spawn(async move {
            // Get the latest root from the WorldIdIdentityManager and send it through the channel
            let latest_root = world_id_identity_manager
                .latest_root()
                .call()
                .await
                .map_err(StateBridgeError::L1ContractError)?;

            root_tx_clone.send(Uint::from_limbs(latest_root.0))?;

            // Initialize filter to listen for tree changed events
            let filter = world_id_identity_manager.event::<TreeChangedFilter>();

            let mut event_stream = filter
                .stream()
                .await
                .map_err(StateBridgeError::L1ContractError)?
                .with_meta();

            // When a new event is received, send the new root through the channel
            while let Some(Ok((event, _))) = event_stream.next().await {
                let new_root = event.post_root.0;
                tracing::info!(?new_root, "New root from chain");
                root_tx_clone.send(Uint::from_limbs(new_root))?;
            }

            Ok(())
        })
    }

    /// Spawns the `StateBridgeService`.
    pub fn spawn(
        &mut self,
    ) -> Result<Vec<JoinHandle<Result<(), StateBridgeError<L1M, L2M>>>>, StateBridgeError<L1M, L2M>>
    {
        if self.state_bridges.is_empty() {
            return Err(StateBridgeError::BridgesNotInitialized);
        }

        let (root_tx, _) = tokio::sync::broadcast::channel::<Hash>(60);

        let mut handles = vec![];
        // Bridges are spawned before the root so that the broadcast channel has active subscribers before the sender is spawned to avoid a SendError
        for bridge in self.state_bridges.iter() {
            handles.push(bridge.spawn(root_tx.subscribe()));
        }

        handles.push(self.listen_for_new_roots(root_tx));

        Ok(handles)
    }
}
