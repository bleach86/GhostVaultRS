use crate::tg_bot::dialogs::utils;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};
use url::Url;

pub fn make_keyboard_main() -> KeyboardMarkup {
    let status_button = KeyboardButton::new("\u{2139}\u{FE0F} Status".to_string());
    let stats_button = KeyboardButton::new("\u{1F4CA} Stats".to_string());
    let bot_settings_button = KeyboardButton::new("\u{2699}\u{FE0F} Bot Settings".to_string());
    let gv_settings_button = KeyboardButton::new("\u{2699}\u{FE0F} GhostVault Options".to_string());

    let ghost_links_button = KeyboardButton::new("\u{1F47B} Ghost Links".to_string());
    let gv_help_button = KeyboardButton::new("\u{2753} Help".to_string());

    // Create keyboard markup
    let keys = KeyboardMarkup::new(vec![
        vec![status_button, stats_button],
        vec![bot_settings_button, gv_settings_button],
        vec![ghost_links_button, gv_help_button],
    ]);

    let keyboard = KeyboardMarkup::persistent(keys);
    let mut keyboard = keyboard.input_field_placeholder("Please choose an option".to_string());
    keyboard.resize_keyboard = Some(true);

    keyboard
}

pub fn make_keyboard_bot_settings() -> KeyboardMarkup {
    let stake_ann_button = KeyboardButton::new("\u{1F4B8} Toggle Stake".to_string());
    let reward_ann_button = KeyboardButton::new("\u{1F4B0} Toggle Reward".to_string());
    let zap_ann_button = KeyboardButton::new("\u{26A1} Toggle Zap".to_string());
    let timezone_button = KeyboardButton::new("\u{1F55B} Set Timezone".to_string());

    let home_button = KeyboardButton::new("\u{1F3E0} Home".to_string());

    // Create keyboard markup
    let keys = KeyboardMarkup::new(vec![
        vec![stake_ann_button, reward_ann_button],
        vec![zap_ann_button, timezone_button],
        vec![home_button],
    ]);

    let keyboard = KeyboardMarkup::persistent(keys);
    let mut keyboard = keyboard.input_field_placeholder("Please choose an option".to_string());
    keyboard.resize_keyboard = Some(true);

    keyboard
}

pub fn make_keyboard_gv_options() -> KeyboardMarkup {
    let ext_pubk_button = KeyboardButton::new("\u{2744}\u{FE0F} CS Key".to_string());
    let reward_button = KeyboardButton::new("\u{1F4B8} Reward Options".to_string());
    let version_button = KeyboardButton::new("\u{1F4CA} Version".to_string());
    let daemon_update_button = KeyboardButton::new("\u{1F6E0}\u{FE0F} Update ghostd".to_string());
    let resync_button = KeyboardButton::new("\u{1F501} Resync".to_string());
    let check_chain_button = KeyboardButton::new("\u{1F517} Check Chain".to_string());
    let recovery_button = KeyboardButton::new("\u{1F4E5} Recovery".to_string());

    let home_button = KeyboardButton::new("\u{1F3E0} Home".to_string());

    // Create keyboard markup
    let keys = KeyboardMarkup::new(vec![
        vec![ext_pubk_button, reward_button],
        vec![version_button, daemon_update_button],
        vec![resync_button, check_chain_button, recovery_button],
        vec![home_button],
    ]);

    let keyboard = KeyboardMarkup::persistent(keys);
    let mut keyboard = keyboard.input_field_placeholder("Please choose an option".to_string());
    keyboard.resize_keyboard = Some(true);

    keyboard
}

