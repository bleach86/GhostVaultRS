use crate::tg_bot::{
    dialogs::utils::{GetDateRangeDialog, GetDateRangeState, HandlerResult},
    keyboards::make_inline_calander,
};
use chrono::{Datelike, Utc};
use teloxide::{adaptors::DefaultParseMode, prelude::*, utils::markdown::escape};

pub async fn start_chart_range_dialogue(
    bot: DefaultParseMode<Bot>,
    dialogue: GetDateRangeDialog,
    q: &CallbackQuery,
    time_zone: &str,
    division: String,
    chart_type: String,
) -> HandlerResult {
    let now = Utc::now();
    let kb = make_inline_calander(now.year(), now.month(), &time_zone);

    let message = escape("👻 Custom Range Selection 👻\n\nPlease select a start date");

    let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
    let msg_id = q.message.as_ref().unwrap().id;

    bot.edit_message_text(chat_id, msg_id, message)
        .reply_markup(kb)
        .await?;

    dialogue
        .update(GetDateRangeState::ReceiveFirstDate {
            division,
            time_zone: time_zone.to_string(),
            chart_type,
        })
        .await?;

    Ok(())
}

pub async fn receive_first_date(
    bot: DefaultParseMode<Bot>,
    dialogue: GetDateRangeDialog,
    q: &CallbackQuery,
    first_date: u64,
    division: String,
    time_zone: &str,
    chart_type: String,
) -> HandlerResult {
    let now = Utc::now();
    let kb = make_inline_calander(now.year(), now.month(), &time_zone);

    let message = escape("👻 Custom Range Selection 👻\n\nPlease select an end date");

    let chat_id: ChatId = q.message.as_ref().unwrap().chat.id;
    let msg_id = q.message.as_ref().unwrap().id;

    bot.edit_message_text(chat_id, msg_id, message)
        .reply_markup(kb)
        .await?;

    dialogue
        .update(GetDateRangeState::ReceiveSecondDate {
            first_date,
            division,
            time_zone: time_zone.to_string(),
            chart_type,
        })
        .await?;

    Ok(())
}
