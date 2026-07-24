//! Spaced repetition scheduling (SM-2): ease factors, intervals and due dates.
//! Pure by design: no I/O, no database and no access to the system clock. The
//! current date arrives as a parameter (`today: NaiveDate`), which keeps every
//! function deterministic and trivially testable.
//!
//! SM-2 is the algorithm behind SuperMemo and, in variations, behind Anki. The
//! idea is that a fact is best reviewed just before it would be forgotten, so
//! each successful recall pushes the next review further away: the card carries
//! an `interval_days` (how long the last gap was) and an `ease_factor` (a
//! per-card multiplier measuring how easy that card is for this person), and a
//! review multiplies one by the other to get the next gap. Recalling well
//! stretches the interval and nudges the ease up; failing collapses the
//! interval back to zero and pushes the ease down, so hard cards come back
//! often and easy ones drift toward months.
//!
//! **This is a simplified variant of SuperMemo's SM-2**, not the original. The
//! published algorithm grades answers from 0 to 5, applies a quadratic formula
//! to the ease factor and keeps a separate repetition counter with fixed first
//! and second intervals. Here the scale is the four buttons users actually
//! understand, the ease adjustments are flat amounts, and the repetition count
//! is implied by `interval_days == 0`. The shape of the curve is the same; the
//! constants are tuned for readability over fidelity.

use crate::models::{Card, Scheduling};
use chrono::{Days, NaiveDate};

/// Lowest ease factor a card may reach.
///
/// Without a floor, a card failed many times would drive its multiplier toward
/// zero and never grow an interval again. SM-2 uses 1.3 and so do we.
const EASE_FLOOR: f32 = 1.3;

/// Ease lost when a card is forgotten.
const AGAIN_PENALTY: f32 = 0.20;

/// Ease lost when a card is recalled with difficulty.
const HARD_PENALTY: f32 = 0.15;

/// Ease gained when a card is recalled effortlessly.
const EASY_BONUS: f32 = 0.15;

/// Interval multiplier for a card answered `Hard`: it grows, but barely.
const HARD_MULTIPLIER: f32 = 1.2;

/// Extra multiplier applied on top of the ease factor for `Easy`.
const EASY_MULTIPLIER: f32 = 1.3;

/// How well the user recalled a card, as offered by the four review buttons.
///
/// A closed set of four values, so an enum makes any other rating impossible to
/// represent and keeps every `match` exhaustive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rating {
    /// Not recalled: the card goes back to the start.
    Again,
    /// Recalled, but with effort.
    Hard,
    /// Recalled correctly. The expected answer.
    Good,
    /// Recalled immediately, with no hesitation.
    Easy,
}

/// Computes the next scheduling for `card` given how the user rated it.
///
/// Pure: same inputs, same output, always. `today` is a parameter precisely so
/// that this function never reads a clock.
///
/// The interval is always computed from the card's **current** ease factor, and
/// the ease adjustment applies to the next review. Doing it the other way round
/// would make a single `Easy` press pay its bonus twice.
pub fn schedule(card: &Card, rating: Rating, today: NaiveDate) -> Scheduling {
    let (interval_days, ease_factor) = match rating {
        // Forgotten: relearn today. The interval collapses to zero rather than
        // shrinking, because a card that was not recalled carries no evidence
        // about how long it can be left alone.
        Rating::Again => (0, card.ease_factor - AGAIN_PENALTY),

        // Recalled with effort: the interval grows slowly, independently of the
        // ease factor, and the ease drops so future intervals grow slower too.
        // The floor of 1 day keeps a fresh card (interval 0) from staying at 0.
        Rating::Hard => (
            round_days(card.interval_days as f32 * HARD_MULTIPLIER).max(1),
            card.ease_factor - HARD_PENALTY,
        ),

        // The expected answer: the interval is multiplied by the ease factor,
        // which is the core of SM-2. A card seen for the first time has no
        // interval to multiply, so it starts at one day. The ease is unchanged:
        // answering as expected is not evidence that the card got easier.
        Rating::Good => {
            let interval = if card.interval_days == 0 {
                1
            } else {
                round_days(card.interval_days as f32 * card.ease_factor)
            };
            (interval, card.ease_factor)
        }

        // Effortless: the normal interval plus a bonus multiplier, and the ease
        // rises so the gap keeps widening. The floor of 2 days makes `Easy`
        // strictly longer than `Good` on a fresh card.
        Rating::Easy => (
            round_days(card.interval_days as f32 * card.ease_factor * EASY_MULTIPLIER).max(2),
            card.ease_factor + EASY_BONUS,
        ),
    };

    Scheduling {
        interval_days,
        // Applied in every branch, including the ones that raise the ease, so
        // the invariant holds no matter which path produced the value.
        ease_factor: ease_factor.max(EASE_FLOOR),
        due_date: add_days(today, interval_days),
    }
}

