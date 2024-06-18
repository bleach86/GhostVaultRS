#![allow(dead_code)]
use crate::{
    config::GVConfig,
    constants::{
        AGVR_ACTIVATION_HEIGHT, DAEMON_PID_FILE, DAEMON_SETTINGS_FILE, DEFAULT_COLD_WALLET,
        DEV_FUND_ADDRESS, MAX_TX_FEES, TMP_PATH,
    },
    file_ops,
    gv_client_methods::CLICaller,
    gv_methods::{self, get_remote_block_chain_info, sha256_digest, PathAndDigest},
    gvdb::{DaemonStatusDB, NewStakeStatusDB, RewardsDB, ZapStatusDB, GVDB},
    rpc::{self, RPCURL},
};
use bitcoincore_zmq::{
    subscribe_async, Message,
    Message::{HashBlock, HashWTx},
};
use futures_util::FutureExt;
use futures_util::StreamExt;
use log::{error, info, trace, warn};
use rand::prelude::SliceRandom;
use rand::Rng;
use reqwest::Client;
use rust_socketio::{
    asynchronous::{Client as sio_Client, ClientBuilder},
    Payload,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::{
    collections::VecDeque,
    error::Error,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex as async_Mutex, RwLock as async_RwLock};
use uuid::Uuid;

#[derive(Debug)]
struct GVDaemonError {
    message: String,
}

impl std::fmt::Display for GVDaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GVDaemonError {}

unsafe impl Send for GVDaemonError {}

unsafe impl Sync for GVDaemonError {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonState {
    pub online: bool,
    pub version: String,
    pub synced: bool,
    pub available: bool,
    pub good_chain: bool,
    pub latest_release: String,
    pub best_block: u32,
    pub best_block_hash: String,
    pub remote_best_block: u32,
    pub remote_best_block_hash: String,
    pub cycle: u32,
}

#[derive(Clone, Debug)]
pub struct DaemonHelper {
    rpcurl: Arc<async_Mutex<RPCURL>>,
    rpc_client: Client,
    daemon_path: PathBuf,
    daemon_data_path: PathBuf,
    config: Arc<async_RwLock<GVConfig>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxidAndWallet {
    pub txid: String,
    pub wallet: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockReward {
    pub total_reward: u64,
    pub stake_reward: u64,
    pub agvr_reward: u64,
    pub stake_kernel: String,
    pub is_coldstake: bool,
}

impl DaemonHelper {
    pub async fn new(config: &Arc<async_RwLock<GVConfig>>, wallet: &str) -> Self {
        let conf = config.read().await;
        let wallet = match wallet {
            "cold" => conf.rpc_wallet.clone(),
            "hot" => conf.rpc_wallet_hot.clone(),
            "no-wallet" => "".to_string(),
            _ => panic!("Invalid wallet"),
        };

        let rpcurl: RPCURL = RPCURL::default().target(
            &conf.rpc_host.as_str(),
            &conf.rpc_port,
            wallet.as_str(),
            &conf.rpc_user.as_str(),
            &conf.rpc_pass.as_str(),
        );

        let rpc_client: Client = Client::new();
        let daemon_path: PathBuf = conf.daemon_path.to_owned();
        let daemon_data_path: PathBuf = conf.daemon_data_dir.to_owned();
        drop(conf);

        let config: Arc<async_RwLock<GVConfig>> = Arc::clone(&config);
        let rpcurl: Arc<async_Mutex<RPCURL>> = Arc::new(async_Mutex::new(rpcurl));

        DaemonHelper {
            rpcurl,
            rpc_client,
            daemon_path,
            daemon_data_path,
            config,
        }
    }

    async fn get_rpcurl(&self) -> RPCURL {
        let rpcurl = self.rpcurl.lock().await;
        rpcurl.clone()
    }

    async fn set_rpcurl(&self, wallet_name: &str) {
        let conf = self.config.read().await;
        let rpcurl_template: RPCURL = RPCURL::default().target(
            &conf.rpc_host.as_str(),
            &conf.rpc_port,
            wallet_name,
            &conf.rpc_user.as_str(),
            &conf.rpc_pass.as_str(),
        );
        let mut rpcurl = self.rpcurl.lock().await;
        *rpcurl = rpcurl_template;
    }

    pub async fn getblockcount(&self) -> Result<Value, Box<dyn Error>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("getblockcount", &self.get_rpcurl().await, &self.rpc_client).await;

        let block_count = match res {
            Ok(ref value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(block_count.to_owned())
    }

    pub async fn getcoldstakinginfo(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> = rpc::call(
            "getcoldstakinginfo",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await;

        let cold_info = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(cold_info)
    }

    pub async fn get_balances(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("getbalances", &self.get_rpcurl().await, &self.rpc_client).await;

        let balances = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(balances)
    }

    pub async fn validate_address(
        &self,
        address: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let args: String = format!("validateaddress {}", address);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let address_info = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(address_info)
    }

    pub async fn get_best_block_hash(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> = rpc::call(
            "getbestblockhash",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await;

        let best_block_hash = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(best_block_hash)
    }

    pub async fn get_address_info(
        &self,
        address: &str,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let args: String = format!("getaddressinfo {}", address);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let address_info = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(address_info)
    }

    pub async fn is_valid_address(
        &self,
        address: &str,
    ) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let res = self.validate_address(address).await?;

        if res.is_object() {
            let is_valid: bool = res.get("isvalid").unwrap().as_bool().unwrap();
            Ok(is_valid)
        } else {
            Ok(false)
        }
    }

    pub async fn getnewaddress(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("getnewaddress", &self.get_rpcurl().await, &self.rpc_client).await;

        let new_address = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(new_address)
    }

    pub async fn getnewextaddress(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> = rpc::call(
            "getnewextaddress",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await;

        let new_address = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(new_address)
    }

    pub async fn getnewstealthaddress(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> = rpc::call(
            "getnewstealthaddress",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await;

        let new_stealth_address = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(new_stealth_address)
    }

    pub async fn getstakinginfo(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("getstakinginfo", &self.get_rpcurl().await, &self.rpc_client).await;

        let staking_info = match res {
            Ok(value) => value,
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(staking_info)
    }

    pub async fn getblock(
        &self,
        block_hash: &str,
        verbosity: u8,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let command: String = format!("getblock {} {}", block_hash, verbosity);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&command, &self.get_rpcurl().await, &self.rpc_client).await;

        let block_info = match res {
            Ok(ref value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(block_info.to_owned())
    }

    pub async fn getuptime(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("uptime", &self.get_rpcurl().await, &self.rpc_client).await;

        let uptime = match res {
            Ok(ref value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(uptime.to_owned())
    }

    pub async fn getnetworkinfo(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("getnetworkinfo", &self.get_rpcurl().await, &self.rpc_client).await;

        let networkinfo = match res {
            Ok(ref value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(networkinfo.to_owned())
    }

    pub async fn getblockchaininfo(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> = rpc::call(
            "getblockchaininfo",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await;

        let blockchaininfo = match res {
            Ok(ref value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(blockchaininfo.to_owned())
    }

    pub async fn list_wallets(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("listwallets", &self.get_rpcurl().await, &self.rpc_client).await;

        let loaded_wallets = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(loaded_wallets)
    }

    pub async fn get_new_mnemonic(
        &self,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("mnemonic new", &self.get_rpcurl().await, &self.rpc_client).await;

        let mnemonic = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(mnemonic)
    }

    pub async fn is_syncing(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> = rpc::call(
            "getblockchaininfo",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await;

        let blockchaininfo = match res {
            Ok(ref value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        let blocks: u64 = blockchaininfo.get("blocks").unwrap().as_u64().unwrap();
        let headers: u64 = blockchaininfo.get("headers").unwrap().as_u64().unwrap();
        let ibdl: bool = blockchaininfo
            .get("initialblockdownload")
            .unwrap()
            .as_bool()
            .unwrap();

        let is_syncing = if ibdl || blocks != headers {
            true
        } else {
            false
        };

        Ok(is_syncing)
    }

    pub async fn load_wallet(
        &self,
        wallet: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let loaded_wallets: Value = self.list_wallets().await?;

        for wall in loaded_wallets.as_array().unwrap() {
            if wallet == wall {
                return Ok(Value::String("Wallet Alrady loaded, ok".to_string()));
            }
        }

        let args: String = format!("loadwallet {} true", wallet);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let wallet_loaded = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(wallet_loaded)
    }

    pub async fn get_reward_addr_from_wallet(
        &self,
    ) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> = rpc::call(
            "walletsettings stakingoptions",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await;

        let reward_value = match res {
            Ok(ref value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        let reward_addr: Option<String> = reward_value
            .get("stakingoptions")
            .and_then(|options| options.as_object())
            .and_then(|options| options.get("rewardaddress"))
            .and_then(|address| address.as_str())
            .map(|address| address.to_string());

        Ok(reward_addr)
    }

    pub async fn set_reward_addr_in_wallet(
        &self,
        reward_addr: Option<&str>,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let setting_req: String = if reward_addr.is_none() {
            r#"{}"#.to_string()
        } else {
            format!(r#"{{"rewardaddress":"{}"}}"#, reward_addr.unwrap())
        };

        let json_data: Value = serde_json::from_str(&setting_req).unwrap();

        let args: String = format!("walletsettings stakingoptions {}", json_data);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let reward_addr_set = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(reward_addr_set)
    }

    pub async fn create_default_wallet(
        &self,
        wallet_name: &str,
        _db: &GVDB,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let seed_value: Value = self.get_new_mnemonic().await.unwrap();
        let mnemonic: &str = seed_value["mnemonic"].as_str().unwrap();

        let args: String = format!("createwallet {wallet_name} false false \"\" false false true");
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let _wallet_created = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        let args: String = format!("extkeyimportmaster \"{mnemonic}\" \"\" false \"GV_DEFAULT_COLD_WALLET\" \"GV_DEFAULT_COLD_WALLET\" -1");

        self.set_rpcurl(wallet_name).await;

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let _import_master = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        let ext_pub_key_value: Value = self.getnewextaddress().await?;
        let ext_pub_key: &str = ext_pub_key_value.as_str().unwrap();
        let internal_anon = self
            .getnewstealthaddress()
            .await?
            .as_str()
            .unwrap()
            .to_string();
        let mut conf = self.config.write().await;

        conf.update_gv_config("EXT_PUB_KEY", ext_pub_key)?;
        conf.update_gv_config("INTERNAL_ANON", &internal_anon)?;
        conf.update_gv_config("MNEMONIC", mnemonic)?;

        drop(conf);

        Ok(seed_value)
    }

    pub async fn validate_mnemonic(
        &self,
        mnemonic: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let args: String = format!(r#"mnemonic decode "" "{}""#, mnemonic);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let mnemonic_valid = match res {
            Ok(_) => true,
            Err(_) => false,
        };

        Ok(mnemonic_valid)
    }

    pub async fn import_wallet(
        &self,
        wallet_name: &str,
        mnemonic: &str,
        db: &GVDB,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mnemonic_valid: bool = self.validate_mnemonic(mnemonic).await?;

        if !mnemonic_valid {
            return Err(Box::new(GVDaemonError {
                message: "Invalid mnemonic".to_string(),
            }));
        }
        // Create wallet
        let args: String = format!(
            r#"createwallet "{}" false false "" false false true"#,
            wallet_name
        );

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let _wallet_created = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        // Set rpcurl
        let conf = self.config.read().await;

        let old_wallet = conf.rpc_wallet.clone();
        let anon_mode: bool = conf.anon_mode;
        drop(conf);

        self.set_rpcurl(wallet_name).await;

        // Import master key
        let args: String = format!(
            r#"extkeyimportmaster "{}" "" false "{}" "{}""#,
            mnemonic, wallet_name, wallet_name
        );
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let import_master = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                self.set_rpcurl(&old_wallet).await;
                return Err(err);
            }
        };

        self.set_rpcurl("").await;

        // Unload old wallet
        let args: String = format!("unloadwallet {} false", old_wallet);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let _wallet_unloaded = match res {
            Ok(ref value) => value.to_owned(),
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        self.set_rpcurl(wallet_name).await;

        let ext_pub_key_value: Value = self.getnewextaddress().await?;
        let ext_pub_key: &str = ext_pub_key_value.as_str().unwrap();
        let internal_anon = self
            .getnewstealthaddress()
            .await?
            .as_str()
            .unwrap()
            .to_string();

        let mut conf = self.config.write().await;

        conf.update_gv_config("EXT_PUB_KEY", ext_pub_key)?;
        conf.update_gv_config("INTERNAL_ANON", &internal_anon)?;

        if anon_mode {
            conf.update_gv_config("REWARD_ADDRESS", &internal_anon)?;
        }

        conf.update_gv_config("MNEMONIC", mnemonic)?;
        conf.update_gv_config("RPC_WALLET", wallet_name)?;

        drop(conf);

        self.check_wallets(db).await?;

        Ok(import_master)
    }

    pub async fn check_wallets(
        &self,
        db: &GVDB,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        info!("Checking wallets...");
        let conf = self.config.read().await;
        let create_on_fail: bool = if conf.rpc_wallet.is_empty() {
            true
        } else {
            false
        };

        let rpc_wallet: String = conf.rpc_wallet.clone();
        let internal_anon = conf.internal_anon.clone();
        let ext_pub_key = conf.ext_pub_key.clone();
        drop(conf);

        let cold_wallet: &str = if create_on_fail {
            DEFAULT_COLD_WALLET
        } else {
            &rpc_wallet
        };

        let loaded_wallets: Value = self.list_wallets().await.unwrap();

        let is_loaded: bool = if rpc_wallet.is_empty() {
            false
        } else {
            loaded_wallets
                .as_array()
                .unwrap()
                .contains(&Value::String(cold_wallet.to_string()))
        };

        if !is_loaded {
            info!("Cold wallet not loaded, attempting to load...");
            let load_cold_wallet: Result<Value, Box<dyn Error + Send + Sync>> =
                self.load_wallet(cold_wallet).await;

            if load_cold_wallet.is_err() {
                if create_on_fail {
                    self.create_default_wallet(cold_wallet, db).await?;

                    let mut conf = self.config.write().await;
                    conf.update_gv_config("RPC_WALLET", cold_wallet)?;
                    drop(conf);
                } else {
                    panic!("Failed to load wallet");
                }
            } else {
                if rpc_wallet != cold_wallet {
                    let mut conf = self.config.write().await;
                    conf.update_gv_config("RPC_WALLET", cold_wallet)?;
                    drop(conf);
                }
            }
        }

        let conf = self.config.read().await;

        self.set_rpcurl(conf.rpc_wallet.as_str()).await;

        let wallet_reward_addr: Option<String> = self.get_reward_addr_from_wallet().await.unwrap();

        if conf.reward_address.is_some() {
            let reward_addr: String = conf.clone().reward_address.unwrap();
            if wallet_reward_addr.is_none() {
                info!("Setting reward address in wallet...");
                self.set_reward_addr_in_wallet(Some(&reward_addr))
                    .await
                    .unwrap();
            } else if wallet_reward_addr.unwrap() != reward_addr {
                info!("Setting reward address in wallet...");
                self.set_reward_addr_in_wallet(Some(&reward_addr))
                    .await
                    .unwrap();
            }
        } else {
            if wallet_reward_addr.is_some() {
                info!("Clearing reward address in wallet...");
                self.set_reward_addr_in_wallet(None).await.unwrap();
            }
        }

        drop(conf);

        if internal_anon.is_none() {
            let anon_addr: String = self
                .getnewstealthaddress()
                .await
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            let mut conf = self.config.write().await;
            conf.update_gv_config("INTERNAL_ANON", &anon_addr)?;

            drop(conf);
        }

        if ext_pub_key.is_none() {
            let ext_pub_key: String = self
                .getnewextaddress()
                .await
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            let mut conf = self.config.write().await;
            conf.update_gv_config("EXT_PUB_KEY", &ext_pub_key)?;

            drop(conf);
        }

        Ok(Value::String("ok".to_string()))
    }

    pub async fn stop_daemon(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        info!("Sending Ghost daemon the shutdown signal...");
        let ghost_daemon_pid: u32 = file_ops::get_pid(&self.daemon_data_path, DAEMON_PID_FILE);

        if !file_ops::pid_exists(ghost_daemon_pid) {
            return Ok(Value::String("Ghost daemon is down".to_string()));
        }

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call("stop", &self.get_rpcurl().await, &self.rpc_client).await;

        let deamon_stop = match res {
            Ok(value) => value,
            Err(err) => {
                error!("{}", err.to_string());

                let err_str = err.to_string();

                if err_str.contains("401 Unauthorized") {
                    info!("Stopping by RPC failed, attempting via ghost-cli...");
                    let res = self.stop_daemon_cli().await;

                    if res.is_ok() {
                        Value::String("ghost core going down".to_string())
                    } else {
                        return Err(err);
                    }
                } else {
                    return Err(err);
                }
            }
        };

        self.wait_for_daemon_shutdown().await;

        Ok(deamon_stop)
    }

    async fn stop_daemon_cli(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let conf = self.config.read().await;

        let daemon_path: PathBuf = conf.daemon_path.clone();
        let daemon_data_dir: PathBuf = conf.daemon_data_dir.clone();
        let daemon_conf_path: PathBuf = daemon_data_dir.join(DAEMON_SETTINGS_FILE);

        let daemon_path_str: &str = daemon_path.to_str().ok_or("Invalid daemon path")?;
        drop(conf);
        let stripped_d_path_str: &str = daemon_path_str
            .strip_suffix("d")
            .ok_or("Invalid daemon path")?;

        let cli_path: PathBuf = PathBuf::from(format!("{}-cli", stripped_d_path_str));

        if cli_path.exists() {
            let command = Command::new(&cli_path)
                .arg(format!("-datadir={}", daemon_data_dir.to_str().unwrap()))
                .arg(format!("-conf={}", daemon_conf_path.to_str().unwrap()))
                .arg("stop")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()?;

            if command.status.success() {
                Ok(())
            } else {
                let error_message = String::from_utf8_lossy(&command.stderr);
                Err(error_message.into())
            }
        } else {
            Err("ghost-cli not found".into())
        }
    }

    pub async fn wait_for_daemon_shutdown(&self) {
        let ghost_daemon_pid: u32 = file_ops::get_pid(&self.daemon_data_path, DAEMON_PID_FILE);
        info!("Waiting for Ghost daemon to shutdown...");
        while file_ops::pid_exists(ghost_daemon_pid) {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        info!("Ghost daemon is fully shut down...");
    }

    pub async fn wait_for_daemon_startup(&self) {
        if self.call_status(true).await.unwrap().is_null() {
            info!("Waiting for Ghost daemon to startup...");

            while self.call_status(true).await.unwrap().is_null() {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }

        info!("Ghost daemon is ready...");
    }

    pub async fn call_status(
        &self,
        restart_on_error: bool,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let res: Result<Value, Box<dyn Error + Send + Sync>> = rpc::call(
            "getblockchaininfo",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await;

        let status: &Value = match res {
            Ok(ref value) => value,
            Err(err) => {
                if restart_on_error {
                    self.parse_error_msg(err.to_string()).await;
                }

                &Value::Null
            }
        };

        Ok(status.to_owned())
    }

    pub async fn get_transaction(
        &self,
        txid: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let command: String = format!("gettransaction {} true true", txid);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&command, &self.get_rpcurl().await, &self.rpc_client).await;

        let tx_details = match res {
            Ok(ref value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(tx_details.to_owned())
    }

    pub async fn cleanup_missing_tx(&self, db: &Arc<GVDB>) {
        info!("Checking missed stakes...");
        let last_status: Option<DaemonStatusDB> = db.get_daemon_status();

        if last_status.is_none() {
            self.import_legacy_history(db).await;
        } else {
            let last_status: DaemonStatusDB = last_status.unwrap();
            let args: String = format!("listsinceblock {} 1 true", last_status.block_hash);

            let res = rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

            let res_opt = match res {
                Ok(value) => Some(value),
                Err(_) => None,
            };

            let tx_array_opt = if res_opt.is_some() {
                let value = res_opt.as_ref().unwrap();
                let tx_array = value.get("transactions").unwrap().as_array();
                tx_array
            } else {
                None
            };

            if tx_array_opt.is_some() {
                let mut tx_array = tx_array_opt.unwrap().clone();
                tx_array.sort_by_key(|tx| {
                    tx.get("confirmations")
                        .unwrap_or(&Value::Number(0.into()))
                        .as_i64()
                        .unwrap()
                });
                let mut count = 0;
                for tx in tx_array.iter().rev() {
                    let trusted: Option<&Value> = tx.get("trusted");

                    if trusted.is_some() {
                        if !trusted.unwrap().as_bool().unwrap() {
                            continue;
                        }
                    }

                    let category: &str = tx.get("category").unwrap().as_str().unwrap();
                    match category {
                        "stake" => {
                            self.process_stake_transaction(tx, &db).await;
                            count += 1;
                        }
                        "receive" => {
                            let is_watchonly = tx
                                .get("involvesWatchonly")
                                .map_or(false, |value| value.as_bool().unwrap());

                            if !is_watchonly {
                                continue;
                            }
                            self.process_received_tx(tx, &db).await;
                        }
                        _ => {}
                    }
                }

                if count > 0 {
                    info!("Successfully imported {count} stakes...");
                }
            }
        }

        for result in db.zap_status_db.iter() {
            match result {
                Ok((key, value)) => {
                    let mut zap_item: ZapStatusDB = serde_json::from_slice(&value).unwrap();
                    let txid: &str = zap_item.txid.as_str();
                    let tx = self.get_transaction(txid).await;

                    let tx = if tx.is_err() {
                        db.remove_zap_status(key).await.unwrap();
                        continue;
                    } else {
                        tx.unwrap()
                    };

                    let confirms: u32 = tx.get("confirmations").unwrap().as_u64().unwrap() as u32;

                    if confirms > 225 {
                        db.remove_zap_status(key).await.unwrap();
                    } else {
                        zap_item.confirmations = confirms;
                        db.set_zap_status(key, &zap_item).await.unwrap();
                    }
                }
                Err(err) => {
                    error!("Error during iteration: {:?}", err);
                }
            }
        }

        let bc_info: Value = self.getblockchaininfo().await.unwrap();
        let height: u32 = bc_info.get("blocks").unwrap().as_u64().unwrap() as u32;
        let block_hash: String = bc_info
            .get("bestblockhash")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let daemon_status: DaemonStatusDB = DaemonStatusDB { height, block_hash };
        db.set_daemon_status(&daemon_status).await.unwrap();
    }

    async fn process_received_tx(&self, tx: &Value, db: &Arc<GVDB>) -> Option<ZapStatusDB> {
        let tx_category: &str = tx.get("category").unwrap().as_str().unwrap();

        if tx_category != "receive" {
            return None;
        }

        let confirms = tx.get("confirmations");
        let confirms = if confirms.is_none() {
            return None;
        } else {
            confirms.unwrap().as_i64().unwrap() as i32
        };

        if confirms > 225 || confirms < 0 {
            return None;
        }

        let txid: &str = tx.get("txid").unwrap().as_str().unwrap();

        let zap_item: Option<ZapStatusDB> = db.get_zap_status(txid);

        if zap_item.is_none() {
            let amount: f64 = tx.get("amount").unwrap().as_f64().unwrap();
            let amount_int: u64 = self.convert_to_sat(amount);
            let first_notice: bool = false;

            let zap_item: ZapStatusDB = ZapStatusDB {
                txid: txid.to_string(),
                amount: amount_int,
                confirmations: confirms as u32,
                first_notice,
            };

            db.set_zap_status(txid.as_bytes(), &zap_item).await.unwrap();
            Some(zap_item)
        } else {
            Some(zap_item.unwrap())
        }
    }

    pub async fn clear_wallet_tx(&self) {
        let _: Value = rpc::call(
            "clearwallettransactions",
            &self.get_rpcurl().await,
            &self.rpc_client,
        )
        .await
        .unwrap();
    }

    pub async fn get_last_stake(
        &self,
    ) -> Result<Option<Value>, Box<dyn std::error::Error + Send + Sync>> {
        let req = r#"{
            "count": 1,
            "category": "stake",
            "include_watchonly": true
        }"#;

        let json_data: Value = serde_json::from_str(req).unwrap();
        let args: String = format!("filtertransactions {}", json_data);

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let last_stake = match res {
            Ok(value) => {
                let value_array = value.as_array().unwrap();
                if value_array.is_empty() {
                    None
                } else {
                    Some(value_array[0].to_owned())
                }
            }
            Err(err) => {
                self.parse_error_msg(err.to_string()).await;
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(last_stake)
    }

    pub async fn import_legacy_history(&self, db: &Arc<GVDB>) {
        let req = r#"{
            "count": 0,
            "include_watchonly": true,
            "sort": "confirmations"
        }"#;

        let json_data: Value = serde_json::from_str(req).unwrap();
        let args: String = format!("filtertransactions {}", json_data);

        let res: Value = rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client)
            .await
            .unwrap();

        let tx_array: &Vec<Value> = res.as_array().unwrap();

        for tx in tx_array.iter() {
            let category: &str = tx.get("category").unwrap().as_str().unwrap();
            let confirms: i64 = tx.get("confirmations").unwrap().as_i64().unwrap();

            if confirms < 0 {
                continue;
            }

            match category {
                "stake" => {
                    self.process_stake_transaction(tx, &db).await;
                }
                "receive" => {
                    let tx_outputs = tx.get("outputs").unwrap().as_array().unwrap();

                    if tx_outputs.is_empty() {
                        continue;
                    }

                    let is_watchonly = tx_outputs[0]
                        .get("involvesWatchonly")
                        .unwrap_or(&Value::Bool(false))
                        .as_bool()
                        .unwrap();

                    if !is_watchonly {
                        continue;
                    }

                    self.process_received_tx(tx, &db).await;
                }
                _ => {
                    continue;
                }
            }
        }
    }

    pub async fn process_stake_transaction(&self, tx: &Value, db: &Arc<GVDB>) -> RewardsDB {
        let timestamp: u64 = tx.get("blocktime").unwrap().as_u64().unwrap();
        let height: u32 = tx.get("blockheight").unwrap().as_u64().unwrap() as u32;

        let block_hash: String = tx.get("blockhash").unwrap().as_str().unwrap().to_string();
        let txid: String = tx.get("txid").unwrap().as_str().unwrap().to_string();

        let block_reward_details: BlockReward = self.get_block_reward(&txid, height).await.unwrap();

        let reward: u64 = block_reward_details.stake_reward;
        let agvr_reward: u64 = block_reward_details.agvr_reward;
        let address: String = block_reward_details.stake_kernel;
        let is_coldstake: bool = block_reward_details.is_coldstake;

        let last_stake_opt = db.rewards_ts_index.last().unwrap();

        let (all_time_reward, all_time_agvr_reward) = match last_stake_opt {
            Some((_, value)) => {
                let stake_info: RewardsDB = serde_json::from_slice(&value).unwrap();
                (
                    stake_info.all_time_reward + reward,
                    stake_info.all_time_agvr_reward + agvr_reward,
                )
            }
            None => (reward, agvr_reward),
        };

        let final_reward: RewardsDB = RewardsDB {
            height,
            timestamp,
            block_hash,
            txid,
            reward,
            agvr_reward,
            all_time_reward,
            all_time_agvr_reward,
            address,
            is_coldstake,
        };

        let confirms: u64 = tx
            .get("confirmations")
            .map_or(0, |val| val.as_u64().unwrap());

        if confirms <= 100 {
            let txid: &str = tx.get("txid").unwrap().as_str().unwrap();
            let timestamp: u64 = tx.get("blocktime").unwrap().as_u64().unwrap();

            let stake_item: NewStakeStatusDB = NewStakeStatusDB {
                txid: txid.to_string(),
                timestamp,
                confirmations: confirms as u32,
                tg_msg_id: None,
            };

            db.set_new_stake_status(txid.as_bytes(), &stake_item)
                .await
                .unwrap();
        }

        db.set_reward(&final_reward).await.unwrap();

        final_reward
    }

    pub async fn get_block_reward(
        &self,
        txid: &str,
        height: u32,
    ) -> Result<BlockReward, Box<dyn std::error::Error + Send + Sync>> {
        let tx_details: Value = self.get_transaction(txid).await.unwrap();

        let tx_vin = &tx_details
            .get("decoded")
            .ok_or("No decoded value")?
            .get("vin")
            .ok_or("Vin not found")?
            .as_array()
            .ok_or("Vin not an array")?;

        let mut in_amount: u64 = 0;
        let mut stake_kernel: String = "".to_string();

        let vout_array = tx_details
            .get("decoded")
            .ok_or("No decoded value")?
            .get("vout")
            .ok_or("Vout not found")?
            .as_array()
            .ok_or("Vout not an array")?;

        for vin in tx_vin.iter() {
            let prev_txid: &str = vin.get("txid").unwrap().as_str().unwrap();
            let prev_vout: u64 = vin.get("vout").unwrap().as_u64().unwrap();

            let prev_tx: Result<Value, Box<dyn Error + Send + Sync>> =
                self.get_transaction(prev_txid).await;

            if prev_tx.is_ok() {
                let prev_tx = prev_tx.unwrap();
                let prev_vout_array = prev_tx
                    .get("decoded")
                    .ok_or("No decoded value")?
                    .get("vout")
                    .ok_or("Vout not found")?
                    .as_array()
                    .ok_or("Vout not an array")?;

                in_amount += prev_vout_array[prev_vout as usize]
                    .get("valueSat")
                    .unwrap()
                    .as_u64()
                    .unwrap();

                if stake_kernel.is_empty() {
                    stake_kernel = self.get_addr_from_vout(&prev_vout_array[prev_vout as usize]);
                }
            } else {
                stake_kernel = self.get_addr_from_vout(&vout_array[1]);
            }
        }

        let is_agvr: bool = if height < AGVR_ACTIVATION_HEIGHT {
            false
        } else {
            vout_array[0].get("gvr_fund_cfwd").is_none()
        };

        let agvr_reward: u64 = if !is_agvr {
            0
        } else {
            let vout_addr = self.get_addr_from_vout(&vout_array[1]);

            let agvr_vout = if vout_addr == stake_kernel { 1 } else { 2 };

            vout_array[agvr_vout]
                .get("valueSat")
                .unwrap()
                .as_u64()
                .unwrap()
        };

        let mut vout_total: u64 = 0;
        let mut is_coldstake: bool = false;

        for vout in vout_array {
            let blacklist_type: Vec<&str> = vec!["data", "anon", "blind"];

            let default_vout_type = Value::String(String::new());

            let vout_type: &str = vout
                .get("type")
                .unwrap_or(&default_vout_type)
                .as_str()
                .unwrap();
            if blacklist_type.contains(&vout_type) {
                continue;
            }

            let vout_addr: String = self.get_addr_from_vout(&vout);
            if DEV_FUND_ADDRESS.contains(&vout_addr.as_str()) {
                continue;
            }

            if vout
                .get("scriptPubKey")
                .unwrap_or(&Value::Object(serde_json::Map::new()))
                .get("stakeaddresses")
                .is_some()
            {
                is_coldstake = true;
            }

            vout_total += vout.get("valueSat").unwrap().as_u64().unwrap();
        }

        let stake_reward: u64 = vout_total - in_amount - agvr_reward;
        let total_reward: u64 = agvr_reward + stake_reward;

        let reward: BlockReward = BlockReward {
            total_reward,
            stake_reward,
            agvr_reward,
            stake_kernel,
            is_coldstake,
        };

        Ok(reward)
    }

    pub async fn get_daemon_version(
        &self,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let conf = self.config.read().await;
        let daemon_path = conf.daemon_path.clone();
        drop(conf);

        let daemon_path = if !daemon_path.exists() {
            error!("ghostd not found! Attempting to download...");
            self.download_daemon().await?;
            let conf = self.config.read().await;
            let daemon_path = conf.daemon_path.clone();
            drop(conf);
            daemon_path
        } else {
            daemon_path
        };

        let output = Command::new(&daemon_path)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Command failed to start")
            .wait_with_output()
            .expect("Failed to wait for child process");

        if output.status.success() {
            let result_string = String::from_utf8_lossy(&output.stdout);
            let version = result_string
                .split("\n")
                .collect::<Vec<&str>>()
                .first()
                .unwrap()
                .split(" ")
                .collect::<Vec<&str>>()
                .last()
                .unwrap()
                .strip_prefix("v")
                .unwrap()
                .split("-")
                .collect::<Vec<&str>>()
                .first()
                .unwrap()
                .strip_suffix(".0")
                .unwrap()
                .to_string();
            Ok(version)
        } else {
            let error_string = String::from_utf8_lossy(&output.stderr);
            let err: String = format!("Command failed with error:\n{}", error_string);
            Err(err.into())
        }
    }

    pub async fn build_script(
        &self,
        stake_addr: &str,
        spend_addr: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let script_req: String = format!(
            r#"
            {{
                "recipe": "ifcoinstake",
                "addrstake": "{stake_addr}",
                "addrspend": "{spend_addr}"
            }}"#
        );

        let json_data: Value = serde_json::from_str(&script_req)?;

        let args: String = format!("buildscript {}", json_data);
        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let script: Value = match res {
            Ok(value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(script)
    }

    pub async fn send_ghost(
        &self,
        addr: &str,
        in_type: &str,
        out_type: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut txids: Vec<Value> = Vec::new();
        let max_fee: f64 = self.convert_from_sat(MAX_TX_FEES);
        let mut output_amt: f64 = 0.0;
        let mut inputs: Vec<String> = Vec::new();

        let unspent: Value = self.list_unspent(in_type).await?;
        let unspent_array: &Vec<Value> = unspent.as_array().unwrap();
        let unspent_len: usize = unspent_array.len();

        for (index, unspent_item) in unspent_array.iter().enumerate() {
            let amount: f64 = unspent_item.get("amount").unwrap().as_f64().unwrap();
            let txid: &str = unspent_item.get("txid").unwrap().as_str().unwrap();
            let vout: u32 = unspent_item.get("vout").unwrap().as_u64().unwrap() as u32;
            let spendable: bool = {
                let safe: bool = unspent_item
                    .get("safe")
                    .unwrap_or(&Value::Bool(false))
                    .as_bool()
                    .unwrap();
                let inner_spendable: bool = unspent_item
                    .get("spendable")
                    .unwrap_or(&Value::Bool(false))
                    .as_bool()
                    .unwrap();

                if in_type == "ghost" {
                    safe && inner_spendable
                } else {
                    safe
                }
            };

            if spendable {
                let input: String = format!(
                    r#"{{
                        "tx": "{txid}",
                        "n": {vout}
                    }}"#
                );

                inputs.push(input);

                output_amt += amount;
            }

            let is_last: bool = index + 1 == unspent_len;

            if is_last && inputs.is_empty() {
                return Ok(Value::Array(txids));
            } else if inputs.is_empty() {
                continue;
            }

            // every 100 inputs we check the fee and send the tx, or if we are at the last unspent item
            if inputs.len() % 100 == 0 || is_last {
                let precise_amount = self.precise(output_amt);

                let outputs: String = format!(
                    r#"
                        [{{
                            "address": "{addr}",
                            "amount": {precise_amount},
                            "subfee": true
                        }}]"#
                );

                let json_data_out: Value = serde_json::from_str(&outputs)?;
                let json_data_in: Value = serde_json::from_value(Value::Array(
                    inputs
                        .iter()
                        .map(|inp| {
                            serde_json::from_str::<Value>(inp).expect("Failed to parse JSON string")
                        })
                        .collect(),
                ))?;

                let args: String = format!(
                    r#"sendtypeto {} {} {} "" "" 12 1 true {{"feeRate":0.00007500,"inputs":{}}}"#,
                    in_type, out_type, json_data_out, json_data_in
                );

                let fee_res = rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

                let fee: Value = match fee_res {
                    Ok(value) => value.get("fee").unwrap().to_owned(),
                    Err(err) => {
                        error!("{}", err.to_string());
                        return Err(err);
                    }
                };

                let fee_amt: f64 = fee.as_f64().unwrap();

                // If the fee is greater than the max fee or we are at the last unspent item
                if fee_amt >= max_fee || is_last {
                    let args: String = format!(
                        r#"sendtypeto {} {} {} "" "" 12 1 false {{"feeRate":0.00007500,"inputs":{}}}"#,
                        in_type, out_type, json_data_out, json_data_in
                    );

                    let res: Result<Value, Box<dyn Error + Send + Sync>> =
                        rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

                    let txid: Value = match res {
                        Ok(value) => value,
                        Err(err) => {
                            error!("{}", err.to_string());
                            return Err(err);
                        }
                    };

                    txids.push(txid);

                    inputs.clear();
                    output_amt = 0.0;
                }
            }
        }

        Ok(Value::Array(txids))
    }

    pub async fn zap_ghost(
        &self,
        spend_addr: &str,
        in_type: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let stake_addr: String = self.get_stake_addr().await?;
        let max_fee: f64 = self.convert_from_sat(MAX_TX_FEES);

        let mut txids: Vec<Value> = Vec::new();

        let cs_script_value: Value = self.build_script(&stake_addr, &spend_addr).await?;
        let cs_script: String = cs_script_value
            .get("hex")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let mut output_amt: f64 = 0.0;

        let mut inputs: Vec<String> = Vec::new();

        let unspent: Value = self.list_unspent(in_type).await?;
        let unspent_array: &Vec<Value> = unspent.as_array().unwrap();
        let unspent_len: usize = unspent_array.len();

        for (index, unspent_item) in unspent_array.iter().enumerate() {
            let amount: f64 = unspent_item.get("amount").unwrap().as_f64().unwrap();
            let txid: &str = unspent_item.get("txid").unwrap().as_str().unwrap();
            let vout: u32 = unspent_item.get("vout").unwrap().as_u64().unwrap() as u32;
            let spendable: bool = {
                let safe: bool = unspent_item
                    .get("safe")
                    .unwrap_or(&Value::Bool(false))
                    .as_bool()
                    .unwrap();
                let inner_spendable: bool = unspent_item
                    .get("spendable")
                    .unwrap_or(&Value::Bool(false))
                    .as_bool()
                    .unwrap();

                if in_type == "ghost" {
                    safe && inner_spendable
                } else {
                    safe
                }
            };

            if spendable {
                let input: String = format!(
                    r#"{{
                        "tx": "{txid}",
                        "n": {vout}
                    }}"#
                );

                inputs.push(input);

                output_amt += amount;
            }

            let is_last: bool = index + 1 == unspent_len;

            if is_last && inputs.is_empty() {
                return Ok(Value::Array(txids));
            } else if inputs.is_empty() {
                continue;
            }

            // every 100 inputs we check the fee and send the tx, or if we are at the last unspent item
            if inputs.len() % 100 == 0 || is_last {
                let precise_amount = self.precise(output_amt);

                let outputs: String = format!(
                    r#"
                        [{{
                            "address": "script",
                            "amount": {precise_amount},
                            "script": "{cs_script}",
                            "subfee": true
                        }}]"#
                );

                let json_data_out: Value = serde_json::from_str(&outputs)?;
                let json_data_in: Value = serde_json::from_value(Value::Array(
                    inputs
                        .iter()
                        .map(|inp| {
                            serde_json::from_str::<Value>(inp).expect("Failed to parse JSON string")
                        })
                        .collect(),
                ))?;

                let args: String = format!(
                    r#"sendtypeto {} ghost {} "" "" 12 1 true {{"feeRate":0.00007500,"inputs":{}}}"#,
                    in_type, json_data_out, json_data_in
                );

                let fee_res = rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

                let fee: Value = match fee_res {
                    Ok(value) => value.get("fee").unwrap().to_owned(),
                    Err(err) => {
                        error!("{}", err.to_string());
                        return Err(err);
                    }
                };

                let fee_amt: f64 = fee.as_f64().unwrap();

                // If the fee is greater than the max fee or we are at the last unspent item
                if fee_amt >= max_fee || is_last {
                    let args: String = format!(
                        r#"sendtypeto {} ghost {} "" "" 12 1 false {{"feeRate":0.00007500,"inputs":{}}}"#,
                        in_type, json_data_out, json_data_in
                    );

                    let res: Result<Value, Box<dyn Error + Send + Sync>> =
                        rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

                    let txid: Value = match res {
                        Ok(value) => value,
                        Err(err) => {
                            error!("{}", err.to_string());
                            return Err(err);
                        }
                    };

                    txids.push(txid);

                    inputs.clear();
                    output_amt = 0.0;
                }
            }
        }

        Ok(Value::Array(txids))
    }

    pub async fn list_unspent(
        &self,
        uns_type: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let args: String = match uns_type {
            "ghost" => format!("listunspent 1 9999999 [] false"),
            "anon" => format!("listunspentanon 12 9999999 [] false"),
            _ => format!("listunspent 1 9999999 [] false"),
        };

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let unspent: Value = match res {
            Ok(value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        Ok(unspent)
    }

    pub async fn get_stake_addr(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let conf = self.config.read().await;

        let ext_pub_key: String = conf.ext_pub_key.clone().unwrap();
        drop(conf);
        let addr_index: i32 = rand::thread_rng().gen_range(0..64);
        let args: String = format!(
            "deriverangekeys {} {} {}",
            addr_index, addr_index, ext_pub_key
        );

        let res: Result<Value, Box<dyn Error + Send + Sync>> =
            rpc::call(&args, &self.get_rpcurl().await, &self.rpc_client).await;

        let stake_addr = match res {
            Ok(value) => value,
            Err(err) => {
                error!("{}", err.to_string());
                return Err(err);
            }
        };

        let addr: String = stake_addr.as_array().unwrap()[0]
            .as_str()
            .unwrap()
            .to_string();

        Ok(addr)
    }

    pub async fn start_daemon(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let conf = self.config.read().await;
        let daemon_path = conf.daemon_path.clone();
        let daemon_hash_opt = conf.daemon_hash.clone();

        let daemon_data_dir = conf.daemon_data_dir.clone();
        let daemon_conf_path = daemon_data_dir.join(DAEMON_SETTINGS_FILE);

        drop(conf);

        if !daemon_path.exists() {
            error!("ghostd not found! Attempting to download...");
            self.download_daemon().await?;
        }

        let valid_hash = if daemon_hash_opt.is_none() {
            false
        } else {
            let expected_daemon_hash = daemon_hash_opt.unwrap();
            let actual_daemon_hash = sha256_digest(&daemon_path)?;
            expected_daemon_hash == actual_daemon_hash
        };

        if !valid_hash {
            error!("ghostd courruption detected! Fetching daemon clean!");
            self.download_daemon().await?;
        }

        let _command: std::process::Child = Command::new(&daemon_path)
            .arg(format!("-datadir={}", daemon_data_dir.to_str().unwrap()))
            .arg(format!("-conf={}", daemon_conf_path.to_str().unwrap()))
            .arg("-daemon")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Ghost daemon failed to start");
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        Ok(())
    }

    async fn download_daemon(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut conf = self.config.write().await;
        let gv_home = conf.gv_home.clone();

        file_ops::rm_dir(&gv_home.join("daemon/")).unwrap();
        file_ops::rm_dir(&PathBuf::from(TMP_PATH)).unwrap();
        let dl_path: PathBuf = loop {
            let dl_path_res = gv_methods::download_daemon().await;
            let dl_path: PathBuf = if let Err(ref err) = dl_path_res {
                error!("Error downloading daemon: {}", err);
                error!("Retrying in 30 seconds...");
                tokio::time::sleep(Duration::from_secs(30)).await;
                continue;
            } else {
                dl_path_res.unwrap()
            };

            break dl_path;
        };

        let path_and_hash: PathAndDigest = gv_methods::extract_archive(&dl_path, &gv_home)?;

        conf.update_gv_config("daemon_path", path_and_hash.daemon_path.to_str().unwrap())?;

        conf.update_gv_config("daemon_hash", path_and_hash.daemon_hash.as_str())?;

        drop(conf);

        Ok(())
    }

    async fn parse_error_msg(&self, err_msg: String) {
        if err_msg.contains("404 Not Found") {
            panic!("Method Not found.");
        } else if err_msg.contains("Connection refused") {
            let _ = self.start_daemon().await;
        }
    }

    pub fn convert_from_sat(&self, value: u64) -> f64 {
        // Converts an integer to a float with 8 digits
        let sat_readable: f64 = value as f64 / 10_f64.powi(8);
        sat_readable
    }

    pub fn convert_to_sat(&self, value: f64) -> u64 {
        // Converts a float to an integer
        let sat_readable: u64 = (value * 10_f64.powi(8)).round() as u64;
        sat_readable
    }

    pub fn precise(&self, input: f64) -> f64 {
        let zeros = 100000000.0;
        let precise = ((input * zeros).round()) / zeros;
        trace!("Precision set from {} to {}.", input, precise);
        return precise;
    }

    fn get_addr_from_vout(&self, vout: &Value) -> String {
        let script_pub_key = match vout.get("scriptPubKey") {
            Some(value) => value,
            None => {
                return "".to_string();
            }
        };

        let addresses = match script_pub_key.get("addresses") {
            Some(value) => value,
            None => {
                return "".to_string();
            }
        };

        let addresses_array = match addresses.as_array() {
            Some(value) => value,
            None => {
                return "".to_string();
            }
        };

        if let Some(first_address) = addresses_array.get(0) {
            if let Some(addr_str) = first_address.as_str() {
                return addr_str.to_string();
            }
        }
        "".to_string()
    }
}

pub async fn listen_zmq(
    listen_addr: &[String],
    cli_address: &str,
    db: Arc<GVDB>,
) -> Result<(), Box<dyn Error>> {
    info!("Starting ZMQ listener...");
    let listen_addr_str: Vec<&str> = listen_addr.iter().map(|s| s.as_str()).collect();
    let mut stream: bitcoincore_zmq::MessageStream = subscribe_async(&listen_addr_str).unwrap();

    while !db.get_server_ready().unwrap().ready {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    let cli_client: CLICaller = CLICaller::new(cli_address, true).await?;

    while let Some(msg) = stream.next().await {
        match &msg {
            Ok(Message::HashWTx(_, _, _)) => {
                let txid_and_wal: TxidAndWallet = match get_tx_hash_and_wallet(&msg) {
                    Ok(txid_wallet) => txid_wallet,
                    Err(_) => {
                        continue;
                    }
                };
                cli_client.call_new_wallet_tx(txid_and_wal).await.unwrap();
            }
            Ok(Message::HashBlock(_, _)) => {
                let block_hash: String = match gethash(&msg) {
                    Ok(hash) => hash,
                    Err(_) => continue,
                };

                cli_client.call_new_block(block_hash).await.unwrap();
            }
            Ok(_) => {
                error!("Got unexpected value from ZMQ.");
            }
            Err(e) => {
                error!("zmq error: {}", e);
            }
        }
    }

    Ok(())
}

fn get_tx_hash_and_wallet<E: Error + Sized>(
    msg: &Result<Message, E>,
) -> Result<TxidAndWallet, String> {
    match msg {
        Ok(msg) => match msg {
            HashWTx(txid, wallet, _) => {
                let res: TxidAndWallet = TxidAndWallet {
                    txid: txid.to_string(),
                    wallet: wallet.to_string(),
                };
                Ok(res)
            }
            _ => Err("Got unexpected value from ZMQ.".to_string()),
        },
        Err(e) => Err(format!("Error: {}", e)),
    }
}

fn gethash<E: Error + Sized>(msg: &Result<Message, E>) -> Result<String, String> {
    match msg {
        Ok(msg) => match msg {
            HashBlock(hash, _) => {
                return Ok(hash.to_string());
            }
            _ => Err("Got unexpected value from ZMQ.".to_string()),
        },
        Err(e) => Err(e.to_string()),
    }
}

async fn handle_event(payload: Payload, _: sio_Client, gv_config: Arc<async_RwLock<GVConfig>>) {
    let conf = gv_config.read().await;
    let cli_addr = conf.cli_address.clone();
    drop(conf);

    let cli_caller = CLICaller::new(&cli_addr, true).await.unwrap();

    match payload {
        Payload::Text(text) => {
            let data = match text.get(0) {
                Some(data) => data,
                None => &Value::Null,
            };

            if data.is_object() {
                let block_hash_opt = data.get("bestblockhash");

                if block_hash_opt.is_none() {
                    return;
                }

                let block_hash: String = block_hash_opt.unwrap().as_str().unwrap().to_string();
                let block_height_opt: Option<&Value> = data.get("blocks");

                if block_height_opt.is_none() {
                    return;
                }

                let block_height = block_height_opt.unwrap().as_u64().unwrap() as u32;

                let _ = cli_caller
                    .call_new_remote_block(block_hash, block_height)
                    .await;
            }
        }
        _ => {} // Do Nothing
    }
}

async fn connect_to_servers(
    urls: &VecDeque<&str>,
    gv_config: Arc<async_RwLock<GVConfig>>,
    is_error: Arc<async_Mutex<bool>>,
) -> Option<sio_Client> {
    for url in urls {
        let is_error_clone = Arc::clone(&is_error);
        let gv_config_clone = Arc::clone(&gv_config);
        match ClientBuilder::new(url.to_string())
            .on("room_message", move |payload, socket| {
                let gv_config_clone = Arc::clone(&gv_config_clone);
                async move {
                    handle_event(payload, socket, gv_config_clone).await;
                }
                .boxed()
            })
            .on("error", move |err, _| {
                // Use the cloned is_error inside this closure
                let is_error_clone = Arc::clone(&is_error_clone);
                async move {
                    let mut is_error = is_error_clone.lock().await;
                    *is_error = true;
                    drop(is_error);
                    error!("Error: {:#?}", err);
                }
                .boxed()
            })
            .connect()
            .await
        {
            Ok(client) => return Some(client),
            Err(err) => {
                error!("Failed to connect to {}: {:?}", url, err);
                continue;
            }
        }
    }

    None
}

pub async fn listen_for_events(
    gv_config: Arc<async_RwLock<GVConfig>>,
    db: Arc<GVDB>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut urls: Vec<&str> = vec![
        "https://api.tuxprint.com",
        "https://api2.tuxprint.com",
        "https://socket.tuxprint.com",
        "https://socket2.tuxprint.com",
    ];
    urls.shuffle(&mut rand::thread_rng());
    let mut url_vec = VecDeque::from_iter(urls);

    let is_error: Arc<async_Mutex<bool>> = Arc::new(async_Mutex::new(false));
    loop {
        // Move the first element to the back
        let first_el = url_vec.pop_front().unwrap();
        url_vec.push_back(first_el);

        let conf_clone: Arc<async_RwLock<GVConfig>> = Arc::clone(&gv_config);
        let conf = conf_clone.read().await;
        let cli_addr: String = conf.cli_address.clone();
        drop(conf);

        while !db.get_server_ready().unwrap().ready {
            info!("SIO Waiting for RPC server to be ready...");
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        info!("Server is ready...");

        let cli_caller: CLICaller = CLICaller::new(&cli_addr, true).await?;

        let is_error_clone: Arc<async_Mutex<bool>> = Arc::clone(&is_error);
        let gv_config_clone: Arc<async_RwLock<GVConfig>> = Arc::clone(&gv_config);

        let socket = match connect_to_servers(&url_vec, gv_config_clone, is_error_clone).await {
            Some(client) => client,
            None => {
                warn!("All servers are unreachable. Retrying...");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let id = Uuid::new_v4();

        // Join the new block room
        socket
            .emit("join", json!({"room": "block", "username": id.to_string()}))
            .await?;

        let remote_bc_info_res = get_remote_block_chain_info().await;

        if remote_bc_info_res.is_err() {
            warn!("Failed to get remote blockchain info. Retrying...");
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        let remote_bc_info = remote_bc_info_res.unwrap();

        let block_hash = remote_bc_info
            .get("bestblockhash")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let block_height = remote_bc_info.get("blocks").unwrap().as_u64().unwrap() as u32;

        let _ = cli_caller
            .call_new_remote_block(block_hash, block_height)
            .await;

        // Heartbeat mechanism
        loop {
            // Check if the connection is still alive

            let mut err_lock = is_error.lock().await;

            if *err_lock {
                warn!("Websocket error detected, reconnecting...");
                *err_lock = false;
                drop(err_lock);
                let _ = socket.disconnect().await;
                break;
            }

            drop(err_lock);

            if let Err(err) = socket.emit("client_message", "a message").await {
                warn!("Error checking heartbeat: {:?}", err);
                let _ = socket.disconnect().await;
                break;
            }
            // Wait for a short duration for a response
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}
