//! GUI-thread HSM owner and its controls. Local feedback stays in GPUI callbacks.

pub mod calendar;
pub mod card_editor;
pub mod card_meta;
pub mod text_field;
pub mod theme;
pub mod widgets;
pub mod window_pin;
mod state_machine;
mod edge_dock;
mod window_chrome;
mod window_taskbar;
mod top_edge_dock;
mod tray_icon;

use crate::application::data_manager::DataClient;
use crate::application::snapshot::Snapshot;
use crate::application::data_manager::Persistence;
use crate::application::ui_thread::thread::evt::UI_Evt;
use crate::application::ui_thread::thread::inbox::UI_Inbox_post;
use rs_hsm_::{SM_Hsm, SM_HsmTrait};
use std::ops::{Deref, DerefMut};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use gpui::{
    div, ease_in_out, point, prelude::*, px, Animation, AnimationExt as _, Context,
    DragMoveEvent, Entity, Focusable, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent,
    Render, Window, WindowControlArea, actions,
};
#[cfg(test)]
use gpui::{Pixels, Point};
use crate::application::snapshot::{Card, Priority, Status};

const HOLE_ANIM_MS: u64 = 280;
const SETTLE_ANIM_MS: u64 = 140;

actions!(editor_keys, [SaveEditor, CloseEditor]);

pub fn bind_editor_keys(cx: &mut gpui::App) {
    text_field::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("escape", CloseEditor, None),
        KeyBinding::new("ctrl-s", SaveEditor, None),
    ]);
}

#[derive(Clone)]
struct DragCard {
    card: Card,
}

struct DragPreview {
    card: Card,
    width: f32,
}

#[cfg(test)]
fn drag_preview_pad(_cursor_offset: Point<Pixels>) -> Point<Pixels> {
    Point {
        x: px(0.),
        y: px(0.),
    }
}

impl Render for DragPreview {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::Theme::for_window(window);
        div()
            .w(px(self.width))
            .h(px(widgets::card_body_height(
                &self.card.title,
                &self.card.notes,
                (self.width - widgets::COLUMN_PAD_X * 2.0).max(0.0),
            )))
            .overflow_hidden()
            .opacity(0.7)
            .child(widgets::CardTile(&self.card, theme))
    }
}

fn hash_id(id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

fn status_key(status: Status) -> u64 {
    match status {
        Status::Todo => 0,
        Status::Doing => 1,
        Status::Done => 2,
    }
}

fn ghost_origin(pointer_x: f32, pointer_y: f32, grab_x: f32, grab_y: f32) -> (f32, f32) {
    (pointer_x - grab_x, pointer_y - grab_y)
}

fn slot_origin_y(list_origin_y: f32, scroll_y: f32, index: usize, pitches: &[f32]) -> f32 {
    list_origin_y + scroll_y + pitches.iter().take(index).sum::<f32>()
}

fn slot_origin_x(list_origin_x: f32) -> f32 {
    list_origin_x + widgets::COLUMN_PAD_X
}

#[cfg(test)]
fn slot_boundary(pitches: &[f32], index: usize) -> f32 {
    let pitch = pitches.get(index).copied().unwrap_or(0.0);
    pitches.iter().take(index).sum::<f32>() + pitch - widgets::SLOT_GAP / 2.0
}

#[cfg(test)]
fn insert_boundary(index: usize) -> f32 {
    let pitches = vec![widgets::SLOT_PITCH; index + 1];
    slot_boundary(&pitches, index)
}

#[cfg(test)]
fn squeeze_down_y(dest_top: f32) -> f32 {
    dest_top - widgets::SLOT_BODY / 2.0 + widgets::INSERT_HYSTERESIS
}

#[cfg(test)]
fn squeeze_up_y(dest_top: f32) -> f32 {
    dest_top + widgets::SLOT_BODY / 2.0 - widgets::INSERT_HYSTERESIS
}

fn dest_visual_span(
    pitches: &[f32],
    index: usize,
    hole_index: usize,
    hole_pitch: f32,
) -> (f32, f32) {
    let packed_top = pitches.iter().take(index).sum::<f32>();
    let body = pitches
        .get(index)
        .copied()
        .map(|pitch| (pitch - widgets::SLOT_GAP).max(0.0))
        .unwrap_or(0.0);
    let top = if index >= hole_index {
        packed_top + hole_pitch
    } else {
        packed_top
    };
    (top, body)
}

fn dest_mid_inside_ghost(
    ghost_top: f32,
    ghost_bottom: f32,
    dest_mid: f32,
    hysteresis: f32,
) -> bool {
    let h = hysteresis.max(0.0);
    ghost_top + h <= dest_mid && dest_mid <= ghost_bottom - h
}

fn insert_vote_for_dest(
    index: usize,
    ghost_top: f32,
    ghost_bottom: f32,
    dest_top: f32,
    dest_body: f32,
    hole_index: usize,
    hysteresis: f32,
) -> Option<usize> {
    if dest_body <= 0.0 {
        return None;
    }
    let dest_mid = dest_top + dest_body / 2.0;
    if !dest_mid_inside_ghost(ghost_top, ghost_bottom, dest_mid, hysteresis) {
        return None;
    }
    if index < hole_index {
        Some(index)
    } else {
        Some(index + 1)
    }
}

fn ghost_insert_votes(
    ghost_top: f32,
    ghost_body: f32,
    pitches: &[f32],
    hole_index: usize,
    hole_pitch: f32,
    hysteresis: f32,
) -> Vec<usize> {
    let ghost_body = ghost_body.max(0.0);
    let ghost_bottom = ghost_top + ghost_body;
    let mut votes = Vec::new();
    for i in 0..pitches.len() {
        let (top, body) = dest_visual_span(pitches, i, hole_index, hole_pitch);
        if let Some(vote) = insert_vote_for_dest(
            i,
            ghost_top,
            ghost_bottom,
            top,
            body,
            hole_index,
            hysteresis,
        ) {
            if votes.last() != Some(&vote) {
                votes.push(vote);
            }
        }
    }
    votes
}

fn ghost_beyond_dest(
    ghost_top: f32,
    ghost_body: f32,
    pitches: &[f32],
    hole_index: usize,
    hole_pitch: f32,
) -> Option<usize> {
    if pitches.is_empty() {
        return Some(0);
    }
    let ghost_bottom = ghost_top + ghost_body.max(0.0);
    let (first_top, _) = dest_visual_span(pitches, 0, hole_index, hole_pitch);
    let last = pitches.len() - 1;
    let (last_top, last_body) = dest_visual_span(pitches, last, hole_index, hole_pitch);
    if ghost_bottom <= first_top {
        Some(0)
    } else if ghost_top >= last_top + last_body {
        Some(pitches.len())
    } else {
        None
    }
}

#[cfg(test)]
fn raw_insert_from_ghost(ghost_top: f32, ghost_body: f32, pitches: &[f32]) -> usize {
    resolve_insert_from_ghost(
        ghost_top,
        ghost_body,
        pitches,
        None,
        widgets::INSERT_HYSTERESIS,
        0,
        0.0,
    )
}

fn pick_insert_vote(votes: &[usize], last: Option<usize>, hole_index: usize) -> usize {
    let prev = last.unwrap_or(hole_index);
    let up = votes.iter().copied().filter(|&v| v < prev).max();
    let down = votes.iter().copied().filter(|&v| v > prev).min();
    match (up, down) {
        (Some(_), Some(_)) => prev,
        (Some(up), None) => up,
        (None, Some(down)) => down,
        (None, None) => prev,
    }
}

fn resolve_insert_from_ghost(
    ghost_top: f32,
    ghost_body: f32,
    pitches: &[f32],
    last: Option<usize>,
    hysteresis: f32,
    hole_index: usize,
    hole_pitch: f32,
) -> usize {
    if pitches.is_empty() {
        return 0;
    }
    let votes = ghost_insert_votes(
        ghost_top,
        ghost_body,
        pitches,
        hole_index,
        hole_pitch,
        hysteresis,
    );
    if votes.is_empty() {
        ghost_beyond_dest(ghost_top, ghost_body, pitches, hole_index, hole_pitch)
            .or(last)
            .unwrap_or(hole_index.min(pitches.len()))
    } else {
        pick_insert_vote(&votes, last, hole_index)
    }
}

fn hole_elem_id(phase: HolePhase, epoch: u64) -> (&'static str, u64) {
    match phase {
        HolePhase::Hold => ("hole-hold", epoch),
        HolePhase::Open => ("hole-open", epoch),
        HolePhase::Close => ("hole-close", epoch),
    }
}

struct HoleBox {
    height: f32,
    min_height: f32,
    max_height: f32,
}

fn hole_layout_height(phase: HolePhase, delta: f32, pitch: f32) -> f32 {
    let delta = delta.clamp(0.0, 1.0);
    match phase {
        HolePhase::Hold => pitch,
        HolePhase::Open => pitch * delta,
        HolePhase::Close => pitch * (1.0 - delta),
    }
}

fn hole_box(phase: HolePhase, delta: f32, pitch: f32) -> HoleBox {
    let height = hole_layout_height(phase, delta, pitch);
    match phase {
        HolePhase::Hold => HoleBox {
            height,
            min_height: pitch,
            max_height: pitch,
        },
        HolePhase::Open | HolePhase::Close => HoleBox {
            height,
            min_height: 0.0,
            max_height: height,
        },
    }
}

struct SqueezeOverlayBox {
    left: f32,
    width: f32,
}

fn squeeze_overlay_box(list_width: f32) -> SqueezeOverlayBox {
    SqueezeOverlayBox {
        left: widgets::COLUMN_PAD_X,
        width: widgets::displayed_card_width(list_width),
    }
}

fn after_card_before_id(ids: &[String], card_id: &str) -> Option<String> {
    let index = ids.iter().position(|id| id == card_id)?;
    ids.get(index + 1).cloned()
}

fn insert_index(ids: &[String], before_id: Option<&str>) -> usize {
    match before_id {
        Some(id) => ids.iter().position(|x| x == id).unwrap_or(ids.len()),
        None => ids.len(),
    }
}

fn card_slot_index(card_index: usize, hole_index: usize) -> usize {
    card_index + usize::from(card_index >= hole_index)
}

fn layout_pitches(dest_pitches: &[f32], hole_index: usize, hole_pitch: f32) -> Vec<f32> {
    let mut pitches = dest_pitches.to_vec();
    pitches.insert(hole_index.min(pitches.len()), hole_pitch);
    pitches
}

fn layout_slot_origin(
    dest_pitches: &[f32],
    hole_index: usize,
    hole_pitch: f32,
    slot_index: usize,
) -> f32 {
    layout_pitches(dest_pitches, hole_index, hole_pitch)
        .iter()
        .take(slot_index)
        .sum()
}

pub struct SM_UI_HostOps {
    pub request_quit: fn(),
    pub request_frame: fn(),
}

pub enum ColumnItem<'a> {
    Hole { epoch: u64, phase: HolePhase },
    Card(&'a Card),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolePhase {
    Hold,
    Open,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoleSnapshot {
    pub status: Status,
    pub phase: HolePhase,
    pub epoch: u64,
}

struct HoleSlot {
    status: Status,
    before_id: Option<String>,
    epoch: u64,
    phase: HolePhase,
    started: Instant,
}

struct CardSlide {
    id: String,
    status: Status,
    #[allow(dead_code)]
    from_index: usize,
    #[allow(dead_code)]
    to_index: usize,
    from_y: f32,
    to_y: f32,
    epoch: u64,
    started: Instant,
}

struct DragSession {
    id: String,
    to_status: Status,
    before_id: Option<String>,
    body: f32,
    pitch: f32,
    width: f32,
}

struct DropSettle {
    id: String,
    card: Card,
    width: f32,
    from_x: f32,
    from_y: f32,
    to_x: f32,
    to_y: f32,
    epoch: u64,
    started: Instant,
    shown: bool,
}

impl DropSettle {
    fn progress(&mut self, now: Instant) -> f32 {
        if !self.shown {
            self.shown = true;
            self.started = now;
            return 0.0;
        }
        let t = (now.duration_since(self.started).as_secs_f32()
            / Duration::from_millis(SETTLE_ANIM_MS).as_secs_f32())
            .min(1.0);
        1.0 - (1.0 - t).powi(3)
    }
}

struct ScrollbarDrag {
    status: Status,
    track_top: f32,
    track_h: f32,
    grab: f32,
    viewport: f32,
    max_scroll: f32,
}

struct Editor {
    kind: card_editor::EditorKind,
    title: String,
    notes: String,
    deadline: String,
    hour: u32,
    minute: u32,
    priority: Priority,
    error: Option<String>,
}

pub struct SM_UI {
    hsm: SM_Hsm<UI_Controls>,
    controls: UI_Controls,
}

// GPUI listeners borrow this owner. Deref keeps local widget operations on its
// controls; coordination requests enter dispatch_evt through the thread Inbox.
impl Deref for SM_UI {
    type Target = UI_Controls;

    fn deref(&self) -> &Self::Target {
        &self.controls
    }
}

impl DerefMut for SM_UI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.controls
    }
}

