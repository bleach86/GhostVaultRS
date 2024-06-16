use crate::gv_client_methods::{AllTimeEarnigns, BarChart};
use chrono::DateTime;
use plotters::prelude::*;
use serde_json::Value;

pub fn make_barchart(data_value: &Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bc_data: BarChart = serde_json::from_value(data_value.to_owned())?;
    let data = bc_data.data;
    let division = bc_data.division.as_str();

    let root = BitMapBackend::new("/tmp/barchart.png", (640, 480)).into_drawing_area();

    root.fill(&RGBColor(23, 26, 26))?;

    let mut last_week: u64 = 0;
    let mut last_value: u64 = 0;

    let mut first_iter = true;

    // Calculate the x-axis range dynamically
    let mut max_week = 0;
    let mut max_stake = 0;
    for inner_vec in &data {
        let week = inner_vec[0];
        let stake = inner_vec[1];

        if first_iter {
            first_iter = false;
            last_week = week;
        }

        if last_week != week {
            last_value += 1;
            last_week = week;
        }

        if last_value > max_week {
            max_week = last_value;
        }

        if stake > max_stake {
            max_stake = stake;
        }
    }

    let y_range = 0..(max_stake + 2);

    let max_y_labels = if max_stake <= 40 {
        max_stake as usize
    } else if max_stake > 40 && max_stake <= 80 {
        max_stake as usize / 2
    } else if max_stake > 80 && max_stake <= 120 {
        max_stake as usize / 4
    } else {
        max_stake as usize / 8
    };

    let mut flattened_data: Vec<(u64, u64, u64)> = Vec::new();

    last_week = 0;
    last_value = 0;
    first_iter = true;

    // Iterate over each inner vector
    for inner_vec in &data {
        // Extract week number and value
        let week = inner_vec[0];
        let value = inner_vec[1];

        if first_iter {
            first_iter = false;
            last_week = week;
        }

        if last_week != week {
            last_value += 1;
            last_week = week;
        }

        flattened_data.push((last_value, week, value));
    }
    let x_range = 0..(max_week + 1);

    let y_desc = match division {
        "month" => "Monthly Stakes",
        "week" => "Weekly Stakes",
        "day" => "Daily Stakes",
        _ => "Stakes",
    };

    let x_desc = match division {
        "month" => "Month",
        "week" => "Week",
        "day" => "Day",
        _ => "",
    };

    let date_range = format!("{} - {}", bc_data.start, bc_data.end);

    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(75)
        .y_label_area_size(40)
        .margin(5)
        .caption(date_range, ("sans-serif", 24.0).with_color(&WHITE))
        .build_cartesian_2d(x_range.clone(), y_range.clone())?;

    let bar_width = 640 / (max_week + 1) as i32;

    let x_label_offset = (bar_width as f64 / 2.0) - 3.5;

    chart
        .configure_mesh()
        //.disable_x_mesh()
        .disable_mesh()
        .bold_line_style(WHITE.mix(0.3))
        .y_desc(y_desc)
        .x_desc(x_desc)
        .axis_desc_style(("sans-serif", 15).into_font().color(&WHITE)) // Set axis description text color
        .y_label_style(("sans-serif", 15).into_font().color(&WHITE)) // Set y-axis label text color
        .x_label_style(
            ("sans-serif", 15)
                .into_font()
                .color(&WHITE)
                .transform(FontTransform::Rotate270),
        )
        .y_labels(max_y_labels)
        .x_labels(52)
        .x_label_formatter(&|x| {
            if x > &flattened_data.last().unwrap().0 {
                return "".to_string();
            }

            let ts = get_ts_from_week(x, flattened_data.to_owned());

            if division == "month" {
                // Assuming x represents Unix timestamp
                let date = DateTime::from_timestamp(ts, 0).unwrap();
                // Format the date as month abbreviation
                let date_str = date.format("%m/%y").to_string();
                format!("{}         ", date_str)
            } else if division == "day" || division == "week" {
                // Assuming x represents Unix timestamp
                let date = DateTime::from_timestamp(ts, 0).unwrap();
                // Format the date as day of the month
                date.format("%d/%m/%y           ").to_string()
            } else {
                x.to_string()
            }
        })
        .x_label_offset(x_label_offset)
        .draw()?;

    let outline_width = if max_week > 45 { 1 } else { 2 };

    chart.draw_series(
        Histogram::vertical(&chart)
            .style(RGBColor(174, 255, 0).mix(0.5).stroke_width(outline_width))
            .data(flattened_data.iter().map(|&x| {
                let val = (x.0, x.2);
                val
            })),
    )?;

    root.present()?;

    Ok(())
}

pub fn make_area_chart(data_value: &Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let chart_data: AllTimeEarnigns = serde_json::from_value(data_value.to_owned())?;
    let data = chart_data.data;

    if data.is_empty() {
        return Err("No Data".into());
    }

    let enum_data = data
        .iter()
        .enumerate()
        .map(|(idx, x)| (idx as u64, x[0], x[1] as u64))
        .collect::<Vec<(u64, f64, u64)>>();

    let x_range = 0..(enum_data.last().unwrap().0 + 1);
    let y_range = (enum_data.first().unwrap().1)..(enum_data.last().unwrap().1 + 1.0);

    let root = BitMapBackend::new("/tmp/total_earnings_chart.png", (640, 480)).into_drawing_area();

    root.fill(&RGBColor(23, 26, 26))?;

    let date_range = format!("{} - {}", chart_data.start, chart_data.end);

    let max_y_val = enum_data.last().unwrap().1 as u64;
    let y_chars = max_y_val.to_string().len() as u32;

    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(75)
        .y_label_area_size(30 + y_chars * 10)
        .margin(5)
        .caption(date_range, ("sans-serif", 24.0).with_color(&WHITE))
        .build_cartesian_2d(x_range, y_range)?;

    chart
        .configure_mesh()
        .disable_mesh()
        .bold_line_style(WHITE.mix(0.3))
        .y_desc("Earnings")
        .x_desc("Date")
        .axis_desc_style(("sans-serif", 15).into_font().color(&WHITE))
        .y_label_style(("sans-serif", 15).into_font().color(&WHITE))
        .x_label_style(("sans-serif", 15).into_font().color(&WHITE))
        .x_label_style(
            ("sans-serif", 15)
                .into_font()
                .color(&WHITE)
                .transform(FontTransform::Rotate270),
        )
        .y_label_formatter(&|y| format!("{}", *y as u64))
        .x_label_formatter(&|x| {
            if x > &enum_data.last().unwrap().0 {
                return "".to_string();
            }

            let ts: i64 = get_ts_from_index(x, enum_data.to_owned());

            let date = DateTime::from_timestamp(ts, 0).unwrap();
            // Format the date as month abbreviation
            let date_str = date.format("%m/%y").to_string();
            format!("{}         ", date_str)
        })
        .x_label_offset(-5.0)
        .x_labels(40)
        .draw()?;

    chart.draw_series(AreaSeries::new(
        enum_data.iter().map(|x| (x.0, x.1)),
        0.0,
        &RGBColor(174, 255, 0).mix(0.3),
    ))?;

    root.present()?;

    Ok(())
}

fn get_ts_from_index(index: &u64, data: Vec<(u64, f64, u64)>) -> i64 {
    for (idx, _, ts) in data.iter() {
        if idx == index {
            return *ts as i64;
        }
    }
    0
}

fn get_ts_from_week(week: &u64, data: Vec<(u64, u64, u64)>) -> i64 {
    for (idx, ts, _) in data.iter() {
        if idx == week {
            return *ts as i64;
        }
    }
    0
}
