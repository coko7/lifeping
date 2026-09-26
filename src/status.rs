//! Pure status computation, shared by the API and (later) the notifier.

use serde::Serialize;
use time::{Duration, OffsetDateTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Unknown,
    Green,
    Yellow,
    Red,
}

/// Boundaries are inclusive on the worse side; a ping in the future
/// (clock change) counts as green.
pub fn compute(
    latest: Option<OffsetDateTime>,
    now: OffsetDateTime,
    yellow_after: Duration,
    red_after: Duration,
) -> Status {
    let Some(latest) = latest else {
        return Status::Unknown;
    };
    let age = now - latest;
    if age < yellow_after {
        Status::Green
    } else if age < red_after {
        Status::Yellow
    } else {
        Status::Red
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    const YELLOW: Duration = Duration::hours(12);
    const RED: Duration = Duration::hours(24);
    const NOW: OffsetDateTime = datetime!(2026-09-27 12:00:00 UTC);

    fn at_age(age: Duration) -> Status {
        compute(Some(NOW - age), NOW, YELLOW, RED)
    }

    #[test]
    fn no_ping_is_unknown() {
        assert_eq!(compute(None, NOW, YELLOW, RED), Status::Unknown);
    }

    #[test]
    fn each_state() {
        assert_eq!(at_age(Duration::ZERO), Status::Green);
        assert_eq!(at_age(Duration::hours(3)), Status::Green);
        assert_eq!(at_age(Duration::hours(18)), Status::Yellow);
        assert_eq!(at_age(Duration::days(3)), Status::Red);
    }

    #[test]
    fn boundaries_are_inclusive_on_the_worse_side() {
        assert_eq!(at_age(YELLOW - Duration::SECOND), Status::Green);
        assert_eq!(at_age(YELLOW), Status::Yellow);
        assert_eq!(at_age(RED - Duration::SECOND), Status::Yellow);
        assert_eq!(at_age(RED), Status::Red);
    }

    #[test]
    fn future_ping_is_green() {
        assert_eq!(at_age(-Duration::hours(1)), Status::Green);
        assert_eq!(at_age(-Duration::days(30)), Status::Green);
    }

    #[test]
    fn serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Status::Yellow).unwrap(),
            "\"yellow\""
        );
    }
}
