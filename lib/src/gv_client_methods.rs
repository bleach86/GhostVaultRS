use context::Context;
use core::time;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tarpc::{client, context, tokio_serde::formats::Json};
use tracing::Instrument;
extern crate colored;
use crate::{constants::VERSION, daemon_helper::TxidAndWallet, GvCLIClient};
use colored::*;
use log::error;
use std::{process::Command as Cmd, time::SystemTime};

fn clear_screen() {
    let _command: Result<std::process::ExitStatus, std::io::Error> =
        Cmd::new("sh").arg("-c").arg("clear").status();
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GVStatus {
    pub uptime: String,
    pub privacy_mode: String,
    pub daemon_version: String,
    pub latest_release: String,
    pub daemon_uptime: String,
    pub daemon_peers: u16,
    pub daemon_synced: String,
    pub best_block: u32,
    pub best_block_hash: String,
    pub best_block_extern: u32,
    pub good_chain: String,
    pub staking_enabled: String,
    pub active_staking: String,
    pub staking_difficulty: f64,
    pub network_stake_weight: f64,
    pub currently_staking: f64,
    pub total_coldstaking: f64,
    pub last_stake: String,
    pub stakes_24: u32,
    pub rewards_24: f64,
    pub agvr_24: f64,
    pub total_24: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingRewards {
    pub total_pending: f64,
    pub staked: f64,
    pub pending_anonymization: f64,
    pub pending_anon_confs: f64,
    pub pending_payout: f64,
    pub payout_run_interval: String,
    pub next_payout_run: String,
    pub min_payout: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BarChart {
    pub data: Vec<Vec<u64>>,
    pub division: String,
    pub start: String,
    pub end: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllTimeEarnigns {
    pub data: Vec<Vec<f64>>,
    pub start: String,
    pub end: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakeTotals {
    pub stakes: u32,
    pub rewards: f64,
    pub agvr: f64,
    pub total: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakingData {
    pub total_staking: f64,
    pub total_coldstaking: f64,
    pub stakes_24h: StakeTotals,
    pub stakes_ytd: StakeTotals,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakingDataOverview {
    pub total_staking: f64,
    pub total_coldstaking: f64,
    pub stakes_24h: StakeTotals,
    pub stakes_7d: StakeTotals,
    pub stakes_14d: StakeTotals,
    pub stakes_30d: StakeTotals,
    pub stakes_90d: StakeTotals,
    pub stakes_180d: StakeTotals,
    pub stakes_ytd: StakeTotals,
    pub stakes_1y: StakeTotals,
    pub stakes_all: StakeTotals,
}

#[derive(Debug, Clone)]
pub struct CLICaller {
    client: GvCLIClient,
    json_out: bool,
    timeout: time::Duration,
}

impl CLICaller {
    pub async fn new(
        cli_address: &str,
        json_out: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let transport =
            match tarpc::serde_transport::tcp::connect(&cli_address, Json::default).await {
                Ok(transport) => transport,
                Err(err) => {
                    error!("Failed to connect to GhostVault server at: {}", cli_address);
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        err,
                    )));
                }
            };

        let client: GvCLIClient = GvCLIClient::new(client::Config::default(), transport).spawn();

        let timeout: time::Duration = std::time::Duration::from_secs(45);

        Ok(CLICaller {
            client,
            json_out,
            timeout,
        })
    }

    pub async fn call_getblockcount(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let daemon_check = tokio::select! {
            res1 = self.client.get_daemon_online(ctx) => { res1 }
            res2 = self.client.get_daemon_online(ctx) => { res2 }
        };

        match daemon_check {
            Ok(result) => {
                if result.is_object() {
                    let res_obj = serde_json::to_string_pretty(&result).unwrap();
                    let msg = format!("GhostVault Not Ready!\n{}", res_obj);
                    self.display_result(&msg);
                    return Ok(result);
                }
            }
            Err(e) => return Err(e.into()),
        }

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.getblockcount(ctx) => { res1 }
                res2 = self.client.getblockcount(ctx) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call getblockcount"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(&result.as_u64().unwrap().to_string());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_new_block(
        &self,
        new_block: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let _result: Result<(), client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.new_block(ctx, new_block.clone()) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call new block"))
        .await;

        Ok(())
    }

    pub async fn call_new_remote_block(
        &self,
        block_hash: String,
        height: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let _result: Result<(), client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.new_remote_block(ctx, block_hash.clone(), height) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call new remote block"))
        .await;

        Ok(())
    }

    pub async fn call_new_wallet_tx(
        &self,
        txid_and_wal: TxidAndWallet,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let _result: Result<(), client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.new_wallet_tx(ctx, txid_and_wal.clone()) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call new wallet tx"))
        .await;

        Ok(())
    }

    pub async fn call_get_daemon_state(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let daemon_check = tokio::select! {
            res1 = self.client.get_daemon_online(ctx) => { res1 }
            res2 = self.client.get_daemon_online(ctx) => { res2 }
        };

        match daemon_check {
            Ok(result) => {
                if result.is_object() {
                    let res_obj = serde_json::to_string_pretty(&result).unwrap();
                    let msg = format!("GhostVault Not Ready!\n{}", res_obj);
                    self.display_result(&msg);
                    return Ok(result);
                }
            }
            Err(e) => return Err(e.into()),
        }
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_daemon_state(ctx) => { res1 }
                //res2 = self.client.get_daemon_state(ctx) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call getblockcount"))
        .await;

        match result {
            Ok(result) => {
                if !self.json_out {
                    display_stats_page(&result);
                }

                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_shutdown(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.shutdown(ctx) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call shutdown"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_enable_telegram_bot(
        &self,
        token: String,
        user: String,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.enable_telegram_bot(ctx, token, user) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call enable_telegram_bot"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_disable_telegram_bot(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.disable_telegram_bot(ctx) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call disable_telegram_bot"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_set_reward_interval(
        &self,
        interval: String,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.set_reward_interval(ctx, interval) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call set_reward_interval"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_get_ext_pub_key(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_ext_pub_key(ctx) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call get_ext_pub_key"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_set_payout_min(
        &self,
        min_payout: f64,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.set_payout_min(ctx, min_payout) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call set_payout_min"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_set_reward_mode(
        &self,
        mode: String,
        addr: Option<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let daemon_check = tokio::select! {
            res1 = self.client.get_daemon_online(ctx) => { res1 }
            res2 = self.client.get_daemon_online(ctx) => { res2 }
        };

        match daemon_check {
            Ok(result) => {
                if result.is_object() {
                    let res_obj = serde_json::to_string_pretty(&result).unwrap();
                    let msg = format!("GhostVault Not Ready!\n{}", res_obj);
                    self.display_result(&msg);
                    return Ok(result);
                }
            }
            Err(e) => return Err(e.into()),
        }

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.set_reward_mode(ctx, mode, addr) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call set_reward_mode"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_process_daemon_update(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.process_daemon_update(ctx) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call process_daemon_update"))
        .await;

        match result {
            Ok(result) => {
                if result.is_boolean() {
                    self.display_result(&result.as_bool().unwrap().to_string());
                } else {
                    self.display_result(result.as_str().unwrap());
                }

                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_get_reward_options(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_reward_options(ctx) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call get_reward_options"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(&serde_json::to_string_pretty(&result).unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_validate_address(
        &self,
        addr: String,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let daemon_check = tokio::select! {
            res1 = self.client.get_daemon_online(ctx) => { res1 }
            res2 = self.client.get_daemon_online(ctx) => { res2 }
        };

        match daemon_check {
            Ok(result) => {
                if result.is_object() {
                    let res_obj = serde_json::to_string_pretty(&result).unwrap();
                    let msg = format!("GhostVault Not Ready!\n{}", res_obj);
                    self.display_result(&msg);
                    return Ok(result);
                }
            }
            Err(e) => return Err(e.into()),
        }

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.validate_address(ctx, addr) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call validate_address"))
        .await;

        match result {
            Ok(result) => {
                let res = serde_json::to_string_pretty(&result).unwrap();
                self.display_result(&res);
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_get_pending_rewards(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let daemon_check = tokio::select! {
            res1 = self.client.get_daemon_online(ctx) => { res1 }
            res2 = self.client.get_daemon_online(ctx) => { res2 }
        };

        match daemon_check {
            Ok(result) => {
                if result.is_object() {
                    let res_obj = serde_json::to_string_pretty(&result).unwrap();
                    let msg = format!("GhostVault Not Ready!\n{}", res_obj);
                    self.display_result(&msg);
                    return Ok(result);
                }
            }
            Err(e) => return Err(e.into()),
        }

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_pending_rewards(ctx) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call get_pending_rewards"))
        .await;

        match result {
            Ok(result) => {
                let res = serde_json::to_string_pretty(&result).unwrap();
                self.display_result(&res);
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_process_reward_payout(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let _result: Result<(), client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.process_payouts(ctx) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call process_payouts"))
        .await;

        Ok(())
    }

    pub async fn call_start_server_tasks(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let _result: Result<(), client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.start_server_tasks(ctx) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call start_server_tasks"))
        .await;

        Ok(())
    }

    pub async fn call_get_version_info(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_version_info(ctx) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call get_version_info"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(&serde_json::to_string_pretty(&result).unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_check_chain(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.check_chain(ctx) => { res1 }
            }
        }
        .instrument(tracing::info_span!("call check_chain"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(&serde_json::to_string_pretty(&result).unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_set_bot_announce(
        &self,
        msg_type: String,
        new_val: bool,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.set_bot_announce(ctx, msg_type, new_val) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call set_bot_announce"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_get_overview(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let daemon_check = tokio::select! {
            res1 = self.client.get_daemon_online(ctx) => { res1 }
            res2 = self.client.get_daemon_online(ctx) => { res2 }
        };

        match daemon_check {
            Ok(result) => {
                if result.is_object() {
                    let res_obj = serde_json::to_string_pretty(&result).unwrap();
                    let msg = format!("GhostVault Not Ready!\n{}", res_obj);
                    self.display_result(&msg);
                    return Ok(result);
                }
            }
            Err(e) => return Err(e.into()),
        }

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_overview(ctx) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call get_overview"))
        .await;

        match result {
            Ok(result) => {
                let staking_data: StakingDataOverview =
                    serde_json::from_value(result.to_owned()).unwrap();
                self.display_result(&serde_json::to_string_pretty(&staking_data).unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_get_mnemonic(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_mnemonic(ctx) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call get_mnemonic"))
        .await;

        match result {
            Ok(result) => {
                if result.is_string() {
                    self.display_result(result.as_str().unwrap());
                } else {
                    self.display_result("Failed to retrieve mnemonic!");
                }
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_import_wallet(
        &self,
        mnemonic: String,
        wallet_name: String,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + time::Duration::from_secs(60 * 120);

        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.import_wallet(ctx, mnemonic, wallet_name) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call import_wallet"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_get_earnings_chart_data(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_earnings_chart_data(ctx, start, end) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call get_earnings_chart_data"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.to_string().as_str());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_get_stake_barchart_data(
        &self,
        start: u64,
        end: u64,
        division: String,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.get_stake_barchart_data(ctx, start, end, division) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call get_stake_heatmap_data"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.to_string().as_str());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_force_resync(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.force_resync(ctx) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call force_resync"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn call_set_timezone(
        &self,
        timezone: String,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut ctx: Context = context::current();
        ctx.deadline = SystemTime::now() + self.timeout;
        let result: Result<Value, client::RpcError> = async move {
            // Send the request twice, just to be safe! ;)
            tokio::select! {
                res1 = self.client.set_timezone(ctx, timezone) => { res1 }
                //res2 = self.client.new_block(context::current(), new_block.clone()) => { res2 }
            }
        }
        .instrument(tracing::info_span!("call set_timezone"))
        .await;

        match result {
            Ok(result) => {
                self.display_result(result.as_str().unwrap());
                Ok(result)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn display_result(&self, result: &str) {
        if !self.json_out {
            println!("{}", result);
        }
    }
}

fn display_stats_page(gv_info: &Value) {
    clear_screen();
    let border = "#".repeat(80).blue();
    let status: GVStatus = serde_json::from_value(gv_info.to_owned()).unwrap();
    let privacy_mode = if status.privacy_mode == "ANON" {
        status.privacy_mode.green()
    } else {
        status.privacy_mode.yellow()
    };

    let curr_ver_int = status
        .daemon_version
        .replace(".", "")
        .parse::<u64>()
        .unwrap();

    let latest_version_int = status
        .latest_release
        .replace(".", "")
        .parse::<u64>()
        .unwrap();

    let node_up_to_date = curr_ver_int >= latest_version_int;

    let current_ver = if node_up_to_date {
        status.daemon_version.green()
    } else {
        status.daemon_version.red()
    };

    let up_to_date = bool_to_yn(node_up_to_date);

    let peers = if status.daemon_peers <= 2 {
        status.daemon_peers.to_string().red()
    } else if status.daemon_peers <= 5 {
        status.daemon_peers.to_string().yellow()
    } else {
        status.daemon_peers.to_string().green()
    };

    let current_staking = if status.currently_staking > 0.0 {
        status.currently_staking.to_string().green()
    } else {
        status.currently_staking.to_string().red()
    };

    let total_cold = if status.total_coldstaking > 0.0 {
        status.total_coldstaking.to_string().green()
    } else {
        status.total_coldstaking.to_string().red()
    };

    let stakes = if status.stakes_24 > 0 {
        status.stakes_24.to_string().green()
    } else {
        status.stakes_24.to_string().red()
    };

    let earned = if status.total_24 > 0.0 {
        status.total_24.to_string().green()
    } else {
        status.total_24.to_string().red()
    };

    let version = format!("v{}", VERSION);

    let formatted_string = format!(
        "\n{}\nGhostVaultRS {}\nUptime/Load Average {:>45}\nprivacy mode {:>52}\nghostd version {:>50}\nghostd up-to-date {:>47}\nghostd running {:>50}\nghostd uptime {:>51}\nghostd responding (RPC) {:>41}\nghostd peers {:>52}\nghostd blocks synced {:>44}\nlast block (local ghostd) {:>39}\n   (SHELTRPointe network) {:>39}\nghostd is good chain {:>44}\nghostd staking enabled {:>42}\nghostd staking currently? {:>39}\nghostd staking difficulty {:>39}\nghostd network stakeweight {:>38}\ncurrently staking {:>47}\ntotal in coldstaking {:>44}\nstakes/earned last 24h {:>30}/{}\n{}",
        border,
        version,
        status.uptime.green(),
        privacy_mode,
        current_ver,
        color_yn(up_to_date),
        "YES".green(),
        status.daemon_uptime.green(),
        "YES".green(),
        peers,
        color_yn(status.daemon_synced),
        status.best_block.to_string().green(),
        status.best_block_extern.to_string().green(),
        color_yn(status.good_chain),
        color_yn(status.staking_enabled),
        color_yn(status.active_staking),
        status.staking_difficulty.to_string().green(),
        status.network_stake_weight.to_string().green(),
        current_staking,
        total_cold,
        stakes,
        earned,
        border
    );

    println!("{}", formatted_string);
}

fn bool_to_yn(bool_val: bool) -> String {
    let new_val: &str = if bool_val { "YES" } else { "NO" };
    new_val.to_string()
}

fn color_yn(item: String) -> ColoredString {
    let res = if item.contains("YES") {
        item.green()
    } else {
        item.red()
    };

    res
}