/// Rounds a computed interval to whole days.
///
/// `round` and not `ceil`: rounding up would add up to a full day of error on
/// every single review, and since intervals compound, that bias would inflate
/// the whole schedule over time. Rounding to nearest keeps the average interval
/// faithful to the multiplier. The `max` guards in the caller handle the cases
/// where rounding to nearest would legitimately produce zero.
///
/// The cast to `u32` saturates rather than wrapping, so an absurdly long
/// interval clamps instead of turning into a small number.
fn round_days(value: f32) -> u32 {
    value.round() as u32
}

/// Adds whole days to a date, saturating at the end of the calendar.
///
/// Only reachable around year 262143, where `chrono` runs out of range.
/// Saturating keeps this function total: it has no error path to report.
fn add_days(today: NaiveDate, days: u32) -> NaiveDate {
    today
        .checked_add_days(Days::new(u64::from(days)))
        .unwrap_or(NaiveDate::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone as _, Utc};
    use uuid::Uuid;

    /// Floating point comparisons are never exact: `2.5 - 0.2` is stored as
    /// 2.299999952316284, not 2.3, because neither value is representable in
    /// binary. Comparing with `==` would make correct code fail, so every ease
    /// assertion allows one ten-thousandth of slack, far tighter than any
    /// difference the algorithm can produce.
    const TOLERANCE: f32 = 1e-4;

    fn assert_ease(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < TOLERANCE,
            "expected ease {expected}, got {actual}"
        );
    }

    fn on(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    /// A card with the scheduling state under test. The content is irrelevant
    /// here: only `interval_days` and `ease_factor` reach the algorithm.
    fn card(interval_days: u32, ease_factor: f32) -> Card {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 24, 10, 0, 0)
            .single()
            .expect("valid timestamp");

        let mut card = Card::new(
            Uuid::nil(),
            "front".to_owned(),
            "back".to_owned(),
            None,
            now,
            on(2026, 7, 24),
        );

        card.interval_days = interval_days;
        card.ease_factor = ease_factor;
        card
    }

    #[test]
    fn fresh_card_again_relearns_today() {
        let today = on(2026, 7, 24);
        let next = schedule(&card(0, 2.5), Rating::Again, today);

        assert_eq!(next.interval_days, 0);
        assert_eq!(next.due_date, today);
        assert_ease(next.ease_factor, 2.3);
    }

    #[test]
    fn fresh_card_hard_waits_one_day() {
        let today = on(2026, 7, 24);
        let next = schedule(&card(0, 2.5), Rating::Hard, today);

        assert_eq!(next.interval_days, 1);
        assert_eq!(next.due_date, on(2026, 7, 25));
        assert_ease(next.ease_factor, 2.35);
    }

    #[test]
    fn fresh_card_good_waits_one_day_and_keeps_the_ease() {
        let today = on(2026, 7, 24);
        let next = schedule(&card(0, 2.5), Rating::Good, today);

        assert_eq!(next.interval_days, 1);
        assert_eq!(next.due_date, on(2026, 7, 25));
        assert_ease(next.ease_factor, 2.5);
    }

    #[test]
    fn fresh_card_easy_waits_two_days() {
        let today = on(2026, 7, 24);
        let next = schedule(&card(0, 2.5), Rating::Easy, today);

        assert_eq!(next.interval_days, 2);
        assert_eq!(next.due_date, on(2026, 7, 26));
        assert_ease(next.ease_factor, 2.65);
    }

    #[test]
    fn mature_card_again_collapses_the_interval() {
        let today = on(2026, 7, 24);
        let next = schedule(&card(10, 2.5), Rating::Again, today);

        assert_eq!(next.interval_days, 0);
        assert_eq!(next.due_date, today);
        assert_ease(next.ease_factor, 2.3);
    }

    #[test]
    fn mature_card_hard_grows_slowly() {
        let today = on(2026, 7, 24);
        let next = schedule(&card(10, 2.5), Rating::Hard, today);

        // 10 * 1.2 = 12
        assert_eq!(next.interval_days, 12);
        assert_eq!(next.due_date, on(2026, 8, 5));
        assert_ease(next.ease_factor, 2.35);
    }

    #[test]
    fn mature_card_good_multiplies_by_the_ease() {
        let today = on(2026, 7, 24);
        let next = schedule(&card(10, 2.5), Rating::Good, today);

        // 10 * 2.5 = 25
        assert_eq!(next.interval_days, 25);
        assert_eq!(next.due_date, on(2026, 8, 18));
        assert_ease(next.ease_factor, 2.5);
    }

    #[test]
    fn mature_card_easy_adds_the_bonus_multiplier() {
        let today = on(2026, 7, 24);
        let next = schedule(&card(10, 2.5), Rating::Easy, today);

        // 10 * 2.5 * 1.3 = 32.5, rounded to 33
        assert_eq!(next.interval_days, 33);
        assert_eq!(next.due_date, on(2026, 8, 26));
        assert_ease(next.ease_factor, 2.65);
    }

    #[test]
    fn repeated_again_never_pushes_the_ease_below_the_floor() {
        let today = on(2026, 7, 24);
        let mut card = card(10, 2.5);

        for _ in 0..20 {
            let next = schedule(&card, Rating::Again, today);

            assert!(
                next.ease_factor >= EASE_FLOOR - TOLERANCE,
                "ease {} fell below the floor",
                next.ease_factor
            );

            card.ease_factor = next.ease_factor;
            card.interval_days = next.interval_days;
        }

        assert_ease(card.ease_factor, EASE_FLOOR);
    }

    #[test]
    fn due_date_crosses_a_month_boundary() {
        // 25 days from 2026-07-25 lands in August.
        let next = schedule(&card(10, 2.5), Rating::Good, on(2026, 7, 25));

        assert_eq!(next.interval_days, 25);
        assert_eq!(next.due_date, on(2026, 8, 19));
    }

    #[test]
    fn due_date_crosses_a_year_boundary() {
        // 25 days from 2026-12-20 lands in the next year.
        let next = schedule(&card(10, 2.5), Rating::Good, on(2026, 12, 20));

        assert_eq!(next.interval_days, 25);
        assert_eq!(next.due_date, on(2027, 1, 14));
    }

    #[test]
    fn due_date_crosses_a_leap_day() {
        // 2028 is a leap year: 5 days from February 26th must include the 29th.
        let next = schedule(&card(4, 1.3), Rating::Good, on(2028, 2, 26));

        assert_eq!(next.interval_days, 5);
        assert_eq!(next.due_date, on(2028, 3, 2));
    }

    #[test]
    fn easy_is_never_shorter_than_good_which_is_never_shorter_than_hard() {
        let today = on(2026, 7, 24);
        let cases = [
            (0, 2.5),
            (1, 1.3),
            (1, 2.5),
            (3, 1.8),
            (10, 2.5),
            (30, 1.3),
            (100, 2.9),
        ];

        for (interval_days, ease_factor) in cases {
            let card = card(interval_days, ease_factor);

            let hard = schedule(&card, Rating::Hard, today).interval_days;
            let good = schedule(&card, Rating::Good, today).interval_days;
            let easy = schedule(&card, Rating::Easy, today).interval_days;

            assert!(
                easy >= good && good >= hard,
                "interval order broken for interval {interval_days} and ease {ease_factor}: \
                 hard {hard}, good {good}, easy {easy}"
            );
        }
    }
}
