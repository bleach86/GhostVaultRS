pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DAEMON_BASE_URL: &str = "https://github.com/ghost-coin/ghost-core/releases/download/";
pub const LATEST_RELEASE_URL: &str = "https://github.com/ghost-coin/ghost-core/releases/latest";
pub const TMP_PATH: &str = "/tmp/GhostVault";
pub const DEFAULT_GV_DIR: &str = "~/.ghostvault/";
pub const DEFAULT_DAEMON_DIR: &str = "~/.ghost/";
pub const DAEMON_PID_FILE: &str = "ghost.pid";
pub const GV_PID_FILE: &str = "ghostvault.pid";
pub const GV_SETTINGS_FILE: &str = "gv_settings.toml";
pub const DAEMON_SETTINGS_FILE: &str = "ghost.conf";
pub const DEFAULT_COLD_WALLET: &str = "GV_COLD";
pub const DEFAULT_HOT_WALLET: &str = "GV_HOT";
pub const DEFAULT_DEAMON_UPDATE: u64 = 60 * 60 * 2; // 2 hours
pub const DEFAULT_SELF_UPDATE: u64 = 60 * 60 * 2; // 2 hours
pub const DEFAULT_PROCESS_REWARDS: i64 = 60 * 15; // 15 minutes
pub const DEFAULT_MIN_PAYOUT: u64 = 10000000; // 0.10000000 Ghost
pub const MIN_TX_VALUE: u64 = 10000000; // 0.10000000 Ghost
pub const MAX_TX_FEES: u64 = 25000000; // 0.25000000 Ghost
pub const AGVR_ACTIVATION_HEIGHT: u32 = 591621;
pub const DEV_FUND_ADDRESS: [&str; 5] = [
    "GgtiuDqVxAzg47yW7oSMmophe3tU8qoE1f",
    "GQJ4unJi6hAzd881YM17rEzPNWaWZ4AR3f",
    "Ga7ECMeX8QUJTTvf9VUnYgTQUFxPChDqqU",
    "GQtToV2LnHGhHy4LRVapLDMaukdDgzZZZV",
    "GSo4N8Q4QTHoC2eWnDQkm86Vs1FhcMGE3Y",
];
