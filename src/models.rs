//! Plain data types shared by the whole application: decks, cards, reviews.
//! Holds structs and enums with `serde` derives and nothing else: no
//! persistence, no scheduling rules, no I/O. Every other module depends on this
//! one, and it depends on none of them.
