//! Application: UIThread + DataManager.
//! Startup owns the GUI runtime and the independent database thread.

pub mod snapshot;
pub mod data_manager;
pub mod ui_thread;

pub fn run() -> Result<(), snapshot::AppError> {
    use crate::bsp::{SST_Bsp, StdBsp};
    use crate::ports;
    use ui_thread::thread::{
        evt::UI_Evt,
        inbox::UI_Inbox_post,
        render_runtime::{RenderRuntime_run, RenderRuntime_setup},
        time_evt::UI_TimeEvt_tick,
    };

    StdBsp::init();
    let wake_rx = RenderRuntime_setup();
    let (database, cards) =
        data_manager::DataManager::open(&data_manager::DataManager::default_db_path(), |result| {
            UI_Inbox_post(UI_Evt::Persisted(result))
        })?;
    let client = database.client();
    ports::SST_spawnSideThread(UI_TimeEvt_tick);
    RenderRuntime_run(wake_rx, snapshot::Snapshot::new(cards), client);
    drop(database);
    Ok(())
}