impl SM_UI {
    pub fn setup(_host: SM_UI_HostOps, snapshot: Snapshot, client: DataClient) -> Self {
        let mut me = Self {
            hsm: SM_Hsm::new(),
            controls: UI_Controls::new(snapshot, client),
        };
        UI_Controls::init(&mut me.hsm, &mut me.controls);
        me
    }

    pub fn dispatch_evt(&mut self, e: &UI_Evt) {
        UI_Controls::dispatch(&mut self.hsm, &mut self.controls, e);
    }

    pub fn uninstall_tray(window: &Window) {
        tray_icon::uninstall(window);
    }

    #[cfg(test)]
    fn open_create(&mut self) {
        self.dispatch_evt(&UI_Evt::NewCard);
    }

    #[cfg(test)]
    fn open_edit(&mut self, id: &str) {
        self.dispatch_evt(&UI_Evt::EditCard { id: id.into() });
    }

    #[cfg(test)]
    fn close_editor(&mut self) {
        self.dispatch_evt(&UI_Evt::CancelEditor);
    }

    #[cfg(test)]
    fn save_editor(&mut self) -> bool {
        self.dispatch_evt(&UI_Evt::SaveEditor);
        self.editor.is_none()
    }

    #[cfg(test)]
    fn delete_editor(&mut self) -> bool {
        self.dispatch_evt(&UI_Evt::DeleteEditor);
        self.editor.is_none()
    }
}

/// Controls owned by SM_UI's HSM context; all GPUI access stays on the GUI thread.
pub struct UI_Controls {
    snapshot: Snapshot,
    persistence: Persistence,
    persist_wake_pending: bool,
    closing: bool,
    pinned: bool,
    dragging: Option<DragSession>,
    holes: Vec<HoleSlot>,
    hole_epoch: u64,
    slides: Vec<CardSlide>,
    slide_epoch: u64,
    last_hover_status: Option<Status>,
    last_insert_index: Option<usize>,
    column_scroll: [gpui::ScrollHandle; 3],
    grab_x: f32,
    grab_y: f32,
    pointer_x: Option<f32>,
    pointer_y: Option<f32>,
    list_origin_x: Option<f32>,
    list_origin_y: Option<f32>,
    list_scroll_y: f32,
    column_frame: [Option<(f32, f32, f32)>; 3],
    settling: Option<DropSettle>,
    scrollbar_drag: Option<ScrollbarDrag>,
    editor: Option<Editor>,
    title_field: Option<Entity<text_field::TextField>>,
    notes_field: Option<Entity<text_field::TextField>>,
    fields_dirty: bool,
    calendar_open: bool,
    calendar_year: i32,
    calendar_month: u32,
    top_dock: top_edge_dock::TopEdgeDock,
}

