use crate::{
    config::GVConfig,
    file_ops,
    gv_client_methods::{BarChart, CLICaller, GVStatus, PendingRewards, StakingDataOverview},
    gvdb::{ServerReadyDB, GVDB},
    tg_bot::{
        bot_tasks::BotRunner,
        charts::charts::{make_area_chart, make_barchart},
        dialogs::{
            chart_range_dialog::{receive_first_date, start_chart_range_dialogue},
            reward_interval_dialog::{
                reward_interval_dialogue_handler, start_update_reward_interval,
            },
            reward_min_dialog::{reward_min_dialogue_handler, start_update_reward_min},
            reward_mode_dialog::{reward_mode_dialogue_handler, start_update_reward_mode},
            utils::{
                get_current_month_year_day, parse_chart_range, GetDateRangeDialog,
                GetDateRangeState, UpdateRewardIntervalDialog, UpdateRewardIntervalState,
                UpdateRewardMinDialog, UpdateRewardMinState, UpdateRewardModeDialog,
                UpdateRewardModeState,
            },
        },
        keyboards::{
            make_inline_calander, make_inline_chart_menu, make_inline_ghost_links_menu,
            make_inline_stake_chart_range_menu, make_inline_stakes_chart_menu,
            make_keyboard_bot_settings, make_keyboard_gv_options, make_keyboard_main,
            make_keyboard_reward_options, make_reward_interval_keyboard, make_reward_mode_keyboard,
            make_stats_info_keyboard, make_timezone_option_keyboard, make_timezone_region_keyboard,
        },
    },
};
use chrono::{NaiveDate, TimeZone};
use chrono_tz::Tz;
use log::info;
use serde_json::Value;
use std::{
    env,
    path::PathBuf,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
    vec,
};
use teloxide::{
    adaptors::DefaultParseMode,
    dispatching::dialogue::InMemStorage,
    payloads::SendMessageSetters,
    prelude::*,
    types::{InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId, ParseMode},
    utils::markdown::escape,
};
use tokio::sync::RwLock as async_RwLock;
use url::Url;

