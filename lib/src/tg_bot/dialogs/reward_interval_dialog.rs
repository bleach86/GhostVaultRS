use crate::{
    gv_client_methods::CLICaller,
    gvdb::{ServerReadyDB, GVDB},
    tg_bot::{
        dialogs::utils::{HandlerResult, UpdateRewardIntervalDialog, UpdateRewardIntervalState},
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

pub async fn reward_interval_dialogue_handler(
    bot: DefaultParseMode<Bot>,
    msg: Message,
    reward_interval_mem: Arc<InMemStorage<UpdateRewardIntervalState>>,
    last_dialog_id: Arc<AtomicI32>,
    reward_interval_dialogue: Dialogue<
        UpdateRewardIntervalState,
        InMemStorage<UpdateRewardIntervalState>,
    >,
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
        reward_interval_dialogue.exit().await.unwrap();

        let last_id = last_dialog_id.load(Ordering::Relaxed);

        if last_id != 0 {
            bot.delete_message(msg.chat.id, MessageId(last_id)).await?;
            last_dialog_id.store(0, Ordering::Relaxed);
        }
        return Ok(());
    }

    let reward_interval_state = reward_interval_dialogue.get().await.unwrap();

    match reward_interval_state {
        Some(UpdateRewardIntervalState::Start) => {
            let dialogue: Dialogue<
                UpdateRewardIntervalState,
                InMemStorage<UpdateRewardIntervalState>,
            > = UpdateRewardIntervalDialog::new(reward_interval_mem, msg.chat.id);

            start_update_reward_interval(
                bot.clone(),
                dialogue,
                msg.clone(),
                last_dialog_id.clone(),
            )
            .await
            .unwrap();
        }
        Some(UpdateRewardIntervalState::ReceiveIntervalMultiplier) => {
            receive_interval_multiplier(
                bot.clone(),
                reward_interval_dialogue,
                msg.clone(),
                last_dialog_id.clone(),
            )
            .await
            .unwrap();
        }
        Some(UpdateRewardIntervalState::ReceiveInterval {
            interval_multiplier,
        }) => {
            receive_interval(
                bot.clone(),
                reward_interval_dialogue,
                interval_multiplier,
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

pub async fn start_update_reward_interval(
    bot: DefaultParseMode<Bot>,
    dialogue: UpdateRewardIntervalDialog,
    msg: Message,
    last_dialog_id: Arc<AtomicI32>,
) -> HandlerResult {
    let confirm_markup = make_inline_cancel_button("cancel_update_reward_interval");

    let message = escape(concat!(
        "The reward interval sets how often GhostVault will check if rewards can be sent.\n\n",
        "The default and minimum interval is 15 minutes.\n",
        "Please choose an interval multiplier."
    ));
    let new_msg = bot
        .send_message(msg.chat.id, message)
        .reply_markup(confirm_markup)
        .await?;

    let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
    last_dialog_id.store(new_id, Ordering::Relaxed);

    dialogue
        .update(UpdateRewardIntervalState::ReceiveIntervalMultiplier)
        .await?;

    Ok(())
}

pub async fn receive_interval_multiplier(
    bot: DefaultParseMode<Bot>,
    dialogue: UpdateRewardIntervalDialog,
    msg: Message,
    last_dialog_id: Arc<AtomicI32>,
) -> HandlerResult {
    let empty_keyboard = InlineKeyboardMarkup::default();
    let last_msg_id = last_dialog_id.load(Ordering::Relaxed);

    // Edit the message reply markup with the empty keyboard
    let _ = bot
        .edit_message_reply_markup(msg.chat.id, MessageId(last_msg_id))
        .reply_markup(empty_keyboard)
        .await;

    let confirm_markup = make_inline_cancel_button("cancel_update_reward_interval");

    let multiplier = match msg.text().unwrap() {
        "MINUTE" => "m".to_string(),
        "HOUR" => "h".to_string(),
        "DAY" => "d".to_string(),
        "WEEK" => "w".to_string(),
        "MONTH" => "M".to_string(),
        "YEAR" => "y".to_string(),
        _ => {
            let message =
                escape("Invalid interval multiplier. Please choose a valid interval multiplier.");
            let new_msg = bot
                .send_message(msg.chat.id, message)
                .reply_markup(confirm_markup)
                .await?;

            let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
            last_dialog_id.store(new_id, Ordering::Relaxed);
            return Ok(());
        }
    };

    let message: String = escape(
        format!(
            "Please enter the number of {}S between payment runs.",
            msg.text().unwrap()
        )
        .as_str(),
    );

    let new_msg: Message = bot
        .send_message(msg.chat.id, message)
        .reply_markup(confirm_markup)
        .await?;

    let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
    last_dialog_id.store(new_id, Ordering::Relaxed);

    dialogue
        .update(UpdateRewardIntervalState::ReceiveInterval {
            interval_multiplier: multiplier,
        })
        .await?;

    Ok(())
}

pub async fn receive_interval(
    bot: DefaultParseMode<Bot>,
    dialogue: UpdateRewardIntervalDialog,
    interval_multiplier: String,
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

    let confirm_markup = make_inline_cancel_button("cancel_update_reward_interval");

    let interval = msg.text().unwrap().parse::<i32>();

    if interval.is_err() {
        let message = escape("Invalid interval. Please send a valid number.");
        let new_msg = bot
            .send_message(msg.chat.id, message)
            .reply_markup(confirm_markup)
            .await?;

        let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
        last_dialog_id.store(new_id, Ordering::Relaxed);
        return Ok(());
    }

    match interval_multiplier.as_str() {
        "m" => {
            if interval.unwrap() < 15 {
                let message = escape("Minimum interval is 15 minutes.");
                let new_msg = bot
                    .send_message(msg.chat.id, message)
                    .reply_markup(confirm_markup)
                    .await?;

                let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
                last_dialog_id.store(new_id, Ordering::Relaxed);
                return Ok(());
            }
        }
        "h" | "d" | "w" | "M" | "y" => {
            if interval.unwrap() < 1 {
                let message =
                    escape(format!("Minimum interval is 1{}.", interval_multiplier).as_str());
                let new_msg = bot
                    .send_message(msg.chat.id, message)
                    .reply_markup(confirm_markup)
                    .await?;

                let new_id: i32 = new_msg.id.to_string().parse::<i32>().unwrap();
                last_dialog_id.store(new_id, Ordering::Relaxed);
                return Ok(());
            }
        }
        _ => {}
    }

    let interval = format!("{}{}", msg.text().unwrap(), interval_multiplier);

    let cli_res = cli_caller.call_set_reward_interval(interval).await.unwrap();

    if cli_res.as_str().unwrap() == "Reward interval updated!" {
        let keyboard = make_keyboard_gv_options();
        let message = escape("Reward interval updated.");
        bot.send_message(msg.chat.id, message)
            .reply_markup(keyboard)
            .await?;
        dialogue.exit().await.unwrap();
        last_dialog_id.store(0, Ordering::Relaxed);
    } else {
        let keyboard = make_keyboard_gv_options();
        let message = escape("Failed to update reward interval.");
        let _new_msg = bot
            .send_message(msg.chat.id, message)
            .reply_markup(keyboard)
            .await?;

        dialogue.exit().await.unwrap();
        last_dialog_id.store(0, Ordering::Relaxed);
    }

    Ok(())
}
