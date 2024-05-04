## WorldID Relay

The `world-id-relay` is responsible for listening to new roots from the `WorldIdIdentityManager` and propagating them various L2s specified within the configuration file. To successfully propagate roots to a target L2, you will need a `StateBridge` contract deployed on L1, a `BridgedWorldID` contract deployed on the target L2 and a cross chain messaging protocol to send the root from L1 to the L2. For a detailed walkthrough on how to set up and deploy the necessary components, you can [read more here](https://worldcoin.org/blog/announcements/new-state-bridge-update-enables-permissionless-integration-world-id).

The `world-id-relay` uses a toml file to specify state bridge configurations. To see an example check out the [bin/relay.toml](./bin/relay.toml).


### Installation
To install the `world-id-relay`, clone this repo and run the following command.

```
cargo install --path .
```


### Usage 
```
Usage: world-id-relay [OPTIONS] --config <CONFIG> --private-key <PRIVATE_KEY>

Options:
  -c, --config <CONFIG>            Path to the TOML state bridge service config file
  -p, --private-key <PRIVATE_KEY>  Private key for account used to send `propagateRoot()` txs
  -h, --help                       Print help
```

#### Example Usage
```
world-id-relay --config bin/config/state_bridge.toml -p <PRIVATE_KEY>
```


## ------------------------------------
## Tests and Docs are still being written 🏗️.
Tests are still being written, assume bugs until tested. If you would like to help contribute on the tests or docs, feel free to open up an issue or make a PR.
## ------------------------------------