#![allow(non_camel_case_types, non_snake_case)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sm_todo::application::ui_thread::render_runtime::{
    RenderRuntime_run, RenderRuntime_setup,
};
use sm_todo::bsp::{StdBsp, SST_Bsp};
use sm_todo::ports;

fn main() {
    StdBsp::init();
    let wake_rx = RenderRuntime_setup();
    ports::SST_spawnSideThread();
    RenderRuntime_run(wake_rx);
}
