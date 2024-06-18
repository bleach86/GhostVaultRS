use crate::file_ops;
use home::home_dir;
use log::info;

use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

pub fn init_data_dir(gv_data_dir: &PathBuf) -> std::io::Result<()> {
    let daemon_dir: PathBuf = PathBuf::from("daemon/");

    file_ops::create_dir(gv_data_dir)?;
    file_ops::create_dir(&gv_data_dir.join(daemon_dir))?;
    create_settings(&gv_data_dir.join("gv_settings.toml"))?;

    Ok(())
}

fn create_settings(path: &PathBuf) -> std::io::Result<()> {
    let home_dir: PathBuf = home_dir().unwrap();
    let legacy_gv_path: PathBuf = home_dir.join("GhostVault/");
    let legacy_config_path: PathBuf = legacy_gv_path.join("daemon.json");

    let env_contents: String = if legacy_config_path.exists() {
        info!("Legacy GhostVault install detected, importing settings...");
        let legacy_conf: Value = file_ops::read_json(&legacy_config_path)?;
        let wallet: String = format!(
            "RPC_WALLET = \"{}\"",
            &legacy_conf
                .get("walletName")
                .unwrap_or(&Value::String(String::new()))
                .as_str()
                .unwrap_or("")
                .to_string()
        );
        let ext_pk: String = format!(
            "EXT_PUB_KEY = \"{}\"",
            &legacy_conf
                .get("extPubKey")
                .unwrap_or(&Value::String(String::new()))
                .as_str()
                .unwrap_or("")
                .to_string()
        );
        let ext_pk_label: String = format!(
            "EXT_PUB_KEY_LABEL = \"{}\"",
            &legacy_conf
                .get("extPubKeyLabel")
                .unwrap_or(&Value::String(String::new()))
                .as_str()
                .unwrap_or("")
                .to_string()
        );
        let reward_addr: String = format!(
            "REWARD_ADDRESS = \"{}\"",
            &legacy_conf
                .get("rewardAddress")
                .unwrap_or(&Value::String(String::new()))
                .as_str()
                .unwrap_or("")
                .to_string()
        );
        let anon_mode: String = format!(
            "ANON_MODE = {}",
            &legacy_conf
                .get("anonMode")
                .unwrap_or(&Value::Bool(false))
                .as_bool()
                .unwrap_or(false)
                .to_string()
        );
        let anon_mode_reward: String = format!(
            "ANON_REWARD_ADDRESS = \"{}\"",
            &legacy_conf
                .get("anonRewardAddress")
                .unwrap_or(&Value::String(String::new()))
                .as_str()
                .unwrap_or("")
                .to_string()
        );

        let internal_anon = format!(
            "INTERNAL_ANON = \"{}\"",
            &legacy_conf
                .get("internalAnon")
                .unwrap_or(&Value::String(String::new()))
                .as_str()
                .unwrap_or("")
                .to_string()
        );
        info!("Disabling legacy cron");
        disable_legacy_cron()?;

        format!(
            "{}\nANNOUNCE_ZAPS = true\nANNOUNCE_STAKES = true\nTIMEZONE = \"UTC\"\nANNOUNCE_REWARDS = true\nCLI_ADDRESS = \"127.0.0.1:50051\"\n{}\n{}\n{}\n{}\n{}\nTELOXIDE_TOKEN = \"\"\nTELEGRAM_USER = \"\"\nDAEMON_PATH = \"\"\nDAEMON_HASH = \"\"\nMIN_REWARD_PAYOUT = 10000000\nMNEMONIC = \"\"\nREWARD_INTERVAL = 900\n{}\n",
            wallet, ext_pk, ext_pk_label, reward_addr, anon_mode, anon_mode_reward, internal_anon
        )
    } else {
        info!("Legacy GhostVault install not found...");
        concat!(
            "RPC_WALLET = \"\"\n",
            "CLI_ADDRESS = \"127.0.0.1:50051\"\n",
            "EXT_PUB_KEY = \"\"\n",
            "EXT_PUB_KEY_LABEL = \"\"\n",
            "REWARD_ADDRESS = \"\"\n",
            "ANON_MODE = false\n",
            "ANON_REWARD_ADDRESS = \"\"\n",
            "TELOXIDE_TOKEN = \"\"\n",
            "TELEGRAM_USER = \"\"\n",
            "DAEMON_PATH = \"\"\n",
            "DAEMON_HASH = \"\"\n",
            "INTERNAL_ANON = \"\"\n",
            "MIN_REWARD_PAYOUT = 10000000\n",
            "MNEMONIC = \"\"\n",
            "REWARD_INTERVAL = 900\n",
            "ANNOUNCE_REWARDS = true\n",
            "ANNOUNCE_STAKES = true\n",
            "ANNOUNCE_ZAPS = true\n",
            "TIMEZONE = \"UTC\"\n",
        )
        .to_string()
    };

    let mut env_file = File::create(path)?;
    env_file.write_all(env_contents.as_bytes())?;

    Ok(())
}

pub fn create_default_daemon_config(path: &PathBuf) -> std::io::Result<()> {
    let ghost_conf_path: PathBuf = path.join("ghost.conf");

    let daemon_conf = concat!(
        "addressindex=1\n",
        "server=1\n",
        "rpcuser=user\n",
        "rpcpassword=password\n",
        "rpcbind=127.0.0.1\n",
        "rpcallowip=127.0.0.1\n",
        "rpcport=51725\n",
        "rpcservertimeout=120\n",
        "rpcthreads=64\n",
        "zmqpubhashblock=tcp://127.0.0.1:28332\n",
        "zmqpubhashwtx=tcp://127.0.0.1:28332\n",
    );

    if !path.exists() {
        file_ops::create_dir(path)?
    }
    let mut env_file = File::create(&ghost_conf_path)?;
    env_file.write_all(daemon_conf.as_bytes())?;

    Ok(())
}

fn disable_legacy_cron() -> std::io::Result<()> {
    let is_crontab_available: bool = file_ops::is_crontab_installed();

    if is_crontab_available {
        let current_cron: String = file_ops::read_crontab();
        let sanitized_cron: String = file_ops::remove_legacy_cron_entry(&current_cron);
        file_ops::write_crontab(&sanitized_cron)?;
        info!("Successfully removed legacy cron entries...");
    } else {
        info!("crontab binary not available, skipping...");
    }

    Ok(())
}
