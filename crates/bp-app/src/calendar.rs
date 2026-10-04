//! A small release-date picker. Selecting a date fills a version; opening it
//! never changes an existing version tag.

use egui::{Button, Popup, PopupCloseBehavior, Ui, Vec2};
use egui_phosphor::regular as icon;
use std::time::{SystemTime, UNIX_EPOCH};

const MONTHS: [&str; 12] = [
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Date {
    year: i32,
    month: u32,
    day: u32,
}

fn month_days(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

impl Date {
    fn parse(value: &str) -> Option<Self> {
        let mut parts = value.trim().split('-');
        let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return None;
        }
        let date = Self {
            year: year.parse().ok()?,
            month: month.parse().ok()?,
            day: day.parse().ok()?,
        };
        ((1..=9999).contains(&date.year)
            && (1..=12).contains(&date.month)
            && (1..=month_days(date.year, date.month)).contains(&date.day))
        .then_some(date)
    }

    fn from_epoch_days(mut days: u64) -> Self {
        // Bound navigation even if the system clock is outside supported years.
        days = days.min(2_932_896); // 9999-12-31, measured from 1970-01-01.
        let mut year = 1970;
        loop {
            let year_days = 337 + month_days(year, 2);
            if days < u64::from(year_days) {
                break;
            }
            days -= u64::from(year_days);
            year += 1;
        }
        let mut month = 1;
        while days >= u64::from(month_days(year, month)) {
            days -= u64::from(month_days(year, month));
            month += 1;
        }
        Self {
            year,
            month,
            day: days as u32 + 1,
        }
    }

    fn monday_offset(self) -> u32 {
        // Gregorian weekday of the first of this month, Monday = 0.
        let offsets = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let year = self.year - i32::from(self.month < 3);
        ((year + year / 4 - year / 100 + year / 400 + offsets[self.month as usize - 1] + 1 + 6) % 7)
            as u32
    }

    fn version(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

pub(crate) struct Calendar {
    year: i32,
    month: u32,
}

impl Default for Calendar {
    fn default() -> Self {
        let days = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86400;
        let date = Date::from_epoch_days(days);
        Self {
            year: date.year,
            month: date.month,
        }
    }
}

impl Calendar {
    fn shift_month(&mut self, delta: i32) {
        let month = ((self.year - 1) * 12 + self.month as i32 - 1 + delta).clamp(0, 9999 * 12 - 1);
        self.year = month / 12 + 1;
        self.month = (month % 12 + 1) as u32;
    }

    pub(crate) fn version_picker(&mut self, ui: &mut Ui, version: &mut String) {
        let response = ui
            .button(crate::theme::icon(icon::CALENDAR))
            .on_hover_text("Choose the icon pack’s release date");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                "Choose pack release date",
            )
        });
        let selected = Date::parse(version);
        if response.clicked()
            && let Some(date) = selected
        {
            self.year = date.year;
            self.month = date.month;
        }
        Popup::from_toggle_button_response(&response)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(248.0);
                ui.label(egui::RichText::new("Pack release date").strong());
                ui.horizontal(|ui| {
                    let previous = ui
                        .add_enabled(
                            self.year > 1 || self.month > 1,
                            Button::new(crate::theme::icon(icon::CARET_LEFT)),
                        )
                        .on_hover_text("Previous month");
                    previous.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            previous.enabled(),
                            "Previous month",
                        )
                    });
                    if previous.clicked() {
                        self.shift_month(-1);
                    }
                    ui.label(MONTHS[self.month as usize - 1]);
                    let label = ui.label("Year");
                    ui.add(
                        egui::DragValue::new(&mut self.year)
                            .range(1..=9999)
                            .speed(1.0),
                    )
                    .labelled_by(label.id);
                    let next = ui
                        .add_enabled(
                            self.year < 9999 || self.month < 12,
                            Button::new(crate::theme::icon(icon::CARET_RIGHT)),
                        )
                        .on_hover_text("Next month");
                    next.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            next.enabled(),
                            "Next month",
                        )
                    });
                    if next.clicked() {
                        self.shift_month(1);
                    }
                });
                let date = Date {
                    year: self.year,
                    month: self.month,
                    day: 1,
                };
                let offset = date.monday_offset();
                let count = month_days(self.year, self.month);
                egui::Grid::new("pack-release-calendar")
                    .spacing(Vec2::splat(4.0))
                    .show(ui, |ui| {
                        for day in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                            ui.add_sized(
                                [32.0, 20.0],
                                egui::Label::new(egui::RichText::new(day).small().weak()),
                            );
                        }
                        ui.end_row();
                        for cell in 0..(offset + count).div_ceil(7) * 7 {
                            if cell >= offset && cell < offset + count {
                                let date = Date {
                                    day: cell - offset + 1,
                                    ..date
                                };
                                let response = ui.add_sized(
                                    [32.0, 28.0],
                                    Button::new(date.day.to_string())
                                        .selected(selected == Some(date)),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        format!(
                                            "{} {}, {}",
                                            MONTHS[date.month as usize - 1],
                                            date.day,
                                            date.year
                                        ),
                                    )
                                });
                                if response.clicked() {
                                    *version = date.version();
                                    ui.close();
                                }
                            } else {
                                ui.allocate_space(Vec2::new(32.0, 28.0));
                            }
                            if cell % 7 == 6 {
                                ui.end_row();
                            }
                        }
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gregorian_dates_cover_leap_years_and_weekday_alignment() {
        assert_eq!(Date::parse("2024-02-29").unwrap().version(), "2024-02-29");
        for invalid in [
            "2026-02-29",
            "1900-02-29",
            "2026-04-31",
            "0000-01-01",
            "2026-13-01",
            "v24",
        ] {
            assert!(Date::parse(invalid).is_none());
        }
        assert!(Date::parse("2000-02-29").is_some());
        assert_eq!(Date::parse("1970-01-01").unwrap().monday_offset(), 3);
        assert_eq!(Date::parse("2026-07-01").unwrap().monday_offset(), 2);
        assert_eq!(Date::from_epoch_days(0).version(), "1970-01-01");
        assert_eq!(Date::from_epoch_days(19782).version(), "2024-02-29");
        assert_eq!(Date::from_epoch_days(u64::MAX).version(), "9999-12-31");
    }

    #[test]
    fn navigation_crosses_years_and_stops_at_supported_limits() {
        let mut calendar = Calendar {
            year: 2026,
            month: 1,
        };
        calendar.shift_month(-1);
        assert_eq!((calendar.year, calendar.month), (2025, 12));
        calendar.shift_month(1);
        assert_eq!((calendar.year, calendar.month), (2026, 1));
        calendar.shift_month(-200000);
        assert_eq!((calendar.year, calendar.month), (1, 1));
        calendar.shift_month(200000);
        assert_eq!((calendar.year, calendar.month), (9999, 12));
    }
}