impl UI_Controls {
    fn new(snapshot: Snapshot, client: DataClient) -> Self {
        Self {
            snapshot,
            persistence: Persistence::new(client),
            persist_wake_pending: false,
            closing: false,
            pinned: true,
            dragging: None,
            holes: Vec::new(),
            hole_epoch: 0,
            slides: Vec::new(),
            slide_epoch: 0,
            last_hover_status: None,
            last_insert_index: None,
            column_scroll: [
                gpui::ScrollHandle::new(),
                gpui::ScrollHandle::new(),
                gpui::ScrollHandle::new(),
            ],
            grab_x: 0.0,
            grab_y: 0.0,
            pointer_x: None,
            pointer_y: None,
            list_origin_x: None,
            list_origin_y: None,
            list_scroll_y: 0.0,
            column_frame: [None, None, None],
            settling: None,
            scrollbar_drag: None,
            editor: None,
            title_field: None,
            notes_field: None,
            fields_dirty: false,
            calendar_open: false,
            calendar_year: 2026,
            calendar_month: 1,
            top_dock: top_edge_dock::TopEdgeDock::new(),
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub fn toggle_pin(&mut self) -> bool {
        self.pinned = !self.pinned;
        self.pinned
    }

    pub fn apply_always_on_top(&self, window: &Window) {
        window_pin::set_always_on_top(window, self.pinned);
    }

    pub fn apply_skip_taskbar(&self, window: &Window) {
        window_taskbar::set_skip_taskbar(window);
    }

    pub fn add_todo_card(&mut self, title: &str) {
        if let Ok(changes) = self.snapshot.create_card(title, "") {
            self.persistence.stage(changes);
        }
    }

    pub fn editor_is_open(&self) -> bool {
        self.editor.is_some()
    }

    pub fn editor_title(&self) -> Option<&str> {
        self.editor.as_ref().map(|editor| editor.title.as_str())
    }

    pub fn editor_error(&self) -> Option<&str> {
        self.editor
            .as_ref()
            .and_then(|editor| editor.error.as_deref())
    }

    fn open_create(&mut self) {
        let _ = card_editor::command_on_create_click();
        self.editor = Some(Editor {
            kind: card_editor::EditorKind::Create,
            title: String::new(),
            notes: String::new(),
            deadline: String::new(),
            hour: card_meta::DEFAULT_DEADLINE_HOUR,
            minute: card_meta::DEFAULT_DEADLINE_MINUTE,
            priority: Priority::Medium,
            error: None,
        });
        self.fields_dirty = true;
        self.sync_calendar_month();
        self.calendar_open = false;
    }

    fn open_edit(&mut self, id: &str) {
        let Some(card) = self.snapshot.cards.iter().find(|card| card.id == id).cloned() else {
            return;
        };
        let (hour, minute) = card_meta::time_from_deadline(card.deadline_at);
        self.editor = Some(Editor {
            kind: card_editor::EditorKind::Edit { id: card.id },
            title: card.title,
            notes: card.notes,
            deadline: card_meta::date_input_from_deadline(card.deadline_at),
            hour,
            minute,
            priority: card.priority,
            error: None,
        });
        self.fields_dirty = true;
        self.sync_calendar_month();
        self.calendar_open = false;
    }

    fn close_editor(&mut self) {
        let _ = card_editor::command_on_editor_cancel();
        self.editor = None;
        self.calendar_open = false;
    }

    pub fn set_editor_title(&mut self, title: &str) {
        if let Some(editor) = &mut self.editor {
            editor.title = title.to_string();
            editor.error = None;
        }
    }

    pub fn set_editor_notes(&mut self, notes: &str) {
        if let Some(editor) = &mut self.editor {
            editor.notes = notes.to_string();
        }
    }

    pub fn set_editor_deadline(&mut self, date: &str, hour: u32, minute: u32) {
        if let Some(editor) = &mut self.editor {
            editor.deadline = date.to_string();
            editor.hour = hour.min(23);
            editor.minute = if minute >= 30 { 30 } else { 0 };
        }
    }

    pub fn set_editor_priority(&mut self, priority: Priority) {
        if let Some(editor) = &mut self.editor {
            editor.priority = priority;
        }
    }

    fn editor_fields(&self) -> Option<card_editor::EditorFields> {
        let editor = self.editor.as_ref()?;
        Some(card_editor::EditorFields {
            title: editor.title.clone(),
            notes: editor.notes.clone(),
            deadline_at: card_meta::deadline_from_date_and_time(
                &editor.deadline,
                editor.hour,
                editor.minute,
            ),
            priority: editor.priority,
        })
    }

    fn save_editor(&mut self) -> bool {
        let Some(editor) = self.editor.as_ref() else {
            return false;
        };
        let Some(fields) = self.editor_fields() else {
            return false;
        };
        let command = card_editor::command_on_editor_save(&editor.kind, fields);
        let result = match command {
            card_editor::PersistCommand::Create(fields) => self.snapshot.create_card_with(
                &fields.title,
                &fields.notes,
                fields.deadline_at,
                fields.priority,
            ),
            card_editor::PersistCommand::Update { id, fields } => self.snapshot.update_card(
                &id,
                &fields.title,
                &fields.notes,
                fields.deadline_at,
                fields.priority,
            ),
            _ => return false,
        };
        match result {
            Ok(changes) => {
                self.persistence.stage(changes);
                self.editor = None;
                true
            }
            Err(err) => {
                if let Some(editor) = &mut self.editor {
                    editor.error = Some(err.to_string());
                }
                false
            }
        }
    }

    fn delete_editor(&mut self) -> bool {
        let Some(editor) = self.editor.as_ref() else {
            return false;
        };
        match card_editor::command_on_editor_delete(&editor.kind) {
            card_editor::PersistCommand::Delete { id } => {
                let Ok(changes) = self.snapshot.delete_card(&id) else { return false; };
                self.persistence.stage(changes);
                self.editor = None;
                true
            }
            _ => false,
        }
    }

    fn ensure_editor_fields(&mut self, cx: &mut Context<SM_UI>, window: &mut Window) {
        if self.title_field.is_none() {
            self.title_field = Some(cx.new(|cx| text_field::TextField::new(cx, "", 22.0, false)));
            self.notes_field = Some(cx.new(|cx| text_field::TextField::new(cx, "", 96.0, true)));
        }
        if !self.fields_dirty {
            return;
        }
        if let Some(editor) = &self.editor {
            let title = editor.title.clone();
            let notes = editor.notes.clone();
            if let Some(field) = &self.title_field {
                field.update(cx, |field, cx| field.set_text(&title, cx));
            }
            if let Some(field) = &self.notes_field {
                field.update(cx, |field, cx| field.set_text(&notes, cx));
            }
        }
        self.fields_dirty = false;
        if let Some(field) = &self.title_field {
            window.focus(&field.read(cx).focus_handle(cx));
        }
    }

    pub(super) fn pull_editor_fields(&mut self, cx: &mut Context<SM_UI>) {
        // A queued open followed by save can be drained before the first render.
        if self.fields_dirty {
            return;
        }
        let Some(editor) = &mut self.editor else {
            return;
        };
        if let Some(field) = &self.title_field {
            editor.title = field.read(cx).text().to_string();
        }
        if let Some(field) = &self.notes_field {
            editor.notes = field.read(cx).text().to_string();
        }
    }

    fn sync_calendar_month(&mut self) {
        if let Some(ymd) = self
            .editor
            .as_ref()
            .and_then(|editor| calendar::ymd_from_date_string(&editor.deadline))
        {
            self.calendar_year = ymd.year;
            self.calendar_month = ymd.month;
            return;
        }
        let now = chrono::Local::now();
        use chrono::Datelike;
        self.calendar_year = now.year();
        self.calendar_month = now.month();
    }

    pub fn cards_in(&self, status: Status) -> Vec<&Card> {
        self.snapshot.cards
            .iter()
            .filter(|card| card.status == status)
            .collect()
    }

    pub fn column_items(&self, status: Status) -> Vec<ColumnItem<'_>> {
        let dragged = self.dragging.as_ref().map(|d| d.id.as_str());
        let mut items: Vec<ColumnItem<'_>> = self.snapshot
            .cards
            .iter()
            .filter(|card| card.status == status)
            .filter(|card| dragged != Some(card.id.as_str()))
            .map(ColumnItem::Card)
            .collect();
        let mut inserts: Vec<(usize, &HoleSlot)> = self
            .holes
            .iter()
            .filter(|hole| hole.status == status)
            .map(|hole| {
                let idx = match &hole.before_id {
                    Some(before) => items
                        .iter()
                        .position(|item| matches!(item, ColumnItem::Card(card) if card.id == *before))
                        .unwrap_or(items.len()),
                    None => items.len(),
                };
                (idx, hole)
            })
            .collect();
        inserts.sort_by_key(|(idx, _)| *idx);
        for (idx, hole) in inserts.into_iter().rev() {
            items.insert(
                idx,
                ColumnItem::Hole {
                    epoch: hole.epoch,
                    phase: hole.phase,
                },
            );
        }
        items
    }

    pub fn hole_epoch(&self) -> u64 {
        self.hole_epoch
    }

    pub fn hole_snapshots(&self) -> Vec<HoleSnapshot> {
        self.holes
            .iter()
            .map(|hole| HoleSnapshot {
                status: hole.status,
                phase: hole.phase,
                epoch: hole.epoch,
            })
            .collect()
    }

    pub fn start_drag(&mut self, id: &str) {
        if self.closing { return; }
        let Some(index) = self.snapshot.cards.iter().position(|card| card.id == id) else {
            return;
        };
        let status = self.snapshot.cards[index].status;
        let card = &self.snapshot.cards[index];
        let inner = self.card_inner_width(status);
        let body = widgets::card_body_height(&card.title, &card.notes, inner);
        let pitch = body + widgets::SLOT_GAP;
        let width = self.ghost_size_for(status).0;
        let before_id = self.snapshot
            .cards
            .iter()
            .filter(|card| card.status == status)
            .skip_while(|card| card.id != id)
            .nth(1)
            .map(|card| card.id.clone());
        self.dragging = Some(DragSession {
            id: id.to_string(),
            to_status: status,
            before_id: before_id.clone(),
            body,
            pitch,
            width,
        });
        self.holes.clear();
        self.slides.clear();
        self.last_hover_status = None;
        self.last_insert_index = None;
        self.settling = None;
        self.pointer_x = None;
        self.pointer_y = None;
        self.holes.push(HoleSlot {
            status,
            before_id,
            epoch: self.hole_epoch,
            phase: HolePhase::Hold,
            started: Instant::now(),
        });
    }

    pub fn set_grab_offset(&mut self, x: f32, y: f32) {
        self.grab_x = x;
        self.grab_y = y;
    }

    fn ghost_size_for(&self, status: Status) -> (f32, f32) {
        let width_of = |s: Status| {
            widgets::displayed_card_width(f32::from(
                self.column_scroll[status_key(s) as usize]
                    .bounds()
                    .size
                    .width,
            ))
        };
        let width = [status, Status::Todo, Status::Doing, Status::Done]
            .into_iter()
            .map(width_of)
            .find(|w| *w > 0.0)
            .unwrap_or(0.0);
        let body = self
            .dragging
            .as_ref()
            .map(|d| d.body)
            .unwrap_or(widgets::SLOT_BODY);
        (width, body)
    }

    fn list_width(&self, status: Status) -> f32 {
        f32::from(
            self.column_scroll[status_key(status) as usize]
                .bounds()
                .size
                .width,
        )
    }

    fn card_inner_width(&self, _status: Status) -> f32 {
        widgets::shared_inner_width(&[
            self.list_width(Status::Todo),
            self.list_width(Status::Doing),
            self.list_width(Status::Done),
        ])
    }

    fn dest_pitches(&self, status: Status) -> Vec<f32> {
        let inner = self.card_inner_width(status);
        self.snapshot.cards
            .iter()
            .filter(|card| card.status == status)
            .filter(|card| self.dragging.as_ref().map(|d| d.id.as_str()) != Some(card.id.as_str()))
            .map(|card| widgets::card_pitch(&card.title, &card.notes, inner))
            .collect()
    }

    fn drag_body(&self) -> f32 {
        self.dragging
            .as_ref()
            .map(|d| d.body)
            .unwrap_or(widgets::SLOT_BODY)
    }

    fn drag_pitch(&self) -> f32 {
        self.dragging
            .as_ref()
            .map(|d| d.pitch)
            .unwrap_or(widgets::SLOT_PITCH)
    }

    pub fn note_pointer(&mut self, x: f32, y: f32) {
        self.pointer_x = Some(x);
        self.pointer_y = Some(y);
    }

    pub fn note_column_frame(
        &mut self,
        status: Status,
        origin_x: f32,
        origin_y: f32,
        scroll_y: f32,
    ) {
        self.column_frame[status_key(status) as usize] = Some((origin_x, origin_y, scroll_y));
    }

    fn list_frame(&self, status: Status) -> Option<(f32, f32, f32)> {
        let scroll = &self.column_scroll[status_key(status) as usize];
        let bounds = scroll.bounds();
        if f32::from(bounds.size.width) > 0.0 {
            return Some((
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(scroll.offset().y),
            ));
        }
        self.column_frame[status_key(status) as usize].or_else(|| {
            Some((self.list_origin_x?, self.list_origin_y?, self.list_scroll_y))
        })
    }

    pub fn note_drag_geometry(
        &mut self,
        pointer_x: f32,
        pointer_y: f32,
        list_origin_x: f32,
        list_origin_y: f32,
        scroll_y: f32,
    ) {
        self.pointer_x = Some(pointer_x);
        self.pointer_y = Some(pointer_y);
        self.list_origin_x = Some(list_origin_x);
        self.list_origin_y = Some(list_origin_y);
        self.list_scroll_y = scroll_y;
    }

    pub fn settling_id(&self) -> Option<&str> {
        self.settling.as_ref().map(|s| s.id.as_str())
    }

    pub fn card_is_hidden(&self, id: &str) -> bool {
        self.dragging.as_ref().is_some_and(|d| d.id == id)
            || self.settling.as_ref().is_some_and(|s| s.id == id)
            || self.slides.iter().any(|slide| slide.id == id)
    }

    pub fn settling_delta(&self) -> Option<(f32, f32)> {
        self.settling
            .as_ref()
            .map(|s| (s.from_x - s.to_x, s.from_y - s.to_y))
    }

    pub fn expire_settle(&mut self) {
        if let Some(settle) = &mut self.settling {
            settle.shown = true;
            if let Some(started) =
                Instant::now().checked_sub(Duration::from_millis(SETTLE_ANIM_MS + 1))
            {
                settle.started = started;
            }
        }
    }

    #[cfg(test)]
    pub fn age_settle_as_if_persist_was_slow(&mut self) {
        if let Some(settle) = &mut self.settling {
            settle.shown = false;
            if let Some(started) =
                Instant::now().checked_sub(Duration::from_millis(SETTLE_ANIM_MS + 1))
            {
                settle.started = started;
            }
        }
    }

    fn capture_settle(
        &mut self,
        id: &str,
        to_status: Status,
        before_id: Option<&str>,
    ) -> Option<DropSettle> {
        let pointer_x = self.pointer_x?;
        let pointer_y = self.pointer_y?;
        let (origin_x, origin_y, scroll_y) = self.list_frame(to_status)?;
        let ids = self.dest_ids(to_status);
        let index = insert_index(&ids, before_id);
        let (from_x, from_y) = ghost_origin(pointer_x, pointer_y, self.grab_x, self.grab_y);
        let to_x = slot_origin_x(origin_x);
        let to_y = slot_origin_y(origin_y, scroll_y, index, &self.dest_pitches(to_status));
        if from_x == to_x && from_y == to_y {
            return None;
        }
        let card = self.snapshot
            .cards
            .iter()
            .find(|card| card.id == id)
            .cloned()?;
        let width = self
            .dragging
            .as_ref()
            .map(|drag| drag.width)
            .unwrap_or_else(|| self.ghost_size_for(to_status).0);
        self.hole_epoch += 1;
        Some(DropSettle {
            id: id.to_string(),
            card,
            width,
            from_x,
            from_y,
            to_x,
            to_y,
            epoch: self.hole_epoch,
            started: Instant::now(),
            shown: false,
        })
    }

    pub fn hover_insert(&mut self, to_status: Status, before_id: Option<&str>) -> bool {
        let (same_column, dragged_id) = {
            let Some(drag) = self.dragging.as_ref() else {
                return false;
            };
            (drag.to_status == to_status, drag.id.clone())
        };
        if before_id == Some(dragged_id.as_str()) {
            return false;
        }
        let next_before = if same_column {
            let ids = self.dest_ids(to_status);
            let target = insert_index(&ids, before_id);
            if target >= ids.len() {
                None
            } else {
                Some(ids[target].clone())
            }
        } else {
            before_id.map(str::to_string)
        };
        {
            let Some(drag) = self.dragging.as_mut() else {
                return false;
            };
            if drag.to_status == to_status && drag.before_id == next_before {
                return false;
            }
            drag.to_status = to_status;
            drag.before_id = next_before.clone();
        }
        self.relocate_hole(to_status, next_before);
        true
    }

    pub fn hover_insert_after(&mut self, to_status: Status, card_id: &str) -> bool {
        let before = after_card_before_id(&self.dest_ids(to_status), card_id);
        self.hover_insert(to_status, before.as_deref())
    }

    pub fn hover_pointer(&mut self, to_status: Status, y: f32) -> bool {
        if self.dragging.is_none() {
            return false;
        }
        let ids = self.dest_ids(to_status);
        let last = if self.last_hover_status == Some(to_status) {
            self.last_insert_index
        } else {
            None
        };
        let pitches = self.dest_pitches(to_status);
        let hole_index = last.unwrap_or_else(|| {
            insert_index(
                &ids,
                self.dragging
                    .as_ref()
                    .and_then(|d| d.before_id.as_deref()),
            )
        });
        let ghost_top = y - self.grab_y;
        let index = resolve_insert_from_ghost(
            ghost_top,
            self.drag_body(),
            &pitches,
            last,
            widgets::INSERT_HYSTERESIS,
            hole_index,
            self.drag_pitch(),
        );
        self.last_hover_status = Some(to_status);
        self.last_insert_index = Some(index);
        let before = ids.get(index).cloned();
        self.hover_insert(to_status, before.as_deref())
    }

    fn dest_ids(&self, status: Status) -> Vec<String> {
        let dragged = self.dragging.as_ref().map(|d| d.id.as_str());
        self.snapshot.cards
            .iter()
            .filter(|card| card.status == status)
            .filter(|card| dragged != Some(card.id.as_str()))
            .map(|card| card.id.clone())
            .collect()
    }

    pub fn drop_card(&mut self, id: &str, to_status: Status, before_id: Option<&str>) {
        if self.closing { return; }
        let settle = self.capture_settle(id, to_status, before_id);
        let Ok(changes) = self.snapshot.move_card(id, to_status, before_id) else { return; };
        self.persistence.stage(changes);
        self.dragging = None;
        self.last_hover_status = None;
        self.last_insert_index = None;
        self.holes.retain(|hole| hole.phase == HolePhase::Close);
        self.slides.clear();
        self.settling = settle;
    }

    pub fn persist_pending(&self) -> bool {
        self.persistence.has_outbox()
    }

    pub(super) fn post_pending(&mut self) {
        self.persist_wake_pending = false;
        self.persistence.post_pending();
    }

    pub(super) fn begin_close(&mut self) {
        self.cancel_drag();
        self.post_pending();
        self.closing = true;
    }

    pub(super) fn ready_to_close(&self) -> bool {
        self.closing && self.persistence.is_saved()
    }

    pub(super) fn close_failed(&mut self) -> bool {
        if self.closing && self.persistence.error().is_some() {
            self.closing = false;
            return true;
        }
        false
    }

    pub fn commit_drop(&mut self) {
        if let Some((id, status, before)) = self
            .dragging
            .as_ref()
            .map(|drag| (drag.id.clone(), drag.to_status, drag.before_id.clone()))
        {
            self.drop_card(&id, status, before.as_deref());
        }
    }

    pub fn cancel_drag(&mut self) {
        let Some(drag) = self.dragging.take() else {
            return;
        };
        self.last_hover_status = None;
        self.last_insert_index = None;
        self.slides.clear();
        let never_moved = self.holes.iter().all(|hole| hole.phase == HolePhase::Hold);
        if never_moved {
            self.holes.clear();
            return;
        }
        self.hole_epoch += 1;
        let now = Instant::now();
        let dest = drag.to_status;
        self.holes.retain_mut(|hole| {
            if hole.status == dest && hole.phase != HolePhase::Close {
                hole.phase = HolePhase::Close;
                hole.epoch = self.hole_epoch;
                hole.started = now;
                true
            } else {
                false
            }
        });
    }

    fn relocate_hole(&mut self, to_status: Status, before_id: Option<String>) {
        if let Some(idx) = self
            .holes
            .iter()
            .position(|hole| hole.status == to_status && hole.phase != HolePhase::Close)
        {
            let old_before = self.holes[idx].before_id.clone();
            let ids = self.dest_ids(to_status);
            self.arm_slides(to_status, &ids, old_before.as_deref(), before_id.as_deref());
            let hole = &mut self.holes[idx];
            hole.before_id = before_id;
            if hole.phase == HolePhase::Open {
                hole.phase = HolePhase::Hold;
            }
            return;
        }
        self.slides.clear();
        self.holes
            .retain(|hole| !(hole.status == to_status && hole.phase == HolePhase::Close));
        self.hole_epoch += 1;
        let now = Instant::now();
        let epoch = self.hole_epoch;
        for hole in &mut self.holes {
            if hole.phase != HolePhase::Close {
                hole.phase = HolePhase::Close;
                hole.epoch = epoch;
                hole.started = now;
            }
        }
        self.holes.push(HoleSlot {
            status: to_status,
            before_id,
            epoch,
            phase: HolePhase::Open,
            started: now,
        });
    }

    fn arm_slides(
        &mut self,
        status: Status,
        ids: &[String],
        old_before: Option<&str>,
        new_before: Option<&str>,
    ) {
        let old_hole = insert_index(ids, old_before);
        let new_hole = insert_index(ids, new_before);
        self.slide_epoch += 1;
        let epoch = self.slide_epoch;
        let now = Instant::now();
        let dest = self.dest_pitches(status);
        let hole_pitch = self.drag_pitch();
        self.slides = ids
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let from_index = card_slot_index(i, old_hole);
                let to_index = card_slot_index(i, new_hole);
                if from_index == to_index {
                    return None;
                }
                Some(CardSlide {
                    id: id.clone(),
                    status,
                    from_index,
                    to_index,
                    from_y: layout_slot_origin(&dest, old_hole, hole_pitch, from_index),
                    to_y: layout_slot_origin(&dest, new_hole, hole_pitch, to_index),
                    epoch,
                    started: now,
                })
            })
            .collect();
    }

    #[cfg(test)]
    pub fn squeeze_offsets(&self) -> Vec<(String, f32)> {
        self.slides
            .iter()
            .map(|slide| (slide.id.clone(), slide.from_y - slide.to_y))
            .collect()
    }

    #[cfg(test)]
    pub fn squeeze_paths(&self) -> Vec<(String, usize, usize)> {
        self.slides
            .iter()
            .map(|slide| (slide.id.clone(), slide.from_index, slide.to_index))
            .collect()
    }

    #[cfg(test)]
    pub fn squeeze_overlay_from_to(&self) -> Vec<(f32, f32)> {
        self.slides
            .iter()
            .map(|slide| (slide.from_y, slide.to_y))
            .collect()
    }

    pub fn flush(&mut self) -> Option<UI_Evt> {
        self.snapshot_column_frames();
        let hole_limit = Duration::from_millis(HOLE_ANIM_MS);
        self.holes
            .retain(|hole| hole.phase != HolePhase::Close || hole.started.elapsed() < hole_limit);
        self.slides
            .retain(|slide| slide.started.elapsed() < hole_limit);
        let settle_limit = Duration::from_millis(SETTLE_ANIM_MS);
        let can_post = match self.settling.as_ref().map(|settle| settle.shown) {
            Some(false) => false,
            Some(true) => {
                if self.settling.as_ref().is_some_and(|settle| settle.started.elapsed() >= settle_limit) {
                    self.settling = None;
                }
                true
            }
            None => true,
        };
        if can_post && self.persistence.has_outbox() && !self.persist_wake_pending {
            self.persist_wake_pending = true;
            Some(UI_Evt::PersistPending)
        } else {
            None
        }
    }

    fn snapshot_column_frames(&mut self) {
        for i in 0..3 {
            let bounds = self.column_scroll[i].bounds();
            if f32::from(bounds.size.width) > 0.0 {
                self.column_frame[i] = Some((
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(self.column_scroll[i].offset().y),
                ));
            }
        }
    }

    fn titlebar(&self, window: &Window, theme: theme::Theme, cx: &mut Context<SM_UI>) -> impl IntoElement {
        let active = window.is_window_active();
        let close = if active { 0xff5f57 } else { theme.traffic_dim };
        let min = if active { 0xfebc2e } else { theme.traffic_dim };
        let max = if active { 0x28c840 } else { theme.traffic_dim };
        let pin_bg = theme.pin_bg(self.pinned);
        let pin_fg = theme.pin_fg(self.pinned);
        let pin_hover = theme.pin_hover(self.pinned);
        div()
            .relative()
            .flex()
            .w_full()
            .h(px(42.))
            .flex_none()
            .items_center()
            .px_3()
            .border_b_1()
            .border_color(theme.line())
            .child(widgets::titlebar_window_title("SunnyMatato", theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pr_2()
                    .child(widgets::traffic_light("close", Some(WindowControlArea::Close), close))
                    .child(widgets::traffic_light("min", None, min).on_click(cx.listener(
                        |_, _, _, cx| {
                            UI_Inbox_post(UI_Evt::HideToTray);
                            cx.notify();
                        },
                    )))
                    .child(widgets::traffic_light("max", Some(WindowControlArea::Max), max)),
            )
            .child(widgets::titlebar_drag("titlebar-drag"))
            .child(
                div()
                    .id("pin-window")
                    .w(px(32.))
                    .h(px(32.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(pin_bg)
                    .text_color(pin_fg)
                    .text_xs()
                    .hover(move |this| this.bg(pin_hover))
                    .on_click(cx.listener(|_, _, _, cx| {
                        UI_Inbox_post(UI_Evt::TogglePin);
                        cx.notify();
                    }))
                    .child(if self.pinned { "●" } else { "○" }),
            )
    }

    fn move_scrollbar_drag(&mut self, pointer_y: f32) -> bool {
        let Some(drag) = self.scrollbar_drag.as_ref() else {
            return false;
        };
        let thumb_top = pointer_y - drag.track_top - drag.grab;
        let scroll_y = widgets::scroll_y_from_thumb_top(
            thumb_top,
            drag.viewport,
            drag.max_scroll,
            drag.track_h,
        );
        let status = drag.status;
        self.column_scroll[status_key(status) as usize]
            .set_offset(point(px(0.), px(-scroll_y)));
        true
    }

    fn column_scrollbar(
        &self,
        status: Status,
        theme: theme::Theme,
        cx: &mut Context<SM_UI>,
    ) -> impl IntoElement {
        let scroll = &self.column_scroll[status_key(status) as usize];
        let viewport = f32::from(scroll.bounds().size.height);
        let max_scroll = f32::from(scroll.max_offset().height);
        let scroll_y = -f32::from(scroll.offset().y);
        let Some((top, thumb_h)) =
            widgets::scrollbar_thumb(viewport, max_scroll, scroll_y, viewport)
        else {
            return div().into_any_element();
        };
        let track_top = f32::from(scroll.bounds().origin.y);
        div()
            .id(("col-scroll", status_key(status)))
            .absolute()
            .top_0()
            .right_0()
            .w(px(8.))
            .h_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    let y = f32::from(ev.position.y) - track_top;
                    if y >= top && y <= top + thumb_h {
                        this.scrollbar_drag = Some(ScrollbarDrag {
                            status,
                            track_top,
                            track_h: viewport,
                            grab: (y - top).clamp(0.0, thumb_h),
                            viewport,
                            max_scroll,
                        });
                    } else {
                        let scroll_y = widgets::scroll_y_from_thumb_top(
                            y - thumb_h / 2.0,
                            viewport,
                            max_scroll,
                            viewport,
                        );
                        this.column_scroll[status_key(status) as usize]
                            .set_offset(point(px(0.), px(-scroll_y)));
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(top))
                    .right(px(1.))
                    .w(px(6.))
                    .h(px(thumb_h))
                    .rounded(px(3.))
                    .bg(theme.line_strong()),
            )
            .into_any_element()
    }

    fn render_column(
        &self,
        title: &'static str,
        status: Status,
        theme: theme::Theme,
        cx: &mut Context<SM_UI>,
    ) -> impl IntoElement {
        let count = self.cards_in(status).len();
        let mut kids = Vec::new();
        for item in self.column_items(status) {
            kids.push(match item {
                ColumnItem::Hole { epoch, phase } => {
                    self.hole_element(epoch, phase, theme, cx).into_any_element()
                }
                ColumnItem::Card(card) => {
                    self.card_element(card, status, theme, cx).into_any_element()
                }
            });
        }
        kids.push(self.append_slot(status, cx).into_any_element());
        for overlay in self.squeeze_overlays_in(status, theme) {
            kids.push(overlay);
        }
        let scroll = self.column_scroll[status_key(status) as usize].clone();
        let list = div()
            .id(title)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .px_3()
            .pb_3()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<DragCard>, _, cx| {
                    if this.dragging.is_none() {
                        return;
                    }
                    this.note_pointer(
                        f32::from(event.event.position.x),
                        f32::from(event.event.position.y),
                    );
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let local = event.event.position.y - event.bounds.origin.y;
                    let scrolled =
                        this.column_scroll[status_key(status) as usize].offset().y;
                    let y = f32::from(local - scrolled);
                    this.note_drag_geometry(
                        f32::from(event.event.position.x),
                        f32::from(event.event.position.y),
                        f32::from(event.bounds.origin.x),
                        f32::from(event.bounds.origin.y),
                        f32::from(scrolled),
                    );
                    this.note_column_frame(
                        status,
                        f32::from(event.bounds.origin.x),
                        f32::from(event.bounds.origin.y),
                        f32::from(scrolled),
                    );
                    if this.hover_pointer(status, y) {
                        cx.notify();
                    }
                },
            ))
            .children(kids);
        let body = div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .child(list)
            .child(self.column_scrollbar(status, theme, cx));
        widgets::Column(title, count, body, theme)
            .drag_over::<DragCard>({
                let gold_soft = theme.gold_soft();
                move |style, _, _, _| style.bg(gold_soft)
            })
            .on_drop(cx.listener(move |this, info: &DragCard, window, cx| {
                let pointer = window.mouse_position();
                this.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                let hovered_here = this
                    .dragging
                    .as_ref()
                    .is_some_and(|d| d.to_status == status);
                if hovered_here {
                    this.commit_drop();
                } else {
                    this.drop_card(&info.card.id, status, None);
                }
                cx.notify();
            }))
    }

    fn card_element(
        &self,
        card: &Card,
        status: Status,
        theme: theme::Theme,
        cx: &mut Context<SM_UI>,
    ) -> impl IntoElement {
        let id = card.id.clone();
        let payload = DragCard {
            card: card.clone(),
        };
        let view = cx.entity().downgrade();
        let hidden = self.card_is_hidden(&id);
        let inner = self.card_inner_width(status);
        let body = widgets::card_body_height(&card.title, &card.notes, inner);
        let pitch = body + widgets::SLOT_GAP;
        let tile = div()
            .w_full()
            .flex_none()
            .h(px(body))
            .min_h(px(body))
            .opacity(if hidden { 0. } else { 1. })
            .child(widgets::CardTile(card, theme));
        div()
            .id(("card", hash_id(&id)))
            .w_full()
            .flex_none()
            .h(px(pitch))
            .min_h(px(pitch))
            .overflow_hidden()
            .cursor_move()
            .on_drag(payload, {
                let view = view.clone();
                move |info, offset, window, cx| {
                    let width = view
                        .update(cx, |this, cx| {
                            this.start_drag(&info.card.id);
                            this.set_grab_offset(f32::from(offset.x), f32::from(offset.y));
                            this.note_pointer(
                                f32::from(window.mouse_position().x),
                                f32::from(window.mouse_position().y),
                            );
                            let this = &mut this.controls;
                            this.note_column_frame(
                                status,
                                f32::from(
                                    this.column_scroll[status_key(status) as usize]
                                        .bounds()
                                        .origin
                                        .x,
                                ),
                                f32::from(
                                    this.column_scroll[status_key(status) as usize]
                                        .bounds()
                                        .origin
                                        .y,
                                ),
                                f32::from(
                                    this.column_scroll[status_key(status) as usize]
                                        .offset()
                                        .y,
                                ),
                            );
                            cx.notify();
                            this.ghost_size_for(status).0
                        })
                        .unwrap_or(0.0);
                    cx.new(|_| DragPreview {
                        card: info.card.clone(),
                        width,
                    })
                }
            })
            .on_drop(cx.listener(move |this, _: &DragCard, window, cx| {
                let pointer = window.mouse_position();
                this.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                this.commit_drop();
                cx.notify();
            }))
            .on_click(cx.listener({
                let id = id.clone();
                move |this, _, _, cx| {
                    if this.dragging.is_some() {
                        return;
                    }
                    UI_Inbox_post(UI_Evt::EditCard { id: id.clone() });
                    cx.notify();
                }
            }))
            .child(tile)
    }

    fn hole_element(
        &self,
        epoch: u64,
        phase: HolePhase,
        theme: theme::Theme,
        cx: &mut Context<SM_UI>,
    ) -> impl IntoElement {
        let id = hole_elem_id(phase, epoch);
        let pitch = self.drag_pitch();
        let body = self.drag_body();
        let slot = match phase {
            HolePhase::Hold => {
                let spec = hole_box(phase, 1.0, pitch);
                div()
                    .id(id)
                    .w_full()
                    .flex_none()
                    .overflow_hidden()
                    .h(px(spec.height))
                    .min_h(px(spec.min_height))
                    .max_h(px(spec.max_height))
                    .on_drop(cx.listener(move |this, _: &DragCard, window, cx| {
                        let pointer = window.mouse_position();
                        this.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                        this.commit_drop();
                        cx.notify();
                    }))
                    .child(widgets::Hole(theme, body))
                    .into_any_element()
            }
            HolePhase::Open | HolePhase::Close => div()
                .id(id)
                .w_full()
                .flex_none()
                .overflow_hidden()
                .min_h_0()
                .on_drop(cx.listener(move |this, _: &DragCard, window, cx| {
                    let pointer = window.mouse_position();
                    this.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                    this.commit_drop();
                    cx.notify();
                }))
                .child(widgets::Hole(theme, body))
                .with_animation(
                    id,
                    Animation::new(Duration::from_millis(HOLE_ANIM_MS)).with_easing(ease_in_out),
                    move |el, delta| {
                        let spec = hole_box(phase, delta, pitch);
                        el.h(px(spec.height))
                            .min_h(px(spec.min_height))
                            .max_h(px(spec.max_height))
                    },
                )
                .into_any_element(),
        };
        slot
    }

    fn append_slot(&self, status: Status, cx: &mut Context<SM_UI>) -> impl IntoElement {
        div()
            .id(("append", status_key(status)))
            .flex_1()
            .w_full()
            .min_h(px(32.))
            .on_drop(cx.listener(move |this, info: &DragCard, window, cx| {
                let pointer = window.mouse_position();
                this.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                this.drop_card(&info.card.id, status, None);
                cx.notify();
            }))
    }

    fn squeeze_overlays_in(&self, status: Status, theme: theme::Theme) -> Vec<gpui::AnyElement> {
        let list_width = f32::from(
            self.column_scroll[status_key(status) as usize]
                .bounds()
                .size
                .width,
        );
        let box_ = squeeze_overlay_box(list_width);
        let left = box_.left;
        let width = box_.width;
        let inner = self.card_inner_width(status);
        self.slides
            .iter()
            .filter(|slide| slide.status == status)
            .filter_map(|slide| {
                let card = self.snapshot.cards.iter().find(|card| card.id == slide.id)?;
                let from_y = slide.from_y;
                let to_y = slide.to_y;
                let body = widgets::card_body_height(&card.title, &card.notes, inner);
                let id = (
                    "card-slide",
                    slide
                        .epoch
                        .wrapping_mul(1_000_003)
                        .wrapping_add(hash_id(&slide.id)),
                );
                let lock_width = move |el: gpui::Stateful<gpui::Div>| {
                    if width > 0.0 {
                        el.left(px(left)).w(px(width)).max_w(px(width))
                    } else {
                        el.left(px(left)).right(px(left))
                    }
                };
                Some(
                    lock_width(
                        div()
                            .id(id)
                            .absolute()
                            .top(px(from_y))
                            .h(px(body))
                            .overflow_hidden(),
                    )
                    .child(widgets::CardTile(card, theme))
                    .with_animation(
                        id,
                        Animation::new(Duration::from_millis(HOLE_ANIM_MS))
                            .with_easing(ease_in_out),
                        move |el, delta| {
                            lock_width(el).top(px(from_y + (to_y - from_y) * delta))
                        },
                    )
                    .into_any_element(),
                )
            })
            .collect()
    }

    fn settle_overlay(&mut self, window: &mut Window, theme: theme::Theme) -> impl IntoElement {
        let Some(settle) = &mut self.settling else {
            return div().into_any_element();
        };
        let delta = settle.progress(Instant::now());
        let origin = point(
            px(settle.from_x + (settle.to_x - settle.from_x) * delta),
            px(settle.from_y + (settle.to_y - settle.from_y) * delta),
        );
        let card = settle.card.clone();
        let width = settle.width;
        let id = ("drop-settle", settle.epoch);
        window.request_animation_frame();
        // Match GPUI's drag preview: apply the window-space origin after layout,
        // preserving fractional pixels instead of rounding absolute left/top.
        gpui::canvas(
            move |_, window, cx| {
                let mut element = div()
                    .id(id)
                    .w(px(width))
                    .h(px(widgets::card_body_height(
                        &card.title,
                        &card.notes,
                        (width - widgets::COLUMN_PAD_X * 2.0).max(0.0),
                    )))
                    .overflow_hidden()
                    .opacity(0.7 + 0.3 * delta)
                    .child(widgets::CardTile(&card, theme))
                    .into_any_element();
                element.prepaint_as_root(origin, gpui::AvailableSpace::min_size(), window, cx);
                element
            },
            |_, mut element, window, cx| element.paint(window, cx),
        )
        .absolute()
        .size_full()
        .into_any_element()
    }

    fn editor_overlay(&self, theme: theme::Theme, cx: &mut Context<SM_UI>) -> impl IntoElement {
        let Some(editor) = &self.editor else {
            return div().into_any_element();
        };
        let heading = match &editor.kind {
            card_editor::EditorKind::Create => "新建卡片",
            card_editor::EditorKind::Edit { .. } => "编辑卡片",
        };
        let can_delete = matches!(editor.kind, card_editor::EditorKind::Edit { .. });
        let hour = editor.hour;
        let minute = editor.minute;
        let has_date = !editor.deadline.is_empty();
        let priority = editor.priority;
        let error = editor.error.clone();
        let mut form = div()
            .id("card-editor")
            .w(px(440.))
            .max_w(px(440.))
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_4()
            .rounded(px(20.))
            .border_1()
            .border_color(theme.line())
            .bg(theme.card())
            .p(px(28.))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(div().text_xl().child(heading))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .min_w_0()
                    .child(div().text_xs().text_color(theme.ink_soft()).child("标题"))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .children(self.title_field.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .min_w_0()
                    .child(div().text_xs().text_color(theme.ink_soft()).child("备注"))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .children(self.notes_field.clone()),
                    ),
            )
            .child({
                let mut deadline = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .min_w_0()
                    .child(div().text_xs().text_color(theme.ink_soft()).child("截止日期"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(date_trigger(&editor.deadline, theme, cx))
                            .child(hour_stepper(hour, minute, has_date, theme, cx)),
                    );
                if self.calendar_open {
                    deadline = deadline.child(month_calendar(
                        self.calendar_year,
                        self.calendar_month,
                        calendar::ymd_from_date_string(&editor.deadline),
                        theme,
                        cx,
                    ));
                }
                deadline
            })
            .child(priority_row(priority, theme, cx));
        if let Some(error) = error {
            form = form.child(div().text_xs().text_color(theme.danger()).child(error));
        }
        form = form.child(editor_actions(can_delete, theme, cx));
        div()
            .id("editor-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.backdrop())
            .occlude()
            .on_click(cx.listener(|_, _, _, cx| {
                UI_Inbox_post(UI_Evt::CancelEditor);
                cx.notify();
            }))
            .child(form)
            .into_any_element()
    }
}

