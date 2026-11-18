//! Pure, platform-independent calendar logic for Calen.

pub mod event;
pub mod ics;
pub mod links;
pub mod notes;
pub mod quickadd;
pub mod recur;

pub use event::{CalendarObject, Event, When};
pub use recur::{Occurrence, expand};
