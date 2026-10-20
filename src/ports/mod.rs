//! SST Kernel port. `run` lives on a side thread.
//! `idle` ticks TimeEvt then sleeps one BSP period.
#![allow(non_upper_case_globals)]

use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use rs_sst::{
    MAX_PRIO, Prio, SST_Bsp, SST_Evt, SST_Kernel, SST_Scheduler, SST_Slot, SST_Task,
    SST_TaskHandler, SST_Ticker, SST_TimeEvt,
};

use crate::application::ui_thread::evt::UI_Evt;
use crate::application::ui_thread::inbox::UI_Inbox_post;
use crate::bsp::{BSP_TICKS_PER_SEC, StdBsp};

pub struct StdPort;

#[derive(Clone, Copy)]
pub enum TickEvt {
    Tick,
}

impl SST_Evt for TickEvt {
    fn sig(&self) -> u16 {
        1
    }
}

/// 100Hz / 50 ticks = 2Hz TimeEvt.
const UI_TICK_PERIOD: u16 = 50;

pub struct UiTick;

impl SST_TaskHandler for UiTick {
    type Evt = TickEvt;

    fn init(&mut self, _ie: Option<&TickEvt>) {
        TIME_EVT.ctor(&TICKER, &TICK_SIG, &AO_UI_TICK);
        TIME_EVT.arm(UI_TICK_PERIOD, UI_TICK_PERIOD);
    }

    fn dispatch(&mut self, _e: &TickEvt) {
        UI_Inbox_post(UI_Evt::Timeout);
    }
}

static StdPort_crit_: Mutex<()> = Mutex::new(());
static StdPort_cvars_: [Condvar; MAX_PRIO] = [const { Condvar::new() }; MAX_PRIO];
static StdPort_sched_: SST_Scheduler<StdPort> = SST_Scheduler::new();
pub static TICKER: SST_Ticker<TickEvt, StdPort> = SST_Ticker::new();
static TICK_SIG: TickEvt = TickEvt::Tick;
static TIME_EVT: SST_TimeEvt<TickEvt, StdPort> = SST_TimeEvt::new();
static AO_UI_TICK: SST_Task<UiTick, StdPort, 4> = SST_Task::new(UiTick);

impl SST_Kernel for StdPort {
    type CritGuard = MutexGuard<'static, ()>;

    fn enterCritical() -> Self::CritGuard {
        StdPort_crit_.lock().expect("crit")
    }

    fn pend(prio: Prio, _cs: &Self::CritGuard) {
        let i = (prio as usize) - 1;
        StdPort_cvars_[i].notify_one();
    }

    fn take(prio: Prio, cs: Self::CritGuard) -> Self::CritGuard {
        let i = (prio as usize) - 1;
        StdPort_cvars_[i].wait(cs).expect("cvar")
    }

    fn idle() {
        TICKER.tick();
        thread::sleep(Duration::from_millis(
            1000 / u64::from(BSP_TICKS_PER_SEC),
        ));
    }

    fn scheduler() -> &'static SST_Scheduler<Self> {
        &StdPort_sched_
    }

    fn setPrio(slot: &'static dyn SST_Slot, _prio: Prio) {
        thread::Builder::new()
            .name("sst_ao".into())
            .spawn(move || loop {
                slot.wait();
                slot.activate();
            })
            .expect("sst_ao");
    }

    fn start() {}

    fn run() -> ! {
        Self::start();
        StdBsp::onStart();
        loop {
            Self::idle();
        }
    }
}

pub fn SST_spawnSideThread() {
    thread::Builder::new()
        .name("sst_run".into())
        .spawn(|| {
            StdPort::scheduler().start(&AO_UI_TICK, 1, None);
            StdPort::run();
        })
        .expect("SST_spawnSideThread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ui_thread::evt::UI_Evt;
    use crate::application::ui_thread::inbox::UI_Inbox_take_all;

    #[test]
    fn time_evt_posts_timeout_after_period() {
        StdPort::scheduler().start(&AO_UI_TICK, 1, None);
        thread::sleep(Duration::from_millis(20));
        for _ in 0..UI_TICK_PERIOD {
            TICKER.tick();
        }
        thread::sleep(Duration::from_millis(50));
        let evts = UI_Inbox_take_all();
        assert!(
            evts.iter().any(|e| matches!(e, UI_Evt::Timeout)),
            "expected Timeout after {UI_TICK_PERIOD} ticks, got {evts:?}"
        );
    }
}
