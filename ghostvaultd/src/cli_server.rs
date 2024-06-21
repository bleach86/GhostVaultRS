#![allow(dead_code)]
use chrono::{DateTime, Datelike, Days, Months, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use futures::{future, prelude::*};
use humantime::{format_duration, FormattedDuration};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use service::{
    config::GVConfig,
    constants::{GV_PID_FILE, MIN_TX_VALUE, TMP_PATH, VERSION},
    daemon_helper::{listen_for_events, listen_zmq, DaemonHelper, DaemonState, TxidAndWallet},
    file_ops,
    gv_client_methods::{
        AllTimeEarnigns, BarChart, GVStatus, PendingRewards, StakeTotals, StakingData,
        StakingDataOverview,
    },
    gv_methods::{self, PathAndDigest},
    gvdb::{
        AddressInfo, DaemonStatusDB, NewStakeStatusDB, RewardsDB, ServerReadyDB, TgBotQueueDB,
        ZapStatusDB, GVDB,
    },
    task_runner,
    task_runner::task_runner,
    GvCLI,
};
use std::{env, net::IpAddr, path::PathBuf, sync::Arc, time::Duration};
use systemstat::{LoadAverage, Platform, System};
use tarpc::{
    context,
    server::{incoming::Incoming, BaseChannel, Channel},
    tokio_serde::formats::Json,
};
use tokio::sync::{Mutex as async_Mutex, RwLock as async_RwLock};

pub struct CpuLoad {
    pub one: f32,
    pub five: f32,
    pub fifteen: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewStake {
    pub height: u32,
    pub block_hash: String,
    pub txid: String,
    pub reward: f64,
    pub agvr_reward: f64,
    pub total_reward: f64,
    pub staking_data: StakingData,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VersionInfo {
    pub gv_version: String,
    pub ghostd_version: String,
    pub latest_release: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RewardOptions {
    pub reward_mode: String,
    pub reward_interval: String,
    pub reward_address: String,
    pub reward_min: f64,
}

#[derive(Clone, Debug)]
struct GvCLIServer {
    daemon: DaemonHelper,
    db: Arc<GVDB>,
    gv_config: Arc<async_RwLock<GVConfig>>,
    daemon_state: Arc<async_Mutex<DaemonState>>,
    tg_bot_active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LastStake {
    pub last_stake_str: String,
    pub timestamp: Option<u64>,
}

impl GvCLIServer {
    async fn new(gv_config: &Arc<async_RwLock<GVConfig>>, db: &Arc<GVDB>) -> Self {
        info!("Starting the GhostVault CLI server...");

        let conf = gv_config.read().await;

        if conf.rpc_wallet.is_empty() {
            panic!("No wallet set in config file!")
        }

        let cli_address: String = conf.cli_address.clone();
        let tg_bot_active: bool = conf.bot_token.is_some() && conf.tg_user.is_some();

        drop(conf);

        let gv_config_clone_task: Arc<async_RwLock<GVConfig>> = Arc::clone(&gv_config);
        let gv_config_clone_zmq: Arc<async_RwLock<GVConfig>> = Arc::clone(&gv_config);
        let gv_config_clone_sio: Arc<async_RwLock<GVConfig>> = Arc::clone(&gv_config);

        let daemon: DaemonHelper = DaemonHelper::new(&gv_config, "cold").await;

        let blockchain_info: Value = daemon.call_status(true).await.unwrap();
        let online: bool = true;

        let synced: bool = !daemon.is_syncing().await.unwrap();

        let best_block: u32 = blockchain_info["blocks"].as_u64().unwrap() as u32;
        let best_block_hash: String = blockchain_info["bestblockhash"]
            .as_str()
            .unwrap()
            .to_string();

        let (remote_bc_info, remote_block_hash, latest_release) = loop {
            let res = tokio::try_join!(
                gv_methods::get_remote_block_chain_info(),
                gv_methods::get_remote_block_hash(best_block),
                gv_methods::get_latest_release()
            );

            match res {
                Ok((bc_info, block_hash, latest_release)) => {
                    break (bc_info, block_hash, latest_release)
                }
                Err(e) => {
                    error!("Error fetching remote blockchain info: {}", e);
                    error!("Retrying in 30 seconds...");
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
            }
        };

        let remote_best_block: u32 = remote_bc_info["blocks"].as_u64().unwrap() as u32;
        let remote_best_block_hash: String = remote_bc_info["bestblockhash"]
            .as_str()
            .unwrap()
            .to_string();

        let good_chain: bool = remote_block_hash == best_block_hash;

        let version: String = daemon.get_daemon_version().await.unwrap();

        let daemon_state: Arc<async_Mutex<DaemonState>> = Arc::new(async_Mutex::new(DaemonState {
            online,
            version,
            synced,
            available: true,
            good_chain,
            latest_release,
            best_block,
            best_block_hash,
            remote_best_block,
            remote_best_block_hash,
            cycle: 0,
        }));

        let cloned_db: Arc<GVDB> = Arc::clone(&db);
        let zmq_db = Arc::clone(&db);
        let sio_db = Arc::clone(&db);

        daemon.cleanup_missing_tx(&db).await;

        let zmq_listen_addr: Vec<String> = get_zmq_listen_addr(gv_config_clone_zmq).await;

        // Start the ZMQ listener on another thread.
        tokio::spawn(async move {
            let _ = listen_zmq(&zmq_listen_addr, &cli_address, zmq_db).await;
        });

        // Start the task runner thread.
        tokio::spawn(async move {
            let _ = task_runner(&cloned_db, &gv_config_clone_task).await;
        });

        tokio::spawn(async move {
            let _ = listen_for_events(gv_config_clone_sio, sio_db).await;
        });

        GvCLIServer {
            daemon,
            db: db.to_owned(),
            gv_config: Arc::clone(&gv_config),
            daemon_state: Arc::clone(&daemon_state),
            tg_bot_active,
        }
    }

    async fn current_daemon_state(&self) -> DaemonState {
        self.daemon_state.lock().await.to_owned()
    }

    async fn daemon_online(&self) -> bool {
        self.daemon_state.lock().await.online
    }

    async fn set_daemon_online(&self, new_state: bool) {
        let mut guard = self.daemon_state.lock().await;
        guard.online = new_state;
    }

    async fn cycle(&self) -> u32 {
        self.daemon_state.lock().await.cycle
    }

    async fn set_cycle(&self, new_cycle: u32) {
        let mut guard = self.daemon_state.lock().await;
        guard.cycle = new_cycle;
    }

    async fn remote_best_block(&self) -> u32 {
        self.daemon_state.lock().await.remote_best_block
    }

    async fn set_remote_best_block(&self, new_block: u32) {
        let mut guard = self.daemon_state.lock().await;
        guard.remote_best_block = new_block;
    }

    async fn remote_best_block_hash(&self) -> String {
        self.daemon_state
            .lock()
            .await
            .remote_best_block_hash
            .to_string()
    }

    async fn set_remote_best_block_hash(&self, new_block_hash: &str) {
        let mut guard = self.daemon_state.lock().await;
        guard.remote_best_block_hash = new_block_hash.to_string();
    }

    async fn daemon_version(&self) -> String {
        self.daemon_state.lock().await.version.to_string()
    }

    async fn set_daemon_version(&self, new_version: &str) {
        let mut guard = self.daemon_state.lock().await;
        guard.version = new_version.to_string();
    }

    async fn daemon_available(&self) -> bool {
        self.daemon_state.lock().await.available
    }

    async fn set_daemon_available(&self, new_state: bool) {
        let mut guard = self.daemon_state.lock().await;
        guard.available = new_state;
    }

    async fn daemon_latest_release(&self) -> String {
        self.daemon_state.lock().await.latest_release.to_string()
    }

    async fn set_latest_release(&self, new_release: &str) {
        let mut guard = self.daemon_state.lock().await;
        guard.latest_release = new_release.to_string();
    }

    async fn daemon_synced(&self) -> bool {
        self.daemon_state.lock().await.synced
    }

    async fn set_daemon_synced(&self, new_state: bool) {
        let mut guard = self.daemon_state.lock().await;
        guard.synced = new_state;
    }

    async fn good_chain(&self) -> bool {
        self.daemon_state.lock().await.good_chain
    }

    async fn set_good_chain(&self, new_state: bool) {
        let mut guard = self.daemon_state.lock().await;
        guard.good_chain = new_state;
    }

    async fn best_block(&self) -> u32 {
        self.daemon_state.lock().await.best_block
    }

    async fn set_best_block(&self, new_block: u32) {
        let mut guard = self.daemon_state.lock().await;
        guard.best_block = new_block;
    }

    async fn best_block_hash(&self) -> String {
        self.daemon_state.lock().await.best_block_hash.to_string()
    }

    async fn set_best_block_hash(&self, new_block_hash: &str) {
        let mut guard = self.daemon_state.lock().await;
        guard.best_block_hash = new_block_hash.to_string();
    }

    async fn daemon_ready(&self) -> bool {
        let daemon_state: DaemonState = self.current_daemon_state().await;
        daemon_state.online
            && daemon_state.synced
            && daemon_state.good_chain
            && daemon_state.available
    }

    async fn check_chain_task(&self) {
        info!("Starting the chain check monitor...");
        let check_seconds: u64 = 60 * 5;
        let mut bad_chain_count = 0;

        loop {
            let sleep_time = if self.daemon_online().await {
                let blockchain_info: DaemonState = self.current_daemon_state().await;
                let best_block: u32 = blockchain_info.best_block;
                let best_block_hash: String = blockchain_info.best_block_hash;

                let remote_block_hash: Value = loop {
                    let remote_hash = gv_methods::get_remote_block_hash(best_block).await;

                    if remote_hash.is_err() {
                        error!(
                            "Error fetching remote block hash: {}",
                            remote_hash.err().unwrap()
                        );
                        error!("Retrying in 30 seconds...");
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        continue;
                    }
                    break remote_hash.unwrap();
                };

                let remote_hash: String = remote_block_hash
                    .get("blockHash")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string();

                let good_chain: bool = remote_hash == best_block_hash;

                self.set_good_chain(good_chain).await;

                let sleep_time: u64 = if !good_chain {
                    bad_chain_count += 1;
                    60 * 2
                } else {
                    bad_chain_count = 0;
                    check_seconds
                };

                if bad_chain_count >= 5 {
                    if self.tg_bot_active {
                        let current_time = chrono::Utc::now();
                        let timestamp: u64 = current_time.timestamp() as u64;

                        let header = format!("👻 Bad Chain Detected! 👻");

                        let msg = Some(format!("GhostVault has detected a mismatch between the local blockchain and remote.\nGhostVault best block: {}\nGhostVault best block hash: {}\nRemote hash: {}", best_block, best_block_hash, remote_hash));

                        let tg_queue: TgBotQueueDB = TgBotQueueDB {
                            timestamp,
                            header,
                            msg,
                            code_block: None,
                            url: None,
                            msg_type: "online".to_string(),
                            reward_txid: None,
                            msg_to_delete: None,
                        };

                        self.db
                            .set_tg_bot_queue(timestamp.to_string().as_bytes(), &tg_queue)
                            .await
                            .unwrap();
                    }
                    bad_chain_count = 0;
                }
                sleep_time
            } else {
                check_seconds
            };

            tokio::time::sleep(tokio::time::Duration::from_secs(sleep_time)).await;
        }
    }

    async fn monitor_daemon_sync(&self) {
        let check_seconds: u64 = 60;
        info!("Starting the daemon sync monitor...");

        let mut last_state: Option<bool> = None;

        loop {
            let sleep_time = if self.daemon_online().await {
                let synced_res = self.daemon.is_syncing().await.map_err(|e| e.to_string());

                let synced = if synced_res.is_err() {
                    self.handle_daemon_offline().await;
                    continue;
                } else {
                    !synced_res.unwrap()
                };

                self.set_daemon_synced(synced).await;

                let sleep_time: u64 = if !synced {
                    3
                } else {
                    if last_state.is_some() && !last_state.unwrap() {
                        self.daemon.cleanup_missing_tx(&self.db).await
                    }
                    check_seconds
                };
                last_state = Some(synced);
                sleep_time
            } else {
                check_seconds
            };

            tokio::time::sleep(tokio::time::Duration::from_secs(sleep_time)).await;
        }
    }

    async fn monitor_daemon_online(&self) {
        let sleep_time: u64 = 1;
        info!("Starting the daemon online monitor...");

        loop {
            if self.daemon_online().await {
                let online_res = self.daemon.getblockcount().await.map_err(|e| e.to_string());

                if online_res.is_err() {
                    self.handle_daemon_offline().await;
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(sleep_time)).await;
        }
    }

    async fn handle_daemon_offline(&self) {
        info!("Daemon offline, waiting for restart...");
        self.set_daemon_online(false).await;
        let mut server_ready: ServerReadyDB = self.db.get_server_ready().unwrap();
        let is_docker: bool = env::vars().any(|(key, _)| key == "DOCKER_RUNNING");

        server_ready.daemon_ready = false;
        server_ready.reason = Some("Daemon offline".to_string());

        self.db.set_server_ready(&server_ready).await.unwrap();

        if is_docker {
            return;
        }

        if self.tg_bot_active {
            let current_time = chrono::Utc::now();
            let timestamp: u64 = current_time.timestamp() as u64;

            let header = format!("👻 Daemon offline! 👻");
            let msg = Some("Daemon offline, waiting for restart...".to_string());

            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                timestamp,
                header,
                msg,
                code_block: None,
                url: None,
                msg_type: "offline".to_string(),
                reward_txid: None,
                msg_to_delete: None,
            };

            self.db
                .set_tg_bot_queue(timestamp.to_string().as_bytes(), &tg_queue)
                .await
                .unwrap();
        }

        self.daemon.wait_for_daemon_startup().await;

        server_ready.daemon_ready = true;
        server_ready.reason = None;
        self.db.set_server_ready(&server_ready).await.unwrap();
        self.set_daemon_online(true).await;

        if self.tg_bot_active {
            let current_time = chrono::Utc::now();
            let timestamp: u64 = current_time.timestamp() as u64;

            let header = format!("👻 Daemon online! 👻");
            let msg = Some("Daemon back online, ready for action!".to_string());

            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                timestamp,
                header,
                msg,
                code_block: None,
                url: None,
                msg_type: "online".to_string(),
                reward_txid: None,
                msg_to_delete: None,
            };

            self.db
                .set_tg_bot_queue(timestamp.to_string().as_bytes(), &tg_queue)
                .await
                .unwrap();
        }
    }

    async fn get_gv_status(&self) -> Result<GVStatus, Box<dyn std::error::Error>> {
        let (
            net_info,
            bc_info,
            daemon_is_syncing,
            staking_info,
            cold_staking_info,
            last_stake_details,
            daemon_up,
        ) = tokio::try_join!(
            self.daemon.getnetworkinfo(),
            self.daemon.getblockchaininfo(),
            self.daemon.is_syncing(),
            self.daemon.getstakinginfo(),
            self.daemon.getcoldstakinginfo(),
            self.get_last_stake(),
            self.daemon.getuptime()
        )
        .unwrap();
        let sys: System = System::new();
        let load_avg: CpuLoad = self.load(&sys);

        let uptime: FormattedDuration = format_duration(sys.uptime().unwrap());

        let uptime_load = format!(
            "{}, {1:.2} {2:.2} {3:.2}",
            uptime, load_avg.one, load_avg.five, load_avg.fifteen
        );

        let conf = self.gv_config.read().await;

        let privacy_mode = if conf.anon_mode {
            "ANON".to_string()
        } else {
            "STANDARD".to_string()
        };

        drop(conf);

        let daemon_uptime_secs: u64 = daemon_up.as_u64().unwrap();
        let daemon_uptime: FormattedDuration =
            format_duration(Duration::from_secs(daemon_uptime_secs));

        let daemon_synced: String = bool_to_yn(!daemon_is_syncing);
        let daemon_peers: u16 = net_info.get("connections").unwrap().as_u64().unwrap() as u16;
        let best_block: u32 = bc_info.get("blocks").unwrap().as_u64().unwrap() as u32;
        let best_block_hash = bc_info
            .get("bestblockhash")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let best_block_extern = self.remote_best_block().await;
        let good_chain: String = bool_to_yn(self.good_chain().await);

        let staking_enabled: String =
            bool_to_yn(staking_info.get("enabled").unwrap().as_bool().unwrap());
        let active_staking: String =
            bool_to_yn(staking_info.get("staking").unwrap().as_bool().unwrap());
        let staking_difficulty: f64 = staking_info.get("difficulty").unwrap().as_f64().unwrap();
        let network_stake_weight: f64 = self.daemon.convert_from_sat(
            staking_info
                .get("netstakeweight")
                .unwrap()
                .as_u64()
                .unwrap(),
        );

        let currently_staking: f64 = cold_staking_info
            .get("currently_staking")
            .unwrap()
            .as_f64()
            .unwrap();

        let total_coldstaking: f64 = cold_staking_info
            .get("coin_in_coldstakeable_script")
            .unwrap()
            .as_f64()
            .unwrap();

        let stakes: StakeTotals = self.get_stakes_days(1).await;

        let stakes_24: u32 = stakes.stakes;
        let earned_24: f64 = stakes.rewards;
        let earned_agvr_24: f64 = stakes.agvr;
        let total_24: f64 = stakes.total;

        let res: GVStatus = GVStatus {
            uptime: uptime_load,
            privacy_mode,
            daemon_version: self.daemon_version().await,
            latest_release: self.daemon_latest_release().await,
            daemon_uptime: daemon_uptime.to_string(),
            daemon_peers,
            daemon_synced,
            best_block,
            best_block_hash,
            best_block_extern,
            good_chain,
            staking_enabled,
            active_staking,
            staking_difficulty,
            network_stake_weight,
            currently_staking,
            total_coldstaking,
            last_stake: last_stake_details.last_stake_str,
            stakes_24,
            rewards_24: earned_24,
            agvr_24: earned_agvr_24,
            total_24,
        };

        Ok(res)
    }

    async fn get_last_stake(&self) -> Result<LastStake, Box<dyn std::error::Error + Send + Sync>> {
        let conf = self.gv_config.read().await;

        let last_tx = self.db.rewards_ts_index.last().unwrap();

        let last_time = match last_tx {
            Some((_key, value)) => {
                let stake_info: RewardsDB = serde_json::from_slice(&value).unwrap();
                let last_time: u64 = stake_info.timestamp;

                let last_stake_time: DateTime<Utc> =
                    Utc.timestamp_opt(last_time as i64, 0).unwrap();

                let n_time: chrono::prelude::NaiveDateTime = NaiveDate::from_ymd_opt(
                    last_stake_time.year(),
                    last_stake_time.month(),
                    last_stake_time.day(),
                )
                .unwrap()
                .and_hms_opt(
                    last_stake_time.hour(),
                    last_stake_time.minute(),
                    last_stake_time.second(),
                )
                .unwrap();

                let time_zone: String = conf.timezone.clone();
                let tz: Tz = Tz::from_str_insensitive(&time_zone).unwrap();

                let tz_time = Tz::from_utc_datetime(&tz, &n_time);

                let last_time_str = tz_time.format("%Y-%m-%d %H:%M:%S %Z").to_string();

                LastStake {
                    last_stake_str: last_time_str,
                    timestamp: Some(last_time),
                }
            }
            None => LastStake {
                last_stake_str: "N/A".to_string(),
                timestamp: None,
            },
        };
        Ok(last_time)
    }

    async fn get_stakes_days(&self, days_or_start: u64) -> StakeTotals {
        let mut stakes: u32 = 0;
        let mut earned_int: u64 = 0;
        let mut earned_agvr_int: u64 = 0;
        let mut earned_total_int: u64 = 0;

        let current_time = chrono::Utc::now();

        let range_end: u64 = current_time.timestamp() as u64;

        let range_start = if days_or_start == 0 {
            let first_stake_opt = self.db.rewards_ts_index.first().unwrap();
            let first_stake = match first_stake_opt {
                Some((_, value)) => {
                    let value: RewardsDB = serde_json::from_slice(&value).unwrap();
                    value.timestamp
                }
                None => 0,
            };
            first_stake
        } else if days_or_start <= 99_999 {
            current_time
                .checked_sub_days(Days::new(days_or_start))
                .unwrap()
                .timestamp() as u64
        } else {
            days_or_start
        };

        for result in self
            .db
            .rewards_ts_index
            .range(range_start.to_be_bytes()..=range_end.to_be_bytes())
        {
            match result {
                Ok((_, value)) => {
                    let value: RewardsDB = serde_json::from_slice(&value).unwrap();
                    stakes += 1;
                    earned_int += value.reward;
                    earned_agvr_int += value.agvr_reward;
                    earned_total_int += value.reward + value.agvr_reward;
                }
                Err(err) => {
                    eprintln!("Error during iteration: {:?}", err);
                }
            }
        }

        let earned: f64 = self.daemon.convert_from_sat(earned_int);
        let earned_agvr: f64 = self.daemon.convert_from_sat(earned_agvr_int);
        let total: f64 = self.daemon.convert_from_sat(earned_total_int);

        StakeTotals {
            stakes,
            rewards: earned,
            agvr: earned_agvr,
            total,
        }
    }

    async fn get_earnings_chart_vec(&self, start: u64, end: u64) -> AllTimeEarnigns {
        let range_start = if start == 0 {
            let first_stake_opt = self.db.rewards_ts_index.first().unwrap();
            let first_stake = match first_stake_opt {
                Some((_, value)) => {
                    let value: RewardsDB = serde_json::from_slice(&value).unwrap();
                    value.timestamp
                }
                None => 0,
            };
            first_stake
        } else {
            start
        };
        let range_end = end;

        let mut heatmap: Vec<Vec<f64>> = Vec::new();

        for (_, result) in self
            .db
            .rewards_ts_index
            .range(range_start.to_be_bytes()..range_end.to_be_bytes())
            .enumerate()
        {
            match result {
                Ok((_, value)) => {
                    let value: RewardsDB = serde_json::from_slice(&value).unwrap();

                    let total_rewards = value.all_time_reward;
                    let total_agvr = value.all_time_agvr_reward;
                    let total_earning = self.daemon.convert_from_sat(total_rewards + total_agvr);

                    heatmap.push(vec![total_earning, value.timestamp as f64]);
                }
                Err(err) => {
                    eprintln!("Error during iteration: {:?}", err);
                }
            }
        }

        let start = self.get_date_str(range_start).await;
        let end = self.get_date_str(range_end).await;

        let earings_data = AllTimeEarnigns {
            data: heatmap,
            start,
            end,
        };

        earings_data
    }

    async fn get_stake_barchart_vec(&self, start: u64, end: u64, division: &str) -> BarChart {
        let range_start = if start == 0 {
            let first_stake_opt = self.db.rewards_ts_index.first().unwrap();
            let first_stake = match first_stake_opt {
                Some((_, value)) => {
                    let value: RewardsDB = serde_json::from_slice(&value).unwrap();
                    value.timestamp
                }
                None => 0,
            };
            first_stake
        } else {
            start
        };
        let range_end = end;

        let mut heatmap: Vec<Vec<u64>> = Vec::new();

        let mut stake_count: u64 = 0;
        let mut current_divisor: u32 = 0;
        let mut first_iter = true;
        let mut ts: u64 = 0;

        let total_stakes = self
            .db
            .rewards_ts_index
            .range(range_start.to_be_bytes()..range_end.to_be_bytes())
            .count();

        for (index, result) in self
            .db
            .rewards_ts_index
            .range(range_start.to_be_bytes()..range_end.to_be_bytes())
            .enumerate()
        {
            match result {
                Ok((_, value)) => {
                    let value: RewardsDB = serde_json::from_slice(&value).unwrap();

                    let date_enum: (u32, u32, u32, u64) = self
                        .get_enumerated_date(value.timestamp, division)
                        .await
                        .unwrap();

                    let divisor = match division {
                        "day" => date_enum.0,
                        "week" => date_enum.1,
                        "month" => date_enum.2,
                        _ => 0,
                    };

                    if first_iter {
                        first_iter = false;
                        current_divisor = divisor;
                        ts = date_enum.3;
                        stake_count += 1;
                        continue;
                    }

                    if divisor == current_divisor {
                        stake_count += 1;

                        if index == total_stakes - 1 {
                            heatmap.push(vec![ts, stake_count]);
                        }
                    } else {
                        heatmap.push(vec![ts, stake_count]);
                        stake_count = 1;

                        let conf = self.gv_config.read().await;
                        let time_zone = conf.timezone.clone();

                        let tz = Tz::from_str_insensitive(&time_zone).unwrap();

                        let start_date = DateTime::from_timestamp(ts as i64, 0)
                            .unwrap()
                            .with_timezone(&tz);
                        let end_date = DateTime::from_timestamp(date_enum.3 as i64, 0)
                            .unwrap()
                            .with_timezone(&tz);

                        match division {
                            "day" => {
                                let days = end_date.signed_duration_since(start_date).num_days();
                                if days > 1 {
                                    for _ in 1..days {
                                        let current_ts = start_date
                                            .checked_add_days(Days::new(1))
                                            .unwrap()
                                            .timestamp()
                                            as u64;
                                        heatmap.push(vec![current_ts, 0]);
                                    }
                                }
                            }
                            "week" => {
                                let weeks =
                                    end_date.signed_duration_since(start_date).num_weeks() as u64;
                                if weeks > 1 {
                                    for _ in 1..weeks {
                                        let current_ts = start_date
                                            .checked_add_days(Days::new(7))
                                            .unwrap()
                                            .timestamp()
                                            as u64;
                                        heatmap.push(vec![current_ts, 0]);
                                    }
                                }
                            }
                            "month" => {
                                let months =
                                    end_date.signed_duration_since(start_date).num_days() / 30;

                                if months > 1 {
                                    for _ in 1..months {
                                        let current_ts = start_date
                                            .checked_add_months(Months::new(1))
                                            .unwrap()
                                            .timestamp()
                                            as u64;
                                        heatmap.push(vec![current_ts, 0]);
                                    }
                                }
                            }
                            _ => {}
                        }

                        current_divisor = divisor;
                        ts = date_enum.3;
                    }
                }
                Err(err) => {
                    eprintln!("Error during iteration: {:?}", err);
                }
            }
        }

        let start = self.get_date_str(range_start).await;
        let end = self.get_date_str(range_end).await;

        let barchart_data = BarChart {
            data: heatmap,
            division: division.to_string(),
            start,
            end,
        };

        barchart_data
    }

    fn load(&self, sys: &System) -> CpuLoad {
        let system_load: LoadAverage = sys.load_average().unwrap();
        let one: f32 = system_load.one;
        let five: f32 = system_load.five;
        let fifteen: f32 = system_load.fifteen;

        CpuLoad { one, five, fifteen }
    }

    async fn get_date_str(&self, timestamp: u64) -> String {
        let conf = self.gv_config.read().await;
        let time_zone = conf.timezone.clone();
        let tz: Tz = Tz::from_str_insensitive(&time_zone).unwrap();

        let naive_datetime = DateTime::from_timestamp(timestamp as i64, 0);
        let datetime = naive_datetime.unwrap();

        let date_str = datetime.with_timezone(&tz).format("%d/%m/%y").to_string();
        date_str
    }

    async fn get_enumerated_date(
        &self,
        timestamp: u64,
        division: &str,
    ) -> Option<(u32, u32, u32, u64)> {
        // Convert the Unix timestamp to a NaiveDateTime
        let naive_datetime = DateTime::from_timestamp(timestamp as i64, 0);

        let conf = self.gv_config.read().await;
        let time_zone = conf.timezone.clone();
        let tz: Tz = Tz::from_str_insensitive(&time_zone).unwrap();

        let datetime = naive_datetime.unwrap();

        let day_of_week = datetime.with_timezone(&tz).ordinal0();
        let week_num = datetime.with_timezone(&tz).iso_week().week0();
        let month_num = datetime.with_timezone(&tz).month0();

        let timestamp = match division {
            "day" => datetime
                .with_time(NaiveTime::MIN)
                .unwrap()
                .with_timezone(&tz)
                .timestamp() as u64,
            "week" => {
                let days_from_sun = datetime.weekday().num_days_from_sunday() as u64;
                let start_of_week = if days_from_sun == 0 {
                    datetime
                        .with_time(NaiveTime::MIN)
                        .unwrap()
                        .with_timezone(&tz)
                        .timestamp() as u64
                } else {
                    datetime
                        .checked_sub_days(Days::new(days_from_sun))
                        .unwrap()
                        .with_time(NaiveTime::MIN)
                        .unwrap()
                        .with_timezone(&tz)
                        .timestamp() as u64
                };

                start_of_week
            }
            "month" => {
                let start_of_month = datetime
                    .with_day(1)
                    .unwrap()
                    .with_time(NaiveTime::MIN)
                    .unwrap()
                    .with_timezone(&tz)
                    .timestamp() as u64;
                start_of_month
            }
            _ => 0,
        };

        Some((day_of_week, week_num, month_num, timestamp))
    }

    async fn do_force_resync(&self) {
        info!("Forcing a resync of the daemon...");
        self.set_daemon_online(false).await;
        self.set_daemon_synced(false).await;

        let mut server_state: ServerReadyDB = self.db.get_server_ready().unwrap();
        server_state.daemon_ready = false;
        server_state.reason = Some("Forcing resync".to_string());
        self.db.set_server_ready(&server_state).await.unwrap();

        self.daemon.stop_daemon().await.unwrap();

        let conf = self.gv_config.read().await;
        let daemon_data_dir: PathBuf = conf.daemon_data_dir.clone();
        drop(conf);

        let blocks_dir: PathBuf = daemon_data_dir.join("blocks/");
        let chainstate_dir: PathBuf = daemon_data_dir.join("chainstate/");
        let peers_file: PathBuf = daemon_data_dir.join("peers.dat");
        let banlist_file: PathBuf = daemon_data_dir.join("banlist.dat");

        // Remove the blocks and chainstate directories.
        // This will force a resync of the daemon.

        // Also remove the peers.dat and banlist.dat files.
        // Get these fresh incase of bad peers.

        file_ops::rm_dir(&blocks_dir).unwrap();
        file_ops::rm_dir(&chainstate_dir).unwrap();
        file_ops::rm_file(&peers_file).unwrap();
        file_ops::rm_file(&banlist_file).unwrap();

        self.daemon.wait_for_daemon_startup().await;
        self.set_daemon_online(true).await;

        server_state.daemon_ready = true;
        server_state.reason = None;
        self.db.set_server_ready(&server_state).await.unwrap();
    }

    async fn do_update(&self, latest_release: &str) {
        info!("New daemon verison found, doing upgrade...");

        let mut daemon_ready: ServerReadyDB = self.db.get_server_ready().unwrap();

        daemon_ready.daemon_ready = false;
        daemon_ready.reason = Some("Daemon update in progress".to_string());

        self.db.set_server_ready(&daemon_ready).await.unwrap();

        let dl_path_res = gv_methods::download_daemon().await;

        let dl_path: PathBuf = if let Err(err) = dl_path_res {
            error!("Error downloading daemon: {}", err);
            daemon_ready.daemon_ready = true;
            daemon_ready.reason = None;
            self.db.set_server_ready(&daemon_ready).await.unwrap();
            return;
        } else {
            dl_path_res.unwrap()
        };

        if self.tg_bot_active {
            let current_time = chrono::Utc::now();
            let timestamp: u64 = current_time.timestamp() as u64;

            let header = format!("👻 Daemon update in progress! 👻\n\n");
            let msg = Some(format!(
                "New release {} found!\nPlease be patient while the daemon is updated.",
                latest_release,
            ));

            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                timestamp,
                header,
                msg,
                code_block: None,
                url: None,
                msg_type: "update".to_string(),
                reward_txid: None,
                msg_to_delete: None,
            };

            self.db
                .set_tg_bot_queue(timestamp.to_string().as_bytes(), &tg_queue)
                .await
                .unwrap();
        }

        self.set_daemon_online(false).await;
        self.daemon.stop_daemon().await.unwrap();

        let mut config = self.gv_config.write().await;

        file_ops::rm_dir(&config.gv_home.join("daemon/")).unwrap();
        file_ops::rm_dir(&PathBuf::from(TMP_PATH)).unwrap();

        let path_and_hash: PathAndDigest =
            gv_methods::extract_archive(&dl_path, &config.gv_home).unwrap();

        config
            .update_gv_config("daemon_path", path_and_hash.daemon_path.to_str().unwrap())
            .unwrap();

        config
            .update_gv_config("daemon_hash", path_and_hash.daemon_hash.as_str())
            .unwrap();

        drop(config);

        self.daemon.wait_for_daemon_startup().await;

        let daemon_version = self.daemon.get_daemon_version().await.unwrap();
        let latest_release_str: String = gv_methods::get_latest_release().await.unwrap();

        self.set_daemon_version(&daemon_version).await;
        self.set_latest_release(&latest_release_str).await;
        self.set_daemon_online(true).await;

        daemon_ready.daemon_ready = true;
        daemon_ready.reason = None;
        self.db.set_server_ready(&daemon_ready).await.unwrap();

        if self.tg_bot_active {
            let current_time = chrono::Utc::now();
            let timestamp: u64 = current_time.timestamp() as u64;

            let header = format!("👻 Daemon update complete! 👻\n\n");
            let msg = Some(format!(
                "Update to version {} complete!\nYour GhostVault is now ready.",
                daemon_version
            ));

            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                timestamp,
                header,
                msg,
                code_block: None,
                url: None,
                msg_type: "update".to_string(),
                reward_txid: None,
                msg_to_delete: None,
            };

            self.db
                .set_tg_bot_queue(timestamp.to_string().as_bytes(), &tg_queue)
                .await
                .unwrap();
        }
    }

    async fn do_flush_rewards_to_anon(&self) {
        let daemon_ready: bool = self.daemon_ready().await;

        if daemon_ready {
            let balances = self.daemon.get_balances().await.unwrap();
            let balance_value = balances.get("mine").unwrap().as_object().unwrap();

            let bal: serde_json::Map<String, Value> = balance_value.to_owned();

            let trusted_pub: f64 = bal.get("trusted").unwrap().as_f64().unwrap();

            let mut conf = self.gv_config.write().await;
            let min_tx: f64 = self.daemon.convert_from_sat(MIN_TX_VALUE);

            if trusted_pub >= min_tx {
                let addr_option: Option<String> = conf.to_owned().internal_anon;

                let addr: String = if addr_option.is_none() {
                    let internal_anon = self
                        .daemon
                        .getnewstealthaddress()
                        .await
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();

                    conf.update_gv_config("INTERNAL_ANON", &internal_anon)
                        .unwrap();

                    internal_anon
                } else {
                    addr_option.unwrap()
                };

                let txid_res = self.daemon.send_ghost(&addr, "ghost", "anon").await;

                println!("txid_res: {:?}", txid_res);

                let txid = match txid_res {
                    Ok(txid) => txid,
                    Err(err) => {
                        error!("Error sending to address: {}", err);
                        return;
                    }
                };

                info!("Payout to anon address: {}", txid);
            }
        }
    }

    async fn do_reward_payout(&self) {
        let daemon_ready: bool = self.daemon_ready().await;
        let current_time = chrono::Utc::now();
        let timestamp: u64 = current_time.timestamp() as u64;

        if daemon_ready {
            let balances = self.daemon.get_balances().await.unwrap();
            let balance_value = balances.get("mine").unwrap().as_object().unwrap();

            let bal: serde_json::Map<String, Value> = balance_value.to_owned();

            let trusted_anon: f64 = bal.get("anon_trusted").unwrap().as_f64().unwrap();

            let conf = self.gv_config.read().await;

            let min_payout: f64 = self.daemon.convert_from_sat(conf.min_reward_payout);

            if trusted_anon >= min_payout {
                let addr_option: Option<String> = conf.anon_reward_address.clone();

                if addr_option.is_some() {
                    let addr: String = addr_option.unwrap();
                    let addr_info: Value = self.daemon.get_address_info(&addr).await.unwrap();
                    let is_stealth: bool = addr_info
                        .get("isstealthaddress")
                        .unwrap_or(&Value::Bool(false))
                        .as_bool()
                        .unwrap();

                    let out_type: &str = if is_stealth { "anon" } else { "ghost" };

                    let is_256bit: bool = addr_info
                        .get("is256bit")
                        .unwrap_or(&Value::Bool(false))
                        .as_bool()
                        .unwrap();

                    if is_256bit {
                        let txids_res = self.daemon.zap_ghost(&addr, "anon").await;

                        let txids = match txids_res {
                            Ok(txids) => txids,
                            Err(err) => {
                                error!("Error zapping to 256bit address: {}", err);
                                return;
                            }
                        };

                        let default_txid: Vec<Value> = Vec::new();
                        let txid_vec = txids.as_array().unwrap_or(&default_txid);

                        if txid_vec.is_empty() {
                            return;
                        }

                        for txid_value in txid_vec {
                            let txid = txid_value.as_str().unwrap().to_string();
                            info!("Zap to public address: {}", txid);
                        }

                        if self.tg_bot_active {
                            let header = format!("👻 Rewards coming your way! 👻");

                            let msg = Some(format!(
                            "Anon rewards in the amount of {} GHOST being zapped to PUBLIC address.",
                            trusted_anon
                        ));

                            let url = {
                                let mut urls: Vec<String> = Vec::new();
                                for txid_value in txid_vec {
                                    urls.push(format!(
                                        "https://ghostscan.io/tx/{}/",
                                        txid_value.as_str().unwrap()
                                    ));
                                }
                                Some(urls)
                            };

                            let msg_type = "rewards".to_string();

                            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                                timestamp,
                                header,
                                msg,
                                code_block: None,
                                url,
                                msg_type,
                                reward_txid: None,
                                msg_to_delete: None,
                            };
                            let txid = txid_vec[0].as_str().unwrap().to_string();
                            self.db
                                .set_tg_bot_queue(txid.as_bytes(), &tg_queue)
                                .await
                                .unwrap();
                        }
                    } else {
                        let txids_res = self.daemon.send_ghost(&addr, "anon", out_type).await;

                        let txids = match txids_res {
                            Ok(txids) => txids,
                            Err(err) => {
                                error!("Error sending to address: {}", err);
                                return;
                            }
                        };

                        let default_txid: Vec<Value> = Vec::new();
                        let txid_vec = txids.as_array().unwrap_or(&default_txid);

                        if txid_vec.is_empty() {
                            return;
                        }

                        for txid_value in txid_vec {
                            let txid = txid_value.as_str().unwrap().to_string();
                            info!("Payout to {} address: {}", out_type.to_uppercase(), txid);
                        }

                        if self.tg_bot_active {
                            let header = format!("👻 Rewards coming your way! 👻");

                            let msg = Some(format!(
                                "Anon rewards in the amount of {} GHOST being sent to {} address.",
                                trusted_anon,
                                out_type.to_uppercase()
                            ));

                            let url = {
                                let mut urls: Vec<String> = Vec::new();
                                for txid_value in txid_vec {
                                    urls.push(format!(
                                        "https://ghostscan.io/tx/{}/",
                                        txid_value.as_str().unwrap()
                                    ));
                                }
                                Some(urls)
                            };

                            let msg_type = "rewards".to_string();

                            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                                timestamp,
                                header,
                                msg,
                                code_block: None,
                                url,
                                msg_type,
                                reward_txid: None,
                                msg_to_delete: None,
                            };

                            let txid = txid_vec[0].as_str().unwrap().to_string();

                            self.db
                                .set_tg_bot_queue(txid.as_bytes(), &tg_queue)
                                .await
                                .unwrap();
                        }
                    }
                }
            }
            drop(conf);
        }
    }

    async fn process_rewards_status(&self) {
        for result in self.db.new_stake_status.iter() {
            match result {
                Ok((key, value)) => {
                    let mut stake_status: NewStakeStatusDB =
                        serde_json::from_slice(&value).unwrap();
                    let txid: String = stake_status.txid.clone();
                    let tx_details_res = self.daemon.get_transaction(&txid).await;

                    let tx_details = if tx_details_res.is_err() {
                        self.db.remove_new_stake_status(&key).await.unwrap();
                        self.db
                            .remove_reward(stake_status.timestamp.to_be_bytes())
                            .await
                            .unwrap();
                        continue;
                    } else {
                        tx_details_res.unwrap()
                    };

                    let tx_output = tx_details.get("details").unwrap().as_array().unwrap();

                    if tx_output.is_empty()
                        || tx_output[0].get("category").unwrap().as_str().unwrap() != "stake"
                    {
                        self.db.remove_new_stake_status(&key).await.unwrap();
                        self.db
                            .remove_reward(stake_status.timestamp.to_be_bytes())
                            .await
                            .unwrap();

                        if self.tg_bot_active {
                            let current_time = chrono::Utc::now();
                            let timestamp: u64 = current_time.timestamp() as u64;

                            let header = format!("👻 Stake removed! 👻");
                            let msg = None;
                            let url = None;
                            let msg_type = "stake_removal".to_string();

                            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                                timestamp,
                                header,
                                msg,
                                code_block: None,
                                url,
                                msg_type,
                                reward_txid: None,
                                msg_to_delete: stake_status.tg_msg_id.clone(),
                            };

                            self.db
                                .set_tg_bot_queue(timestamp.to_string().as_bytes(), &tg_queue)
                                .await
                                .unwrap();
                        }

                        continue;
                    }

                    let confirms: u64 = tx_details
                        .get("confirmations")
                        .map_or(0, |val| val.as_u64().unwrap());

                    stake_status.confirmations = confirms as u32;

                    if confirms > 100 {
                        self.do_flush_rewards_to_anon().await;
                        self.db.remove_new_stake_status(&key).await.unwrap();
                    } else {
                        self.db
                            .set_new_stake_status(&key, &stake_status)
                            .await
                            .unwrap();
                    }
                }
                Err(err) => {
                    eprintln!("Error during iteration: {:?}", err);
                }
            }
        }
    }

    async fn process_zap_status(&self) {
        for result in self.db.zap_status_db.iter() {
            match result {
                Ok((key, value)) => {
                    let mut zap_status: ZapStatusDB = serde_json::from_slice(&value).unwrap();
                    let txid: String = zap_status.txid.clone();
                    let tx_details: Value = self.daemon.get_transaction(&txid).await.unwrap();
                    let confirms: i64 = tx_details
                        .get("confirmations")
                        .map_or(0, |val| val.as_i64().unwrap());

                    if confirms < 0 {
                        self.db.remove_zap_status(&key).await.unwrap();
                        continue;
                    }

                    let current_time = chrono::Utc::now();
                    let timestamp: u64 = current_time.timestamp() as u64;

                    if confirms >= 225 {
                        if self.tg_bot_active {
                            let header = format!("👻 Zap Now Staking! 👻");
                            let msg = Some(format!(
                                "The deposit of {} GHOST in your GhostVault is now staking!",
                                self.daemon.convert_from_sat(zap_status.amount)
                            ));

                            let url = None;
                            let msg_type = "zap".to_string();

                            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                                timestamp,
                                header,
                                msg,
                                code_block: None,
                                url,
                                msg_type,
                                reward_txid: None,
                                msg_to_delete: None,
                            };

                            let in_tg_queue: Option<TgBotQueueDB> = self.db.get_tg_bot_queue(&key);
                            if in_tg_queue.is_none() {
                                self.db.set_tg_bot_queue(&key, &tg_queue).await.unwrap();
                            }
                        }
                        self.db.remove_zap_status(&key).await.unwrap();
                    } else {
                        zap_status.confirmations = confirms as u32;
                        if self.tg_bot_active {
                            let amount = self.daemon.convert_from_sat(zap_status.amount);

                            let in_msg_que = self.db.get_tg_bot_queue(&key).is_some();

                            let header = format!("👻 New Zap Detected! 👻");

                            let msg = Some(format!(
                                "New deposit of {} GHOST is in your GhostVault!",
                                amount
                            ));

                            let url = Some(vec![format!("https://ghostscan.io/tx/{}/", txid)]);

                            let msg_type = "zap".to_string();

                            let tg_queue: TgBotQueueDB = TgBotQueueDB {
                                timestamp,
                                header,
                                msg,
                                code_block: None,
                                url,
                                msg_type,
                                reward_txid: None,
                                msg_to_delete: None,
                            };

                            if !zap_status.first_notice && !in_msg_que {
                                self.db
                                    .set_tg_bot_queue(txid.as_bytes(), &tg_queue)
                                    .await
                                    .unwrap();
                                zap_status.first_notice = true;
                                self.db
                                    .set_zap_status(txid.as_bytes(), &zap_status)
                                    .await
                                    .unwrap();
                            }
                        }

                        self.db.set_zap_status(&key, &zap_status).await.unwrap();
                    }
                }
                Err(err) => {
                    eprintln!("Error during iteration: {:?}", err);
                }
            }
        }
    }
}

