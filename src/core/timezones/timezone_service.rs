use chrono::Utc;
use chrono_tz::Tz;

pub struct TeamTimezone {
    pub label: &'static str,
    pub tz_name: &'static str,
    pub note: &'static str,
}

pub struct TimezoneDisplay {
    pub twelve_hour: String,
    pub twenty_four_hour: String,
    pub date_fragment: String,
    pub relative_timestamp: i64,
}

pub struct TimezoneService {
    timezones: Vec<TeamTimezone>,
}

impl TimezoneService {
    pub fn new() -> Self {
        Self {
            timezones: vec![
                TeamTimezone {
                    label: "🕐 Pacific",
                    tz_name: "America/Los_Angeles",
                    note: "Seattle / Vancouver",
                },
                TeamTimezone {
                    label: "🕑 Mountain",
                    tz_name: "America/Denver",
                    note: "Denver / Calgary",
                },
                TeamTimezone {
                    label: "🕒 Central",
                    tz_name: "America/Chicago",
                    note: "Austin / Chicago",
                },
                TeamTimezone {
                    label: "🕓 Eastern",
                    tz_name: "America/New_York",
                    note: "New York / Toronto",
                },
                TeamTimezone {
                    label: "🕔 UK",
                    tz_name: "Europe/London",
                    note: "London / Belfast",
                },
                TeamTimezone {
                    label: "🕕 Central Europe",
                    tz_name: "Europe/Berlin",
                    note: "Amsterdam / Paris",
                },
                TeamTimezone {
                    label: "🕖 India",
                    tz_name: "Asia/Kolkata",
                    note: "Bengaluru / Mumbai",
                },
                TeamTimezone {
                    label: "🕗 Australia",
                    tz_name: "Australia/Sydney",
                    note: "Sydney / Melbourne",
                },
            ],
        }
    }

    pub fn get_team_timezones(&self) -> Vec<(&TeamTimezone, TimezoneDisplay)> {
        let utc_now = Utc::now();

        self.timezones
            .iter()
            .map(|tz_def| {
                let tz: Tz = tz_def.tz_name.parse().unwrap_or(chrono_tz::UTC);
                let now = utc_now.with_timezone(&tz);

                let twelve_hour = now.format("%I:%M %p").to_string();
                // lstrip("0") logic: if starts with 0, remove it.
                let twelve_hour = if let Some(stripped) = twelve_hour.strip_prefix('0') {
                    stripped.to_string()
                } else {
                    twelve_hour
                };

                let twenty_four_hour = now.format("%H:%M").to_string();
                let date_fragment = now.format("%a %d %b").to_string();
                let relative_timestamp = now.timestamp();

                (
                    tz_def,
                    TimezoneDisplay {
                        twelve_hour,
                        twenty_four_hour,
                        date_fragment,
                        relative_timestamp,
                    },
                )
            })
            .collect()
    }
}
