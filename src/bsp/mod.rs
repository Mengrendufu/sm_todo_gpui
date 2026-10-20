//! Board support: tick rate. TimeEvt tick runs in Kernel idle, not here.

pub use rs_sst::SST_Bsp;

pub const BSP_TICKS_PER_SEC: u32 = 100;

pub struct StdBsp;

impl SST_Bsp for StdBsp {
    fn init() {}

    fn onStart() {}
}