impl GvCLI for GvCLIServer {
    async fn getblockcount(self, _: context::Context) -> Value {
        let blocks = self.daemon.getblockcount().await.unwrap();
        blocks
    }

    async fn shutdown(self, _: context::Context) -> Value {
        let conf = self.gv_config.read().await;
        let gv_data_dir = conf.gv_home.clone();
        drop(conf);
        let pid_file: PathBuf = gv_data_dir.join(GV_PID_FILE);
        file_ops::rm_file(&pid_file).unwrap();

        let is_docker: bool = env::vars().any(|(key, _)| key == "DOCKER_RUNNING");

        if is_docker {
            let _ = self.daemon.stop_daemon().await;
        }

        tokio::spawn(async move {
            do_shutdown().await;
        });
        Value::String("GhostVault going down for shutdown...".to_string())
    }

    async fn get_daemon_state(self, _: context::Context) -> Value {
        serde_json::to_value(self.get_gv_status().await.unwrap()).unwrap()
    }

    async fn get_ext_pub_key(self, _: context::Context) -> Value {
        let mut conf = self.gv_config.write().await;
        let ext_pub_key = conf.ext_pub_key.clone();

        if ext_pub_key.is_none() {
            let ext_pub_key = self
                .daemon
                .getnewextaddress()
                .await
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            conf.update_gv_config("ext_pub_key", &ext_pub_key).unwrap();
            return Value::String(ext_pub_key);
        }

        Value::String(ext_pub_key.unwrap())
    }