async fn command_handler(
    bot: DefaultParseMode<Bot>,
    msg: Message,
    gv_config: Arc<async_RwLock<GVConfig>>,
    db: Arc<GVDB>,
    reward_mode_mem: Arc<InMemStorage<UpdateRewardModeState>>,
    last_dialog_id: Arc<AtomicI32>,
    reward_interval_mem: Arc<InMemStorage<UpdateRewardIntervalState>>,
    reward_min_mem: Arc<InMemStorage<UpdateRewardMinState>>,
    chart_range_mem: Arc<InMemStorage<GetDateRangeState>>,
) -> ResponseResult<()> {
    let conf = gv_config.read().await;
    let auth_user = conf.to_owned().tg_user.unwrap();
    let cli_address = conf.to_owned().cli_address;
    drop(conf);

    let cli_caller = CLICaller::new(&cli_address, true).await.unwrap();

    if msg.chat.id.to_string() != auth_user {
        return Ok(());
    }

    let message_option = msg.text();

    let user_message = if message_option.is_none() {
        return Ok(());
    } else {
        message_option.unwrap()
    };

    let reward_update_dialogue: Dialogue<
        UpdateRewardModeState,
        InMemStorage<UpdateRewardModeState>,
    > = UpdateRewardModeDialog::new(reward_mode_mem.clone(), msg.chat.id);

    let reward_update_state = reward_update_dialogue.get().await;

    if let Ok(Some(_)) = reward_update_state {
        reward_mode_dialogue_handler(
            bot.clone(),
            msg.clone(),
            reward_mode_mem.clone(),
            last_dialog_id.clone(),
            reward_update_dialogue.clone(),
            &cli_caller,
            &db,
        )
        .await?;

        return Ok(());
    }

    let reward_interval_dialogue: Dialogue<
        UpdateRewardIntervalState,
        InMemStorage<UpdateRewardIntervalState>,
    > = UpdateRewardIntervalDialog::new(reward_interval_mem.clone(), msg.chat.id);

    let reward_interval_state = reward_interval_dialogue.get().await;

    if let Ok(Some(_)) = reward_interval_state {
        reward_interval_dialogue_handler(
            bot.clone(),
            msg.clone(),
            reward_interval_mem.clone(),
            last_dialog_id.clone(),
            reward_interval_dialogue.clone(),
            &cli_caller,
            &db,
        )
        .await?;

        return Ok(());
    }

    let reward_min_dialogue: Dialogue<UpdateRewardMinState, InMemStorage<UpdateRewardMinState>> =
        UpdateRewardMinDialog::new(reward_min_mem.clone(), msg.chat.id);

    let reward_min_state = reward_min_dialogue.get().await;

    if let Ok(Some(_)) = reward_min_state {
        reward_min_dialogue_handler(
            bot.clone(),
            msg.clone(),
            reward_min_mem.clone(),
            last_dialog_id.clone(),
            reward_min_dialogue.clone(),
            &cli_caller,
            &db,
        )
        .await?;

        return Ok(());
    }

    let chart_range_dialogue: Dialogue<GetDateRangeState, InMemStorage<GetDateRangeState>> =
        GetDateRangeDialog::new(chart_range_mem.clone(), msg.chat.id);

    let chart_range_state = chart_range_dialogue.get().await;

    if let Ok(Some(_)) = chart_range_state {
        reward_min_dialogue_handler(
            bot.clone(),
            msg.clone(),
            reward_min_mem.clone(),
            last_dialog_id.clone(),
            reward_min_dialogue.clone(),
            &cli_caller,
            &db,
        )
        .await?;

        return Ok(());
    }

    let server_ready: ServerReadyDB = db.get_server_ready().unwrap();

    match user_message.to_lowercase().as_str() {
        cmd if cmd.starts_with("\u{2753} help") => {
            let reply = escape("👻 GhostVault Help 👻\n\n");

            let help_link_button =
                InlineKeyboardMarkup::default().append_row(vec![InlineKeyboardButton::url(
                    "GhostVault Help",
                    Url::parse("https://ghostveterans.net/vps/").unwrap(),
                )]);

            bot.send_message(auth_user, reply)
                .reply_markup(help_link_button)
                .await?
        }
        cmd if cmd.starts_with("/start") => {
            let keyboard = make_keyboard_main();

            let welcome_message =
                escape("👻 Welcome to your personal GhostVault! 👻\n Please choose an option");

            bot.send_message(msg.chat.id, welcome_message)
                .reply_markup(keyboard)
                .await?
        }
        cmd if cmd.starts_with("/status") || cmd.starts_with("\u{2139}\u{FE0F} status") => {
            if !server_ready.daemon_ready || !server_ready.ready {
                let reason = server_unready_message(&server_ready);

                let message = escape("Ghost daemon unavailable.\nReason:");

                let reasoned_message = format!("{}{}", message, reason);

                bot.send_message(msg.chat.id, reasoned_message).await?
            } else {
                reply_status(&bot, &msg, &gv_config).await?
            }
        }
        cmd if cmd.starts_with("/stats") || cmd.starts_with("\u{1F4CA} stats") => {
            let keyboard = make_stats_info_keyboard();

            let stats_message = escape("👻 Stats 👻\n Please choose an option");

            bot.send_message(msg.chat.id, stats_message)
                .reply_markup(keyboard)
                .await?
        }
        cmd if cmd.starts_with("/bot_settings")
            || cmd.starts_with("\u{2699}\u{FE0F} bot settings") =>
        {
            let reply = get_bot_settings(&gv_config).await;
            let keyboard = make_keyboard_bot_settings();

            bot.send_message(msg.chat.id, reply)
                .reply_markup(keyboard)
                .await?
        }
        cmd if cmd.starts_with("\u{1F47B} ghost links") => {
            let keyboard = make_inline_ghost_links_menu();

            let ghost_links_message = escape("👻 Ghost Links 👻");

            bot.send_message(msg.chat.id, ghost_links_message)
                .reply_markup(keyboard)
                .await?
        }
        cmd if cmd.starts_with("\u{1F3E0} home")
            || vec!["home", "/home", "keyboard", "/keyboard"].contains(&cmd) =>
        {
            let keyboard = make_keyboard_main();

            let home_message = escape("\u{1F3E0} Home");

            bot.send_message(msg.chat.id, home_message)
                .reply_markup(keyboard)
                .await?
        }
        cmd if cmd.starts_with("\u{1F4B8} toggle stake") => {
            let conf = gv_config.read().await;
            let toggle = !conf.announce_stakes;
            drop(conf);

            cli_caller
                .call_set_bot_announce("stake".to_string(), toggle)
                .await
                .unwrap();

            let reply = get_bot_settings(&gv_config).await;

            bot.send_message(msg.chat.id, reply).await?
        }
        cmd if cmd.starts_with("\u{1F4B0} toggle reward") => {
            let conf = gv_config.read().await;
            let toggle = !conf.announce_rewards;
            drop(conf);

            cli_caller
                .call_set_bot_announce("reward".to_string(), toggle)
                .await
                .unwrap();

            let reply = get_bot_settings(&gv_config).await;

            bot.send_message(msg.chat.id, reply).await?
        }
        cmd if cmd.starts_with("\u{26A1} toggle zap") => {
            let conf = gv_config.read().await;
            let toggle = !conf.announce_zaps;
            drop(conf);

            cli_caller
                .call_set_bot_announce("zap".to_string(), toggle)
                .await
                .unwrap();

            let reply = get_bot_settings(&gv_config).await;

            bot.send_message(msg.chat.id, reply).await?
        }
        cmd if cmd.starts_with("\u{2699}\u{FE0F} ghostvault options") => {
            let keyboard = make_keyboard_gv_options();

            let gv_options_message = escape("👻 GhostVault Options 👻\n Please choose an option");

            bot.send_message(msg.chat.id, gv_options_message)
                .reply_markup(keyboard)
                .await?
        }
        cmd if cmd.starts_with("\u{2744}\u{FE0F} cs key") => {
            let cli_resp: Value = cli_caller.call_get_ext_pub_key().await.unwrap();

            let ext_pub_key: String = cli_resp.as_str().unwrap().to_string();

            let key_mono: String = format!("`{}`", ext_pub_key);

            let header = escape("👻 CS Key 👻\n\n");
            let message = escape(format!("Your GhostVault CS Key is\n\n").as_str());

            let reply: String = format!("{}{}{}", header, message, key_mono);

            bot.send_message(msg.chat.id, reply).await?
        }
        cmd if cmd.starts_with("\u{1F4CA} version") => {
            let cli_resp: Value = cli_caller.call_get_version_info().await.unwrap();

            let gv_version: &str = cli_resp["gv_version"].as_str().unwrap();
            let ghostd_version: &str = cli_resp["ghostd_version"].as_str().unwrap();
            let latest_release: &str = cli_resp["latest_release"].as_str().unwrap();

            let header: String = escape("👻 GhostVault Version Info 👻\n\n");
            let message: String = escape(
                format!(
                    "GhostVault: {}\nGhostd: {}\nLatest Ghostd Release: {}",
                    gv_version, ghostd_version, latest_release
                )
                .as_str(),
            );

            let reply: String = format!("{}{}", header, message);

            bot.send_message(msg.chat.id, reply).await?
        }
        cmd if cmd.starts_with("\u{1F501} resync") => {
            let good_chain = cli_caller
                .call_check_chain()
                .await
                .unwrap()
                .as_bool()
                .unwrap();

            let confirm_markup = InlineKeyboardMarkup::default().append_row(vec![
                InlineKeyboardButton::callback("Confirm", "confirm_resync"),
                InlineKeyboardButton::callback("Cancel", "cancel_resync"),
            ]);

            let message = if good_chain {
                escape(
                    "Your GhostVault is properly synced!\n\nAre you sure you want to start the resync operation?\nThis will take a long time and staking will be unavailable for the duration.",
                )
            } else {
                escape(
                    "WARNING: your GhostVault has a bad sync!\n\nAre you sure you want to start the resync operation?\nThis will take a long time and staking will be unavailable for the duration.",
                )
            };

            let sent_message = bot
                .send_message(msg.chat.id, &message)
                .reply_markup(confirm_markup)
                .await?;
            sent_message
        }
        cmd if cmd.starts_with("\u{1F517} check chain") => {
            let cli_resp: Value = cli_caller.call_check_chain().await.unwrap();

            let good_chain = cli_resp.as_bool().unwrap();

            let message = if good_chain {
                escape("Your GhostVault is properly synced!")
            } else {
                escape("WARNING: your GhostVault has a bad sync!")
            };

            bot.send_message(msg.chat.id, message).await?
        }
        cmd if cmd.starts_with("\u{1F6E0}\u{FE0F} update ghostd") => {
            let cli_resp: Value = cli_caller.call_process_daemon_update().await.unwrap();

            let header = escape("👻 Ghostd Update 👻\n\n");

            let sent_message = if cli_resp.is_string() {
                let new_version = cli_resp.as_str().unwrap();

                if new_version.contains("Failed to check for updates!") {
                    let message = escape(
                        format!(
                            "{}Failed to check for updates!\nPlease try again later.",
                            header
                        )
                        .as_str(),
                    );
                    bot.send_message(msg.chat.id, message).await?
                } else {
                    let message = escape(
                        format!(
                            "{}New update found!\nUpdating ghostd to version: {}",
                            header, new_version
                        )
                        .as_str(),
                    );
                    bot.send_message(msg.chat.id, message).await?
                }
            } else {
                let message = escape(format!("{}Ghostd is already up to date", header).as_str());
                bot.send_message(msg.chat.id, message).await?
            };

            sent_message
        }
        cmd if cmd.starts_with("\u{1F4B8} reward options") => {
            let keyboard = make_keyboard_reward_options();

            let cli_res: Value = cli_caller.call_get_reward_options().await.unwrap();
            let reward_options: String = serde_json::to_string_pretty(&cli_res).unwrap();
            let code_block: String = format!("\n```\n{}\n```\n", reward_options);
            let header: String = escape("👻 Reward Options 👻\n\n");
            let choose_opt: String = escape("\nPlease choose an option");

            let explainer = escape(concat!(
                "reward_address: The address that rewards are sent to. This will be blank in DEFAULT mode.\n",
                "reward_interval: This is how often GhostVault will check if rewards can be sent. Only applies to ANON mode.\n",
                "reward_min: Available rewards must be this much before they are sent. Only applies to ANON mode.\n",
                "reward_mode: This is the reward mode that GhostVault is in.\n\n",
            ));

            let send_message: String =
                format!("{}{}{}{}", header, explainer, code_block, choose_opt);

            bot.send_message(msg.chat.id, send_message)
                .reply_markup(keyboard)
                .await?
        }
        cmd if cmd.starts_with("\u{1F4B8} set reward mode & address") => {
            if server_ready.daemon_ready && server_ready.ready {
                let keyboard = make_reward_mode_keyboard();

                if last_dialog_id.load(Ordering::Relaxed) != 0 {
                    return Ok(());
                }

                let new_msg = bot
                    .send_message(msg.chat.id, "👻 Reward Mode Updater 👻")
                    .reply_markup(keyboard)
                    .await?;

                let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
                last_dialog_id.store(new_id, Ordering::Relaxed);

                reward_update_dialogue
                    .update(UpdateRewardModeState::Start)
                    .await
                    .unwrap();

                start_update_reward_mode(
                    bot.clone(),
                    reward_update_dialogue.clone(),
                    msg.clone(),
                    last_dialog_id.clone(),
                )
                .await
                .unwrap();
            } else {
                let reason = server_unready_message(&server_ready);

                let message = escape("Ghost daemon unavailable.\nReason:");
                let reasoned_message = format!("{}{}", message, reason);

                bot.send_message(msg.chat.id, reasoned_message).await?;
            }

            return Ok(());
        }

        cmd if cmd.starts_with("\u{1F4CA} set reward interval") => {
            if server_ready.daemon_ready && server_ready.ready {
                let keyboard = make_reward_interval_keyboard();

                if last_dialog_id.load(Ordering::Relaxed) != 0 {
                    return Ok(());
                }

                let new_msg = bot
                    .send_message(msg.chat.id, "👻 Reward Interval Updater 👻")
                    .reply_markup(keyboard)
                    .await?;

                let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
                last_dialog_id.store(new_id, Ordering::Relaxed);

                reward_interval_dialogue
                    .update(UpdateRewardIntervalState::Start)
                    .await
                    .unwrap();

                start_update_reward_interval(
                    bot.clone(),
                    reward_interval_dialogue.clone(),
                    msg.clone(),
                    last_dialog_id.clone(),
                )
                .await
                .unwrap();
            } else {
                let reason = server_unready_message(&server_ready);

                let message = escape("Ghost daemon unavailable.\nReason:");
                let reasoned_message = format!("{}{}", message, reason);

                bot.send_message(msg.chat.id, reasoned_message).await?;
            }

            return Ok(());
        }

        cmd if cmd.starts_with("\u{1F4B0} set payout min") => {
            if server_ready.daemon_ready && server_ready.ready {
                if last_dialog_id.load(Ordering::Relaxed) != 0 {
                    return Ok(());
                }

                let new_msg = bot
                    .send_message(msg.chat.id, "👻 Payout Min Updater 👻")
                    .await?;

                let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
                last_dialog_id.store(new_id, Ordering::Relaxed);

                reward_min_dialogue
                    .update(UpdateRewardMinState::Start)
                    .await
                    .unwrap();

                start_update_reward_min(
                    bot.clone(),
                    reward_min_dialogue.clone(),
                    msg.clone(),
                    last_dialog_id.clone(),
                )
                .await
                .unwrap();
            } else {
                let reason = server_unready_message(&server_ready);

                let message = escape("Ghost daemon unavailable.\nReason:");
                let reasoned_message = format!("{}{}", message, reason);

                bot.send_message(msg.chat.id, reasoned_message).await?;
            }
            return Ok(());
        }

        cmd if cmd.starts_with("\u{1F55B} set timezone") => {
            let message = escape("👻 Timezone Updater 👻\n\nPlease select your region.");

            let kb = make_timezone_region_keyboard();

            bot.send_message(msg.chat.id, message)
                .reply_markup(kb)
                .await?
        }

        cmd if cmd.starts_with("\u{1F4CA} charts") => {
            let kb = make_inline_chart_menu();

            let message = escape("👻 Charts 👻\n\nPlease select a chart type");

            bot.send_message(msg.chat.id, message)
                .reply_markup(kb)
                .await?
        }

        cmd if cmd.starts_with("\u{1F4CB} overview") => {
            if server_ready.daemon_ready && server_ready.ready {
                let cli_res: Value = cli_caller.call_get_overview().await.unwrap();
                let header: String = escape("👻 Overview 👻\n\n");
                let staking_data: StakingDataOverview = serde_json::from_value(cli_res).unwrap();

                let overview: String = serde_json::to_string_pretty(&staking_data).unwrap();
                let code_block: String = format!("```\n{}\n```\n", overview);

                let message: String = format!("{}{}", header, code_block);

                bot.send_message(msg.chat.id, message).await?
            } else {
                let reason = server_unready_message(&server_ready);

                let message = escape("Ghost daemon unavailable.\nReason:");

                let reasoned_message = format!("{}{}", message, reason);

                bot.send_message(msg.chat.id, reasoned_message).await?
            }
        }

        cmd if cmd.starts_with("\u{1F4B0} pending rewards") => {
            if !server_ready.daemon_ready || !server_ready.ready {
                let reason = server_unready_message(&server_ready);

                let message = escape("Ghost daemon unavailable.\nReason:");

                let reasoned_message = format!("{}{}", message, reason);

                bot.send_message(msg.chat.id, reasoned_message).await?
            } else {
                let cli_res = cli_caller.call_get_pending_rewards().await.unwrap();

                let header = escape("👻 Pending Rewards 👻\n\n");

                let pending_rewards: PendingRewards = serde_json::from_value(cli_res).unwrap();

                let pending_rewards: String =
                    serde_json::to_string_pretty(&pending_rewards).unwrap();
                let code_block: String = format!("\n```\n{}\n```\n", pending_rewards);

                let message = format!("{}{}", header, code_block);

                bot.send_message(msg.chat.id, message).await?
            }
        }

        cmd if cmd.starts_with("\u{1F4E5} recovery") => {
            let cli_res = cli_caller.call_get_mnemonic().await.unwrap();

            let cold_recovery = if cli_res.is_string() {
                Some(cli_res.as_str().unwrap().to_string())
            } else {
                None
            };

            let mnemonic = cold_recovery.unwrap_or("Mnemonic not found.".to_string());

            let header = escape("👻 Recovery Mnemonic 👻\n");
            let message = escape(format!("Your recovery mnemonic is\n\n").as_str());
            let code_block = format!("`{}`", mnemonic);
            let reply = format!("{}{}{}", header, message, code_block);

            bot.send_message(msg.chat.id, reply).await?
        }
        _ => {
            return Ok(());
        }
    };

    Ok(())
}

