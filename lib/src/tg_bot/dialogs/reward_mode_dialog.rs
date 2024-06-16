use crate::{
    gv_client_methods::CLICaller,
    gvdb::{AddressInfo, ServerReadyDB, GVDB},
    tg_bot::{
        dialogs::utils::{HandlerResult, UpdateRewardModeDialog, UpdateRewardModeState},
        keyboards::{make_inline_cancel_button, make_keyboard_gv_options, make_keyboard_main},
        tg_bot::server_unready_message,
    },
};
use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};
use teloxide::{
    adaptors::DefaultParseMode,
    dispatching::dialogue::InMemStorage,
    payloads::SendMessageSetters,
    prelude::*,
    types::{InlineKeyboardMarkup, MessageId},
    utils::markdown::escape,
};

pub async fn reward_mode_dialogue_handler(
    bot: DefaultParseMode<Bot>,
    msg: Message,
    reward_mode_mem: Arc<InMemStorage<UpdateRewardModeState>>,
    last_dialog_id: Arc<AtomicI32>,
    reward_update_dialogue: Dialogue<UpdateRewardModeState, InMemStorage<UpdateRewardModeState>>,
    cli_caller: &CLICaller,
    db: &Arc<GVDB>,
) -> ResponseResult<()> {
    let server_ready: ServerReadyDB = db.get_server_ready().unwrap();

    if !server_ready.daemon_ready || !server_ready.ready {
        let reason = server_unready_message(&server_ready);

        let message = escape("Ghost daemon unavailable.\nReason:");
        let reasoned_message = format!("{}{}", message, reason);

        let keyboard = make_keyboard_main();

        bot.send_message(msg.chat.id, reasoned_message)
            .reply_markup(keyboard)
            .await?;
        reward_update_dialogue.exit().await.unwrap();

        let last_id = last_dialog_id.load(Ordering::Relaxed);

        if last_id != 0 {
            bot.delete_message(msg.chat.id, MessageId(last_id)).await?;
            last_dialog_id.store(0, Ordering::Relaxed);
        }

        return Ok(());
    }

    let reward_update_state = reward_update_dialogue.get().await.unwrap();

    match reward_update_state {
        Some(UpdateRewardModeState::Start) => {
            let dialogue: Dialogue<UpdateRewardModeState, InMemStorage<UpdateRewardModeState>> =
                UpdateRewardModeDialog::new(reward_mode_mem, msg.chat.id);

            start_update_reward_mode(bot.clone(), dialogue, msg.clone(), last_dialog_id.clone())
                .await
                .unwrap();
        }
        Some(UpdateRewardModeState::ReceiveRewardMode) => {
            receive_reward_mode(
                bot.clone(),
                reward_update_dialogue,
                msg.clone(),
                last_dialog_id.clone(),
                &cli_caller,
            )
            .await
            .unwrap();
        }
        Some(UpdateRewardModeState::ReceiveAddress { reward_mode }) => {
            receive_address(
                bot.clone(),
                reward_update_dialogue,
                reward_mode,
                msg.clone(),
                last_dialog_id.clone(),
                &cli_caller,
            )
            .await
            .unwrap();
        }
        _ => {}
    }

    return Ok(());
}

pub async fn start_update_reward_mode(
    bot: DefaultParseMode<Bot>,
    dialogue: UpdateRewardModeDialog,
    msg: Message,
    last_dialog_id: Arc<AtomicI32>,
) -> HandlerResult {
    let confirm_markup = make_inline_cancel_button("cancel_update_reward_mode");

    let message = escape(concat!(
        "reward_mode can be one of the following:\n\n",
        "DEFAULT:\nRewards are sent to the original address. This bypasses GhostVault. Rewards are auto zapped in this mode.\n\n",
        "STANDARD:\nRewards are sent to any valid Ghost address of your choosing. This bypasses GhostVault. Rewards are NOT auto zapped in this mode.\n\n",
        "ANON:\nRewards are sent to any valid Ghost address of your choosing. This mode sends ALL rewards through GhostVault's internal anon address.\n",
        "Rewards are auto zapped in this mode if a 256bit address (one that starts with a 2) is provided.\n\n",
        "Please choose a reward mode."
    ));
    let new_msg = bot
        .send_message(msg.chat.id, message)
        .reply_markup(confirm_markup)
        .await?;

    let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
    last_dialog_id.store(new_id, Ordering::Relaxed);

    dialogue
        .update(UpdateRewardModeState::ReceiveRewardMode)
        .await?;

    Ok(())
}

