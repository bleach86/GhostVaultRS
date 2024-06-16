use crate::{
    config::GVConfig,
    gvdb::{NewStakeStatusDB, TgBotQueueDB, GVDB},
    tg_bot::keyboards::make_link_button,
};
use log::{info, warn};
use std::sync::Arc;
use teloxide::{
    adaptors::DefaultParseMode, payloads::SendMessageSetters, prelude::*, types::MessageId,
    utils::markdown::escape,
};
use tokio::sync::RwLock as async_RwLock;

#[derive(Clone)]
pub struct BotRunner {
    bot: DefaultParseMode<Bot>,
    tg_user: String,
    gv_config: Arc<async_RwLock<GVConfig>>,
    db: Arc<GVDB>,
}

impl BotRunner {
    pub async fn new(
        gv_config: &Arc<async_RwLock<GVConfig>>,
        db: Arc<GVDB>,
        bot: DefaultParseMode<Bot>,
    ) -> Self {
        info!("Task runner starting...");

        let conf = gv_config.read().await;
        let tg_user: String = conf.to_owned().tg_user.unwrap();

        drop(conf);

        // Wait for the server to be ready
        while !db.get_server_ready().unwrap().ready {
            info!("Waiting for server to be ready...");
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        info!("Server is ready!");

        BotRunner {
            bot,
            tg_user,
            gv_config: Arc::clone(gv_config),
            db,
        }
    }

    pub async fn background_task(&self) {
        loop {
            let current_time = chrono::Utc::now();
            let timestamp: u64 = current_time.timestamp() as u64;
            let five_minutes: u64 = 300;

            let conf = self.gv_config.read().await;

            for result in self.db.tg_bot_queue.iter() {
                match result {
                    Ok((key, value)) => {
                        let msg_details: TgBotQueueDB =
                            serde_json::from_slice::<TgBotQueueDB>(&value).unwrap();

                        let msg_req_time: u64 = msg_details.timestamp;

                        if timestamp - msg_req_time > five_minutes {
                            self.db.tg_bot_queue.remove(key).unwrap();
                            continue;
                        }

                        match msg_details.msg_type.as_str() {
                            "rewards" => {
                                if !conf.announce_rewards {
                                    self.db.remove_tg_bot_queue(key).await.unwrap();
                                    continue;
                                }
                            }
                            "stake" => {
                                if !conf.announce_stakes {
                                    self.db.remove_tg_bot_queue(key).await.unwrap();
                                    continue;
                                }
                            }
                            "zap" => {
                                if !conf.announce_zaps {
                                    self.db.remove_tg_bot_queue(key).await.unwrap();
                                    continue;
                                }
                            }
                            "offline" | "online" => {
                                // Do nothing
                            }
                            "stake_removal" => {
                                if msg_details.msg_to_delete.is_some() {
                                    let msg_id: MessageId = msg_details.msg_to_delete.unwrap();
                                    let _ =
                                        self.bot.delete_message(self.tg_user.clone(), msg_id).await;
                                }
                                self.db.remove_tg_bot_queue(key).await.unwrap();
                                continue;
                            }
                            _ => {
                                info!("Unknown message type: {}", msg_details.msg_type);
                                self.db.tg_bot_queue.remove(key).unwrap();
                                continue;
                            }
                        }

                        let mut message = String::from(
                            escape(format!("{}\n\n", msg_details.header).as_str()).as_str(),
                        );

                        if msg_details.code_block.is_some() {
                            message.push_str(
                                format!("```\n{}\n```\n", msg_details.code_block.unwrap()).as_str(),
                            );
                        }

                        if msg_details.msg.is_some() {
                            message.push_str(
                                escape(format!("{}\n", msg_details.msg.unwrap()).as_str()).as_str(),
                            );
                        }

                        let sent_msg_res = if msg_details.url.is_some() {
                            let links = msg_details.url.unwrap();
                            let keyboard = make_link_button(&links, "View on Ghostscan");

                            self.bot
                                .send_message(self.tg_user.clone(), message)
                                .reply_markup(keyboard)
                                .await
                        } else {
                            self.bot.send_message(self.tg_user.clone(), message).await
                        };

                        let sent_msg = if sent_msg_res.is_err() {
                            let err_msg = sent_msg_res.err().unwrap();
                            warn!("Error sending message: {:?}", err_msg);
                            continue;
                        } else {
                            sent_msg_res.unwrap()
                        };

                        if msg_details.msg_type.as_str() == "stake"
                            && msg_details.reward_txid.is_some()
                        {
                            let reward_txid = msg_details.reward_txid.unwrap();

                            let stake_status: Option<NewStakeStatusDB> =
                                self.db.get_new_stake_status(reward_txid.as_bytes());

                            if stake_status.is_some() {
                                let mut stake_status: NewStakeStatusDB = stake_status.unwrap();
                                stake_status.tg_msg_id = Some(sent_msg.id);

                                let _ = self
                                    .db
                                    .set_new_stake_status(reward_txid.as_bytes(), &stake_status)
                                    .await;
                            }
                        }

                        self.db.tg_bot_queue.remove(key).unwrap();
                    }
                    Err(e) => {
                        info!("Error reading from db: {}", e);
                    }
                }
            }

            drop(conf);

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }
}