async fn callback_handler(
    bot: DefaultParseMode<Bot>,
    q: CallbackQuery,
    gv_config: Arc<async_RwLock<GVConfig>>,
    _db: Arc<GVDB>,
    reward_mode_mem: Arc<InMemStorage<UpdateRewardModeState>>,
    last_dialog_id: Arc<AtomicI32>,
    reward_interval_mem: Arc<InMemStorage<UpdateRewardIntervalState>>,
    reward_min_mem: Arc<InMemStorage<UpdateRewardMinState>>,
    chart_range_mem: Arc<InMemStorage<GetDateRangeState>>,
) -> ResponseResult<()> {
    if let Some(data) = q.clone().data {
        match data.as_str() {
            "confirm_resync" => {
                let conf = gv_config.read().await;
                let cli_address = conf.to_owned().cli_address;
                let user = conf.to_owned().tg_user.unwrap();
                drop(conf);

                let cli_caller = CLICaller::new(&cli_address, true).await.unwrap();
                cli_caller.call_force_resync().await.unwrap();

                bot.answer_callback_query(q.id).await?;
                bot.delete_message(user.clone(), q.message.unwrap().id)
                    .await?;

                let message = escape("Resync operation started\nThis will take a while.");

                bot.send_message(user.clone(), message).await?;
            }
            "cancel_resync" => {
                let conf = gv_config.read().await;
                let user = conf.to_owned().tg_user.unwrap();
                drop(conf);
                bot.answer_callback_query(q.id).await?;
                bot.delete_message(user, q.message.unwrap().id).await?;
            }
            "cancel_update_reward_mode" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;
                let dialogue = UpdateRewardModeDialog::new(reward_mode_mem, chat_id);
                let current_dialog = dialogue.get().await.unwrap();

                bot.answer_callback_query(q.id).await?;

                let keyboard = make_keyboard_gv_options();

                if !current_dialog.is_none() {
                    dialogue.exit().await.unwrap();
                }

                bot.send_message(chat_id, "Cancelled")
                    .reply_markup(keyboard)
                    .await?;
                last_dialog_id.store(0, Ordering::Relaxed);
                bot.delete_message(chat_id, msg_id).await?;
            }

            "cancel_update_reward_interval" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;
                let dialogue = UpdateRewardIntervalDialog::new(reward_interval_mem, chat_id);
                let current_dialog = dialogue.get().await.unwrap();

                bot.answer_callback_query(q.id).await?;

                let keyboard = make_keyboard_gv_options();

                if !current_dialog.is_none() {
                    dialogue.exit().await.unwrap();
                }
                bot.send_message(chat_id, "Cancelled")
                    .reply_markup(keyboard)
                    .await?;
                last_dialog_id.store(0, Ordering::Relaxed);
                bot.delete_message(chat_id, msg_id).await?;
            }
            "cancel_update_reward_min" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;
                let dialogue = UpdateRewardMinDialog::new(reward_min_mem, chat_id);
                let current_dialog = dialogue.get().await.unwrap();

                bot.answer_callback_query(q.id).await?;

                let keyboard = make_keyboard_gv_options();

                if !current_dialog.is_none() {
                    dialogue.exit().await.unwrap();
                }
                bot.send_message(chat_id, "Cancelled")
                    .reply_markup(keyboard)
                    .await?;
                last_dialog_id.store(0, Ordering::Relaxed);
                bot.delete_message(chat_id, msg_id).await?;
            }
            btn_press if btn_press.starts_with("next_month") => {
                let split_msg = btn_press.split(",").collect::<Vec<&str>>();
                let month: u32 = split_msg[1].parse::<u32>().unwrap();
                let year: i32 = split_msg[2].parse::<i32>().unwrap();

                let year_month: (i32, u32) = if month == 12 {
                    (year + 1, 1)
                } else {
                    (year, month + 1)
                };

                let conf = gv_config.read().await;
                let timezone = conf.to_owned().timezone;

                let kb = make_inline_calander(year_month.0, year_month.1, &timezone);
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let _ = bot
                    .edit_message_reply_markup(chat_id, msg_id)
                    .reply_markup(kb)
                    .await;
            }
            btn_press if btn_press.starts_with("prev_month") => {
                let split_msg = btn_press.split(",").collect::<Vec<&str>>();
                let month: u32 = split_msg[1].parse::<u32>().unwrap();
                let year: i32 = split_msg[2].parse::<i32>().unwrap();

                let year_month: (i32, u32) = if month == 1 {
                    (year - 1, 12)
                } else {
                    (year, month - 1)
                };

                let conf = gv_config.read().await;
                let timezone = conf.to_owned().timezone;

                let kb = make_inline_calander(year_month.0, year_month.1, &timezone);
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let _ = bot
                    .edit_message_reply_markup(chat_id, msg_id)
                    .reply_markup(kb)
                    .await;
            }
            btn_press if btn_press.starts_with("date_selection") => {
                let split_msg = btn_press.split(",").collect::<Vec<&str>>();
                let day: u32 = split_msg[1].parse::<u32>().unwrap();
                let month: u32 = split_msg[2].parse::<u32>().unwrap();
                let year: i32 = split_msg[3].parse::<i32>().unwrap();
                let q_clone = q.clone();

                bot.answer_callback_query(q.id).await?;

                let chart_range_dialogue: Dialogue<
                    GetDateRangeState,
                    InMemStorage<GetDateRangeState>,
                > = GetDateRangeDialog::new(
                    chart_range_mem.clone(),
                    q.message.as_ref().unwrap().chat.id,
                );

                let chart_range_state = chart_range_dialogue.get().await.unwrap();

                let conf = gv_config.read().await;
                let time_zone = conf.to_owned().timezone;
                let tz = Tz::from_str_insensitive(&time_zone).unwrap();

                drop(conf);

                let timestamp: u64 = tz
                    .with_ymd_and_hms(year, month, day, 0, 0, 0)
                    .unwrap()
                    .timestamp() as u64;

                if chart_range_state.is_some() {
                    match chart_range_state {
                        Some(GetDateRangeState::ReceiveFirstDate {
                            division,
                            time_zone,
                            chart_type,
                        }) => {
                            receive_first_date(
                                bot.clone(),
                                chart_range_dialogue.clone(),
                                &q_clone,
                                timestamp,
                                division,
                                &time_zone,
                                chart_type,
                            )
                            .await
                            .unwrap();
                        }
                        Some(GetDateRangeState::ReceiveSecondDate {
                            first_date,
                            division,
                            time_zone: _,
                            chart_type,
                        }) => {
                            let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                            let msg_id = q.message.as_ref().unwrap().id;
                            let end_time = NaiveDate::from_ymd_opt(year, month, day)
                                .unwrap()
                                .and_hms_opt(23, 59, 59)
                                .unwrap()
                                .and_local_timezone(tz)
                                .unwrap()
                                .timestamp() as u64;
                            if end_time <= first_date {
                                let message = escape("End date must be after start date");
                                bot.edit_message_text(chat_id, msg_id, message).await?;
                                chart_range_dialogue.exit().await.unwrap();
                                return Ok(());
                            }

                            let chart_range = (first_date, end_time);
                            chart_range_dialogue.exit().await.unwrap();

                            let _ = bot.delete_message(chat_id, msg_id).await;

                            if chart_type == "earnings_chart" {
                                send_earnings_chart(chart_range, &bot, &q_clone, gv_config).await?;
                            } else {
                                send_barchart(chart_range, &bot, &q_clone, gv_config, &division)
                                    .await?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            btn_press if btn_press.starts_with("current_date") => {
                let conf = gv_config.read().await;
                let timezone = conf.to_owned().timezone;
                let current_ymd = get_current_month_year_day(&timezone);
                let kb = make_inline_calander(current_ymd.0, current_ymd.1, &timezone);
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let _ = bot
                    .edit_message_reply_markup(chat_id, msg_id)
                    .reply_markup(kb)
                    .await;
            }
            btn_press if btn_press.starts_with("tz_region_selection") => {
                let split_msg = btn_press.split(",").collect::<Vec<&str>>();
                let region: &str = split_msg[1];

                if region == "UTC" {
                    let conf = gv_config.read().await;
                    let cli_address = conf.to_owned().cli_address;
                    drop(conf);

                    let cli_caller = CLICaller::new(&cli_address, true).await.unwrap();
                    cli_caller
                        .call_set_timezone("UTC".to_string())
                        .await
                        .unwrap();

                    let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                    let msg_id = q.message.as_ref().unwrap().id;

                    let kb = make_keyboard_main();

                    bot.send_message(chat_id, "Timezone set to UTC")
                        .reply_markup(kb)
                        .await?;

                    last_dialog_id.store(0, Ordering::Relaxed);
                    bot.delete_message(chat_id, msg_id).await?;
                } else {
                    let kb = make_timezone_option_keyboard(region, 1).unwrap();

                    let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                    let msg_id = q.message.as_ref().unwrap().id;

                    let message = escape(
                        format!(
                            "👻 Timezone Updater 👻\n\nSelected region: {}\nPlease select a City.",
                            region
                        )
                        .as_str(),
                    );

                    bot.edit_message_text(chat_id, msg_id, message)
                        .reply_markup(kb)
                        .await?;
                }
            }
            btn_press if btn_press.starts_with("tz_selection") => {
                let split_msg = btn_press.split(",").collect::<Vec<&str>>();
                let region: &str = split_msg[1];
                let city = split_msg[2];

                let tz = format!("{}/{}", region, city);

                let conf = gv_config.read().await;
                let cli_address = conf.to_owned().cli_address;
                drop(conf);

                let cli_caller = CLICaller::new(&cli_address, true).await.unwrap();
                cli_caller.call_set_timezone(tz.clone()).await.unwrap();

                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_keyboard_main();

                let message = escape(format!("Timezone set to {}", tz).as_str());

                bot.send_message(chat_id, message).reply_markup(kb).await?;

                last_dialog_id.store(0, Ordering::Relaxed);
                bot.delete_message(chat_id, msg_id).await?;
            }
            btn_press if btn_press.starts_with("tz_page_back") => {
                let split_msg = btn_press.split(",").collect::<Vec<&str>>();
                let region: &str = split_msg[1];

                let page = split_msg[2].parse::<u8>().unwrap();

                let kb = if page == 1 {
                    make_timezone_region_keyboard()
                } else {
                    make_timezone_option_keyboard(region, page - 1).unwrap()
                };

                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id: MessageId = q.message.as_ref().unwrap().id;

                let message = escape(
                    format!(
                        "👻 Timezone Updater 👻\n\nSelected region: {}\nPlease select a City.",
                        region
                    )
                    .as_str(),
                );

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }
            btn_press if btn_press.starts_with("tz_page_next") => {
                let split_msg = btn_press.split(",").collect::<Vec<&str>>();
                let region: &str = split_msg[1];

                let page = split_msg[2].parse::<u8>().unwrap();

                let kb = make_timezone_option_keyboard(region, page + 1).unwrap();

                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let message = escape(
                    format!(
                        "👻 Timezone Updater 👻\n\nSelected region: {}\nPlease select a City.",
                        region
                    )
                    .as_str(),
                );

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }

            btn_press if btn_press.starts_with("stake_chart_selection") => {
                let split_msg = btn_press.split(",").collect::<Vec<&str>>();
                let chart_type: &str = split_msg[1];
                let chart_range = split_msg[2];
                let conf = gv_config.read().await;
                let q_ctx = q.clone();

                let division = match chart_type {
                    "stakes_day_chart" => "day",
                    "stakes_week_chart" => "week",
                    "stakes_month_chart" => "month",
                    "earnings_chart" => "earnings",
                    _ => "day",
                };

                let time_zone = conf.to_owned().timezone;
                drop(conf);

                if chart_range == "custom_range" {
                    let chart_range_dialog = GetDateRangeDialog::new(
                        chart_range_mem.clone(),
                        q_ctx.message.as_ref().unwrap().chat.id,
                    );

                    start_chart_range_dialogue(
                        bot.clone(),
                        chart_range_dialog,
                        &q,
                        &time_zone,
                        division.to_string(),
                        chart_type.to_string(),
                    )
                    .await
                    .unwrap();
                    return Ok(());
                }

                let start_end = parse_chart_range(chart_range, &time_zone);

                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let _ = bot.delete_message(chat_id, msg_id).await?;

                if chart_type == "earnings_chart" {
                    send_earnings_chart(start_end, &bot, &q, gv_config).await?;
                } else {
                    send_barchart(start_end, &bot, &q, gv_config, division).await?;
                }
            }

            "tz_back" => {
                let kb = make_timezone_region_keyboard();

                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let message = escape("👻 Timezone Updater 👻\n\nPlease select your region.");

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }

            "back_to_stake_chart" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_inline_chart_menu();

                let message = escape("👻 Charts 👻\n\nPlease select a chart type");

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }

            "stake_chart" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_inline_stakes_chart_menu();

                let message = escape("👻 Stake Charts 👻\n\nPlease select a divisor");

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }

            "earnings_chart" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_inline_stake_chart_range_menu("earnings_chart".to_string());

                let message = escape("👻 Earnings Charts 👻\n\nPlease select a range");

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }

            "stakes_day_chart" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_inline_stake_chart_range_menu("stakes_day_chart".to_string());

                let message = escape("👻 Range Selection 👻\n\nPlease select a range");

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }
            "stakes_month_chart" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_inline_stake_chart_range_menu("stakes_month_chart".to_string());

                let message = escape("👻 Range Selection 👻\n\nPlease select a range");

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }
            "stakes_week_chart" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_inline_stake_chart_range_menu("stakes_week_chart".to_string());

                let message = escape("👻 Range Selection 👻\n\nPlease select a range");

                bot.edit_message_text(chat_id, msg_id, message)
                    .reply_markup(kb)
                    .await?;
            }

            "cancel_select_tz" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_keyboard_bot_settings();

                bot.send_message(chat_id, "Cancelled")
                    .reply_markup(kb)
                    .await?;

                last_dialog_id.store(0, Ordering::Relaxed);
                bot.delete_message(chat_id, msg_id).await?;
            }

            "cancel_select_chart" => {
                let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
                let msg_id = q.message.as_ref().unwrap().id;

                let kb = make_stats_info_keyboard();

                bot.send_message(chat_id, "Cancelled")
                    .reply_markup(kb)
                    .await?;

                last_dialog_id.store(0, Ordering::Relaxed);
                bot.delete_message(chat_id, msg_id).await?;

                let chart_range_dialog = GetDateRangeDialog::new(chart_range_mem.clone(), chat_id);

                if chart_range_dialog.get().await.unwrap().is_some() {
                    chart_range_dialog.exit().await.unwrap();
                }
            }

            " " | "\t\t" => {
                bot.answer_callback_query(q.id).await?;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn get_bot_settings(gv_config: &Arc<async_RwLock<GVConfig>>) -> String {
    let conf = gv_config.read().await;
    let stake_announce = if conf.announce_stakes {
        "Stake announcments: ✅\n"
    } else {
        "Stake announcments: ❌\n"
    };

    let reward_announce = if conf.announce_rewards {
        "Reward announcments: ✅\n"
    } else {
        "Reward announcments: ❌\n"
    };

    let zap_announce = if conf.announce_zaps {
        "Zap announcments: ✅\n"
    } else {
        "Zap announcments: ❌\n"
    };

    let timezone = conf.timezone.clone().to_uppercase();

    let reply = escape(
        format!(
            "Bot Settings\n\n{}{}{}\nTimezone: {}",
            stake_announce, reward_announce, zap_announce, timezone
        )
        .as_str(),
    );

    reply
}

pub fn server_unready_message(server_ready: &ServerReadyDB) -> String {
    let reason = server_ready.reason.clone().unwrap_or("".to_string());
    let res = match reason.as_str() {
        "Daemon offline" => escape("Ghostd is offline. Please wait for it to start."),
        "Daemon update in progress" => {
            escape("Ghostd update in progress. Please wait for it to finish before making changes.")
        }
        "Importing Wallet" => escape("Importing wallet. Please wait for it to finish."),
        "Forcing resync" => {
            escape("Forcing resync. Please wait for it to finish before making changes.")
        }
        _ => escape("Unknown error! Please try again later."),
    };

    res
}

async fn reply_status(
    bot: &DefaultParseMode<Bot>,
    msg: &Message,
    gv_config: &Arc<async_RwLock<GVConfig>>,
) -> ResponseResult<Message> {
    let conf = gv_config.read().await;
    let cli_address = conf.to_owned().cli_address;
    drop(conf);

    let cli_caller: CLICaller = CLICaller::new(&cli_address, true).await.unwrap();
    let cli_resp: Value = cli_caller.call_get_daemon_state().await.unwrap();
    let status: GVStatus = serde_json::from_value(cli_resp.clone()).unwrap();
    let pretty_string = serde_json::to_string_pretty(&status).unwrap();
    let reply_escaped = escape(format!("{}", pretty_string).as_str());
    let header: String = escape(format!("👻 GhostVault Status 👻").as_str());
    let reply: String = format!("{}\n\n```\n{}\n```", header, reply_escaped);
    let msg: Message = bot.send_message(msg.chat.id, reply).await?;

    Ok(msg)
}

async fn send_barchart(
    start_end: (u64, u64),
    bot: &DefaultParseMode<Bot>,
    q: &CallbackQuery,
    gv_config: Arc<async_RwLock<GVConfig>>,
    division: &str,
) -> ResponseResult<()> {
    let kb = InlineKeyboardMarkup::default();

    let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
    let conf = gv_config.read().await;

    let cli_caller = CLICaller::new(&conf.cli_address, true).await.unwrap();

    let cli_res = cli_caller
        .call_get_stake_barchart_data(start_end.0, start_end.1, division.to_string())
        .await
        .unwrap();

    let bc_data: BarChart = serde_json::from_value(cli_res.to_owned()).unwrap();
    let data = bc_data.data;

    if data.is_empty() {
        let message = escape("No data available for the selected range");
        let kb = make_stats_info_keyboard();

        bot.send_message(chat_id, message).reply_markup(kb).await?;
        return Ok(());
    }

    let mk_chart = make_barchart(&cli_res);

    if mk_chart.is_err() {
        let message = escape("No data available for the selected range");

        bot.send_message(chat_id, message).await?;
    } else {
        let _ = mk_chart.unwrap();

        let chart_path = PathBuf::from("/tmp/barchart.png");

        if !chart_path.exists() {
            let message = escape("Error generating chart. Please try again later.");

            bot.send_message(chat_id, message).await?;
        } else {
            let chart_file = InputFile::file(chart_path.clone());

            let message = escape("👻 Stake Chart 👻");

            bot.send_photo(chat_id, chart_file)
                .caption(message)
                .reply_markup(kb)
                .await?;

            file_ops::rm_file(&chart_path).unwrap();
        }
    }

    Ok(())
}

async fn send_earnings_chart(
    start_end: (u64, u64),
    bot: &DefaultParseMode<Bot>,
    q: &CallbackQuery,
    gv_config: Arc<async_RwLock<GVConfig>>,
) -> ResponseResult<()> {
    let kb = InlineKeyboardMarkup::default();

    let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
    let conf = gv_config.read().await;

    let cli_caller = CLICaller::new(&conf.cli_address, true).await.unwrap();

    let chart_data = cli_caller
        .call_get_earnings_chart_data(start_end.0, start_end.1)
        .await
        .unwrap();

    let mk_chart = make_area_chart(&chart_data);

    if mk_chart.is_err() {
        let message = escape("No data available for the selected range");

        bot.send_message(chat_id, message).await?;
    } else {
        let _ = mk_chart.unwrap();

        let chart_path = PathBuf::from("/tmp/total_earnings_chart.png");

        if !chart_path.exists() {
            let message = escape("Error generating chart. Please try again later.");

            bot.send_message(chat_id, message).await?;
        } else {
            let chart_file = InputFile::file(chart_path.clone());

            let message = escape("👻 Earnings Chart 👻");

            bot.send_photo(chat_id, chart_file)
                .caption(message)
                .reply_markup(kb)
                .await?;

            file_ops::rm_file(&chart_path).unwrap();
        }
    }

    Ok(())
}

pub async fn run_tg_bot(config_clone_tg_bot: Arc<async_RwLock<GVConfig>>, db: Arc<GVDB>) {
    let bot_conf: Arc<async_RwLock<GVConfig>> = Arc::clone(&config_clone_tg_bot);
    let conf = config_clone_tg_bot.read().await;
    let bot_token: Option<String> = conf.bot_token.clone();
    drop(conf);
    env::set_var("TELOXIDE_TOKEN", bot_token.as_ref().unwrap());
    info!("Starting Telegram bot...");
    let bot: DefaultParseMode<Bot> = Bot::from_env().parse_mode(ParseMode::MarkdownV2);

    let commands_db: Arc<GVDB> = Arc::clone(&db);
    let bot_runner_db: Arc<GVDB> = Arc::clone(&db);

    let bot_runner: BotRunner =
        BotRunner::new(&config_clone_tg_bot, bot_runner_db, bot.clone()).await;

    // Spawn the bot background task
    tokio::spawn(async move {
        bot_runner.background_task().await;
    });

    let last_dialog_id: Arc<AtomicI32> = Arc::new(AtomicI32::new(0));

    // Start the command handling REPL

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(
            |bot: DefaultParseMode<Bot>,
             gv_config: Arc<async_RwLock<GVConfig>>,
             db: Arc<GVDB>,
             msg: Message,
             last_dialog_id: Arc<AtomicI32>,
             reward_mode_mem: Arc<InMemStorage<UpdateRewardModeState>>,
             reward_interval_mem: Arc<InMemStorage<UpdateRewardIntervalState>>,
             reward_min_mem: Arc<InMemStorage<UpdateRewardMinState>>,
             chart_range_mem: Arc<InMemStorage<GetDateRangeState>>| async move {
                command_handler(
                    bot,
                    msg,
                    gv_config,
                    db,
                    reward_mode_mem,
                    last_dialog_id,
                    reward_interval_mem,
                    reward_min_mem,
                    chart_range_mem,
                )
                .await?;
                respond(())
            },
        ))
        .branch(Update::filter_callback_query().endpoint(
            |bot: DefaultParseMode<Bot>,
             gv_config: Arc<async_RwLock<GVConfig>>,
             db: Arc<GVDB>,
             callback_query: CallbackQuery,
             last_dialog_id: Arc<AtomicI32>,
             reward_mode_mem: Arc<InMemStorage<UpdateRewardModeState>>,
             reward_interval_mem: Arc<InMemStorage<UpdateRewardIntervalState>>,
             reward_min_mem: Arc<InMemStorage<UpdateRewardMinState>>,
             chart_range_mem: Arc<InMemStorage<GetDateRangeState>>| async move {
                callback_handler(
                    bot,
                    callback_query,
                    gv_config,
                    db,
                    reward_mode_mem,
                    last_dialog_id,
                    reward_interval_mem,
                    reward_min_mem,
                    chart_range_mem,
                )
                .await?;
                respond(())
            },
        ));

    let reward_mode_mem: Arc<InMemStorage<UpdateRewardModeState>> =
        InMemStorage::<UpdateRewardModeState>::new();
    let reward_interval_mem: Arc<InMemStorage<UpdateRewardIntervalState>> =
        InMemStorage::<UpdateRewardIntervalState>::new();
    let reward_min_mem: Arc<InMemStorage<UpdateRewardMinState>> =
        InMemStorage::<UpdateRewardMinState>::new();
    let chart_range_mem: Arc<InMemStorage<GetDateRangeState>> =
        InMemStorage::<GetDateRangeState>::new();

    Dispatcher::builder(bot.clone(), handler)
        // Pass the shared state to the handler as a dependency.
        .dependencies(dptree::deps![
            bot_conf,
            commands_db,
            reward_mode_mem,
            last_dialog_id,
            reward_interval_mem,
            reward_min_mem,
            chart_range_mem
        ])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
