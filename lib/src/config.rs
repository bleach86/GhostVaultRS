use crate::{
    constants::{
        DAEMON_SETTINGS_FILE, DEFAULT_HOT_WALLET, DEFAULT_PROCESS_REWARDS, GV_SETTINGS_FILE,
    },
    daemon_helper::DaemonHelper,
    file_ops,
};
use log::info;
use serde_json::Value as json_Value;
use std::{error::Error, path::PathBuf};
use toml::Value as toml_Value;

#[derive(Debug, Clone)]
pub struct GVConfig {
    pub bot_token: Option<String>,
    pub tg_user: Option<String>,
    pub ext_pub_key: Option<String>,
    pub ext_pub_key_label: Option<String>,
    pub reward_address: Option<String>,
    pub anon_mode: bool,
    pub anon_reward_address: Option<String>,
    pub internal_anon: Option<String>,
    pub rpc_host: String,
    pub rpc_port: u16,
    pub rpc_wallet: String,
    pub rpc_wallet_hot: String,
    pub rpc_user: String,
    pub rpc_pass: String,
    pub cli_address: String,
    pub gv_home: PathBuf,
    pub config_file: PathBuf,
    pub daemon_data_dir: PathBuf,
    pub daemon_path: PathBuf,
    pub daemon_hash: Option<String>,
    pub min_reward_payout: u64,
    pub mnemonic: Option<String>,
    pub reward_interval: u64,
    pub zmq_block_host: String,
    pub zmq_tx_host: String,
    pub announce_stakes: bool,
    pub announce_zaps: bool,
    pub announce_rewards: bool,
    pub timezone: String,
}

trait EmptyAsNone {
    fn empty_as_none(self) -> Option<String>;
}

