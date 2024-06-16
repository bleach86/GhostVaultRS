use crate::{
    config::GVConfig,
    constants::{DEFAULT_DEAMON_UPDATE, DEFAULT_MIN_PAYOUT, DEFAULT_SELF_UPDATE},
    gv_client_methods::CLICaller,
    gvdb::{ServerReadyDB, Task, GVDB},
};
use log::info;
use std::sync::Arc;
use tokio::sync::RwLock as async_RwLock;

pub async fn task_runner(db: &Arc<GVDB>, gv_config: &Arc<async_RwLock<GVConfig>>) {
    info!("Starting the task service...");
    let tasks_to_complete: Vec<&str> = vec!["daemon_update", "self_update", "process_rewards"];
    let current_time: i64 = get_current_time();
    let cloned_tasks: Vec<&str> = tasks_to_complete.clone();
    let runner_tasks: Vec<&str> = tasks_to_complete.clone();
    let conf = gv_config.read().await;

    for task in tasks_to_complete {
        let is_scheduled: Option<Task> = db.get_task(task.as_bytes());

        if is_scheduled.is_none() {
            let id: u8 = get_index(&cloned_tasks, task).unwrap() as u8;
            let run_interval: i64 = match task {
                "daemon_update" => DEFAULT_DEAMON_UPDATE,
                "self_update" => DEFAULT_SELF_UPDATE,
                "process_rewards" => conf.reward_interval,

                _ => continue,
            } as i64;
            let next_run: i64 = current_time;

            let min_payout: Option<u64> = if task == "process_rewards" {
                Some(DEFAULT_MIN_PAYOUT)
            } else {
                None
            };

            let task_entry: Task = Task {
                id,
                name: task.to_string(),
                run_interval,
                next_run,
                min_payout,
                task_running: false,
            };

            db.set_task(task.as_bytes(), &task_entry).await.unwrap();
        }
    }

    let wait_rpc_db = Arc::clone(&db);
    let wait_rpc_config = Arc::clone(&gv_config);

    tokio::spawn(async move {
        wait_rpc_server_ready(&wait_rpc_db, &wait_rpc_config).await;
    });

    // run one time tasks

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        let server_ready = db.get_server_ready().unwrap();
        if server_ready.ready {
            break;
        }
    }

    let cli_caller: CLICaller = CLICaller::new(&conf.cli_address, true).await.unwrap();
    drop(conf);
    cli_caller.call_start_server_tasks().await.unwrap();

    loop {
        let current_time: i64 = get_current_time();

        for task in runner_tasks.iter() {
            let task_details: Option<Task> = db.get_task(task.as_bytes());
            let task_details: Task = if task_details.is_none() {
                continue;
            } else {
                task_details.unwrap()
            };

            if task_details.task_running {
                continue;
            }

            let do_execute: bool = current_time >= task_details.next_run;

            if do_execute {
                let db_clone = Arc::clone(&db);

                let conf_clone = Arc::clone(&gv_config);

                match task {
                    &"daemon_update" => {
                        tokio::spawn(async move {
                            daemon_update_callback(&db_clone, &conf_clone).await;
                        });
                    }
                    &"self_update" => {
                        tokio::spawn(async move {
                            self_update_callback(&db_clone, &conf_clone).await;
                        });
                    }
                    &"process_rewards" => {
                        tokio::spawn(async move {
                            process_rewards_callback(&db_clone, &conf_clone).await;
                        });
                    }
                    _ => (),
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
}

fn get_index(v: &Vec<&str>, val: &str) -> Option<u64> {
    for (index, item) in v.iter().enumerate() {
        if item == &val {
            return Some(index as u64);
        }
    }
    None
}

async fn daemon_update_callback(db: &Arc<GVDB>, gv_config: &Arc<async_RwLock<GVConfig>>) {
    let task: &str = "daemon_update";
    info!("Running task: {}", task);
    let mut task_details: Task = db.get_task(task.as_bytes()).unwrap();
    toggle_running(db, task, &mut task_details).await;

    let conf = gv_config.read().await;

    let cli_caller: CLICaller = CLICaller::new(&conf.cli_address, true).await.unwrap();
    drop(conf);
    cli_caller.call_process_daemon_update().await.unwrap();

    schedule_next(db, task, &mut task_details).await;
}

async fn self_update_callback(db: &Arc<GVDB>, _gv_config: &Arc<async_RwLock<GVConfig>>) {
    let task: &str = "self_update";
    info!("Running task: {}", task);
    let mut task_details: Task = db.get_task(task.as_bytes()).unwrap();
    toggle_running(db, task, &mut task_details).await;

    schedule_next(db, task, &mut task_details).await;
}

async fn process_rewards_callback(db: &Arc<GVDB>, gv_config: &Arc<async_RwLock<GVConfig>>) {
    let task: &str = "process_rewards";
    info!("Running task: {}", task);
    let mut task_details: Task = db.get_task(task.as_bytes()).unwrap();
    toggle_running(db, task, &mut task_details).await;

    let conf = gv_config.read().await;

    let cli_caller: CLICaller = CLICaller::new(&conf.cli_address, true).await.unwrap();
    drop(conf);
    cli_caller.call_process_reward_payout().await.unwrap();

    schedule_next(db, task, &mut task_details).await;
}

async fn schedule_next(db: &Arc<GVDB>, task: &str, task_details: &mut Task) {
    let current_time: i64 = get_current_time();
    let next_time: i64 = task_details.run_interval + current_time;
    task_details.next_run = next_time;

    db.set_task(task.as_bytes(), task_details).await.unwrap();
    toggle_running(db, task, task_details).await;
}

async fn toggle_running(db: &Arc<GVDB>, task: &str, task_details: &mut Task) {
    task_details.task_running = !task_details.task_running;

    db.set_task(task.as_bytes(), &task_details).await.unwrap();
}

pub async fn update_payout_interval(db: &Arc<GVDB>, new_interval: i64) -> std::io::Result<()> {
    let task: &str = "process_rewards";
    let mut task_details: Task = db.get_task(task.as_bytes()).unwrap();
    task_details.run_interval = new_interval;

    db.set_task(task.as_bytes(), &task_details).await.unwrap();

    Ok(())
}

pub async fn get_next_payout_time(db: &Arc<GVDB>) -> std::io::Result<i64> {
    let task: &str = "process_rewards";
    let task_details: Task = db.get_task(task.as_bytes()).unwrap();
    let next_run: i64 = task_details.next_run;

    Ok(next_run)
}

pub async fn update_payout_min(db: &Arc<GVDB>, new_min_payout: u64) -> std::io::Result<()> {
    let task: &str = "process_rewards";
    let mut task_details: Task = db.get_task(task.as_bytes()).unwrap();
    task_details.min_payout = Some(new_min_payout);

    db.set_task(task.as_bytes(), &task_details).await.unwrap();

    Ok(())
}

fn get_current_time() -> i64 {
    let current_time = chrono::Utc::now();
    let timestamp: i64 = current_time.timestamp();
    timestamp
}

async fn wait_rpc_server_ready(db: &Arc<GVDB>, gv_config: &Arc<async_RwLock<GVConfig>>) {
    info!("Waiting for the RPC server to be ready...");
    let conf = gv_config.read().await;
    let cli_address: String = conf.cli_address.clone();
    drop(conf);

    while CLICaller::new(&cli_address, false).await.is_err() {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    let ready: ServerReadyDB = ServerReadyDB {
        ready: true,
        daemon_ready: true,
        reason: None,
    };

    db.set_server_ready(&ready).await.unwrap();
}
