//! Inbox owns the ordered UI event queue. All semantic events enter here,
//! including local widget HSM input; callbacks may still change local properties.
//! Only RenderRuntime drains and routes events.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_channel::Sender;

use crate::application::ui_thread::thread::evt::UI_Evt;

pub struct UI_Inbox {
    q: VecDeque<UI_Evt>,
}

impl UI_Inbox {
    pub const fn new() -> Self {
        Self {
            q: VecDeque::new(),
        }
    }

    pub fn post(&mut self, e: UI_Evt) {
        self.q.push_back(e);
    }

    pub fn take_all(&mut self) -> Vec<UI_Evt> {
        self.q.drain(..).collect()
    }
}

static INBOX: Mutex<UI_Inbox> = Mutex::new(UI_Inbox::new());
static WAKE: Mutex<Option<Sender<()>>> = Mutex::new(None);

pub fn UI_Inbox_bind(tx: Sender<()>) {
    *WAKE.lock().expect("wake") = Some(tx);
}

pub fn UI_Inbox_post(e: UI_Evt) {
    INBOX.lock().expect("inbox").post(e);
    if let Some(tx) = WAKE.lock().expect("wake").as_ref() {
        let _ = tx.try_send(());
    }
}

pub fn UI_Inbox_take_all() -> Vec<UI_Evt> {
    INBOX.lock().expect("inbox").take_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_all_drains_posted_in_order() {
        let mut inbox = UI_Inbox::new();
        inbox.post(UI_Evt::Timeout);
        inbox.post(UI_Evt::Timeout);
        assert_eq!(
            inbox.take_all(),
            vec![UI_Evt::Timeout, UI_Evt::Timeout]
        );
        assert!(inbox.take_all().is_empty());
    }
}