    async fn enable_telegram_bot(self, _: context::Context, token: String, user: String) -> Value {
        let mut conf = self.gv_config.write().await;

        let plausible_userid = user.parse::<u64>();

        if plausible_userid.is_err() {
            return Value::String("Invalid user ID!".to_string());
        }

        let token_is_valid: bool = gv_methods::validate_bot_token(&token).await.unwrap();

        if !token_is_valid {
            return Value::String("Invalid bot token!".to_string());
        }

        conf.update_gv_config("TELOXIDE_TOKEN", &token).unwrap();
        conf.update_gv_config("TELEGRAM_USER", &user).unwrap();
        Value::String("Telegram bot enabled!".to_string())
    }

    async fn disable_telegram_bot(self, _: context::Context) -> Value {
        let mut conf = self.gv_config.write().await;
        conf.update_gv_config("TELOXIDE_TOKEN", "").unwrap();
        conf.update_gv_config("TELEGRAM_USER", "").unwrap();
        Value::String("Telegram bot disabled!".to_string())
    }

    async fn set_reward_interval(self, _: context::Context, interval: String) -> Value {
        let second: i64 = 1;
        let minute: i64 = 60 * second;
        let hour: i64 = 60 * minute;
        let day: i64 = 24 * hour;
        let week: i64 = 7 * day;
        let month: i64 = 30 * day;
        let year: i64 = 365 * day;

        let multiplier: char = interval.chars().last().unwrap();

        let striped_str: String = interval.replace(multiplier, "");

        let timeframe: i64 = if striped_str.is_empty() {
            return Value::String("Invalid interval!".to_string());
        } else {
            let parsed_str = striped_str.parse::<i64>();
            if parsed_str.is_err() {
                return Value::String("Invalid interval!".to_string());
            } else {
                parsed_str.unwrap()
            }
        };

        let interval: i64 = match multiplier {
            's' => timeframe * second,
            'm' => timeframe * minute,
            'h' => timeframe * hour,
            'd' => timeframe * day,
            'w' => timeframe * week,
            'M' => timeframe * month,
            'y' => timeframe * year,
            _ => return Value::String("Invalid interval!".to_string()),
        };
        let mut conf = self.gv_config.write().await;
        conf.update_gv_config("reward_interval", &interval.to_string())
            .unwrap();

        task_runner::update_payout_interval(&self.db, interval)
            .await
            .unwrap();

        Value::String("Reward interval updated!".to_string())
    }