fn date_trigger(
    deadline: &str,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let label = if deadline.is_empty() {
        "选择日期".to_string()
    } else {
        deadline.to_string()
    };
    let muted = deadline.is_empty();
    div()
        .id("deadline-date")
        .flex_1()
        .min_w_0()
        .h(px(36.))
        .px_3()
        .rounded_lg()
        .border_1()
        .border_color(theme.line_strong())
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .on_click(cx.listener(|this, _, _, cx| {
            this.calendar_open = !this.calendar_open;
            if this.calendar_open {
                this.sync_calendar_month();
            }
            cx.notify();
        }))
        .child(
            div()
                .text_sm()
                .text_color(if muted {
                    theme.ink_faint()
                } else {
                    theme.ink()
                })
                .child(label),
        )
        .child(
            div()
                .id("deadline-clear")
                .text_xs()
                .text_color(theme.ink_faint())
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(editor) = &mut this.editor {
                        editor.deadline.clear();
                    }
                    this.calendar_open = false;
                    cx.notify();
                }))
                .child(if muted { "" } else { "清除" }),
        )
}

fn month_calendar(
    year: i32,
    month: u32,
    selected: Option<calendar::Ymd>,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let cells = calendar::month_cells(year, month);
    let heading = format!("{year}年{month}月");
    let weekdays = ["一", "二", "三", "四", "五", "六", "日"];
    let mut grid = div().flex().flex_col().gap_1();
    let mut header = div().flex();
    for day in weekdays {
        header = header.child(
            div()
                .flex_1()
                .text_center()
                .text_xs()
                .text_color(theme.ink_faint())
                .child(day),
        );
    }
    grid = grid.child(header);
    for week in 0..6 {
        let mut row = div().flex();
        for col in 0..7 {
            let cell = cells[week * 7 + col];
            row = row.child(day_cell(cell, selected, theme, cx));
        }
        grid = grid.child(row);
    }
    div()
        .mt_1()
        .p_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.line())
        .bg(theme.card())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb_2()
                .child(step_btn("cal-prev", "‹", true, theme, cx, |this| {
                    let (y, m) = calendar::shift_month(this.calendar_year, this.calendar_month, -1);
                    this.calendar_year = y;
                    this.calendar_month = m;
                }))
                .child(div().text_sm().child(heading))
                .child(step_btn("cal-next", "›", true, theme, cx, |this| {
                    let (y, m) = calendar::shift_month(this.calendar_year, this.calendar_month, 1);
                    this.calendar_year = y;
                    this.calendar_month = m;
                })),
        )
        .child(grid)
}