pub fn make_keyboard_reward_options() -> KeyboardMarkup {
    let reward_mode_button = KeyboardButton::new("\u{1F4B8} Set Reward Mode & Address".to_string());
    let payout_min_button = KeyboardButton::new("\u{1F4B0} Set Payout Min".to_string());
    let reward_interval_button = KeyboardButton::new("\u{1F4CA} Set Reward Interval".to_string());

    let gv_options_button = KeyboardButton::new("\u{2699}\u{FE0F} GhostVault Options".to_string());

    let home_button = KeyboardButton::new("\u{1F3E0} Home".to_string());

    // Create keyboard markup
    let keys = KeyboardMarkup::new(vec![
        vec![reward_mode_button],
        vec![reward_interval_button, payout_min_button],
        vec![gv_options_button, home_button],
    ]);

    let keyboard = KeyboardMarkup::persistent(keys);
    let mut keyboard = keyboard.input_field_placeholder("Please choose an option".to_string());
    keyboard.resize_keyboard = Some(true);

    keyboard
}

pub fn make_reward_mode_keyboard() -> KeyboardMarkup {
    let default_button = KeyboardButton::new("DEFAULT".to_string());
    let standard_button = KeyboardButton::new("STANDARD".to_string());
    let anon_button = KeyboardButton::new("ANON".to_string());

    // Create keyboard markup
    let keys = KeyboardMarkup::new(vec![
        vec![anon_button],
        vec![default_button, standard_button],
    ]);

    let keyboard = KeyboardMarkup::persistent(keys);
    let mut keyboard = keyboard.input_field_placeholder("Please choose an option".to_string());
    keyboard.resize_keyboard = Some(true);

    keyboard
}

pub fn make_reward_interval_keyboard() -> KeyboardMarkup {
    let miniute_button = KeyboardButton::new("MINUTE".to_string());
    let hour_button = KeyboardButton::new("HOUR".to_string());
    let day_button = KeyboardButton::new("DAY".to_string());
    let week_button = KeyboardButton::new("WEEK".to_string());
    let month_button = KeyboardButton::new("MONTH".to_string());
    let year_button = KeyboardButton::new("YEAR".to_string());

    // Create keyboard markup
    let keys = KeyboardMarkup::new(vec![
        vec![miniute_button, hour_button, day_button],
        vec![week_button, month_button, year_button],
    ]);

    let keyboard = KeyboardMarkup::persistent(keys);
    let mut keyboard = keyboard.input_field_placeholder("Please choose an option".to_string());
    keyboard.resize_keyboard = Some(true);

    keyboard
}

pub fn make_timezone_region_keyboard() -> InlineKeyboardMarkup {
    let json_data = utils::get_timezone_opts();

    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    let mut row: Vec<InlineKeyboardButton> = Vec::new();

    let utc_row = vec![InlineKeyboardButton::callback(
        "UTC",
        "tz_region_selection,UTC".to_string(),
    )];

    keyboard.push(utc_row);

    for (category, _) in json_data.as_object().unwrap() {
        let cat_callback = format!("tz_region_selection,{}", category);

        let button = InlineKeyboardButton::callback(category, cat_callback);
        row.push(button);

        if row.len() == 4 {
            keyboard.push(row);
            row = Vec::new();
        }
    }

    if !row.is_empty() {
        keyboard.push(row);
    }

    let cancel_row = vec![InlineKeyboardButton::callback(
        "Cancel",
        "cancel_select_tz".to_string(),
    )];

    keyboard.push(cancel_row);

    InlineKeyboardMarkup::new(keyboard)
}

