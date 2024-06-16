use crate::{
    gv_client_methods::CLICaller,
    gvdb::{ServerReadyDB, GVDB},
    tg_bot::{
        dialogs::utils::{HandlerResult, UpdateRewardMinDialog, UpdateRewardMinState},
        keyboards::{make_inline_cancel_button, make_keyboard_gv_options, make_keyboard_main},
        tg_bot::server_unready_message,
    },
};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};
use teloxide::{
    adaptors::DefaultParseMode,
    dispatching::dialogue::InMemStorage,
    payloads::SendMessageSetters,
    prelude::*,
    types::{InlineKeyboardMarkup, KeyboardMarkup, MessageId},
    utils::markdown::escape,
};

pub async fn reward_min_dialogue_handler(
    bot: DefaultParseMode<Bot>,
    msg: Message,
    reward_min_mem: Arc<InMemStorage<UpdateRewardMinState>>,
    last_dialog_id: Arc<AtomicI32>,
    reward_min_dialogue: Dialogue<UpdateRewardMinState, InMemStorage<UpdateRewardMinState>>,
    cli_caller: &CLICaller,
    db: &Arc<GVDB>,
) -> ResponseResult<()> {
    let server_ready: ServerReadyDB = db.get_server_ready().unwrap();

    if !server_ready.daemon_ready || !server_ready.ready {
        let reason: String = server_unready_message(&server_ready);

        let message: String = escape("Ghost daemon unavailable.\nReason:");

        let reasoned_message: String = format!("{}{}", message, reason);
        let keyboard: KeyboardMarkup = make_keyboard_main();

        bot.send_message(msg.chat.id, reasoned_message)
            .reply_markup(keyboard)
            .await?;
        reward_min_dialogue.exit().await.unwrap();

        let last_id = last_dialog_id.load(Ordering::Relaxed);

        if last_id != 0 {
            bot.delete_message(msg.chat.id, MessageId(last_id)).await?;
            last_dialog_id.store(0, Ordering::Relaxed);
        }
        return Ok(());
    }
    let reward_min_state = reward_min_dialogue.get().await.unwrap();

    match reward_min_state {
        Some(UpdateRewardMinState::Start) => {
            let dialogue: Dialogue<UpdateRewardMinState, InMemStorage<UpdateRewardMinState>> =
                UpdateRewardMinDialog::new(reward_min_mem, msg.chat.id);

            start_update_reward_min(bot.clone(), dialogue, msg.clone(), last_dialog_id.clone())
                .await
                .unwrap();
        }
        Some(UpdateRewardMinState::ReceiveMinimum) => {
            receive_minimum(
                bot.clone(),
                reward_min_dialogue,
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

pub async fn start_update_reward_min(
    bot: DefaultParseMode<Bot>,
    dialogue: UpdateRewardMinDialog,
    msg: Message,
    last_dialog_id: Arc<AtomicI32>,
) -> HandlerResult {
    let confirm_markup = make_inline_cancel_button("cancel_update_reward_min");

    let message = escape(concat!(
        "The reward minimum sets the minimum amount of rewards that must be available before they are sent.\n\n",
        "The default minimum is 0.1 GHOST.\n",
        "Please enter the minimum amount of rewards."
    ));
    let new_msg = bot
        .send_message(msg.chat.id, message)
        .reply_markup(confirm_markup)
        .await?;

    let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
    last_dialog_id.store(new_id, Ordering::Relaxed);

    dialogue
        .update(UpdateRewardMinState::ReceiveMinimum)
        .await?;

    Ok(())
}

pub async fn receive_minimum(
    bot: DefaultParseMode<Bot>,
    dialogue: UpdateRewardMinDialog,
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

    let confirm_markup = make_inline_cancel_button("cancel_update_reward_min");

    let min = msg.text().unwrap().parse::<f64>();

    if min.is_err() {
        let message = escape("Invalid minimum. Please send a valid number.");
        let new_msg = bot
            .send_message(msg.chat.id, message)
            .reply_markup(confirm_markup)
            .await?;

        let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
        last_dialog_id.store(new_id, Ordering::Relaxed);
        return Ok(());
    }

    let min: f64 = min.unwrap();

    if min < 0.1 {
        let message = escape("Minimum reward must be at least 0.1 GHOST.");
        let new_msg = bot
            .send_message(msg.chat.id, message)
            .reply_markup(confirm_markup)
            .await?;

        let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
        last_dialog_id.store(new_id, Ordering::Relaxed);
        return Ok(());
    }

    let cli_res: Value = cli_caller.call_set_payout_min(min).await.unwrap();
    let res_str: &str = cli_res.as_str().unwrap();

    if res_str != "Minimum payout updated!" {
        let message = escape(format!("Error updating minimum reward: {}", res_str).as_str());
        let _new_msg = bot
            .send_message(msg.chat.id, message)
            .reply_markup(confirm_markup)
            .await?;
        last_dialog_id.store(0, Ordering::Relaxed);
        return Ok(());
    }

    let keyboard: KeyboardMarkup = make_keyboard_gv_options();

    let message: String = escape(format!("Minimum reward set to: {} GHOST", min).as_str());
    let _new_msg: Message = bot
        .send_message(msg.chat.id, message)
        .reply_markup(keyboard)
        .await?;

    last_dialog_id.store(0, Ordering::Relaxed);

    dialogue.exit().await?;

    Ok(())
}