    async fn set_payout_min(self, _: context::Context, min: f64) -> Value {
        let mut conf = self.gv_config.write().await;
        let min_int: u64 = self.daemon.convert_to_sat(min);

        if min_int < MIN_TX_VALUE {
            return Value::String("Minimum payout too low!".to_string());
        }

        conf.update_gv_config("min_reward_payout", &min_int.to_string())
            .unwrap();

        task_runner::update_payout_min(&self.db, min_int)
            .await
            .unwrap();

        Value::String("Minimum payout updated!".to_string())
    }

    async fn set_reward_mode(
        self,
        _: context::Context,
        mode: String,
        addr: Option<String>,
    ) -> Value {
        let mut conf = self.gv_config.write().await;

        match mode.to_uppercase().as_str() {
            "ANON" => {
                if addr.is_none() {
                    return Value::String("An address is required for anon mode!".to_string());
                }

                let addr: &String = addr.as_ref().unwrap();

                let addr_info = self.daemon.get_address_info(addr).await;

                if addr_info.is_err() {
                    return Value::String("Invalid address!".to_string());
                }

                let addr_info: Value = addr_info.unwrap();

                let is_mine: bool = addr_info
                    .get("ismine")
                    .unwrap_or(&Value::Bool(false))
                    .as_bool()
                    .unwrap();

                if is_mine {
                    return Value::String("Cannot use a address owned by GhostVault!".to_string());
                }

                let mut internal_anon: String =
                    conf.internal_anon.clone().unwrap_or("".to_string());

                if internal_anon.is_empty() {
                    let anon_addr = self
                        .daemon
                        .getnewstealthaddress()
                        .await
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();
                    conf.update_gv_config("internal_anon", &anon_addr).unwrap();
                } else {
                    let addr_info = self.daemon.get_address_info(&internal_anon).await;

                    let addr_err: bool = addr_info.is_err();

                    let addr_is_valid: bool = if addr_err {
                        false
                    } else {
                        let addr_info: Value = addr_info.unwrap();

                        let is_stealth = addr_info
                            .get("isstealthaddress")
                            .unwrap_or(&Value::Bool(false))
                            .as_bool()
                            .unwrap();

                        let is_mine = addr_info
                            .get("ismine")
                            .unwrap_or(&Value::Bool(false))
                            .as_bool()
                            .unwrap();

                        is_stealth && is_mine
                    };

                    if !addr_is_valid {
                        let anon_addr = self
                            .daemon
                            .getnewstealthaddress()
                            .await
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_string();
                        conf.update_gv_config("internal_anon", &anon_addr).unwrap();
                        internal_anon = anon_addr;
                    }
                }

                conf.update_gv_config("reward_address", &internal_anon)
                    .unwrap();
                conf.update_gv_config("anon_mode", "true").unwrap();
                conf.update_gv_config("anon_reward_address", addr).unwrap();
                self.daemon
                    .set_reward_addr_in_wallet(Some(&internal_anon))
                    .await
                    .unwrap();
                return Value::String("Reward mode updated!".to_string());
            }
            "STANDARD" => {
                if addr.is_none() {
                    return Value::String("An address is required for standard mode!".to_string());
                }

                let addr: &String = addr.as_ref().unwrap();

                let addr_info = self.daemon.get_address_info(addr).await;

                if addr_info.is_err() {
                    return Value::String("Invalid address!".to_string());
                }

                let addr_info: Value = addr_info.unwrap();

                let is_mine: bool = addr_info
                    .get("ismine")
                    .unwrap_or(&Value::Bool(false))
                    .as_bool()
                    .unwrap();

                if is_mine {
                    return Value::String("Cannot use a address owned by GhostVault!".to_string());
                }

                conf.update_gv_config("reward_address", addr).unwrap();
                conf.update_gv_config("anon_mode", "false").unwrap();
                self.daemon
                    .set_reward_addr_in_wallet(Some(addr))
                    .await
                    .unwrap();
                return Value::String("Reward mode updated!".to_string());
            }
            "DEFAULT" => {
                conf.update_gv_config("anon_mode", "false").unwrap();
                conf.update_gv_config("reward_address", "").unwrap();
                self.daemon.set_reward_addr_in_wallet(None).await.unwrap();
                return Value::String("Reward mode updated!".to_string());
            }
            _ => {
                return Value::String("Invalid mode!".to_string());
            }
        }
    }

