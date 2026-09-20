//! Keyboard-first dashboard, with pure state updates kept separate from terminal I/O.

mod model;
mod render;
mod runtime;

pub use model::{
    Dashboard, DashboardDevice, DashboardSample, DeviceState, Effect, Event, Key, MetricSample,
    Operation, Overlay, PollKind, PollPolicy,
};
pub use render::{render, render_to_string};
pub use runtime::run;
