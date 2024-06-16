use chrono::{DateTime, Datelike, Days, Months, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::Value;
use teloxide::{dispatching::dialogue::InMemStorage, prelude::*};

#[derive(Clone, Default, Debug)]
pub enum UpdateRewardModeState {
    #[default]
    Start,
    ReceiveRewardMode,
    ReceiveAddress {
        reward_mode: String,
    },
}

#[derive(Clone, Default, Debug)]
pub enum UpdateRewardIntervalState {
    #[default]
    Start,
    ReceiveIntervalMultiplier,
    ReceiveInterval {
        interval_multiplier: String,
    },
}

#[derive(Clone, Default, Debug)]
pub enum UpdateRewardMinState {
    #[default]
    Start,
    ReceiveMinimum,
}

#[derive(Clone, Default, Debug)]
pub enum GetDateRangeState {
    #[default]
    Start,
    ReceiveFirstDate {
        division: String,
        time_zone: String,
        chart_type: String,
    },
    ReceiveSecondDate {
        first_date: u64,
        division: String,
        time_zone: String,
        chart_type: String,
    },
}

pub type UpdateRewardModeDialog =
    Dialogue<UpdateRewardModeState, InMemStorage<UpdateRewardModeState>>;
pub type UpdateRewardIntervalDialog =
    Dialogue<UpdateRewardIntervalState, InMemStorage<UpdateRewardIntervalState>>;
pub type UpdateRewardMinDialog = Dialogue<UpdateRewardMinState, InMemStorage<UpdateRewardMinState>>;
pub type GetDateRangeDialog = Dialogue<GetDateRangeState, InMemStorage<GetDateRangeState>>;
pub type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

pub fn month_calendar(year: i32, month: u32) -> Vec<Vec<Option<u32>>> {
    let first_day_of_month = NaiveDate::from_ymd_opt(year, month, 1).unwrap();

    let days_in_month = first_day_of_month
        .iter_days()
        .take(31)
        .filter(|d| {
            if d.month() != month {
                return false;
            }
            true
        })
        .count();

    let first_weekday = (first_day_of_month.weekday().number_from_sunday() - 1) as usize;

    // Create a matrix to represent the calendar
    let mut calendar = vec![vec![None; 7]; (days_in_month + first_weekday + 6) / 7];

    //Fill in the days of the month
    for day in 1..=days_in_month {
        let row = (day + first_weekday - 1) / 7;
        let col = (day + first_weekday - 1) % 7;
        calendar[row][col] = Some(day as u32);
    }

    calendar
}

pub fn get_current_month_year_day(time_zone: &str) -> (i32, u32, u32) {
    let dt: DateTime<Utc> = Utc::now();
    let n_time: chrono::prelude::NaiveDateTime =
        NaiveDate::from_ymd_opt(dt.year(), dt.month(), dt.day())
            .unwrap()
            .and_hms_opt(dt.hour(), dt.minute(), dt.second())
            .unwrap();

    let tz: Tz = Tz::from_str_insensitive(time_zone).unwrap();
    let now: DateTime<Tz> = Tz::from_utc_datetime(&tz, &n_time);

    let year = now.year();
    let month = now.month();
    let day = now.day();

    (year, month, day)
}

pub fn parse_chart_range(range: &str, time_zone: &str) -> (u64, u64) {
    let dt = Utc::now();
    let n_time = NaiveDate::from_ymd_opt(dt.year(), dt.month(), dt.day())
        .unwrap()
        .and_hms_opt(dt.hour(), dt.minute(), dt.second())
        .unwrap();

    let tz: Tz = Tz::from_str_insensitive(time_zone).unwrap();
    let now: DateTime<Tz> = Tz::from_utc_datetime(&tz, &n_time);

    let end = now.timestamp() as u64;

    let start = match range {
        "last_two_weeks" => now.checked_sub_days(Days::new(14)).unwrap().timestamp() as u64,
        "last_month" => now.checked_sub_months(Months::new(1)).unwrap().timestamp() as u64,
        "last_three_months" => now.checked_sub_months(Months::new(3)).unwrap().timestamp() as u64,
        "last_six_months" => now.checked_sub_months(Months::new(6)).unwrap().timestamp() as u64,
        "last_year" => now.checked_sub_months(Months::new(12)).unwrap().timestamp() as u64,
        "year_to_date" => {
            let jan_one = now
                .with_month(1)
                .unwrap()
                .with_day(1)
                .unwrap()
                .with_hour(0)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap();
            jan_one.timestamp() as u64
        }
        "max" => 0,
        _ => now.checked_sub_days(Days::new(14)).unwrap().timestamp() as u64,
    };

    (start, end)
}

pub fn get_timezone_opts() -> Value {
    let json_dada = r#"
    {
      "Africa": {
        "total_pages": 3,
        "page-1": [
          "Abidjan",
          "Accra",
          "Addis_Ababa",
          "Algiers",
          "Asmara",
          "Bamako",
          "Bangui",
          "Banjul",
          "Bissau",
          "Blantyre",
          "Brazzaville",
          "Bujumbura",
          "Cairo",
          "Casablanca",
          "Ceuta",
          "Conakry",
          "Dakar",
          "Dar_es_Salaam",
          "Djibouti",
          "Douala",
          "El_Aaiun",
          "Freetown",
          "Gaborone",
          "Harare"
        ],
        "page-2": [
          "Johannesburg",
          "Juba",
          "Kampala",
          "Khartoum",
          "Kigali",
          "Kinshasa",
          "Lagos",
          "Libreville",
          "Lome",
          "Luanda",
          "Lubumbashi",
          "Lusaka",
          "Malabo",
          "Maputo",
          "Maseru",
          "Mbabane",
          "Mogadishu",
          "Monrovia",
          "Nairobi",
          "Ndjamena",
          "Niamey",
          "Nouakchott",
          "Ouagadougou",
          "Porto-Novo"
        ],
        "page-3": [
          "Sao_Tome",
          "Tripoli",
          "Tunis",
          "Windhoek"
        ]
    },
    "America": {
      "total_pages": 6,
      "page-1": [
        "Adak",
        "Anchorage",
        "Anguilla",
        "Antigua",
        "Araguaina",
        "Aruba",
        "Asuncion",
        "Atikokan",
        "Bahia",
        "Bahia_Banderas",
        "Barbados",
        "Belem",
        "Belize",
        "Blanc-Sablon",
        "Boa_Vista",
        "Bogota",
        "Boise",
        "Buenos_Aires",
        "Cambridge_Bay",
        "Campo_Grande",
        "Cancun",
        "Caracas",
        "Catamarca",
        "Cayenne"
      ],
      "page-2": [
        "Cayman",
        "Chicago",
        "Chihuahua",
        "Costa_Rica",
        "Creston",
        "Cuiaba",
        "Curacao",
        "Danmarkshavn",
        "Dawson",
        "Dawson_Creek",
        "Denver",
        "Detroit",
        "Dominica",
        "Edmonton",
        "Eirunepe",
        "El_Salvador",
        "Fort_Nelson",
        "Fortaleza",
        "Glace_Bay",
        "Godthab",
        "Goose_Bay",
        "Grand_Turk",
        "Grenada",
        "Guadeloupe"
      ],
      "page-3": [
        "Guatemala",
        "Guayaquil",
        "Guyana",
        "Halifax",
        "Havana",
        "Hermosillo",
        "Indianapolis",
        "Inuvik",
        "Iqaluit",
        "Jamaica",
        "Juneau",
        "Kralendijk",
        "La_Paz",
        "Lima",
        "Los_Angeles",
        "Lower_Princes",
        "Maceio",
        "Managua",
        "Manaus",
        "Marigot",
        "Martinique",
        "Matamoros",
        "Mazatlan",
        "Menominee"
      ],
      "page-4": [
        "Merida",
        "Metlakatla",
        "Mexico_City",
        "Miquelon",
        "Moncton",
        "Monterrey",
        "Montevideo",
        "Montreal",
        "Montserrat",
        "Nassau",
        "New_York",
        "Nipigon",
        "Nome",
        "Noronha",
        "Nuuk",
        "Ojinaga",
        "Panama",
        "Pangnirtung",
        "Paramaribo",
        "Phoenix",
        "Port-au-Prince",
        "Port_of_Spain",
        "Porto_Velho",
        "Puerto_Rico"
      ],
      "page-5": [
        "Punta_Arenas",
        "Rainy_River",
        "Rankin_Inlet",
        "Recife",
        "Regina",
        "Resolute",
        "Rio_Branco",
        "Santarem",
        "Santiago",
        "Santo_Domingo",
        "Sao_Paulo",
        "Scoresbysund",
        "Sitka",
        "St_Barthelemy",
        "St_Johns",
        "St_Kitts",
        "St_Lucia",
        "St_Thomas",
        "St_Vincent",
        "Swift_Current",
        "Tegucigalpa",
        "Thule",
        "Thunder_Bay",
        "Tijuana"
      ],
      "page-6": [
        "Toronto",
        "Tortola",
        "Vancouver",
        "Whitehorse",
        "Winnipeg",
        "Yakutat",
        "Yellowknife"
      ]
    },
    "Antarctica": {
      "total_pages": 1,
      "page-1": [
        "Casey",
        "Davis",
        "DumontDUrville",
        "Macquarie",
        "Mawson",
        "McMurdo",
        "Palmer",
        "Rothera",
        "South_Pole",
        "Syowa",
        "Troll",
        "Vostok"
      ]
    },
    "Arctic": {
      "total_pages": 1,
      "page-1": [
        "Longyearbyen"
      ]
    },
    "Asia": {
      "total_pages": 4,
      "page-1": [
        "Aden",
        "Almaty",
        "Amman",
        "Anadyr",
        "Aqtau",
        "Aqtobe",
        "Ashgabat",
        "Atyrau",
        "Baghdad",
        "Bahrain",
        "Baku",
        "Bangkok",
        "Barnaul",
        "Beirut",
        "Bishkek",
        "Brunei",
        "Chita",
        "Choibalsan",
        "Colombo",
        "Damascus",
        "Dhaka",
        "Dili",
        "Dubai",
        "Dushanbe"
      ],
      "page-2": [
        "Famagusta",
        "Gaza",
        "Hebron",
        "Ho_Chi_Minh",
        "Hong_Kong",
        "Hovd",
        "Irkutsk",
        "Jakarta",
        "Jayapura",
        "Jerusalem",
        "Kabul",
        "Kamchatka",
        "Karachi",
        "Kathmandu",
        "Khandyga",
        "Kolkata",
        "Krasnoyarsk",
        "Kuala_Lumpur",
        "Kuching",
        "Kuwait",
        "Macau",
        "Magadan",
        "Makassar",
        "Manila"
      ],
      "page-3": [
        "Muscat",
        "Nicosia",
        "Novokuznetsk",
        "Novosibirsk",
        "Omsk",
        "Oral",
        "Phnom_Penh",
        "Pontianak",
        "Pyongyang",
        "Qatar",
        "Qostanay",
        "Qyzylorda",
        "Riyadh",
        "Sakhalin",
        "Samarkand",
        "Seoul",
        "Shanghai",
        "Singapore",
        "Srednekolymsk",
        "Taipei",
        "Tashkent",
        "Tbilisi",
        "Tehran",
        "Thimphu"
      ],
      "page-4": [
        "Tokyo",
        "Tomsk",
        "Ulaanbaatar",
        "Urumqi",
        "Ust-Nera",
        "Vientiane",
        "Vladivostok",
        "Yakutsk",
        "Yangon",
        "Yekaterinburg",
        "Yerevan"
      ]
    },
    "Atlantic": {
      "total_pages": 1,
      "page-1": [
        "Azores",
        "Bermuda",
        "Canary",
        "Cape_Verde",
        "Faroe",
        "Madeira",
        "Reykjavik",
        "South_Georgia",
        "St_Helena",
        "Stanley"
      ]
    },
    "Australia": {
      "total_pages": 1,
      "page-1": [
        "Adelaide",
        "Brisbane",
        "Broken_Hill",
        "Currie",
        "Darwin",
        "Eucla",
        "Hobart",
        "Lindeman",
        "Lord_Howe",
        "Melbourne",
        "Perth",
        "Sydney"
      ]
    },
    "Europe": {
      "total_pages": 3,
      "page-1": [
        "Amsterdam",
        "Andorra",
        "Astrakhan",
        "Athens",
        "Belgrade",
        "Berlin",
        "Bratislava",
        "Brussels",
        "Bucharest",
        "Budapest",
        "Busingen",
        "Chisinau",
        "Copenhagen",
        "Dublin",
        "Gibraltar",
        "Guernsey",
        "Helsinki",
        "Isle_of_Man",
        "Istanbul",
        "Jersey",
        "Kaliningrad",
        "Kiev",
        "Kirov",
        "Lisbon"
      ],
      "page-2": [
        "Ljubljana",
        "London",
        "Luxembourg",
        "Madrid",
        "Malta",
        "Mariehamn",
        "Minsk",
        "Monaco",
        "Moscow",
        "Oslo",
        "Paris",
        "Podgorica",
        "Prague",
        "Riga",
        "Rome",
        "Samara",
        "San_Marino",
        "Sarajevo",
        "Saratov",
        "Simferopol",
        "Skopje",
        "Sofia",
        "Stockholm",
        "Tallinn"
      ],
      "page-3": [
        "Tirane",
        "Ulyanovsk",
        "Uzhgorod",
        "Vaduz",
        "Vatican",
        "Vienna",
        "Vilnius",
        "Volgograd",
        "Warsaw",
        "Zagreb",
        "Zaporozhye",
        "Zurich"
      ]
    },
    "Indian": {
      "total_pages": 1,
      "page-1": [
        "Antananarivo",
        "Chagos",
        "Christmas",
        "Cocos",
        "Comoro",
        "Kerguelen",
        "Mahe",
        "Maldives",
        "Mauritius",
        "Mayotte",
        "Reunion"
      ]
    },
    "Pacific": {
      "total_pages": 2,
      "page-1": [
        "Apia",
        "Auckland",
        "Bougainville",
        "Chatham",
        "Chuuk",
        "Easter",
        "Efate",
        "Enderbury",
        "Fakaofo",
        "Fiji",
        "Funafuti",
        "Galapagos",
        "Gambier",
        "Guadalcanal",
        "Guam",
        "Honolulu",
        "Kiritimati",
        "Kosrae",
        "Kwajalein",
        "Majuro",
        "Marquesas",
        "Midway",
        "Nauru",
        "Niue"
      ],
      "page-2": [
        "Norfolk",
        "Noumea",
        "Pago_Pago",
        "Palau",
        "Pitcairn",
        "Pohnpei",
        "Port_Moresby",
        "Rarotonga",
        "Saipan",
        "Tahiti",
        "Tarawa",
        "Tongatapu",
        "Wake",
        "Wallis"
      ]
    }
  }
"#;
    serde_json::from_str(json_dada).unwrap()
}