    async fn new_block(self, _: context::Context, new_block: String) {
        if new_block != self.best_block_hash().await {
            info!("New block from daemon: {new_block}");
            let block_value: Value = self.daemon.getblock(&new_block, 1).await.unwrap();
            let block_height: u32 = block_value.get("height").unwrap().as_u64().unwrap() as u32;
            let cycle: u32 = self.cycle().await + 1;

            let block_hash: String = new_block.clone();

            let new_status: DaemonStatusDB = DaemonStatusDB {
                height: block_height,
                block_hash,
            };

            let synced: bool = !self.daemon.is_syncing().await.unwrap();

            self.db.set_daemon_status(&new_status).await.unwrap();

            let is_ready = self.daemon_ready().await;

            if is_ready {
                let _ = self.process_zap_status().await;
                let _ = self.process_rewards_status().await;
            }

            self.set_best_block(block_height).await;
            self.set_best_block_hash(&new_block).await;
            self.set_daemon_synced(synced).await;
            self.set_cycle(cycle).await;
        }
    }

    async fn new_remote_block(self, _: context::Context, block_hash: String, height: u32) {
        if block_hash != self.remote_best_block_hash().await {
            info!("New block from remote: {block_hash}");

            self.set_remote_best_block(height).await;
            self.set_remote_best_block_hash(&block_hash).await;
        }
    }

