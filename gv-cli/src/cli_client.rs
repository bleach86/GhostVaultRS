use serde::ser::StdError;
use service::{
    config,
    config::GVConfig,
    constants::{DEFAULT_DAEMON_DIR, DEFAULT_GV_DIR, GV_PID_FILE, VERSION},
    file_ops,
    gv_client_methods::{CLICaller, GVStatus, StakingDataOverview},
};
use std::{
    env::{self},
    path::PathBuf,
    sync::Arc,
};

use std::process::exit;

#[derive(Debug, Clone)]
struct Flags {
    gv_data_dir: Option<String>,
    daemon_data_dir: Option<String>,
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let mut flags: Flags = Flags {
        gv_data_dir: None,
        daemon_data_dir: None,
    };

    let mut rpc_method: &str = "";
    let mut rpc_method_args: Vec<String> = Vec::new();
    let mut is_json: bool = false;
    let mut multi_param: Vec<String> = Vec::new();

    for arg in &args[1..] {
        if arg.starts_with("--") {
            let split: Vec<&str> = arg.strip_prefix("--").unwrap_or("").split("=").collect();

            match split.get(0) {
                Some(&"gv-data-dir") => {
                    flags.gv_data_dir = Some(split.get(1).unwrap_or(&"").to_string());
                }
                Some(&"daemon-data-dir") => {
                    flags.daemon_data_dir = Some(split.get(1).unwrap_or(&"").to_string());
                }
                Some(&"json") => {
                    is_json = true;
                }
                Some(&"help") => {
                    display_help();
                    return;
                }
                Some(&"version") | Some(&"v") => {
                    display_version();
                    return;
                }
                _ => continue,
            }
        } else {
            if rpc_method.is_empty() {
                rpc_method = arg;
            } else {
                if !multi_param.is_empty() && !arg.to_string().ends_with("\"") {
                    multi_param.push(arg.to_string());
                    continue;
                }

                if arg.to_string().starts_with("\"") && multi_param.is_empty() {
                    multi_param.push(arg.strip_prefix("\"").unwrap().to_string());
                    continue;
                }
                if arg.to_string().ends_with("\"") && !multi_param.is_empty() {
                    multi_param.push(arg.strip_suffix("\"").unwrap().to_string());
                    let final_multi: String = multi_param.join(" ");
                    rpc_method_args.push(final_multi);
                    multi_param = Vec::new();
                    continue;
                }
                rpc_method_args.push(arg.to_string())
            }
        }
    }

    match rpc_method {
        "help" | "-h" | "-help" | "" => {
            display_help();
            return;
        }
        "version" | "-v" | "-version" => {
            display_version();
            return;
        }
        _ => {}
    }

    let gv_data_dir: PathBuf = flags
        .gv_data_dir
        .map(|dir| file_ops::expand_user(&dir))
        .unwrap_or_else(|| file_ops::expand_user(DEFAULT_GV_DIR));

    let daemon_data_dir: PathBuf = flags
        .daemon_data_dir
        .map(|dir| file_ops::expand_user(&dir))
        .unwrap_or_else(|| file_ops::expand_user(DEFAULT_DAEMON_DIR));

    if !gv_data_dir.exists() {
        let msg = "GV Data dir not found, exiting";
        println!("{}", msg);
        exit(1);
    }

    if !daemon_data_dir.exists() {
        let msg = "Ghost daemon data dir not found, exiting";
        println!("{}", msg);
        exit(1);
    }

    let config: Arc<config::GVConfig> =
        Arc::new(GVConfig::new(&gv_data_dir, &daemon_data_dir).unwrap());

    let gv_client_res = CLICaller::new(&config.cli_address, is_json).await;

    if gv_client_res.is_err() {
        let pid_from_file: u32 = file_ops::get_pid(&gv_data_dir, GV_PID_FILE);
        if file_ops::pid_exists(pid_from_file) {
            let err_msg = gv_client_res.err().unwrap();
            println!(
                "GhostVault is running at PID {}, but the RPC server is not ready.\nError: {}",
                pid_from_file, err_msg
            );
        } else {
            let err_msg = gv_client_res.err().unwrap();
            println!("GhostVault server not running\nError: {}", err_msg);
        }
        exit(1);
    }

    let gv_client = gv_client_res.unwrap();