impl EmptyAsNone for toml::Value {
    fn empty_as_none(self) -> Option<String> {
        match self {
            toml_Value::String(s) if s.is_empty() => None,
            toml_Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl EmptyAsNone for String {
    fn empty_as_none(self) -> Option<String> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

impl<'a> EmptyAsNone for &'a str {
    fn empty_as_none(self) -> Option<String> {
        if self.is_empty() {
            None
        } else {
            Some(self.to_string())
        }
    }
}

pub trait IntIntoBool {
    fn to_bool(&self) -> Option<bool>;
}

impl IntIntoBool for u64 {
    fn to_bool(&self) -> Option<bool> {
        Some(*self >= 1)
    }
}

impl IntIntoBool for i64 {
    fn to_bool(&self) -> Option<bool> {
        Some(*self >= 1)
    }
}

impl GVConfig {
    pub fn new(gv_home: &PathBuf, daemon_data_dir: &PathBuf) -> Result<Self, Box<dyn Error>> {
        log::info!("Reading Configuration...");
        let toml_file_path = gv_home.join(PathBuf::from(GV_SETTINGS_FILE));
        let toml_content = std::fs::read_to_string(&toml_file_path)?;

        let gv_conf: toml_Value = toml::from_str(&toml_content)?;

        let daemon_conf: json_Value =
            file_ops::ghost_config_to_value(&daemon_data_dir.join(DAEMON_SETTINGS_FILE))?;

        let bot_token: Option<String> = gv_conf
            .get("TELOXIDE_TOKEN")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();
        let tg_user: Option<String> = gv_conf
            .get("TELEGRAM_USER")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();

        let rpc_host: String = daemon_conf
            .get("rpcbind")
            .unwrap_or(&serde_json::Value::String("127.0.0.1".to_string()))
            .as_str()
            .unwrap_or("127.0.0.1")
            .to_string();

        let rpc_port: u16 = daemon_conf
            .get("rpcport")
            .unwrap_or(&serde_json::Value::Number(51725.into()))
            .as_u64()
            .unwrap_or(51725) as u16;
        let rpc_wallet: String = gv_conf
            .get("RPC_WALLET")
            .unwrap_or(&toml_Value::String(String::new()))
            .as_str()
            .unwrap_or("")
            .to_string();

        let rpc_user: String = daemon_conf
            .get("rpcuser")
            .unwrap_or(&serde_json::Value::String("user".to_string()))
            .as_str()
            .unwrap_or("user")
            .to_string();
        let rpc_pass: String = daemon_conf
            .get("rpcpassword")
            .unwrap_or(&serde_json::Value::String("password".to_string()))
            .as_str()
            .unwrap_or("password")
            .to_string();
        let cli_address: String = gv_conf
            .get("CLI_ADDRESS")
            .unwrap_or(&toml_Value::String("127.0.0.1:50051".to_string()))
            .as_str()
            .unwrap_or("127.0.0.1:50051")
            .to_string();

        let config_file: PathBuf = toml_file_path;

        let ext_pub_key: Option<String> = gv_conf
            .get("EXT_PUB_KEY")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();
        let ext_pub_key_label: Option<String> = gv_conf
            .get("EXT_PUB_KEY_LABEL")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();
        let reward_address: Option<String> = gv_conf
            .get("REWARD_ADDRESS")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();
        let anon_mode: bool = gv_conf
            .get("ANON_MODE")
            .unwrap_or(&toml_Value::Boolean(false))
            .as_bool()
            .unwrap_or(false);
        let anon_reward_address: Option<String> = gv_conf
            .get("ANON_REWARD_ADDRESS")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();

        let internal_anon: Option<String> = gv_conf
            .get("INTERNAL_ANON")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();

        let daemon_path: PathBuf = PathBuf::from(gv_conf["DAEMON_PATH"].as_str().unwrap_or(""));
        let daemon_hash: Option<String> = gv_conf
            .get("DAEMON_HASH")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();

        let gv_home: PathBuf = gv_home.to_owned();
        let daemon_data_dir: PathBuf = daemon_data_dir.to_owned();

        let rpc_wallet_hot: String = DEFAULT_HOT_WALLET.to_string();

        let min_reward_payout: u64 = gv_conf
            .get("MIN_REWARD_PAYOUT")
            .unwrap_or(&toml_Value::Integer(10000000))
            .as_integer()
            .unwrap_or(10000000) as u64;

        let zmq_block_host: String = daemon_conf
            .get("zmqpubhashblock")
            .unwrap_or(&serde_json::Value::String(
                "tcp://127.0.0.1:28332".to_string(),
            ))
            .as_str()
            .unwrap_or("tcp://127.0.0.1:28332")
            .to_string();

        let zmq_tx_host: String = daemon_conf
            .get("zmqpubhashwtx")
            .unwrap_or(&serde_json::Value::String(
                "tcp://127.0.0.1:28332".to_string(),
            ))
            .as_str()
            .unwrap_or("tcp://127.0.0.1:28332")
            .to_string();

        let reward_interval: u64 = gv_conf
            .get("REWARD_INTERVAL")
            .unwrap_or(&toml_Value::Integer(DEFAULT_PROCESS_REWARDS))
            .as_integer()
            .unwrap_or(DEFAULT_PROCESS_REWARDS) as u64;

        let announce_stakes: bool = gv_conf
            .get("ANNOUNCE_STAKES")
            .unwrap_or(&toml_Value::Boolean(true))
            .as_bool()
            .unwrap_or(true);
        let announce_zaps: bool = gv_conf
            .get("ANNOUNCE_ZAPS")
            .unwrap_or(&toml_Value::Boolean(true))
            .as_bool()
            .unwrap_or(true);
        let announce_rewards: bool = gv_conf
            .get("ANNOUNCE_REWARDS")
            .unwrap_or(&toml_Value::Boolean(true))
            .as_bool()
            .unwrap_or(true);
        let timezone = gv_conf
            .get("TIMEZONE")
            .unwrap_or(&toml_Value::String("UTC".to_string()))
            .as_str()
            .unwrap_or("UTC")
            .to_string();
        let mnemonic: Option<String> = gv_conf
            .get("MNEMONIC")
            .unwrap_or(&toml_Value::String(String::new()))
            .clone()
            .empty_as_none();

        let config = GVConfig {
            bot_token,
            tg_user,
            ext_pub_key,
            ext_pub_key_label,
            reward_address,
            anon_mode,
            anon_reward_address,
            internal_anon,
            rpc_host,
            rpc_port,
            rpc_wallet,
            rpc_wallet_hot,
            rpc_user,
            rpc_pass,
            cli_address,
            gv_home,
            config_file,
            daemon_data_dir,
            daemon_path,
            daemon_hash,
            min_reward_payout,
            mnemonic,
            reward_interval,
            zmq_block_host,
            zmq_tx_host,
            announce_stakes,
            announce_zaps,
            announce_rewards,
            timezone,
        };

        Ok(config)
    }

    pub async fn validate_daemon_conf(&self, daemon: &DaemonHelper) -> Result<(), Box<dyn Error>> {
        let daemon_conf_file: PathBuf = self.daemon_data_dir.join(DAEMON_SETTINGS_FILE);
        let daemon_conf: json_Value = file_ops::ghost_config_to_value(&daemon_conf_file)?;

        let required_keys: &[(&str, &str)] = &[
            ("rpcuser", "user"),
            ("rpcpassword", "password"),
            ("rpcbind", "127.0.0.1"),
            ("rpcallowip", "127.0.0.1"),
            ("rpcport", "51725"),
            ("zmqpubhashblock", "tcp://127.0.0.1:28332"),
            ("zmqpubhashwtx", "tcp://127.0.0.1:28332"),
        ];

        let mut missing_keys: Vec<(&str, &str)> = Vec::new();

        for (index, &(key, _)) in required_keys.iter().enumerate() {
            if daemon_conf.get(key).is_none() {
                missing_keys.push(required_keys[index])
            }
        }

        if !missing_keys.is_empty() {
            info!("Invalid ghostd config found! Attempting to fix...");

            daemon.stop_daemon().await.unwrap();

            for (key, value) in missing_keys {
                file_ops::update_ghost_config(&daemon_conf_file, key, Some(value))?;
            }
        }

        Ok(())
    }

    pub fn update_gv_config(
        &mut self,
        field_name: &str,
        new_value: &str,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        // Update the corresponding field in the struct
        match field_name.to_lowercase().as_str() {
            "teloxide_token" => self.bot_token = new_value.empty_as_none(),
            "telegram_user" => self.tg_user = new_value.empty_as_none(),
            "rpc_wallet" => self.rpc_wallet = new_value.to_string(),
            "cli_address" => self.cli_address = new_value.to_string(),
            "ext_pub_key" => self.ext_pub_key = new_value.empty_as_none(),
            "ext_pub_key_label" => self.ext_pub_key_label = new_value.empty_as_none(),
            "reward_address" => self.reward_address = new_value.empty_as_none(),
            "internal_anon" => self.internal_anon = new_value.empty_as_none(),
            "reward_interval" => {
                self.reward_interval = new_value
                    .parse::<u64>()
                    .map_err(|_| "Invalid value for reward_interval")?
            }
            "min_reward_payout" => {
                self.min_reward_payout = new_value
                    .parse::<u64>()
                    .map_err(|_| "Invalid value for min_payout")?
            }
            "mnemonic" => self.mnemonic = new_value.empty_as_none(),
            "anon_mode" => {
                self.anon_mode = if new_value.to_lowercase().contains("true") {
                    true
                } else {
                    false
                }
            }
            "anon_reward_address" => self.anon_reward_address = new_value.empty_as_none(),
            "daemon_path" => self.daemon_path = PathBuf::from(new_value),
            "daemon_hash" => self.daemon_hash = new_value.empty_as_none(),
            "announce_stakes" => {
                self.announce_stakes = if new_value.to_lowercase().contains("true") {
                    true
                } else {
                    false
                }
            }
            "announce_zaps" => {
                self.announce_zaps = if new_value.to_lowercase().contains("true") {
                    true
                } else {
                    false
                }
            }
            "announce_rewards" => {
                self.announce_rewards = if new_value.to_lowercase().contains("true") {
                    true
                } else {
                    false
                }
            }
            "timezone" => self.timezone = new_value.to_string(),
            _ => {
                return Err(format!("Invalid field name: {}", field_name).into());
            }
        }

        // Update the corresponding field in the TOML file
        let toml_content = std::fs::read_to_string(&self.config_file)?;
        let mut toml_value: toml_Value = toml::from_str(&toml_content)?;

        let field_value = match field_name.to_lowercase().as_str() {
            "anon_mode" | "announce_stakes" | "announce_zaps" | "announce_rewards" => {
                toml::Value::Boolean(new_value.to_lowercase() == "true")
            }
            "min_reward_payout" | "reward_interval" => {
                toml::Value::Integer(new_value.parse::<i64>()?)
            }
            _ => toml::Value::String(new_value.to_string()),
        };

        if let Some(toml_field) = toml_value.get_mut(field_name.to_uppercase()) {
            *toml_field = field_value;
        } else {
            toml_value
                .as_table_mut()
                .unwrap()
                .insert(field_name.to_uppercase(), field_value);
        }

        let updated_toml_content = toml::to_string_pretty(&toml_value)?;

        std::fs::write(&self.config_file, updated_toml_content)?;

        Ok(self.clone())
    }
}