fn day_cell(
    cell: calendar::MonthCell,
    selected: Option<calendar::Ymd>,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let on = selected == Some(cell.ymd);
    let fg = if !cell.in_month {
        theme.ink_faint()
    } else if on {
        theme.on_primary()
    } else {
        theme.ink()
    };
    let bg = if on { theme.gold() } else { theme.card() };
    let ymd = cell.ymd;
    div()
        .id(("cal-day", (ymd.year as u64) << 16 | (ymd.month as u64) << 8 | ymd.day as u64))
        .flex_1()
        .h(px(28.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(fg)
        .bg(bg)
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(editor) = &mut this.editor {
                editor.deadline = calendar::date_string_from_ymd(ymd);
            }
            this.calendar_year = ymd.year;
            this.calendar_month = ymd.month;
            this.calendar_open = false;
            cx.notify();
        }))
        .child(format!("{}", ymd.day))
}

fn hour_stepper(
    hour: u32,
    minute: u32,
    enabled: bool,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let label = format!("{hour:02}:{minute:02}");
    div()
        .flex()
        .items_center()
        .gap_1()
        .opacity(if enabled { 1. } else { 0.45 })
        .child(step_btn("hour-dec", "−", enabled, theme, cx, |this| {
            if let Some(editor) = &mut this.editor {
                let (hour, minute) = card_meta::step_half_hour(editor.hour, editor.minute, -1);
                editor.hour = hour;
                editor.minute = minute;
            }
        }))
        .child(
            div()
                .w(px(56.))
                .text_center()
                .text_sm()
                .child(label),
        )
        .child(step_btn("hour-inc", "+", enabled, theme, cx, |this| {
            if let Some(editor) = &mut this.editor {
                let (hour, minute) = card_meta::step_half_hour(editor.hour, editor.minute, 1);
                editor.hour = hour;
                editor.minute = minute;
            }
        }))
}