    async fn new_wallet_tx(self, _: context::Context, txid_and_wal: TxidAndWallet) {
        let txid: String = txid_and_wal.txid;
        let wallet: String = txid_and_wal.wallet;

        let conf = self.gv_config.read().await;

        if wallet == conf.rpc_wallet {
            let tx_details: Value = self.daemon.get_transaction(&txid).await.unwrap();
            let tx_io: &Vec<Value> = tx_details.get("details").unwrap().as_array().unwrap();

            if tx_io.is_empty() {
                return;
            }

            let tx_category: &str = tx_io[0].get("category").unwrap().as_str().unwrap();

            let current_time = chrono::Utc::now();
            let timestamp: u64 = current_time.timestamp() as u64;

            let is_stake: bool = match tx_category {
                "stake" => true,
                _ => false,
            };

            if is_stake {
                let reward: RewardsDB = self
                    .daemon
                    .process_stake_transaction(&tx_details, &self.db)
                    .await;
                info!("New stake reward: {:?}", reward);

                let stake_new_status = NewStakeStatusDB {
                    txid: txid.clone(),
                    confirmations: 1,
                    timestamp: reward.timestamp,
                    tg_msg_id: None,
                };

                let _ = self
                    .db
                    .set_new_stake_status(txid.as_bytes(), &stake_new_status)
                    .await;

                if self.tg_bot_active {
                    let cs_info = self.daemon.getcoldstakinginfo().await.unwrap();

                    let total_staking = cs_info.get("currently_staking").unwrap().as_f64().unwrap();
                    let total_coldstaking = cs_info
                        .get("coin_in_coldstakeable_script")
                        .unwrap()
                        .as_f64()
                        .unwrap();

                    let stakes_24h: StakeTotals = self.get_stakes_days(1).await;

                    let january_first: chrono::prelude::NaiveDateTime =
                        NaiveDate::from_ymd_opt(current_time.year(), 1, 1)
                            .unwrap()
                            .and_hms_opt(0, 0, 0)
                            .unwrap();
                    let time_zone: String = conf.timezone.clone();
                    let tz: Tz = Tz::from_str_insensitive(&time_zone).unwrap();

                    let start_year: u64 =
                        january_first.and_local_timezone(tz).unwrap().timestamp() as u64;
                    let stakes_ytd: StakeTotals = self.get_stakes_days(start_year).await;

                    let staking_data: StakingData = StakingData {
                        total_staking,
                        total_coldstaking,
                        stakes_24h,
                        stakes_ytd,
                    };

                    let new_stake: NewStake = NewStake {
                        height: reward.height,
                        block_hash: reward.block_hash.clone(),
                        txid: reward.txid.clone(),
                        reward: self.daemon.convert_from_sat(reward.reward),
                        agvr_reward: self.daemon.convert_from_sat(reward.agvr_reward),
                        total_reward: self
                            .daemon
                            .convert_from_sat(reward.reward + reward.agvr_reward),
                        staking_data,
                    };

                    let msg: Option<String> = None;

                    let code_block: Option<String> =
                        Some(serde_json::to_string_pretty(&new_stake).unwrap());

                    let header: String = format!("👻 New Block Found! 👻");
                    let url = Some(vec![format!("https://ghostscan.io/tx/{}/", txid)]);

                    let msg_type = "stake".to_string();

                    let tg_queue: TgBotQueueDB = TgBotQueueDB {
                        timestamp,
                        header,
                        msg,
                        code_block,
                        url,
                        msg_type,
                        reward_txid: Some(reward.txid.clone()),
                        msg_to_delete: None,
                    };

                    let in_tg_queue = self.db.get_tg_bot_queue(txid.as_bytes());
                    if in_tg_queue.is_none() {
                        self.db
                            .set_tg_bot_queue(txid.as_bytes(), &tg_queue)
                            .await
                            .unwrap();
                    }
                }
            } else {
                info!("wallet tx!");

                let mut is_incoming_zap = false;
                let mut amount_int = 0;
                let mut amount: f64 = 0.0;

                for tx in tx_io {
                    let is_watchonly = tx
                        .get("involvesWatchonly")
                        .map_or(false, |val| val.as_bool().unwrap());

                    let is_receive = match tx.get("category").unwrap().as_str().unwrap() {
                        "receive" => true,
                        _ => false,
                    };

                    if is_watchonly && is_receive {
                        is_incoming_zap = true;
                        amount += tx.get("amount").unwrap().as_f64().unwrap();
                        amount_int += self.daemon.convert_to_sat(amount);
                    }
                }

                if is_incoming_zap {
                    let confirms = tx_details
                        .get("confirmations")
                        .map_or(0, |val| val.as_i64().unwrap());

                    if confirms < 0 {
                        return;
                    }

                    if confirms < 225 {
                        let in_queue = self.db.get_zap_status(txid.as_bytes());
                        let first_notice = false;
                        let confirmations = confirms as u32;

                        if in_queue.is_none() {
                            let zap_status = ZapStatusDB {
                                txid: txid.clone(),
                                amount: amount_int,
                                confirmations,
                                first_notice,
                            };
                            self.db
                                .set_zap_status(txid.as_bytes(), &zap_status)
                                .await
                                .unwrap();

                            if self.tg_bot_active {
                                let header = format!("👻 New Zap Detected! 👻");

                                let msg = Some(format!(
                                    "New deposit of {} GHOST is in your GhostVault!",
                                    amount
                                ));

                                let url = Some(vec![format!("https://ghostscan.io/tx/{}/", txid)]);

                                let msg_type = "zap".to_string();

                                let tg_queue: TgBotQueueDB = TgBotQueueDB {
                                    timestamp,
                                    header,
                                    msg,
                                    code_block: None,
                                    url,
                                    msg_type,
                                    reward_txid: None,
                                    msg_to_delete: None,
                                };

                                let mut zap_status =
                                    self.db.get_zap_status(txid.as_bytes()).unwrap();

                                let in_tg_queue = self.db.get_tg_bot_queue(txid.as_bytes());
                                if in_tg_queue.is_none() && !zap_status.first_notice {
                                    self.db
                                        .set_tg_bot_queue(txid.as_bytes(), &tg_queue)
                                        .await
                                        .unwrap();
                                    zap_status.first_notice = true;
                                    self.db
                                        .set_zap_status(txid.as_bytes(), &zap_status)
                                        .await
                                        .unwrap();
                                }
                            }
                        }
                    }
                }
            }
        }

        drop(conf);
    }

