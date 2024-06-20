#![allow(non_snake_case)]
#[macro_use]
extern crate log;
use clap::Parser;
use daemonize::Daemonize;
use log::LevelFilter;
use log4rs::{
    append::console::ConsoleAppender,
    append::rolling_file::policy::compound::{
        roll::fixed_window::FixedWindowRoller, trigger::size::SizeTrigger, CompoundPolicy,
    },
    append::rolling_file::RollingFileAppender,
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
};
use serde_json::Value;
use service::{
    config,
    config::GVConfig,
    constants::{DEFAULT_DAEMON_DIR, DEFAULT_GV_DIR, GV_PID_FILE},
    daemon_helper::DaemonHelper,
    file_ops, gv_home_init, gv_methods,
    gv_methods::PathAndDigest,
    gvdb::{ServerReadyDB, GVDB},
    term_link::Link,
    tg_bot::tg_bot,
};
use std::{env, path::PathBuf, process::exit, sync::Arc};
use systemstat::Duration;
use tokio::runtime::Runtime;
use tokio::sync::RwLock as async_RwLock;

mod cli_server;

#[derive(Parser, Debug)]
struct Flags {
    /// Sets GhostVault's data directory.
    #[clap(long)]
    gv_data_dir: Option<String>,
    /// Sets the Ghost daemon data directory.
    #[clap(long)]
    daemon_data_dir: Option<String>,
    /// Run GhostVault in the console without daemonizing.
    #[clap(short, long)]
    console: bool,
}

fn main() {
    let link = Link::new("https://ghostprivacy.net");
    println!("{}", link);

    let flags: Flags = Flags::parse();
    let daemon_data_dir: PathBuf = flags
        .daemon_data_dir
        .map(|dir| file_ops::expand_user(&dir))
        .unwrap_or_else(|| file_ops::expand_user(DEFAULT_DAEMON_DIR));
    env::set_var("GV_GHOST_HOME", daemon_data_dir.to_str().unwrap());

    let gv_data_dir: PathBuf = flags
        .gv_data_dir
        .map(|dir| file_ops::expand_user(&dir))
        .unwrap_or_else(|| file_ops::expand_user(DEFAULT_GV_DIR));

    let first_run: bool = if !gv_data_dir.exists() {
        info!("GV Data dir not found, creating...");
        gv_home_init::init_data_dir(&gv_data_dir).unwrap();
        true
    } else {
        false
    };

    let log_file_path: PathBuf = gv_data_dir.join("logs/ghostvault.log");

    let roller: FixedWindowRoller = FixedWindowRoller::builder()
        .build(&log_file_path.with_extension("{}.gz").to_str().unwrap(), 3)
        .expect("Failed to build roller");

    let policy: CompoundPolicy = CompoundPolicy::new(
        Box::new(SizeTrigger::new(1024 * 1024 * 10)), // 10 MB
        Box::new(roller),
    );

    let file_appender: RollingFileAppender = RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::default()))
        .build(log_file_path, Box::new(policy))
        .expect("Failed to create file appender");

    let console_appender: ConsoleAppender = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::default()))
        .build();

    let log_config: Config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .appender(Appender::builder().build("console", Box::new(console_appender)))
        .build(
            Root::builder()
                .appender("file")
                .appender("console")
                .build(LevelFilter::Info),
        )
        .expect("Failed to create log4rs configuration");

    log4rs::init_config(log_config).expect("Failed to initialize log4rs");

    let do_daemon: bool = flags.console.clone() == false;
    let is_windows: bool = cfg!(target_os = "windows");

    let is_docker = env::vars().any(|(key, _)| key == "DOCKER_RUNNING");

    // Prevent running duplicate instances of GhostVault
    let pid_file: PathBuf = gv_data_dir.join(GV_PID_FILE);

    let pid_from_file: u32 = file_ops::get_pid(&gv_data_dir, GV_PID_FILE);
    if file_ops::pid_exists(pid_from_file) && !is_docker {
        let running_msg: String = format!(
            "Detected running GhostVault instance at PID: {}",
            pid_from_file
        );

        info!("{}", running_msg);
        info!("Exiting!");

        exit(0);
    }
    file_ops::make_pid_file(&gv_data_dir, GV_PID_FILE).unwrap();

    env::set_var("GV_HOME", gv_data_dir.to_str().unwrap());

    let ghost_conf_path: PathBuf = daemon_data_dir.join("ghost.conf");
    if !ghost_conf_path.exists() {
        gv_home_init::create_default_daemon_config(&daemon_data_dir).unwrap();
    }

    if do_daemon && !is_windows {
        let daemonize = Daemonize::new().pid_file(pid_file).chown_pid_file(true);

        match daemonize.start() {
            Ok(_) => {
                let rt: Runtime = Runtime::new().unwrap();

                // Run the server in the background
                rt.block_on(async {
                    run_init(&gv_data_dir, &daemon_data_dir, first_run).await;
                });
            }
            Err(e) => error!("Error, {}", e),
        }
    } else {
        let rt: Runtime = Runtime::new().unwrap();

        // Run the server
        rt.block_on(async {
            run_init(&gv_data_dir, &daemon_data_dir, first_run).await;
        });
    }
}