fn step_btn(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
    on_press: impl Fn(&mut SM_UI) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(28.))
        .h(px(28.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(theme.line_strong())
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            if enabled {
                on_press(this);
                cx.notify();
            }
        }))
        .child(label)
}

fn priority_row(
    selected: Priority,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_xs().text_color(theme.ink_soft()).child("优先级"))
        .child(
            div()
                .flex()
                .gap_2()
                .child(priority_choice(
                    "prio-low",
                    "低",
                    Priority::Low,
                    selected,
                    theme,
                    cx,
                ))
                .child(priority_choice(
                    "prio-med",
                    "中",
                    Priority::Medium,
                    selected,
                    theme,
                    cx,
                ))
                .child(priority_choice(
                    "prio-high",
                    "高",
                    Priority::High,
                    selected,
                    theme,
                    cx,
                )),
        )
}

fn priority_choice(
    id: &'static str,
    label: &'static str,
    value: Priority,
    selected: Priority,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let on = selected == value;
    let (fg, bg) = if on {
        theme.priority_tag(value)
    } else {
        (theme.ink(), theme.clear())
    };
    let border = if on {
        theme.clear()
    } else {
        theme.line_strong()
    };
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_xs()
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_editor_priority(value);
            cx.notify();
        }))
        .child(label)
}

fn editor_actions(
    can_delete: bool,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let mut row = div().flex().items_center().gap_2().pt_2();
    if can_delete {
        row = row.child(
            div()
                .id("editor-delete")
                .flex_1()
                .text_xs()
                .text_color(theme.danger())
                .cursor_pointer()
                .on_click(cx.listener(|_, _, _, cx| {
                    UI_Inbox_post(UI_Evt::DeleteEditor);
                    cx.notify();
                }))
                .child("删除"),
        );
    } else {
        row = row.child(div().flex_1());
    }
    row.child(
        div()
            .id("editor-cancel")
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.line_strong())
            .text_xs()
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| {
                UI_Inbox_post(UI_Evt::CancelEditor);
                cx.notify();
            }))
            .child("取消"),
    )
    .child(
        div()
            .id("editor-save")
            .px_3()
            .py_1()
            .rounded_md()
            .bg(theme.gold())
            .text_color(theme.on_primary())
            .text_xs()
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| {
                UI_Inbox_post(UI_Evt::SaveEditor);
                cx.notify();
            }))
            .child("保存"),
    )
}

impl Render for SM_UI {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.controls.render_controls(window, cx)
    }
}

impl UI_Controls {
    fn persistence_status(&self, theme: theme::Theme, cx: &mut Context<SM_UI>) -> gpui::AnyElement {
        if let Some(error) = self.persistence.error() {
            div().id("persistence-error").flex().gap_2().text_xs().text_color(theme.danger())
                .child(format!("未保存：{error}"))
                .child(div().id("retry-persistence").cursor_pointer().child("重试")
                    .on_click(cx.listener(|_, _, _, _| UI_Inbox_post(UI_Evt::RetryPersistence))))
                .into_any_element()
        } else {
            div().into_any_element()
        }
    }

    fn render_controls(&mut self, window: &mut Window, cx: &mut Context<SM_UI>) -> impl IntoElement {
        // Preserve the first landing frame before scheduling persistence.
        if let Some(e) = self.flush() {
            UI_Inbox_post(e);
        }
        self.wire_top_edge_dock(window, cx);
        self.sync_window_hover(window, cx);
        self.sync_dock_hold_open(window, cx);
        tray_icon::install(window);
        if self.editor.is_some() {
            self.ensure_editor_fields(cx, window);
        }
        let theme = theme::Theme::for_window(window);
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg())
            .text_color(theme.ink())
            .on_action(cx.listener(|this, _: &SaveEditor, _, cx| {
                if this.editor.is_some() {
                    UI_Inbox_post(UI_Evt::SaveEditor);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &CloseEditor, _, cx| {
                if this.editor.is_some() {
                    UI_Inbox_post(UI_Evt::CancelEditor);
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.dragging.is_some() {
                    this.note_pointer(
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                }
                if this.move_scrollbar_drag(f32::from(event.position.y)) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.scrollbar_drag = None;
                    if this.dragging.is_some() {
                        this.cancel_drag();
                    }
                    cx.notify();
                }),
            )
            .child(self.titlebar(window, theme, cx))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_3()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(theme.line())
                    .child(div().text_xl().text_color(theme.gold()).child("TODO"))
                    .child(
                        div()
                            .id("add-card")
                            .relative()
                            .size(px(32.))
                            .rounded(px(16.))
                            .bg(theme.gold())
                            .cursor_pointer()
                            .hover(|this| this.bg(theme.gold_hover()))
                            .on_click(cx.listener(|_, _, _, cx| {
                                UI_Inbox_post(UI_Evt::NewCard);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(9.5))
                                    .top(px(14.8))
                                    .w(px(13.))
                                    .h(px(2.4))
                                    .rounded(px(1.2))
                                    .bg(theme.on_primary()),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(px(14.8))
                                    .top(px(9.5))
                                    .w(px(2.4))
                                    .h(px(13.))
                                    .rounded(px(1.2))
                                    .bg(theme.on_primary()),
                            ),
                    )
                    .child(self.persistence_status(theme, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .p_6()
                    .child(self.render_column("待办", Status::Todo, theme, cx))
                    .child(self.render_column("进行中", Status::Doing, theme, cx))
                    .child(self.render_column("完成", Status::Done, theme, cx)),
            )
            .child(self.settle_overlay(window, theme))
            .child(self.editor_overlay(theme, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::data_manager::DataManager;

    fn test_ui() -> (DataManager, SM_UI) {
        let (database, cards) = DataManager::open_in_memory(|_| {}).unwrap();
        let ui = SM_UI::setup(host(), Snapshot::new(cards), database.client());
        (database, ui)
    }

    fn flush(sm: &mut SM_UI) {
        if let Some(e) = sm.controls.flush() {
            sm.dispatch_evt(&e);
        }
    }

    fn host() -> SM_UI_HostOps {
        SM_UI_HostOps {
            request_quit: || {},
            request_frame: || {},
        }
    }

    fn titles(sm: &SM_UI, status: Status) -> Vec<&str> {
        sm.cards_in(status)
            .iter()
            .map(|card| card.title.as_str())
            .collect()
    }

    fn ids(sm: &SM_UI, status: Status) -> Vec<String> {
        sm.cards_in(status)
            .iter()
            .map(|card| card.id.clone())
            .collect()
    }

    fn column_marks(sm: &SM_UI, status: Status) -> Vec<String> {
        sm.column_items(status)
            .into_iter()
            .map(|item| match item {
                ColumnItem::Hole { .. } => "□".to_string(),
                ColumnItem::Card(card) => card.title.clone(),
            })
            .collect()
    }

    fn squeeze_offsets(sm: &SM_UI) -> Vec<(String, f32)> {
        sm.squeeze_offsets()
            .into_iter()
            .map(|(id, dy)| {
                let title = sm.snapshot
                    .cards
                    .iter()
                    .find(|card| card.id == id)
                    .map(|card| card.title.clone())
                    .unwrap_or(id);
                (title, dy)
            })
            .collect()
    }

    fn squeeze_paths(sm: &SM_UI) -> Vec<(String, usize, usize)> {
        sm.squeeze_paths()
            .into_iter()
            .map(|(id, from, to)| {
                let title = sm.snapshot
                    .cards
                    .iter()
                    .find(|card| card.id == id)
                    .map(|card| card.title.clone())
                    .unwrap_or(id);
                (title, from, to)
            })
            .collect()
    }

    #[test]
    fn drag_preview_does_not_pad_cursor_offset() {
        let grab = Point {
            x: px(24.),
            y: px(16.),
        };
        assert_eq!(
            drag_preview_pad(grab),
            Point {
                x: px(0.),
                y: px(0.),
            }
        );
    }

    #[test]
    fn window_starts_pinned() {
        let (_database, sm) = test_ui();
        assert!(sm.is_pinned());
    }

    #[test]
    fn editor_or_drag_holds_the_top_dock_open() {
        let (_database, mut sm) = test_ui();
        assert!(!sm.dock_hold_open());
        sm.open_create();
        assert!(sm.dock_hold_open());
        sm.close_editor();
        assert!(!sm.dock_hold_open());
        sm.add_todo_card("A");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        assert!(sm.dock_hold_open());
        sm.cancel_drag();
        assert!(!sm.dock_hold_open());
    }

    #[test]
    fn toggle_pin_flips_always_on_top_flag() {
        let (_database, mut sm) = test_ui();
        assert!(!sm.toggle_pin());
        assert!(!sm.is_pinned());
        assert!(sm.toggle_pin());
        assert!(sm.is_pinned());
    }

    #[test]
    fn add_todo_card_lands_in_todo() {
        let (_database, mut sm) = test_ui();
        assert!(sm.cards_in(Status::Todo).is_empty());
        sm.add_todo_card("Hello");
        assert_eq!(sm.cards_in(Status::Todo).len(), 1);
        assert_eq!(sm.cards_in(Status::Todo)[0].title, "Hello");
        assert!(sm.cards_in(Status::Doing).is_empty());
        assert!(sm.cards_in(Status::Done).is_empty());
    }

    #[test]
    fn drop_card_to_doing_appends() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.drop_card(&a, Status::Doing, None);
        assert_eq!(titles(&sm, Status::Todo), vec!["B"]);
        assert_eq!(titles(&sm, Status::Doing), vec!["A"]);
    }

    #[test]
    fn drop_card_inserts_before_target() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let todo = ids(&sm, Status::Todo);
        sm.drop_card(&todo[2], Status::Todo, Some(&todo[0]));
        assert_eq!(titles(&sm, Status::Todo), vec!["C", "A", "B"]);
    }

    #[test]
    fn start_drag_leaves_hole_in_place() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        assert_eq!(column_marks(&sm, Status::Todo), vec!["□", "B"]);
        assert_eq!(titles(&sm, Status::Todo), vec!["A", "B"]);
    }

    #[test]
    fn hover_doing_moves_hole_without_persist() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.hover_insert(Status::Doing, None);
        assert_eq!(column_marks(&sm, Status::Todo), vec!["□", "B"]);
        assert_eq!(column_marks(&sm, Status::Doing), vec!["□"]);
        assert_eq!(titles(&sm, Status::Todo), vec!["A", "B"]);
        assert!(sm.cards_in(Status::Doing).is_empty());
    }

    #[test]
    fn commit_drop_persists_hovered_slot() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.hover_insert(Status::Doing, None);
        sm.commit_drop();
        assert_eq!(titles(&sm, Status::Todo), vec!["B"]);
        assert_eq!(titles(&sm, Status::Doing), vec!["A"]);
        assert_eq!(column_marks(&sm, Status::Doing), vec!["A"]);
    }

    #[test]
    fn drop_keeps_store_write_off_the_release_frame() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.set_grab_offset(12.0, 8.0);
        sm.note_drag_geometry(200.0, 90.0, 40.0, 50.0, 0.0);
        sm.hover_insert(Status::Doing, None);
        sm.commit_drop();
        assert_eq!(titles(&sm, Status::Doing), vec!["A"]);
        assert!(sm.persist_pending());
        flush(&mut sm);
        assert!(sm.persist_pending());
        sm.settling.as_mut().unwrap().progress(Instant::now());
        flush(&mut sm);
        assert!(!sm.persist_pending());
        assert_eq!(titles(&sm, Status::Doing), vec!["A"]);
    }

    #[test]
    fn cancel_drag_restores_without_persist() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.hover_insert(Status::Doing, None);
        sm.cancel_drag();
        assert_eq!(titles(&sm, Status::Todo), vec!["A", "B"]);
        assert!(sm.cards_in(Status::Doing).is_empty());
        assert_eq!(column_marks(&sm, Status::Todo), vec!["A", "B"]);
    }

    fn phases(sm: &SM_UI) -> Vec<(Status, HolePhase)> {
        sm.hole_snapshots()
            .into_iter()
            .map(|h| (h.status, h.phase))
            .collect()
    }

    #[test]
    fn start_drag_holds_hole_at_full_height() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        assert_eq!(sm.hole_epoch(), 0);
        assert_eq!(phases(&sm), vec![(Status::Todo, HolePhase::Hold)]);
    }

