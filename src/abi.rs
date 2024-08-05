use alloy::sol;

sol! {
    #[derive(Debug, PartialEq, Eq)]
    #[sol(rpc)]
    contract IBridgedWorldID {
        function latestRoot() public view virtual returns (uint256);
         event RootAdded(uint256 root, uint128 timestamp);
        error NoRootsSeen();
    }

    #[derive(Debug, PartialEq, Eq)]
    #[sol(rpc)]
    contract IStateBridge {
        function propagateRoot() external;
    }

}

// NOTE: we need two sol! macros because there are two `latestRoot` functions
sol! {
    #[derive(Debug, PartialEq, Eq)]
    #[sol(rpc)]
    contract IWorldIDIdentityManager {
        function latestRoot() external returns (uint256);
        event TreeChanged(uint256 indexed preRoot, uint8 indexed kind, uint256 indexed postRoot);
    }
}