pub fn make_timezone_option_keyboard(key: &str, page: u8) -> Option<InlineKeyboardMarkup> {
    let json_data = utils::get_timezone_opts();

    if let Some(timezones) = json_data.get(key) {
        let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();
        let mut row: Vec<InlineKeyboardButton> = Vec::new();

        let tz_page = format!("page-{}", page);
        let page_array = timezones.get(tz_page).unwrap().as_array().unwrap();

        for timezone in page_array {
            if let Some(timezone_str) = timezone.as_str() {
                let callback_data = format!("tz_selection,{},{}", key, timezone_str);
                let button = InlineKeyboardButton::callback(timezone_str, callback_data);
                row.push(button);

                if row.len() == 4 {
                    keyboard.push(row);
                    row = Vec::new();
                }
            }
        }

        if !row.is_empty() {
            keyboard.push(row);
        }

        let total_pages: u8 = timezones.get("total_pages").unwrap().as_u64().unwrap() as u8;

        let back_button: InlineKeyboardButton = if page == 1 {
            InlineKeyboardButton::callback("\u{2B05}\u{FE0F} Back", "tz_back".to_string())
        } else {
            let back_callback = format!("tz_page_back,{},{}", key, page);
            InlineKeyboardButton::callback("\u{2B05}\u{FE0F} Back", back_callback)
        };

        let next_button: InlineKeyboardButton = if page == total_pages {
            InlineKeyboardButton::callback("\t\t", " ".to_string())
        } else {
            let next_callback: String = format!("tz_page_next,{},{}", key, page);
            InlineKeyboardButton::callback("Next \u{27A1}\u{FE0F}", next_callback)
        };

        let nav_row: Vec<InlineKeyboardButton> = vec![
            back_button,
            InlineKeyboardButton::callback("Cancel", "cancel_select_tz".to_string()),
            next_button,
        ];

        keyboard.push(nav_row);

        Some(InlineKeyboardMarkup::new(keyboard))
    } else {
        None
    }
}

pub fn make_stats_info_keyboard() -> KeyboardMarkup {
    let overview_button = KeyboardButton::new("\u{1F4CB} Overview");
    let pending_rewards_button = KeyboardButton::new("\u{1F4B0} Pending Rewards");
    let charts_button = KeyboardButton::new("\u{1F4CA} Charts");

    let home_button = KeyboardButton::new("\u{1F3E0} Home");

    // Create keyboard markup
    let keys = KeyboardMarkup::new(vec![
        vec![overview_button, pending_rewards_button],
        vec![charts_button],
        vec![home_button],
    ]);

    let keyboard = KeyboardMarkup::persistent(keys);
    let mut keyboard = keyboard.input_field_placeholder("Please choose an option".to_string());
    keyboard.resize_keyboard = Some(true);

    keyboard
}

pub fn make_inline_stakes_chart_menu() -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    let week_button = InlineKeyboardButton::callback("Stakes/Week", "stakes_week_chart");
    let month_button = InlineKeyboardButton::callback("Stakes/Month", "stakes_month_chart");
    let day_button = InlineKeyboardButton::callback("Stakes/Day", "stakes_day_chart");

    let back_button = InlineKeyboardButton::callback("Back", "back_to_stake_chart");
    let cancel_button = InlineKeyboardButton::callback("Cancel", "cancel_select_chart");

    let row1 = vec![day_button, week_button, month_button];
    let row2 = vec![back_button, cancel_button];

    keyboard.push(row1);
    keyboard.push(row2);

    InlineKeyboardMarkup::new(keyboard)
}

pub fn make_inline_stake_chart_range_menu(chart_type: String) -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    let last_two_weeks_button = InlineKeyboardButton::callback(
        "Last 2 Weeks",
        format!("stake_chart_selection,{},last_two_weeks", chart_type),
    );
    let last_month_button = InlineKeyboardButton::callback(
        "Last Month",
        format!("stake_chart_selection,{},last_month", chart_type),
    );
    let last_three_months_button = InlineKeyboardButton::callback(
        "Last 3 Months",
        format!("stake_chart_selection,{},last_three_months", chart_type),
    );
    let last_six_months_button = InlineKeyboardButton::callback(
        "Last 6 Months",
        format!("stake_chart_selection,{},last_six_months", chart_type),
    );
    let year_to_date_button = InlineKeyboardButton::callback(
        "Year to Date",
        format!("stake_chart_selection,{},year_to_date", chart_type),
    );
    let last_year_button = InlineKeyboardButton::callback(
        "Last Year",
        format!("stake_chart_selection,{},last_year", chart_type),
    );
    let max_button =
        InlineKeyboardButton::callback("Max", format!("stake_chart_selection,{},max", chart_type));
    let custom_range_button = InlineKeyboardButton::callback(
        "Custom Range",
        format!("stake_chart_selection,{},custom_range", chart_type),
    );

    let back_button = match chart_type.as_str() {
        "earnings_chart" => InlineKeyboardButton::callback("Back", "back_to_stake_chart"),
        _ => InlineKeyboardButton::callback("Back", "stake_chart"),
    };
    let cancel_button = InlineKeyboardButton::callback("Cancel", "cancel_select_chart");

    let row1 = vec![
        last_two_weeks_button,
        last_month_button,
        last_three_months_button,
    ];
    let row2 = vec![
        last_six_months_button,
        year_to_date_button,
        last_year_button,
    ];
    let row3 = vec![max_button, custom_range_button];
    let row4 = vec![back_button, cancel_button];

    keyboard.push(row1);
    keyboard.push(row2);
    keyboard.push(row3);
    keyboard.push(row4);

    InlineKeyboardMarkup::new(keyboard)
}

