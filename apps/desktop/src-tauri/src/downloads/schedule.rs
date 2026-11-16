//! "Downloads nach Zeitplan": downloads only run inside a daily time window.

use chrono::Timelike;

use crate::error::{AppError, AppResult};
use crate::models::DownloadSchedule;

/// Minutes after midnight of "HH:MM".
fn minutes(time: &str) -> Option<u32> {
    let (hours, mins) = time.split_once(':')?;
    if hours.len() != 2 || mins.len() != 2 {
        return None;
    }
    let (hours, mins): (u32, u32) = (hours.parse().ok()?, mins.parse().ok()?);
    (hours < 24 && mins < 60).then_some(hours * 60 + mins)
}

pub fn validate(schedule: &DownloadSchedule) -> AppResult<()> {
    if minutes(&schedule.from).is_none() || minutes(&schedule.until).is_none() {
        return Err(AppError::InvalidInput(tr!(
            "Zeiten im Zeitplan haben die Form 01:30.",
            "Times in the schedule look like 01:30."
        )));
    }
    Ok(())
}

/// Whether downloads may run at `now` (minutes after midnight).
pub fn open_at(schedule: &DownloadSchedule, now: u32) -> bool {
    if !schedule.enabled {
        return true;
    }
    let (Some(from), Some(until)) = (minutes(&schedule.from), minutes(&schedule.until)) else {
        return true;
    };
    match from.cmp(&until) {
        std::cmp::Ordering::Equal => true,
        std::cmp::Ordering::Less => (from..until).contains(&now),
        // Over midnight, e.g. 23:00 to 07:00.
        std::cmp::Ordering::Greater => now >= from || now < until,
    }
}

/// Whether downloads may run now (local time).
pub fn open_now(schedule: &DownloadSchedule) -> bool {
    let now = chrono::Local::now();
    open_at(schedule, now.hour() * 60 + now.minute())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(from: &str, until: &str) -> DownloadSchedule {
        DownloadSchedule {
            enabled: true,
            from: from.into(),
            until: until.into(),
        }
    }

    #[test]
    fn windows_within_a_day_and_over_midnight() {
        let night = window("01:00", "07:00");
        assert!(!open_at(&night, 59));
        assert!(open_at(&night, 60));
        assert!(open_at(&night, 6 * 60 + 59));
        assert!(!open_at(&night, 7 * 60));

        let late = window("23:00", "06:30");
        assert!(open_at(&late, 23 * 60 + 30));
        assert!(open_at(&late, 0));
        assert!(!open_at(&late, 6 * 60 + 30));
        assert!(!open_at(&late, 12 * 60));

        assert!(open_at(&window("08:00", "08:00"), 3));
        let mut off = night.clone();
        off.enabled = false;
        assert!(open_at(&off, 12 * 60));
    }

    #[test]
    fn checks_the_times() {
        assert!(validate(&window("01:00", "23:59")).is_ok());
        for bad in ["1:00", "24:00", "12:60", "ab:cd", ""] {
            assert!(validate(&window(bad, "07:00")).is_err(), "{bad}");
        }
    }
}