async fn run_init(gv_home: &PathBuf, daemon_data_dir: &PathBuf, first_run: bool) {
    let config: Arc<async_RwLock<GVConfig>> = startup(&gv_home, &daemon_data_dir, first_run)
        .await
        .expect("Failed to start up");

    let mut conf = config.write().await;
    let config_clone_tg_bot = Arc::clone(&config);
    let config_clone_rpc = Arc::clone(&config);

    let mut bot_token: Option<String> = conf.bot_token.clone();
    let mut tg_user: Option<String> = conf.tg_user.clone();

    for (key, value) in env::vars() {
        match key.as_str() {
            "TELOXIDE_TOKEN" => {
                bot_token = Some(value.clone());
                conf.update_gv_config("TELOXIDE_TOKEN", &value).unwrap();
            }
            "GV_TG_USER" => {
                tg_user = Some(value.clone());
                conf.update_gv_config("TELEGRAM_USER", &value).unwrap();
            }
            _ => {}
        }
    }

    drop(conf);

    let db: Arc<GVDB> = Arc::new(GVDB::new(&gv_home).await);
    let bot_db = Arc::clone(&db);

    let ready: ServerReadyDB = ServerReadyDB {
        ready: false,
        daemon_ready: false,
        reason: None,
    };

    db.set_server_ready(&ready).await.unwrap();

    // Start the rpc server for the CLI
    let start_rpc = tokio::spawn(async move {
        start_rpc_server(&config_clone_rpc, &db).await;
    });

    // start the telegram bot if the credentials are present.
    if !bot_token.is_none() && !tg_user.is_none() {
        let start_bot = tokio::spawn(async move {
            tg_bot::run_tg_bot(config_clone_tg_bot, bot_db).await;
        });

        start_bot.await.expect("Failed to await background task");
    } else {
        start_rpc.await.expect("Failed to await background task");
    }
}

async fn startup(
    gv_home: &PathBuf,
    daemon_data_dir: &PathBuf,
    first_run: bool,
) -> std::io::Result<Arc<async_RwLock<GVConfig>>> {
    let config_data: config::GVConfig = GVConfig::new(&gv_home, &daemon_data_dir).unwrap();

    let config: Arc<async_RwLock<GVConfig>> = Arc::new(async_RwLock::new(config_data));

    let mut conf_lock = config.write().await;

    if !conf_lock.daemon_path.exists() {
        info!("Ghost daemon not found, fetching...");

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

        let path_and_hash: PathAndDigest = gv_methods::extract_archive(&dl_path, &gv_home).unwrap();

        conf_lock
            .update_gv_config("daemon_path", path_and_hash.daemon_path.to_str().unwrap())
            .unwrap();

        conf_lock
            .update_gv_config("daemon_hash", path_and_hash.daemon_hash.as_str())
            .unwrap();
    }
    drop(conf_lock);

    let daemon: DaemonHelper = DaemonHelper::new(&config, "cold").await;

    let conf_lock = config.read().await;
    let check_daemon_config = conf_lock.validate_daemon_conf(&daemon).await;
    drop(conf_lock);

    if check_daemon_config.is_err() {
        error!("Invalid ghostd configuration!");
        exit(1);
    }

    if first_run {
        daemon.stop_daemon().await.expect("Failed to stop daemon");
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    daemon.wait_for_daemon_startup().await;

    let db: GVDB = GVDB::new(&gv_home).await;
    let check_wallets: Result<Value, Box<dyn std::error::Error + Send + Sync>> =
        daemon.check_wallets(&db).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    if check_wallets.is_err() {
        panic!("Failed to load wallet");
    }

    Ok(config)
}

async fn start_rpc_server(gv_config: &Arc<async_RwLock<GVConfig>>, db: &Arc<GVDB>) {
    info!("Starting CLI server...");

    // Run the server in the background
    if let Err(err) = cli_server::run_server(gv_config, db).await {
        error!("Error running server: {:?}", err);
    }
}
