//! GUI thread execution: event domain, Inbox, runtime routing and time-event pump.
//! Control ownership and scheduling belong to the sibling hsm module.

pub mod evt;
pub mod inbox;
pub mod render_runtime;
pub mod time_evt;
