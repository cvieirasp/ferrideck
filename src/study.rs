//! Spaced repetition scheduling (SM-2): ease factors, intervals and due dates.
//! Pure by design: no I/O, no database and no access to the system clock. The
//! current date arrives as a parameter (`today: NaiveDate`), which keeps every
//! function deterministic and trivially testable.