    async fn set_bot_announce(
        self,
        _: context::Context,
        msg_type: String,
        new_value: bool,
    ) -> Value {
        let mut conf = self.gv_config.write().await;

        match msg_type.to_uppercase().as_str() {
            "STAKE" => {
                conf.update_gv_config("ANNOUNCE_STAKES", &new_value.to_string())
                    .unwrap();
            }
            "ZAP" => {
                conf.update_gv_config("ANNOUNCE_ZAPS", &new_value.to_string())
                    .unwrap();
            }
            "REWARD" => {
                conf.update_gv_config("ANNOUNCE_REWARDS", &new_value.to_string())
                    .unwrap();
            }
            "ALL" => {
                conf.update_gv_config("ANNOUNCE_STAKES", &new_value.to_string())
                    .unwrap();
                conf.update_gv_config("ANNOUNCE_ZAPS", &new_value.to_string())
                    .unwrap();
                conf.update_gv_config("ANNOUNCE_REWARDS", &new_value.to_string())
                    .unwrap();
            }
            _ => {
                return Value::String("Invalid message type!".to_string());
            }
        }
        drop(conf);

        Value::String("Bot announcement updated!".to_string())
    }

    async fn get_version_info(self, _: context::Context) -> Value {
        let gv_version: String = VERSION.to_string();
        let daemon_state: DaemonState = self.current_daemon_state().await;

        let ghostd_version: String = daemon_state.version.clone();
        let latest_release: String = daemon_state.latest_release.clone();

        let version_info: VersionInfo = VersionInfo {
            gv_version,
            ghostd_version,
            latest_release,
        };
        serde_json::to_value(version_info).unwrap()
    }

    async fn get_reward_options(self, _: context::Context) -> Value {
        let conf = self.gv_config.read().await;
        let anon_mode = conf.anon_mode;
        let reward_address = if anon_mode {
            conf.anon_reward_address.clone().unwrap()
        } else {
            if conf.reward_address.is_none() {
                "".to_string()
            } else {
                conf.reward_address.clone().unwrap()
            }
        };

        let reward_mode = if anon_mode {
            "ANON".to_string()
        } else if !anon_mode && !reward_address.is_empty() {
            "STANDARD".to_string()
        } else {
            "DEFAULT".to_string()
        };

        let reward_interval_secs: Duration = Duration::from_secs(conf.reward_interval);
        let reward_interval: String = format_duration(reward_interval_secs).to_string();
        let reward_min: f64 = self.daemon.convert_from_sat(conf.min_reward_payout);

        let rewards: RewardOptions = RewardOptions {
            reward_mode,
            reward_address,
            reward_interval,
            reward_min,
        };

        serde_json::to_value(rewards).unwrap()
    }

    async fn check_chain(self, _: context::Context) -> Value {
        let daemon_info = self.current_daemon_state().await;
        Value::Bool(daemon_info.good_chain)
    }

    async fn validate_address(self, _: context::Context, address: String) -> Value {
        let addr_info = self.daemon.get_address_info(&address).await;

        let is_valid = !addr_info.is_err();

        let is_mine: bool = if is_valid {
            addr_info
                .as_ref()
                .unwrap()
                .get("ismine")
                .unwrap_or(&Value::Bool(false))
                .as_bool()
                .unwrap()
        } else {
            false
        };

        let is_256bit: bool = if is_valid {
            addr_info
                .as_ref()
                .unwrap()
                .get("is256bit")
                .unwrap_or(&Value::Bool(false))
                .as_bool()
                .unwrap()
        } else {
            false
        };

        let addr_info: AddressInfo = AddressInfo {
            is_mine,
            is_valid,
            is_256bit,
        };

        serde_json::to_value(addr_info).unwrap()
    }

    async fn get_pending_rewards(self, _: context::Context) -> Value {
        let balances = self.daemon.get_balances().await.unwrap();
        let my_balances = balances.get("mine").unwrap().as_object().unwrap();

        let trusted: f64 = my_balances.get("trusted").unwrap().as_f64().unwrap();
        let untrusted_pending: f64 = my_balances
            .get("untrusted_pending")
            .unwrap()
            .as_f64()
            .unwrap();
        let immature: f64 = my_balances.get("immature").unwrap().as_f64().unwrap();
        let staked: f64 = my_balances.get("staked").unwrap().as_f64().unwrap();
        let anon_trusted: f64 = my_balances.get("anon_trusted").unwrap().as_f64().unwrap();
        let anon_immature: f64 = my_balances.get("anon_immature").unwrap().as_f64().unwrap();
        let anon_pending: f64 = my_balances
            .get("anon_untrusted_pending")
            .unwrap()
            .as_f64()
            .unwrap();

        let total_pending: f64 = trusted
            + untrusted_pending
            + immature
            + staked
            + anon_trusted
            + anon_pending
            + anon_immature;

        let conf = self.gv_config.read().await;

        let next_payout_time: i64 = task_runner::get_next_payout_time(&self.db).await.unwrap();

        let next_payout_time: DateTime<Utc> =
            DateTime::from_timestamp(next_payout_time, 0).unwrap();
        let time_zone: String = conf.timezone.clone();
        let tz: Tz = Tz::from_str_insensitive(&time_zone).unwrap();
        let next_payout_run: String = next_payout_time.with_timezone(&tz).to_string();

        let pending_rewards: PendingRewards = PendingRewards {
            total_pending,
            staked,
            pending_anonymization: trusted + untrusted_pending + immature,
            pending_anon_confs: anon_immature + anon_pending,
            pending_payout: anon_trusted,
            payout_run_interval: format_duration(Duration::from_secs(conf.reward_interval))
                .to_string(),
            next_payout_run,
            min_payout: self.daemon.convert_from_sat(conf.min_reward_payout),
        };

        serde_json::to_value(&pending_rewards).unwrap()
    }