    match rpc_method {
        "getdaemonstate" | "status" => {
            let daemon_state = gv_client.call_get_daemon_state().await;

            if let Ok(daemon_state) = daemon_state {
                if is_json {
                    let status: GVStatus = serde_json::from_value(daemon_state.clone()).unwrap();
                    println!("{}", serde_json::to_string_pretty(&status).unwrap());
                }
            } else if let Err(err) = daemon_state {
                handle_command_error(err);
            }
        }
        "setrewardmode" => {
            let len_args = rpc_method_args.len();

            if len_args < 1 {
                println!("Method 'setrewardmode' missing required mode.");
                return;
            }

            let mode = rpc_method_args[0].to_uppercase();

            let addr = if mode != "DEFAULT" {
                if len_args < 2 {
                    println!("Method 'setrewardmode' missing required address.");
                    return;
                }
                Some(rpc_method_args[1].to_string())
            } else {
                None
            };

            let reward_mode_res = gv_client.call_set_reward_mode(mode, addr).await;

            if let Ok(reward_mode) = reward_mode_res {
                if is_json {
                    println!("{}", reward_mode.as_str().unwrap());
                }
            } else if let Err(err) = reward_mode_res {
                handle_command_error(err);
            }
        }
        "setminpayout" => {
            if rpc_method_args.len() < 1 {
                println!("Method 'setminpayout' missing required amount.");
                return;
            }

            let min_payout: f64 = rpc_method_args[0].parse::<f64>().unwrap();

            let min_payout_res = gv_client.call_set_payout_min(min_payout).await;

            if let Ok(min_payout) = min_payout_res {
                if is_json {
                    println!("{}", min_payout.as_str().unwrap());
                }
            } else if let Err(err) = min_payout_res {
                handle_command_error(err);
            }
        }
        "setrewardtime" => {
            if rpc_method_args.len() < 1 {
                println!("Method 'setpayoutime' missing required interval.");
                return;
            }

            let interval = rpc_method_args[0].to_string();

            let interval_res = gv_client.call_set_reward_interval(interval).await;

            if let Ok(interval) = interval_res {
                if is_json {
                    println!("{}", interval.as_str().unwrap());
                }
            } else if let Err(err) = interval_res {
                handle_command_error(err);
            }
        }
        "enablebot" => {
            if rpc_method_args.len() < 1 {
                println!("Method 'enabletelegrambot' missing required token.");
                return;
            } else if rpc_method_args.len() < 2 {
                println!("Method 'enabletelegrambot' missing required user.");
                return;
            }

            let token: String = rpc_method_args[0].to_string();
            let user: String = rpc_method_args[1].to_string();

            let enable_bot_res = gv_client.call_enable_telegram_bot(token, user).await;

            if let Ok(enable_bot) = enable_bot_res {
                if is_json {
                    println!("{}", enable_bot.as_str().unwrap());
                } else {
                    println!(
                        "Telegram bot enabled. Restart GhostVault for changes to take effect."
                    );
                }
            } else if let Err(err) = enable_bot_res {
                handle_command_error(err);
            }
        }
        "disablebot" => {
            let disable_bot_res = gv_client.call_disable_telegram_bot().await;

            if let Ok(disable_bot) = disable_bot_res {
                if is_json {
                    println!("{}", disable_bot.as_str().unwrap());
                } else {
                    println!(
                        "Telegram bot disabled. Restart GhostVault for changes to take effect."
                    );
                }
            } else if let Err(err) = disable_bot_res {
                handle_command_error(err);
            }
        }
        "setbotannounce" => {
            if rpc_method_args.len() < 1 {
                println!("Method 'setbotannounce' missing required message type.");
                return;
            } else if rpc_method_args.len() < 2 {
                println!("Method 'setbotannounce' missing required value.");
                return;
            }

            let msg_type: String = rpc_method_args[0].to_string();
            let new_val_opt = rpc_method_args[1].parse::<bool>();
            let new_val = match new_val_opt {
                Ok(val) => val,
                Err(_) => {
                    println!("Method 'setbotannounce' value must be a boolean.");
                    return;
                }
            };

            let set_bot_announce_res = gv_client.call_set_bot_announce(msg_type, new_val).await;

            if let Ok(set_bot_announce) = set_bot_announce_res {
                if is_json {
                    println!("{}", set_bot_announce.as_str().unwrap());
                }
            } else if let Err(err) = set_bot_announce_res {
                handle_command_error(err);
            }
        }
        "extpubkey" => {
            let ext_pub_key_res = gv_client.call_get_ext_pub_key().await;

            if let Ok(ext_pub_key) = ext_pub_key_res {
                if is_json {
                    println!("{}", ext_pub_key.as_str().unwrap());
                }
            } else if let Err(err) = ext_pub_key_res {
                handle_command_error(err);
            }
        }
        "shutdown" => {
            let shutdown_res = gv_client.call_shutdown().await;

            if let Ok(shutdown) = shutdown_res {
                if is_json {
                    println!("{}", shutdown.as_str().unwrap());
                }
            } else if let Err(err) = shutdown_res {
                handle_command_error(err);
            }
        }
        "forceresync" => {
            let force_resync_res = gv_client.call_force_resync().await;

            if let Ok(force_resync) = force_resync_res {
                if is_json {
                    println!("{}", force_resync);
                }
            } else if let Err(err) = force_resync_res {
                handle_command_error(err);
            }
        }
        "getoverview" | "stats" => {
            let overview_res = gv_client.call_get_overview().await;

            if let Ok(overview) = overview_res {
                if is_json {
                    let staking_data: StakingDataOverview =
                        serde_json::from_value(overview.clone()).unwrap();
                    println!("{}", serde_json::to_string_pretty(&staking_data).unwrap());
                }
            } else if let Err(err) = overview_res {
                handle_command_error(err);
            }
        }
        "getmnemonic" => {
            let mnemonic_res = gv_client.call_get_mnemonic().await;

            if let Ok(mnemonic) = mnemonic_res {
                if is_json {
                    if mnemonic.is_string() {
                        println!("{}", mnemonic.as_str().unwrap());
                    } else {
                        println!("{}", mnemonic);
                    }
                }
            } else if let Err(err) = mnemonic_res {
                handle_command_error(err);
            }
        }
        "settimezone" => {
            if rpc_method_args.len() < 1 {
                println!("Method 'settimezone' missing required timezone.");
                return;
            }

            let timezone: String = rpc_method_args[0].to_string();

            let set_timezone_res = gv_client.call_set_timezone(timezone).await;

            if let Ok(set_timezone) = set_timezone_res {
                if is_json {
                    println!("{}", set_timezone.as_str().unwrap());
                }
            } else if let Err(err) = set_timezone_res {
                handle_command_error(err);
            }
        }
        "importwallet" => {
            if rpc_method_args.len() < 1 {
                println!("Method 'importwallet' missing required mnemonic.");
                return;
            }
            if rpc_method_args.len() < 2 {
                println!("Method 'importwallet' missing required wallet name.");
                return;
            }

            let mnemonic: String = rpc_method_args[0].to_string();
            let wallet_name: String = rpc_method_args[1].to_string();

            if !is_json {
                println!("Importing wallet\nThis may take a long time, please be patient.");
            }

            let import_wallet_res = gv_client.call_import_wallet(mnemonic, wallet_name).await;

            if let Ok(import_wallet) = import_wallet_res {
                if is_json {
                    println!("{}", import_wallet.as_str().unwrap());
                }
            } else if let Err(err) = import_wallet_res {
                handle_command_error(err);
            }
        }
        "version" => display_version(),
        "" | "help" => display_help(),
        _ => println!("Method '{}' not found.", rpc_method),
    }
}

