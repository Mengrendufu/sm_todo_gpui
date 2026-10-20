//! SST Kernel port. `run` lives on a side thread.
//! `idle` advances SST and the linked GUI time events, then sleeps one BSP period.
#![allow(non_upper_case_globals)]

use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use rs_sst::{
    MAX_PRIO, Prio, SST_Bsp, SST_Evt, SST_Kernel, SST_Scheduler, SST_Slot, SST_Ticker,
};

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

static StdPort_crit_: Mutex<()> = Mutex::new(());
static StdPort_cvars_: [Condvar; MAX_PRIO] = [const { Condvar::new() }; MAX_PRIO];
static StdPort_sched_: SST_Scheduler<StdPort> = SST_Scheduler::new();
pub static TICKER: SST_Ticker<TickEvt, StdPort> = SST_Ticker::new();
static StdPort_idle_hook_: OnceLock<fn()> = OnceLock::new();

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
        StdPort_idle_hook_.get().expect("idle hook not bound")();
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

/// Bind one time-event pump before starting. It runs once per idle tick on
/// `sst_run` and must only advance timers and post events, without waiting for
/// event handling or performing GUI work.
pub fn SST_spawnSideThread(idle_hook: fn()) {
    StdPort_idle_hook_.set(idle_hook).expect("idle hook already bound");
    thread::Builder::new()
        .name("sst_run".into())
        .spawn(|| {
            StdPort::run();
        })
        .expect("SST_spawnSideThread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ui_thread::thread::evt::UI_Evt;
    use crate::application::ui_thread::thread::inbox::UI_Inbox_take_all;
    use crate::application::ui_thread::thread::time_evt::UI_TimeEvt_tick;

    static HOOK_THREAD: Mutex<Option<thread::ThreadId>> = Mutex::new(None);

    #[test]
    fn idle_posts_gui_timeout_on_its_own_thread_without_a_forwarder() {
        StdPort_idle_hook_.set(|| {
            *HOOK_THREAD.lock().unwrap() = Some(thread::current().id());
            UI_TimeEvt_tick();
        }).unwrap();
        let idle_thread = thread::spawn(|| {
            for _ in 0..BSP_TICKS_PER_SEC / 2 {
                StdPort::idle();
            }
            thread::current().id()
        }).join().unwrap();
        assert_eq!(*HOOK_THREAD.lock().unwrap(), Some(idle_thread));
        assert_eq!(UI_Inbox_take_all(), vec![UI_Evt::Timeout]);
    }
}
