#![allow(non_camel_case_types, non_snake_case)]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use sm_todo::application::ui_thread::thread::render_runtime::{
    RenderRuntime_run, RenderRuntime_setup,
};
use sm_todo::bsp::{StdBsp, SST_Bsp};
use sm_todo::application::ui_thread::thread::time_evt::UI_TimeEvt_tick;
use sm_todo::ports;

fn main() {
    StdBsp::init();
    let wake_rx = RenderRuntime_setup();
    ports::SST_spawnSideThread(UI_TimeEvt_tick);
    RenderRuntime_run(wake_rx);
}