    #[test]
    fn hover_insert_closes_old_hole_and_opens_new() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.hover_insert(Status::Doing, None);
        assert!(sm.hole_epoch() > 0);
        assert_eq!(
            phases(&sm),
            vec![
                (Status::Todo, HolePhase::Close),
                (Status::Doing, HolePhase::Open),
            ]
        );
        assert_eq!(column_marks(&sm, Status::Todo), vec!["□", "B"]);
        assert_eq!(column_marks(&sm, Status::Doing), vec!["□"]);
    }

    #[test]
    fn hover_insert_bumps_epoch_on_each_move() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.hover_insert(Status::Doing, None);
        let first = sm.hole_epoch();
        sm.hover_insert(Status::Done, None);
        assert!(sm.hole_epoch() > first);
    }

    #[test]
    fn same_column_squeeze_slides_cards_instead_of_a_second_hole() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[0]);
        sm.hover_insert(Status::Todo, Some(&todo[2]));
        let todo_holes: Vec<_> = sm
            .hole_snapshots()
            .into_iter()
            .filter(|h| h.status == Status::Todo)
            .collect();
        assert_eq!(todo_holes.len(), 1);
        assert_ne!(todo_holes[0].phase, HolePhase::Close);
        assert_eq!(column_marks(&sm, Status::Todo), vec!["B", "□", "C"]);
        assert_eq!(
            squeeze_offsets(&sm),
            vec![("B".to_string(), widgets::SLOT_PITCH)]
        );
        assert_eq!(squeeze_paths(&sm), vec![("B".to_string(), 1, 0)]);
    }

    #[test]
    fn two_card_column_squeeze_does_not_leave_a_third_slot() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[1]);
        assert_eq!(column_marks(&sm, Status::Todo), vec!["A", "□"]);
        sm.hover_insert(Status::Todo, Some(&todo[0]));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["□", "A"]);
        let todo_holes: Vec<_> = sm
            .hole_snapshots()
            .into_iter()
            .filter(|h| h.status == Status::Todo)
            .collect();
        assert_eq!(todo_holes.len(), 1);
        assert_eq!(
            squeeze_offsets(&sm),
            vec![("A".to_string(), -widgets::SLOT_PITCH)]
        );
    }

    #[test]
    fn squeeze_up_overlay_starts_at_the_old_slot() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[0]);
        sm.hover_insert(Status::Todo, Some(&todo[2]));
        assert_eq!(
            sm.squeeze_overlay_from_to(),
            vec![(widgets::SLOT_PITCH, 0.0)]
        );
        assert!(sm.card_is_hidden(&todo[1]));
    }

    #[test]
    fn squeeze_overlay_starts_from_old_hole_height() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("Tall");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        sm.snapshot.cards[0].notes = "a\nb\nc\nd".to_string();
        let tall = widgets::card_pitch("Tall", "a\nb\nc\nd", 0.0);
        assert!(tall > widgets::SLOT_PITCH);
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[0]);
        sm.hover_insert(Status::Todo, Some(&todo[2]));
        assert_eq!(sm.squeeze_overlay_from_to(), vec![(tall, 0.0)]);
    }

    #[test]
    fn squeeze_overlay_width_matches_displayed_card() {
        let list_width = 360.0;
        let box_ = squeeze_overlay_box(list_width);
        assert_eq!(box_.left, widgets::COLUMN_PAD_X);
        assert_eq!(box_.width, widgets::displayed_card_width(list_width));
        assert_eq!(box_.left + box_.width + widgets::COLUMN_PAD_X, list_width);
        assert!(box_.width < list_width);
    }

    #[test]
    fn paired_open_close_holes_share_one_slot_of_height() {
        for delta in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let open = hole_box(HolePhase::Open, delta, widgets::SLOT_PITCH);
            let close = hole_box(HolePhase::Close, delta, widgets::SLOT_PITCH);
            assert_eq!(open.height + close.height, widgets::SLOT_PITCH);
            assert_eq!(open.min_height, 0.0);
            assert_eq!(close.min_height, 0.0);
            assert_eq!(open.max_height, open.height);
            assert_eq!(close.max_height, close.height);
        }
        let hold = hole_box(HolePhase::Hold, 0.0, widgets::SLOT_PITCH);
        assert_eq!(hold.height, widgets::SLOT_PITCH);
        assert_eq!(hold.min_height, widgets::SLOT_PITCH);
        assert_eq!(hold.max_height, widgets::SLOT_PITCH);
    }

    #[test]
    fn same_column_squeeze_jumps_to_pointer_slot() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        sm.add_todo_card("D");
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[0]);
        sm.hover_insert(Status::Todo, Some(&todo[3]));
        assert_eq!(
            column_marks(&sm, Status::Todo),
            vec!["B", "C", "□", "D"]
        );
    }

    #[test]
    fn slot_pitch_is_card_body_plus_column_gap() {
        assert_eq!(
            widgets::SLOT_PITCH,
            widgets::SLOT_BODY + widgets::SLOT_GAP
        );
        assert_eq!(
            widgets::SLOT_BODY,
            widgets::SLOT_PAD_Y * 2.0
                + widgets::SLOT_BORDER_Y * 2.0
                + widgets::SLOT_FACE
        );
        assert_eq!(
            widgets::SLOT_FACE,
            widgets::SLOT_TITLE + widgets::SLOT_MARKS_GAP + widgets::SLOT_MARKS
        );
        assert_eq!(widgets::SLOT_GAP, 8.0);
    }

    #[test]
    fn ghost_covering_dest_mids_steps_the_nearest_slot() {
        let n = vec![widgets::SLOT_PITCH; 3];
        let ghost_body = 200.0;
        assert_eq!(
            resolve_insert_from_ghost(
                20.0,
                ghost_body,
                &n,
                None,
                widgets::INSERT_HYSTERESIS,
                0,
                widgets::SLOT_PITCH,
            ),
            1
        );
        assert_eq!(
            resolve_insert_from_ghost(
                0.0,
                widgets::SLOT_BODY,
                &n,
                None,
                widgets::INSERT_HYSTERESIS,
                0,
                widgets::SLOT_PITCH,
            ),
            0
        );
    }

    #[test]
    fn thin_overlap_does_not_squeeze() {
        let n = vec![widgets::SLOT_PITCH; 2];
        let ghost_top = widgets::SLOT_PITCH + widgets::SLOT_BODY - 1.0;
        assert_eq!(
            raw_insert_from_ghost(ghost_top, widgets::SLOT_BODY, &n),
            0
        );
    }

    #[test]
    fn ghost_top_past_dest_mid_squeezes_from_below() {
        let n = vec![widgets::SLOT_PITCH; 2];
        let hole_pitch = widgets::SLOT_PITCH;
        assert_eq!(
            resolve_insert_from_ghost(
                squeeze_up_y(widgets::SLOT_PITCH),
                widgets::SLOT_BODY,
                &n,
                Some(2),
                widgets::INSERT_HYSTERESIS,
                2,
                hole_pitch,
            ),
            1
        );
    }

    #[test]
    fn ghost_bottom_contact_inserts_after() {
        let n = vec![widgets::SLOT_PITCH; 2];
        let ghost_top = 1.0;
        assert_eq!(
            raw_insert_from_ghost(ghost_top, widgets::SLOT_BODY, &n),
            1
        );
    }

    #[test]
    fn upward_volume_keeps_moving_from_last_slot() {
        let n = vec![widgets::SLOT_PITCH; 2];
        assert_eq!(
            resolve_insert_from_ghost(
                squeeze_up_y(0.0),
                widgets::SLOT_BODY,
                &n,
                Some(1),
                widgets::INSERT_HYSTERESIS,
                1,
                widgets::SLOT_PITCH,
            ),
            0
        );
    }

    #[test]
    fn downward_thin_overlap_below_hole_does_not_squeeze() {
        let n = vec![widgets::SLOT_PITCH; 2];
        let hole_pitch = widgets::SLOT_PITCH;
        let ghost_top = hole_pitch - widgets::SLOT_BODY + 1.0;
        assert_eq!(
            resolve_insert_from_ghost(
                ghost_top,
                widgets::SLOT_BODY,
                &n,
                Some(0),
                3.0,
                0,
                hole_pitch,
            ),
            0
        );
    }

    #[test]
    fn downward_half_overlap_below_hole_squeezes() {
        let n = vec![widgets::SLOT_PITCH; 2];
        let hole_pitch = widgets::SLOT_PITCH;
        let ghost_top = squeeze_down_y(hole_pitch);
        assert_eq!(
            resolve_insert_from_ghost(
                ghost_top,
                widgets::SLOT_BODY,
                &n,
                Some(0),
                3.0,
                0,
                hole_pitch,
            ),
            1
        );
    }

    #[test]
    fn ghost_insert_skips_hysteresis_when_index_jumps() {
        let n = vec![widgets::SLOT_PITCH; 3];
        assert_eq!(
            resolve_insert_from_ghost(0.0, widgets::SLOT_BODY, &n, Some(2), 3.0, usize::MAX, 0.0),
            0
        );
        assert_eq!(
            resolve_insert_from_ghost(10.0, widgets::SLOT_BODY, &[], None, 3.0, usize::MAX, 0.0),
            0
        );
    }

    #[test]
    fn after_card_before_id_is_next_or_append() {
        let ids = vec!["b".to_string(), "c".to_string()];
        assert_eq!(after_card_before_id(&ids, "b").as_deref(), Some("c"));
        assert_eq!(after_card_before_id(&ids, "c"), None);
    }

    #[test]
    fn top_half_squeezes_hole_after_the_card() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[0]);
        assert!(sm.hover_insert_after(Status::Todo, &todo[1]));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["B", "□", "C"]);
    }

    #[test]
    fn collision_uses_ghost_volume_not_pointer() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[0]);
        sm.set_grab_offset(0.0, 0.0);
        sm.hover_pointer(Status::Todo, 90.0);
        let from_top = column_marks(&sm, Status::Todo);
        sm.start_drag(&todo[0]);
        sm.set_grab_offset(0.0, widgets::SLOT_BODY - 1.0);
        sm.hover_pointer(Status::Todo, 90.0);
        let from_bottom = column_marks(&sm, Status::Todo);
        assert_ne!(from_top, from_bottom);
    }

    #[test]
    fn upward_ghost_top_contact_opens_hole_ahead() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[2]);
        sm.set_grab_offset(0.0, 0.0);
        let y = squeeze_up_y(widgets::SLOT_PITCH);
        assert!(sm.hover_pointer(Status::Todo, y));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["A", "□", "B"]);
    }

    #[test]
    fn upward_ghost_volume_reaches_column_top() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let todo = ids(&sm, Status::Todo);
        sm.start_drag(&todo[2]);
        sm.set_grab_offset(0.0, 0.0);
        assert!(sm.hover_pointer(Status::Todo, squeeze_up_y(widgets::SLOT_PITCH)));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["A", "□", "B"]);
        assert!(sm.hover_pointer(Status::Todo, squeeze_up_y(0.0)));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["□", "A", "B"]);
    }

    #[test]
    fn downward_thin_overlap_does_not_squeeze() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let id = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&id);
        sm.set_grab_offset(0.0, 0.0);
        let y = widgets::SLOT_PITCH - widgets::SLOT_BODY + 1.0;
        sm.hover_pointer(Status::Todo, y);
        assert_eq!(column_marks(&sm, Status::Todo), vec!["□", "B", "C"]);
    }

    #[test]
    fn downward_onto_card_opens_hole_ahead() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let id = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&id);
        sm.set_grab_offset(0.0, 0.0);
        assert!(sm.hover_pointer(
            Status::Todo,
            squeeze_down_y(widgets::SLOT_PITCH)
        ));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["B", "□", "C"]);
    }

    #[test]
    fn ghost_bottom_must_pass_dest_mid_by_hysteresis_to_squeeze_down() {
        let n = vec![widgets::SLOT_PITCH; 2];
        let hole_pitch = widgets::SLOT_PITCH;
        let dest_mid = hole_pitch + widgets::SLOT_BODY / 2.0;
        let h = widgets::INSERT_HYSTERESIS;
        let ghost_body = widgets::SLOT_BODY;
        assert_eq!(
            resolve_insert_from_ghost(
                dest_mid - ghost_body,
                ghost_body,
                &n,
                Some(0),
                h,
                0,
                hole_pitch,
            ),
            0
        );
        assert_eq!(
            resolve_insert_from_ghost(
                dest_mid + h - ghost_body,
                ghost_body,
                &n,
                Some(0),
                h,
                0,
                hole_pitch,
            ),
            1
        );
    }

    #[test]
    fn ghost_top_must_pass_dest_mid_by_hysteresis_to_squeeze_up() {
        let n = vec![widgets::SLOT_PITCH; 2];
        let hole_pitch = widgets::SLOT_PITCH;
        let dest_mid = widgets::SLOT_BODY / 2.0;
        let h = widgets::INSERT_HYSTERESIS;
        let ghost_body = widgets::SLOT_BODY;
        assert_eq!(
            resolve_insert_from_ghost(
                dest_mid,
                ghost_body,
                &n,
                Some(1),
                h,
                1,
                hole_pitch,
            ),
            1
        );
        assert_eq!(
            resolve_insert_from_ghost(
                dest_mid - h,
                ghost_body,
                &n,
                Some(1),
                h,
                1,
                hole_pitch,
            ),
            0
        );
    }

    #[test]
    fn reverse_squeeze_up_after_down_onto_the_card_above() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let id = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&id);
        sm.set_grab_offset(0.0, 0.0);
        assert!(sm.hover_pointer(
            Status::Todo,
            squeeze_down_y(widgets::SLOT_PITCH)
        ));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["B", "□", "C"]);
        assert!(sm.hover_pointer(
            Status::Todo,
            squeeze_up_y(0.0)
        ));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["□", "B", "C"]);
    }

    #[test]
    fn upward_onto_card_opens_hole_ahead() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let id = ids(&sm, Status::Todo)[2].clone();
        sm.start_drag(&id);
        assert!(sm.hover_pointer(Status::Todo, insert_boundary(0) + 1.0));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["A", "□", "B"]);
    }

    #[test]
    fn upward_squeeze_reaches_column_top() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let id = ids(&sm, Status::Todo)[2].clone();
        sm.start_drag(&id);
        assert!(sm.hover_pointer(Status::Todo, 0.0));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["□", "A", "B"]);
    }

    #[test]
    fn staying_on_same_card_does_not_flip_hole() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let id = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&id);
        sm.set_grab_offset(0.0, 0.0);
        let y = squeeze_down_y(widgets::SLOT_PITCH);
        assert!(sm.hover_pointer(Status::Todo, y));
        let marks = column_marks(&sm, Status::Todo);
        assert!(!sm.hover_pointer(Status::Todo, y + 1.0));
        assert_eq!(column_marks(&sm, Status::Todo), marks);
    }

    #[test]
    fn leaving_card_still_moves_hole() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.add_todo_card("C");
        let id = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&id);
        sm.set_grab_offset(0.0, 0.0);
        assert!(sm.hover_pointer(
            Status::Todo,
            squeeze_down_y(widgets::SLOT_PITCH)
        ));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["B", "□", "C"]);
        assert!(sm.hover_pointer(
            Status::Todo,
            widgets::SLOT_PITCH * 2.0 + widgets::SLOT_BODY
        ));
        assert_eq!(column_marks(&sm, Status::Todo), vec!["B", "C", "□"]);
    }

    #[test]
    fn drop_settle_is_at_most_half_of_hole_anim() {
        assert!(SETTLE_ANIM_MS * 2 <= HOLE_ANIM_MS);
        assert!(SETTLE_ANIM_MS > 0);
    }

    #[test]
    fn ghost_origin_is_pointer_minus_grab() {
        assert_eq!(ghost_origin(100.0, 80.0, 12.0, 20.0), (88.0, 60.0));
    }

    #[test]
    fn slot_origin_y_is_list_plus_index_pitch() {
        assert_eq!(
            slot_origin_y(40.0, -8.0, 2, &[widgets::SLOT_PITCH; 3]),
            40.0 - 8.0 + 2.0 * widgets::SLOT_PITCH
        );
    }

    #[test]
    fn commit_drop_settles_card_from_ghost_into_slot() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.set_grab_offset(12.0, 8.0);
        sm.note_drag_geometry(200.0, 90.0, 40.0, 50.0, 0.0);
        sm.hover_insert(Status::Doing, None);
        sm.commit_drop();
        assert_eq!(sm.settling_id(), Some(a.as_str()));
        assert!(sm.card_is_hidden(&a));
        let settle = sm.settling_delta().unwrap();
        assert!(settle.0.abs() > 1.0 || settle.1.abs() > 1.0);
        sm.expire_settle();
        flush(&mut sm);
        assert_eq!(sm.settling_id(), None);
        assert!(!sm.card_is_hidden(&a));
    }

    #[test]
    fn drop_starts_at_fractional_preview_origin_and_moves_on_the_next_frame() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.dragging.as_mut().unwrap().width = 317.25;
        sm.set_grab_offset(12.25, 8.5);
        sm.note_drag_geometry(200.5, 90.75, 40.0, 50.0, 0.0);
        sm.hover_insert(Status::Doing, None);
        sm.commit_drop();

        let settle = sm.settling.as_mut().unwrap();
        assert_eq!((settle.from_x, settle.from_y), (188.25, 82.25));
        assert_eq!(settle.width, 317.25);
        let first_frame = Instant::now();
        assert_eq!(settle.progress(first_frame), 0.0);
        let next_frame = settle.progress(first_frame + Duration::from_millis(16));
        assert!(next_frame > 0.25, "landing must move promptly, without an ease-in pause");
        assert_eq!(settle.progress(first_frame + Duration::from_millis(SETTLE_ANIM_MS)), 1.0);
    }

    #[test]
    fn same_column_drop_uses_remaining_cards_heights_for_the_landing_target() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        sm.snapshot.cards[0].notes = "one\ntwo\nthree".into();
        let a = sm.snapshot.cards[0].id.clone();
        sm.start_drag(&a);
        sm.note_drag_geometry(200.0, 1000.0, 40.0, 50.0, 0.0);
        sm.hover_pointer(Status::Todo, 1000.0);
        let expected_y = 50.0 + sm.dest_pitches(Status::Todo).iter().sum::<f32>();
        sm.commit_drop();
        assert_eq!(sm.settling.as_ref().unwrap().to_y, expected_y);
        assert_eq!(titles(&sm, Status::Todo), vec!["B", "A"]);
    }

    #[test]
    fn subpixel_drop_distance_is_animated_instead_of_snapping() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.note_drag_geometry(52.5, 50.25, 40.0, 50.0, 0.0);
        sm.commit_drop();
        assert_eq!(sm.settling_delta(), Some((0.5, 0.25)));
        assert!(sm.card_is_hidden(&a));
    }

    #[test]
    fn append_drop_uses_its_committed_target_instead_of_the_last_hover_index() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.note_drag_geometry(200.0, 90.0, 40.0, 50.0, 0.0);
        sm.last_hover_status = Some(Status::Todo);
        sm.last_insert_index = Some(0);
        let expected_y = 50.0 + sm.dest_pitches(Status::Todo).iter().sum::<f32>();
        sm.drop_card(&a, Status::Todo, None);
        assert_eq!(sm.settling.as_ref().unwrap().to_y, expected_y);
        assert_eq!(titles(&sm, Status::Todo), vec!["B", "A"]);
    }

    #[test]
    fn flush_does_not_drop_settle_before_first_paint() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.set_grab_offset(12.0, 8.0);
        sm.note_drag_geometry(200.0, 90.0, 40.0, 50.0, 0.0);
        sm.hover_insert(Status::Doing, None);
        sm.commit_drop();
        assert_eq!(sm.settling_id(), Some(a.as_str()));
        sm.age_settle_as_if_persist_was_slow();
        flush(&mut sm);
        assert_eq!(sm.settling_id(), Some(a.as_str()));
        sm.expire_settle();
        flush(&mut sm);
        assert_eq!(sm.settling_id(), None);
    }

    #[test]
    fn drop_settles_from_pointer_and_dest_frame_without_list_hit() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let a = ids(&sm, Status::Todo)[0].clone();
        sm.start_drag(&a);
        sm.set_grab_offset(12.0, 8.0);
        sm.note_pointer(200.0, 90.0);
        sm.note_column_frame(Status::Doing, 400.0, 80.0, 0.0);
        sm.hover_insert(Status::Doing, None);
        sm.commit_drop();
        assert_eq!(sm.settling_id(), Some(a.as_str()));
        let settle = sm.settling_delta().unwrap();
        assert!(settle.0.abs() > 1.0 || settle.1.abs() > 1.0);
    }

    #[test]
    fn open_create_does_not_persist_a_card() {
        let (_database, mut sm) = test_ui();
        sm.open_create();
        assert!(sm.editor_is_open());
        assert!(sm.cards_in(Status::Todo).is_empty());
    }

    #[test]
    fn save_create_persists_editor_fields() {
        let (_database, mut sm) = test_ui();
        sm.open_create();
        sm.set_editor_title("买菜");
        sm.set_editor_notes("今晚");
        sm.set_editor_deadline("2026-08-24", 18, 0);
        sm.set_editor_priority(Priority::High);
        assert!(sm.save_editor());
        assert!(!sm.editor_is_open());
        let card = &sm.cards_in(Status::Todo)[0];
        assert_eq!(card.title, "买菜");
        assert_eq!(card.notes, "今晚");
        assert_eq!(card.priority, Priority::High);
        assert_eq!(
            card.deadline_at,
            card_meta::deadline_from_date_and_hour("2026-08-24", 18)
        );
    }

    #[test]
    fn save_create_persists_half_hour_deadline() {
        let (_database, mut sm) = test_ui();
        sm.open_create();
        sm.set_editor_title("买菜");
        sm.set_editor_deadline("2026-08-24", 18, 30);
        assert!(sm.save_editor());
        let card = &sm.cards_in(Status::Todo)[0];
        assert_eq!(
            card.deadline_at,
            card_meta::deadline_from_date_and_time("2026-08-24", 18, 30)
        );
    }

    #[test]
    fn save_create_rejects_blank_title() {
        let (_database, mut sm) = test_ui();
        sm.open_create();
        sm.set_editor_title("   ");
        assert!(!sm.save_editor());
        assert!(sm.editor_is_open());
        assert_eq!(sm.editor_error(), Some("标题不能为空"));
        assert!(sm.cards_in(Status::Todo).is_empty());
    }

    #[test]
    fn save_edit_updates_existing_card() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        let id = ids(&sm, Status::Todo)[0].clone();
        sm.open_edit(&id);
        assert_eq!(sm.editor_title(), Some("A"));
        sm.set_editor_title("B");
        sm.set_editor_notes("note");
        sm.set_editor_priority(Priority::Low);
        assert!(sm.save_editor());
        assert!(!sm.editor_is_open());
        let card = &sm.cards_in(Status::Todo)[0];
        assert_eq!(card.title, "B");
        assert_eq!(card.notes, "note");
        assert_eq!(card.priority, Priority::Low);
    }

    #[test]
    fn cancel_edit_does_not_persist() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        let id = ids(&sm, Status::Todo)[0].clone();
        sm.open_edit(&id);
        sm.set_editor_title("B");
        sm.close_editor();
        assert!(!sm.editor_is_open());
        assert_eq!(titles(&sm, Status::Todo), vec!["A"]);
    }

    #[test]
    fn delete_edit_removes_the_card() {
        let (_database, mut sm) = test_ui();
        sm.add_todo_card("A");
        sm.add_todo_card("B");
        let id = ids(&sm, Status::Todo)[0].clone();
        sm.open_edit(&id);
        assert!(sm.delete_editor());
        assert!(!sm.editor_is_open());
        assert_eq!(titles(&sm, Status::Todo), vec!["B"]);
    }
}