pub async fn receive_reward_mode(
    bot: DefaultParseMode<Bot>,
    dialogue: UpdateRewardModeDialog,
    msg: Message,
    last_dialog_id: Arc<AtomicI32>,
    cli_caller: &CLICaller,
) -> HandlerResult {
    let empty_keyboard = InlineKeyboardMarkup::default();
    let last_msg_id = last_dialog_id.load(Ordering::Relaxed);

    // Edit the message reply markup with the empty keyboard

    let _ = bot
        .edit_message_reply_markup(msg.chat.id, MessageId(last_msg_id))
        .reply_markup(empty_keyboard)
        .await;

    let confirm_markup = make_inline_cancel_button("cancel_update_reward_mode");

    match msg.text() {
        Some(text) => match text.to_uppercase().as_str() {
            "DEFAULT" => {
                let cli_res = cli_caller
                    .call_set_reward_mode("DEFAULT".to_string(), None)
                    .await
                    .unwrap();

                if cli_res.as_str().unwrap() == "Reward mode updated!" {
                    let keyboard = make_keyboard_gv_options();
                    let message = escape("Reward mode updated to DEFAULT.");
                    bot.send_message(msg.chat.id, message)
                        .reply_markup(keyboard)
                        .await?;
                    dialogue.exit().await.unwrap();
                    last_dialog_id.store(0, Ordering::Relaxed);
                } else {
                    let cancel_markup = make_inline_cancel_button("cancel_update_reward_mode");
                    let message = escape("Failed to update reward mode.");
                    let _new_msg = bot
                        .send_message(msg.chat.id, message)
                        .reply_markup(cancel_markup)
                        .await?;

                    dialogue.exit().await.unwrap();
                    last_dialog_id.store(0, Ordering::Relaxed);
                };
            }
            "STANDARD" | "ANON" => {
                let cancel_markup = make_inline_cancel_button("cancel_update_reward_mode");
                let message = escape("Please provide your reward address.");
                let new_msg = bot
                    .send_message(msg.chat.id, message)
                    .reply_markup(cancel_markup)
                    .await?;

                let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
                last_dialog_id.store(new_id, Ordering::Relaxed);

                dialogue
                    .update(UpdateRewardModeState::ReceiveAddress {
                        reward_mode: text.into(),
                    })
                    .await?;
            }
            _ => {
                let cancel_markup = make_inline_cancel_button("cancel_update_reward_mode");
                let message = escape("Invalid reward mode. Please choose a valid reward mode.");
                let new_msg = bot
                    .send_message(msg.chat.id, message)
                    .reply_markup(cancel_markup)
                    .await?;

                let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
                last_dialog_id.store(new_id, Ordering::Relaxed);
            }
        },
        None => {
            let message = escape("Send me plain text.");
            bot.send_message(msg.chat.id, message)
                .reply_markup(confirm_markup)
                .await?;
        }
    }

    Ok(())
}

pub async fn receive_address(
    bot: DefaultParseMode<Bot>,
    dialogue: UpdateRewardModeDialog,
    reward_mode: String,
    msg: Message,
    last_dialog_id: Arc<AtomicI32>,
    cli_caller: &CLICaller,
) -> HandlerResult {
    let empty_keyboard = InlineKeyboardMarkup::default();
    let last_msg_id = last_dialog_id.load(Ordering::Relaxed);

    // Edit the message reply markup with the empty keyboard
    let _ = bot
        .edit_message_reply_markup(msg.chat.id, MessageId(last_msg_id))
        .reply_markup(empty_keyboard)
        .await;

    let addr = msg.text().unwrap();

    let cli_res = cli_caller
        .call_validate_address(addr.to_string())
        .await
        .unwrap();

    let addr_info: AddressInfo = serde_json::from_value(cli_res).unwrap();

    if !addr_info.is_valid {
        let cancel_markup = make_inline_cancel_button("cancel_update_reward_mode");
        let message = escape("Invalid address. Please provide a valid address.");
        let new_msg = bot
            .send_message(msg.chat.id, message)
            .reply_markup(cancel_markup)
            .await?;

        let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
        last_dialog_id.store(new_id, Ordering::Relaxed);
    } else if addr_info.is_mine {
        let cancel_markup = make_inline_cancel_button("cancel_update_reward_mode");
        let message = escape("Address belongs to GhostVault! Please provide a different address.");
        let new_msg = bot
            .send_message(msg.chat.id, message)
            .reply_markup(cancel_markup)
            .await?;

        let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
        last_dialog_id.store(new_id, Ordering::Relaxed);
    } else {
        let new_mode = reward_mode.clone().to_uppercase();
        let cli_res = cli_caller
            .call_set_reward_mode(new_mode, Some(addr.to_string()))
            .await
            .unwrap();

        if cli_res.as_str().unwrap() == "Reward mode updated!" {
            let keyboard = make_keyboard_gv_options();
            let new_mode = reward_mode.clone().to_uppercase();

            let auto_zap = if new_mode != "STANDARD" && addr_info.is_256bit {
                "\n256bit address detected. Rewards will be auto zapped."
            } else {
                ""
            };

            let message =
                escape(format!("Reward mode updated to {}.{}", new_mode, auto_zap).as_str());
            bot.send_message(msg.chat.id, message)
                .reply_markup(keyboard)
                .await?;
            dialogue.exit().await.unwrap();
            last_dialog_id.store(0, Ordering::Relaxed);
        } else {
            let keyboard = make_keyboard_gv_options();
            let message = escape("Failed to update reward mode.");
            let _new_msg = bot
                .send_message(msg.chat.id, message)
                .reply_markup(keyboard)
                .await?;

            dialogue.exit().await.unwrap();
            last_dialog_id.store(0, Ordering::Relaxed);
        };
    }

    Ok(())
}