    async fn process_daemon_update(self, _: context::Context) -> Value {
        info!("Checking for new update");
        let version_str: String = self.daemon.get_daemon_version().await.unwrap();
        let latest_release_res: Result<String, Box<dyn std::error::Error + Send + Sync>> =
            gv_methods::get_latest_release().await;

        let latest_release_str: String = if latest_release_res.is_err() {
            return Value::String("Failed to check for updates!".to_string());
        } else {
            latest_release_res.unwrap()
        };

        let version: u64 = version_str.replace(".", "").parse::<u64>().unwrap();
        let latest_release: u64 = latest_release_str.replace(".", "").parse::<u64>().unwrap();

        if latest_release > version {
            let release_clone = latest_release_str.clone();
            tokio::spawn(async move {
                let _ = self.do_update(&release_clone).await;
            });
            return Value::String(latest_release_str);
        } else {
            info!("Daemon is up to date!");
            return Value::Bool(false);
        }
    }

    async fn get_daemon_online(self, _: context::Context) -> Value {
        let daemon_online: bool = self.daemon_online().await;

        if !daemon_online {
            self.db.gvdb.flush_async().await.unwrap();
            let server_ready = self.db.get_server_ready().unwrap();
            serde_json::to_value(server_ready).unwrap()
        } else {
            Value::Bool(daemon_online)
        }
    }

    async fn get_stake_barchart_data(
        self,
        _: context::Context,
        start: u64,
        end: u64,
        division: String,
    ) -> Value {
        let stake_data: BarChart = self.get_stake_barchart_vec(start, end, &division).await;
        serde_json::to_value(stake_data).unwrap()
    }

    async fn get_earnings_chart_data(self, _: context::Context, start: u64, end: u64) -> Value {
        let earnings_data: AllTimeEarnigns = self.get_earnings_chart_vec(start, end).await;
        serde_json::to_value(earnings_data).unwrap()
    }

    async fn process_payouts(self, _: context::Context) {
        tokio::spawn(async move {
            self.do_reward_payout().await;
        });
    }

    async fn force_resync(self, _: context::Context) -> Value {
        tokio::spawn(async move {
            self.do_force_resync().await;
        });

        Value::String("Forcing a resync of the daemon...".to_string())
    }

    async fn get_overview(self, _: context::Context) -> Value {
        let cs_info = self.daemon.getcoldstakinginfo().await.unwrap();

        let current_time = chrono::Utc::now();
        let conf = self.gv_config.read().await;

        let total_staking = cs_info.get("currently_staking").unwrap().as_f64().unwrap();
        let total_coldstaking = cs_info
            .get("coin_in_coldstakeable_script")
            .unwrap()
            .as_f64()
            .unwrap();

        let stakes_all: StakeTotals = self.get_stakes_days(0).await;
        let stakes_24h: StakeTotals = self.get_stakes_days(1).await;
        let stakes_7d: StakeTotals = self.get_stakes_days(7).await;
        let stakes_14d: StakeTotals = self.get_stakes_days(14).await;
        let stakes_30d: StakeTotals = self.get_stakes_days(30).await;
        let stakes_90d: StakeTotals = self.get_stakes_days(90).await;
        let stakes_180d: StakeTotals = self.get_stakes_days(180).await;
        let stakes_1y: StakeTotals = self.get_stakes_days(365).await;

        let january_first: chrono::prelude::NaiveDateTime =
            NaiveDate::from_ymd_opt(current_time.year(), 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap();
        let time_zone: String = conf.timezone.clone();
        let tz: Tz = Tz::from_str_insensitive(&time_zone).unwrap();

        let start_year: u64 = january_first.and_local_timezone(tz).unwrap().timestamp() as u64;
        let stakes_ytd: StakeTotals = self.get_stakes_days(start_year).await;

        let staking_data = StakingDataOverview {
            total_staking,
            total_coldstaking,
            stakes_24h,
            stakes_7d,
            stakes_14d,
            stakes_30d,
            stakes_90d,
            stakes_180d,
            stakes_1y,
            stakes_ytd,
            stakes_all,
        };

        serde_json::to_value(staking_data).unwrap()
    }

    async fn get_mnemonic(self, _: context::Context) -> Value {
        let conf = self.gv_config.read().await;
        let mnemonic = conf.mnemonic.clone();

        if mnemonic.is_none() {
            Value::Null
        } else {
            Value::String(mnemonic.unwrap())
        }
    }

    async fn import_wallet(self, _: context::Context, mnemonic: String, name: String) -> Value {
        let mnemonic = mnemonic.trim();

        let mnemonic_valid = self.daemon.validate_mnemonic(mnemonic).await.unwrap();

        if !mnemonic_valid {
            return Value::String("Invalid mnemonic!".to_string());
        } else {
            let mut server_ready: ServerReadyDB = self.db.get_server_ready().unwrap();

            server_ready.daemon_ready = false;
            server_ready.reason = Some("Importing Wallet".to_string());

            self.db.set_server_ready(&server_ready).await.unwrap();
            self.set_daemon_available(false).await;
            let res = self.daemon.import_wallet(&name, mnemonic, &self.db).await;
            match res {
                Ok(_) => {
                    let _ = tokio::spawn(async move {
                        self.db.clear_db().await.unwrap();
                        self.daemon.cleanup_missing_tx(&self.db).await;
                        self.set_daemon_available(true).await;
                        server_ready.daemon_ready = true;
                        server_ready.reason = None;
                        self.db.set_server_ready(&server_ready).await.unwrap();
                    });

                    Value::String("Wallet imported!".to_string())
                }
                Err(err) => Value::String(format!("Error importing wallet: {:?}", err)),
            }
        }
    }

    async fn start_server_tasks(self, _: context::Context) {
        let self_ref = Arc::new(async_RwLock::new(self));

        let self_clone = Arc::clone(&self_ref);
        let self_clone2 = Arc::clone(&self_ref);
        let self_clone3 = Arc::clone(&self_ref);

        tokio::spawn(async move {
            let self_lock = self_clone.read().await;
            self_lock.monitor_daemon_sync().await;
        });

        tokio::spawn(async move {
            let self_lock = self_clone2.read().await;
            self_lock.check_chain_task().await;
        });

        tokio::spawn(async move {
            let self_lock = self_clone3.read().await;
            self_lock.monitor_daemon_online().await;
        });
    }

    async fn set_timezone(self, _: context::Context, timezone: String) -> Value {
        let valid_timezone = Tz::from_str_insensitive(&timezone);

        if valid_timezone.is_err() {
            return Value::String("Invalid timezone!".to_string());
        }

        let mut conf = self.gv_config.write().await;
        conf.update_gv_config("TIMEZONE", &timezone).unwrap();
        Value::String("Timezone updated!".to_string())
    }
}

fn bool_to_yn(bool_val: bool) -> String {
    let new_val: &str = if bool_val { "YES" } else { "NO" };
    new_val.to_string()
}

async fn spawn(fut: impl Future<Output = ()> + Send + 'static) {
    tokio::spawn(fut);
}

async fn do_shutdown() {
    info!("GhostVault going down for shutdown...");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    std::process::exit(0);
}

async fn get_zmq_listen_addr(gv_config: Arc<async_RwLock<GVConfig>>) -> Vec<String> {
    let conf = gv_config.read().await;
    let zmq_block_host = conf.zmq_block_host.clone();
    let zmq_tx_host = conf.zmq_tx_host.clone();
    drop(conf);

    let mut host_vec: Vec<String> = Vec::new();

    if zmq_block_host == zmq_tx_host {
        host_vec.push(zmq_block_host);
    } else {
        host_vec.push(zmq_block_host);
        host_vec.push(zmq_tx_host);
    }

    host_vec
}

pub async fn run_server(
    gv_config: &Arc<async_RwLock<GVConfig>>,
    db: &Arc<GVDB>,
) -> anyhow::Result<()> {
    let conf = gv_config.read().await;
    let conf_clone: GVConfig = conf.clone();
    let split_ip: Vec<&str> = conf_clone.cli_address.split(":").collect::<Vec<&str>>();
    drop(conf);

    let server_addr = (
        IpAddr::V4(split_ip[0].parse().unwrap()),
        split_ip[1].parse::<u16>().unwrap(),
    );
    let server = GvCLIServer::new(gv_config, db).await;
    let mut listener = tarpc::serde_transport::tcp::listen(&server_addr, Json::default).await?;
    tracing::info!("Listening on port {}", listener.local_addr().port());
    listener.config_mut().max_frame_length(usize::MAX);
    listener
        .filter_map(|r| future::ready(r.ok()))
        .map(BaseChannel::with_defaults)
        .max_channels_per_key(10, |t| t.transport().peer_addr().unwrap().ip())
        .map(|channel| channel.execute(server.clone().serve()).for_each(spawn))
        .buffer_unordered(10)
        .for_each(|_| async {})
        .await;

    Ok(())
}
