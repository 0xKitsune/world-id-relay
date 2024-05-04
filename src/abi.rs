use ethers::middleware::contract::abigen;

abigen!(
    IWorldIDIdentityManager,
    r#"[
        function latestRoot() external returns (uint256)
        event TreeChanged(uint256 indexed preRoot, uint8 indexed kind, uint256 indexed postRoot)
    ]"#;

    IStateBridge,
    r#"[
        function propagateRoot() external
    ]"#;

    IBridgedWorldID,
    r#"[
        event RootAdded(uint256 root, uint128 timestamp)
        function latestRoot() public view virtual returns (uint256)
        error NoRootsSeen()
    ]"#,
    event_derives(serde::Deserialize, serde::Serialize)

);
