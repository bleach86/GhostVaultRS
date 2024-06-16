use serde_json::Value;

pub mod config;
pub mod constants;
pub mod daemon_helper;
pub mod file_ops;
pub mod gv_client_methods;
pub mod gv_home_init;
pub mod gv_methods;
pub mod gvdb;
pub mod rpc;
pub mod task_runner;
pub mod term_link;
pub mod tg_bot {
    pub mod bot_tasks;
    pub mod keyboards;
    pub mod tg_bot;
    pub mod dialogs {
        pub mod chart_range_dialog;
        pub mod reward_interval_dialog;
        pub mod reward_min_dialog;
        pub mod reward_mode_dialog;
        pub mod utils;
    }
    pub mod charts {
        pub mod charts;
    }
}

use crate::daemon_helper::TxidAndWallet;

#[tarpc::service]
pub trait GvCLI {
    async fn getblockcount() -> Value;
    async fn shutdown() -> Value;
    async fn force_resync() -> Value;
    async fn set_reward_mode(mode: String, addr: Option<String>) -> Value;
    async fn set_payout_min(min: f64) -> Value;
    async fn get_ext_pub_key() -> Value;
    async fn set_reward_interval(interval: String) -> Value;
    async fn enable_telegram_bot(token: String, user: String) -> Value;
    async fn disable_telegram_bot() -> Value;
    async fn new_block(block_hash: String);
    async fn get_daemon_state() -> Value;
    async fn new_wallet_tx(txid_and_wal: TxidAndWallet);
    async fn process_daemon_update() -> Value;
    async fn process_payouts();
    async fn start_server_tasks();
    async fn set_bot_announce(msg_type: String, new_val: bool) -> Value;
    async fn get_version_info() -> Value;
    async fn check_chain() -> Value;
    async fn get_reward_options() -> Value;
    async fn validate_address(addr: String) -> Value;
    async fn get_daemon_online() -> Value;
    async fn get_stake_barchart_data(start: u64, end: u64, division: String) -> Value;
    async fn get_earnings_chart_data(start: u64, end: u64) -> Value;
    async fn set_timezone(timezone: String) -> Value;
    async fn get_pending_rewards() -> Value;
    async fn get_overview() -> Value;
    async fn get_mnemonic() -> Value;
    async fn import_wallet(mnemonic: String, name: String) -> Value;
    async fn new_remote_block(block_hash: String, height: u32);
}
