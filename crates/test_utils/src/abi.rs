use ethers::prelude::abigen;

abigen!(
    MockWorldID,
    "abi/MockWorldIDIdentityManager.json",
    event_derives(serde::Deserialize, serde::Serialize)
);

abigen!(
    MockStateBridge,
    "abi/MockStateBridge.json",
    event_derives(serde::Deserialize, serde::Serialize)
);

abigen!(
    MockBridgedWorldID,
    "abi/MockBridgedWorldID.json",
    event_derives(serde::Deserialize, serde::Serialize)
);
