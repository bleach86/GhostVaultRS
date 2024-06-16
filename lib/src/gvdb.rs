extern crate sled;
use serde::{Deserialize, Serialize};
use sled::{Db, Result, Tree};
use std::path::PathBuf;
use teloxide::types::MessageId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RewardsDB {
    pub height: u32,
    pub timestamp: u64,
    pub block_hash: String,
    pub txid: String,
    pub reward: u64,
    pub agvr_reward: u64,
    pub all_time_reward: u64,
    pub all_time_agvr_reward: u64,
    pub address: String,
    pub is_coldstake: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: u8,
    pub name: String,
    pub run_interval: i64,
    pub next_run: i64,
    pub min_payout: Option<u64>,
    pub task_running: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressInfo {
    pub is_mine: bool,
    pub is_valid: bool,
    pub is_256bit: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonStatusDB {
    pub height: u32,
    pub block_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZapStatusDB {
    pub txid: String,
    pub amount: u64,
    pub confirmations: u32,
    pub first_notice: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewStakeStatusDB {
    pub txid: String,
    pub confirmations: u32,
    pub timestamp: u64,
    pub tg_msg_id: Option<MessageId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColdRecovery {
    pub seed_words: String,
}

#[derive(Clone, Debug)]
pub struct GVDB {
    pub rewards_ts_index: Tree,
    pub tx_db: Tree,
    pub daemon_status_db: Tree,
    pub cold_recovery_db: Tree,
    pub task_queue: Tree,
    pub tg_bot_queue: Tree,
    pub zap_status_db: Tree,
    pub gvdb: Db,
    pub new_stake_status: Tree,
    pub server_ready_db: Tree,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TgBotQueueDB {
    pub timestamp: u64,
    pub header: String,
    pub msg: Option<String>,
    pub code_block: Option<String>,
    pub url: Option<Vec<String>>,
    pub msg_type: String,
    pub reward_txid: Option<String>,
    pub msg_to_delete: Option<MessageId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerReadyDB {
    pub ready: bool,
    pub daemon_ready: bool,
    pub reason: Option<String>,
}

impl GVDB {
    pub async fn new(gv_home: &PathBuf) -> Self {
        let db_dir: std::path::PathBuf = gv_home.join("gv_database/");
        let db: Db = sled::Config::new()
            .cache_capacity(500000000)
            .path(&db_dir)
            .open()
            .unwrap();

        let rewards_ts_index: Tree = db.open_tree(b"rewards").unwrap();
        let tx_db: Tree = db.open_tree(b"tx").unwrap();
        let daemon_status_db: Tree = db.open_tree(b"daemon_status").unwrap();
        let server_ready_db: Tree = db.open_tree(b"server_readyness").unwrap();
        let cold_recovery_db: Tree = db.open_tree(b"cold_recovery").unwrap();
        let task_queue: Tree = db.open_tree(b"task_queue").unwrap();
        let tg_bot_queue: Tree = db.open_tree(b"tg_bot_queue").unwrap();
        let zap_status_db: Tree = db.open_tree(b"zap_status").unwrap();
        let new_stake_status: Tree = db.open_tree(b"new_stake_status").unwrap();

        GVDB {
            rewards_ts_index,
            tx_db,
            daemon_status_db,
            cold_recovery_db,
            task_queue,
            tg_bot_queue,
            zap_status_db,
            gvdb: db,
            new_stake_status,
            server_ready_db,
        }
    }

    pub async fn clear_db(&self) -> Result<()> {
        self.rewards_ts_index.clear().unwrap();
        self.tx_db.clear().unwrap();
        self.daemon_status_db.clear().unwrap();
        self.cold_recovery_db.clear().unwrap();
        self.zap_status_db.clear().unwrap();
        self.new_stake_status.clear().unwrap();

        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub async fn set_reward(&self, reward: &RewardsDB) -> Result<()> {
        let key = reward.timestamp.to_be_bytes();
        let value: Vec<u8> = serde_json::to_vec(&reward).unwrap();
        self.rewards_ts_index.insert(key, value).unwrap();
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub fn get_reward(&self, key: impl AsRef<[u8]>) -> Option<RewardsDB> {
        if let Some(result) = self.rewards_ts_index.get(key).unwrap() {
            let value: RewardsDB = serde_json::from_slice(&result).unwrap();
            Some(value)
        } else {
            None
        }
    }

    pub async fn remove_reward(&self, key: impl AsRef<[u8]>) -> Result<()> {
        self.rewards_ts_index.remove(key).unwrap();
        self.gvdb.flush_async().await.unwrap();
        Ok(())
    }

    pub async fn set_task(&self, key: impl AsRef<[u8]>, task: &Task) -> Result<()> {
        let value: Vec<u8> = serde_json::to_vec(&task).unwrap();
        self.task_queue.insert(key, value).unwrap();
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub fn get_task(&self, key: impl AsRef<[u8]>) -> Option<Task> {
        if let Some(result) = self.task_queue.get(key).unwrap() {
            let value: Task = serde_json::from_slice(&result).unwrap();
            Some(value)
        } else {
            None
        }
    }

    pub async fn remove_task(&self, key: impl AsRef<[u8]>) -> Result<()> {
        self.task_queue.remove(key)?;
        self.gvdb.flush_async().await.unwrap();
        Ok(())
    }

    pub async fn set_tg_bot_queue(&self, key: impl AsRef<[u8]>, task: &TgBotQueueDB) -> Result<()> {
        let value: Vec<u8> = serde_json::to_vec(&task).unwrap();
        self.tg_bot_queue.insert(key, value).unwrap();
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub fn get_tg_bot_queue(&self, key: impl AsRef<[u8]>) -> Option<TgBotQueueDB> {
        if let Some(result) = self.tg_bot_queue.get(key).unwrap() {
            let value: TgBotQueueDB = serde_json::from_slice(&result).unwrap();
            Some(value)
        } else {
            None
        }
    }

    pub async fn remove_tg_bot_queue(&self, key: impl AsRef<[u8]>) -> Result<()> {
        self.tg_bot_queue.remove(key)?;
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub async fn set_zap_status(
        &self,
        key: impl AsRef<[u8]>,
        zap_status: &ZapStatusDB,
    ) -> Result<()> {
        let value: Vec<u8> = serde_json::to_vec(&zap_status).unwrap();
        self.zap_status_db.insert(key, value).unwrap();
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub fn get_zap_status(&self, key: impl AsRef<[u8]>) -> Option<ZapStatusDB> {
        if let Some(result) = self.zap_status_db.get(key).unwrap() {
            let value: ZapStatusDB = serde_json::from_slice(&result).unwrap();
            Some(value)
        } else {
            None
        }
    }

    pub async fn remove_zap_status(&self, key: impl AsRef<[u8]>) -> Result<()> {
        self.zap_status_db.remove(key)?;
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub async fn set_cold_recovery(&self, wallet: &str, cold_recover: &ColdRecovery) -> Result<()> {
        let key = wallet.as_bytes();
        let value: Vec<u8> = serde_json::to_vec(&cold_recover).unwrap();
        self.cold_recovery_db.insert(key, value).unwrap();
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub fn get_cold_recovery(&self, key: impl AsRef<[u8]>) -> Option<ColdRecovery> {
        if let Some(result) = self.cold_recovery_db.get(key).unwrap() {
            let value: ColdRecovery = serde_json::from_slice(&result).unwrap();
            Some(value)
        } else {
            None
        }
    }

    pub async fn set_daemon_status(&self, status: &DaemonStatusDB) -> Result<()> {
        let key: &[u8; 13] = b"daemon_status";
        let value: Vec<u8> = serde_json::to_vec(&status).unwrap();
        self.daemon_status_db.insert(key, value).unwrap();
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub fn get_daemon_status(&self) -> Option<DaemonStatusDB> {
        if let Some(result) = self.daemon_status_db.get(b"daemon_status").unwrap() {
            let value: DaemonStatusDB = serde_json::from_slice(&result).unwrap();
            Some(value)
        } else {
            None
        }
    }

    pub async fn set_new_stake_status(
        &self,
        key: impl AsRef<[u8]>,
        status: &NewStakeStatusDB,
    ) -> Result<()> {
        let value: Vec<u8> = serde_json::to_vec(&status).unwrap();
        self.new_stake_status.insert(key, value).unwrap();
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub fn get_new_stake_status(&self, key: impl AsRef<[u8]>) -> Option<NewStakeStatusDB> {
        if let Some(result) = self.new_stake_status.get(key).unwrap() {
            let value: NewStakeStatusDB = serde_json::from_slice(&result).unwrap();
            Some(value)
        } else {
            None
        }
    }

    pub async fn remove_new_stake_status(&self, key: impl AsRef<[u8]>) -> Result<()> {
        self.new_stake_status.remove(key)?;
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub async fn set_server_ready(&self, status: &ServerReadyDB) -> Result<()> {
        let key: &[u8; 12] = b"server_ready";
        let value: Vec<u8> = serde_json::to_vec(&status).unwrap();
        self.server_ready_db.insert(key, value).unwrap();
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }

    pub fn get_server_ready(&self) -> Option<ServerReadyDB> {
        if let Some(result) = self.server_ready_db.get(b"server_ready").unwrap() {
            let value: ServerReadyDB = serde_json::from_slice(&result).unwrap();
            Some(value)
        } else {
            None
        }
    }

    pub async fn remove_server_ready(&self) -> Result<()> {
        self.server_ready_db.remove(b"server_ready")?;
        self.gvdb.flush_async().await.unwrap();

        Ok(())
    }
}
