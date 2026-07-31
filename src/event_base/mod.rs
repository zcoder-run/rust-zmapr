// region:    --- Modules

mod event_base_error;
mod event_new;
mod event_once;
mod event_xpxc;

pub(crate) use event_base_error::{EventBaseError, EventBaseResult};
pub(crate) use event_new::{
	new_mpsc_bounded, new_mpsc_bounded_default, new_once, DEFAULT_CHANNEL_CAPACITY,
};
pub(crate) use event_once::{OnceRx, OnceTx};
pub(crate) use event_xpxc::{MpscRx, MpscTx};

// endregion: --- Modules