pub fn make_inline_chart_menu() -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    let stakes_button = InlineKeyboardButton::callback("Stakes Over Time", "stake_chart");
    let earnings_button = InlineKeyboardButton::callback("Total Earnings", "earnings_chart");

    let cancel_button = InlineKeyboardButton::callback("Cancel", "cancel_select_chart");

    let row1 = vec![stakes_button, earnings_button];
    let row2 = vec![cancel_button];

    keyboard.push(row1);
    keyboard.push(row2);

    InlineKeyboardMarkup::new(keyboard)
}

pub fn make_inline_cancel_button(callback: &str) -> InlineKeyboardMarkup {
    let confirm_markup = InlineKeyboardMarkup::default()
        .append_row(vec![InlineKeyboardButton::callback("Cancel", callback)]);

    confirm_markup
}

pub fn make_inline_ghost_links_menu() -> InlineKeyboardMarkup {
    let website_link_button = InlineKeyboardButton::url(
        "Ghost Website",
        Url::parse("https://ghostprivacy.net").expect("Failed to parse URL"),
    );

    let main_tg_link_button = InlineKeyboardButton::url(
        "Main Telegram",
        Url::parse("https://t.me/ghostcoinbymcafee").expect("Failed to parse URL"),
    );

    let help_tg_link_button = InlineKeyboardButton::url(
        "Help Telegram",
        Url::parse("https://t.me/Ghosthelp").expect("Failed to parse URL"),
    );

    let ghost_pay_link_button = InlineKeyboardButton::url(
        "Ghost Pay",
        Url::parse("https://t.me/GhostPayBot").expect("Failed to parse URL"),
    );

    let russian_tg_link_button = InlineKeyboardButton::url(
        "Russian Telegram",
        Url::parse("https://t.me/ghost_ru2").expect("Failed to parse URL"),
    );

    let gs_link_button = InlineKeyboardButton::url(
        "Ghostscan Explorer",
        Url::parse("https://ghostscan.io").expect("Failed to parse URL"),
    );

    let myghost_explorer_link_button = InlineKeyboardButton::url(
        "MyGhost Explorer",
        Url::parse("https://explorer.myghost.org").expect("Failed to parse URL"),
    );

    let vet_list_link_button = InlineKeyboardButton::url(
        "Vet List",
        Url::parse("https://explorer.myghost.org/vetlist").expect("Failed to parse URL"),
    );

    let gh_repo_link_button = InlineKeyboardButton::url(
        "Ghost Github",
        Url::parse("https://github.com/ghost-coin/").expect("Failed to parse URL"),
    );

    let sheltr_link_button = InlineKeyboardButton::url(
        "SHELTR Wallet",
        Url::parse("https://app.sheltrwallet.com").expect("Failed to parse URL"),
    );

    let ghost_docs_link_button = InlineKeyboardButton::url(
        "Ghost Docs",
        Url::parse("https://ghostveterans.net").expect("Failed to parse URL"),
    );

    let secret_link_button = InlineKeyboardButton::url(
        "Ghost Secret",
        Url::parse("https://secret.ghostbyjohnmcafee.com").expect("Failed to parse URL"),
    );

    let keyboard = vec![
        vec![website_link_button],
        vec![main_tg_link_button, help_tg_link_button],
        vec![russian_tg_link_button, ghost_pay_link_button],
        vec![gs_link_button, myghost_explorer_link_button],
        vec![vet_list_link_button],
        vec![sheltr_link_button, secret_link_button],
        vec![ghost_docs_link_button],
        vec![gh_repo_link_button],
    ];

    InlineKeyboardMarkup::new(keyboard)
}

