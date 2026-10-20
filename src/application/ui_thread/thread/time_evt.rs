//! GUI time events. The linked tick runs on `sst_run`; handlers run on the GUI thread.

use std::sync::Mutex;

use crate::bsp::BSP_TICKS_PER_SEC;

use super::evt::UI_Evt;
use super::inbox::UI_Inbox_post;

const UI_TICK_PERIOD: u16 = (BSP_TICKS_PER_SEC / 2) as u16;
static REMAINING: Mutex<u16> = Mutex::new(UI_TICK_PERIOD);

fn advance_(remaining: &mut u16) -> bool {
    *remaining -= 1;
    if *remaining == 0 {
        *remaining = UI_TICK_PERIOD;
        true
    } else {
        false
    }
}

/// Called once per BSP tick by the background idle loop; only queues Timeout.
pub fn UI_TimeEvt_tick() {
    let expired = advance_(&mut REMAINING.lock().expect("GUI time event"));
    if expired {
        UI_Inbox_post(UI_Evt::Timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_repeats_every_half_second_of_ticks_without_early_firing() {
        let mut remaining = UI_TICK_PERIOD;
        let fired: Vec<_> = (1..=UI_TICK_PERIOD * 3)
            .filter(|_| advance_(&mut remaining))
            .collect();
        assert_eq!(fired, vec![UI_TICK_PERIOD, UI_TICK_PERIOD * 2, UI_TICK_PERIOD * 3]);
    }
}