fn handle_command_error(err: Box<dyn StdError>) {
    println!("Error: {}", err.to_string());
    if err.to_string().contains("Connection refused") {
        println!("Ensure that the GhostVault server is runing and try again.")
    }
}

fn display_version() {
    println!("GhostVault CLI v{}", VERSION);
}

fn display_help() {
    display_version();
    println!("Usage: gv-cli [OPTIONS] [METHOD] [ARGS]");
    println!("\nOptions:");
    println!("  --gv-data-dir=GV_DATA_DIR    Set the GhostVault data directory");
    println!("  --daemon-data-dir=DAEMON_DATA_DIR    Set the Ghost daemon data directory");
    println!("  --json    Output in JSON format");
    println!("\nMethods:");
    println!("  status    Get the current state of GhostVault");
    println!("  setrewardmode MODE [ADDRESS]    Set the reward mode");
    println!("  setminpayout AMOUNT    Set the minimum payout amount");
    println!("  setrewardtime INTERVAL    Set how often payouts are processed, in seconds");
    println!("  enablebot TOKEN USER    Enable the Telegram bot (Restart required)");
    println!("  disablebot    Disable the Telegram bot (Restart required)");
    println!("  setbotannounce TYPE VALUE    Set the bot announcement value");
    println!("  extpubkey    Get the extended public key for zapping");
    println!("  shutdown    Shutdown the GhostVault server");
    println!("  forceresync    Force a resync of ghostd");
    println!("  stats    Get the staking overview");
    println!("  getmnemonic    Get the wallet mnemonic");
    println!("  settimezone TIMEZONE    Set the timezone");
    println!("  importwallet MNEMONIC WALLET_NAME    Import a wallet");
    println!("  version    Display the GhostVault CLI version");
    println!("\nExamples:");
    println!("  gv-cli setrewardmode DEFAULT");
    println!("  gv-cli setrewardmode ANON \"ANON_REWARD_ADDRESS\"");
    println!("  gv-cli setminpayout 25.5");
    println!("  gv-cli setrewardtime 900");
    println!("  gv-cli enablebot \"TELOXIDE_TOKEN\" \"TELEGRAM_USER\"");
    println!("  gv-cli disablebot");
    println!("  gv-cli setbotannounce rewards true");
    println!("  gv-cli extpubkey");
    println!("  gv-cli shutdown");
    println!("  gv-cli forceresync");
    println!("  gv-cli stats");
    println!("  gv-cli status");
    println!("  gv-cli getmnemonic");
    println!("  gv-cli importwallet \"words between quotes\" WALLET_NAME");
    println!("  gv-cli settimezone \"America/New_York\"");
}