pub fn make_link_button(links: &Vec<String>, msg: &str) -> InlineKeyboardMarkup {
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    for link in links.iter() {
        let url = Url::parse(link).expect("Failed to parse URL");
        let button = InlineKeyboardButton::url(msg, url);
        keyboard.push(vec![button]);
    }

    InlineKeyboardMarkup::new(keyboard)
}

pub fn make_inline_calander(year: i32, month: u32, timezone: &str) -> InlineKeyboardMarkup {
    let mut calendar = utils::month_calendar(year, month);
    let mut keyboard = Vec::new();

    let selected_year_month = (year, month);

    let current_ymd = utils::get_current_month_year_day(timezone);
    let current_year_month = (current_ymd.0, current_ymd.1);

    let highlight_day = if selected_year_month == current_year_month {
        true
    } else {
        false
    };

    let months = vec![
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    let days = vec!["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

    let month_year = format!("{}-{}", months[month as usize - 1], year.to_string());

    let header = vec![InlineKeyboardButton::callback(
        month_year.clone(),
        " ".to_string(),
    )];

    keyboard.push(header);

    let mut days_row = Vec::new();
    for day in days.iter() {
        days_row.push(InlineKeyboardButton::callback(
            day.to_string(),
            " ".to_string(),
        ));
    }

    keyboard.push(days_row);

    let mut current_day_displayed = false;

    for week in calendar.iter_mut() {
        let mut row = Vec::new();
        for day in week.iter_mut() {
            match day {
                Some(d) => {
                    if current_day_displayed {
                        row.push(InlineKeyboardButton::callback(
                            " ".to_string(),
                            " ".to_string(),
                        ));
                        continue;
                    }

                    let day_text = if highlight_day && *d == current_ymd.2 {
                        current_day_displayed = true;
                        format!("*{}", d)
                    } else {
                        d.to_string()
                    };

                    let callback_data = format!("date_selection,{},{},{}", d, month, year);
                    row.push(InlineKeyboardButton::callback(day_text, callback_data));
                }
                None => {
                    row.push(InlineKeyboardButton::callback(
                        " ".to_string(),
                        " ".to_string(),
                    ));
                }
            }
        }
        keyboard.push(row);
    }

    let prev_month = InlineKeyboardButton::callback(
        "\u{2B05}\u{FE0F} Prev",
        format!("prev_month,{},{}", month, year),
    );

    let current_date_callback = if highlight_day { " " } else { "current_date" };

    let current_date_text = if highlight_day { "\t\t" } else { "Current" };

    let spacer_or_current =
        InlineKeyboardButton::callback(current_date_text, current_date_callback);
    let next_month = if highlight_day {
        InlineKeyboardButton::callback("\t\t", " ".to_string())
    } else {
        InlineKeyboardButton::callback(
            "Next \u{27A1}\u{FE0F}",
            format!("next_month,{},{}", month, year),
        )
    };

    let footer = vec![prev_month, spacer_or_current, next_month];
    keyboard.push(footer);

    let cancel = vec![InlineKeyboardButton::callback(
        "Cancel",
        "cancel_select_chart",
    )];

    keyboard.push(cancel);

    InlineKeyboardMarkup::new(keyboard)
}
