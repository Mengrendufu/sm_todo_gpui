//! Charmtone Crush Pantera tokens, matching Tauri `App.css` light `:root` and dark `@media`.

use gpui::{rgb, rgba, Rgba};
use kanban_core::Priority;

/// `#fffaf1` `--bg`
pub const BG: u32 = 0xfffaf1;
/// `#201f26` `--ink`
pub const INK: u32 = 0x201f26;
/// `#605f6b` `--ink-soft`
pub const INK_SOFT: u32 = 0x605f6b;
/// `#858392` `--ink-faint`
pub const INK_FAINT: u32 = 0x858392;
/// `rgba(32, 31, 38, 0.1)` `--line` packed RRGGBBAA
pub const LINE: u32 = 0x201f261a;
/// `rgba(107, 80, 255, 0.22)` `--line-strong`
pub const LINE_STRONG: u32 = 0x6b50ff38;
/// `rgba(107, 80, 255, 0.07)` `--fill`
pub const FILL: u32 = 0x6b50ff12;
/// `#f7f6fb` `--card`
pub const CARD: u32 = 0xf7f6fb;
/// `#6b50ff` `--gold`
pub const GOLD: u32 = 0x6b50ff;
/// `rgba(107, 80, 255, 0.16)` `--gold-soft`
pub const GOLD_SOFT: u32 = 0x6b50ff29;
/// `#8b75ff` primary / add-card hover
pub const GOLD_HOVER: u32 = 0x8b75ff;
/// `#ff60ff` `--dolly`
pub const DOLLY: u32 = 0xff60ff;
/// `#e8fe96` `--zest`
pub const ZEST: u32 = 0xe8fe96;
/// `#eb4268` `--danger`
pub const DANGER: u32 = 0xeb4268;
/// `#fffaf1` `--on-primary`
pub const ON_PRIMARY: u32 = 0xfffaf1;
/// `rgba(32, 31, 38, 0.48)` modal backdrop
pub const BACKDROP: u32 = 0x201f267a;
/// Inactive traffic-light grey
pub const TRAFFIC_DIM: u32 = 0xa2a0ad;
/// `#605f6b` / `rgba(96, 95, 107, 0.14)`
pub const PRIORITY_LOW_FG: u32 = 0x605f6b;
pub const PRIORITY_LOW_BG: u32 = 0x605f6b24;
/// `--ink` / `color-mix(zest 62%, transparent)`
pub const PRIORITY_MEDIUM_FG: u32 = 0x201f26;
pub const PRIORITY_MEDIUM_BG: u32 = 0xe8fe969e;
/// `--danger` / `rgba(235, 66, 104, 0.14)`
pub const PRIORITY_HIGH_FG: u32 = 0xeb4268;
pub const PRIORITY_HIGH_BG: u32 = 0xeb426824;
/// `--danger` / `rgba(235, 66, 104, 0.12)`
pub const REMAIN_OVERDUE_FG: u32 = 0xeb4268;
pub const REMAIN_OVERDUE_BG: u32 = 0xeb42681f;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub bg: u32,
    pub ink: u32,
    pub ink_soft: u32,
    pub ink_faint: u32,
    pub line: u32,
    pub line_strong: u32,
    pub fill: u32,
    pub card: u32,
    pub gold: u32,
    pub gold_soft: u32,
    pub gold_hover: u32,
    pub dolly: u32,
    pub zest: u32,
    pub danger: u32,
    pub on_primary: u32,
    pub backdrop: u32,
    pub traffic_dim: u32,
    pub priority_low_fg: u32,
    pub priority_low_bg: u32,
    pub priority_medium_fg: u32,
    pub priority_medium_bg: u32,
    pub priority_high_fg: u32,
    pub priority_high_bg: u32,
    pub remain_overdue_fg: u32,
    pub remain_overdue_bg: u32,
    pub dark: bool,
}

impl Theme {
    pub fn light() -> Self {
        Self {
            bg: BG,
            ink: INK,
            ink_soft: INK_SOFT,
            ink_faint: INK_FAINT,
            line: LINE,
            line_strong: LINE_STRONG,
            fill: FILL,
            card: CARD,
            gold: GOLD,
            gold_soft: GOLD_SOFT,
            gold_hover: GOLD_HOVER,
            dolly: DOLLY,
            zest: ZEST,
            danger: DANGER,
            on_primary: ON_PRIMARY,
            backdrop: BACKDROP,
            traffic_dim: TRAFFIC_DIM,
            priority_low_fg: PRIORITY_LOW_FG,
            priority_low_bg: PRIORITY_LOW_BG,
            priority_medium_fg: PRIORITY_MEDIUM_FG,
            priority_medium_bg: PRIORITY_MEDIUM_BG,
            priority_high_fg: PRIORITY_HIGH_FG,
            priority_high_bg: PRIORITY_HIGH_BG,
            remain_overdue_fg: REMAIN_OVERDUE_FG,
            remain_overdue_bg: REMAIN_OVERDUE_BG,
            dark: false,
        }
    }

    pub fn dark() -> Self {
        Self {
            bg: 0x201f26,
            ink: 0xecebf0,
            ink_soft: 0xbfbcc8,
            ink_faint: INK_FAINT,
            line: 0xbfbcc81f,
            line_strong: 0x6b50ff61,
            fill: 0x6b50ff1f,
            card: 0x2d2c36,
            gold: GOLD,
            gold_soft: 0x6b50ff38,
            gold_hover: GOLD_HOVER,
            dolly: DOLLY,
            zest: ZEST,
            danger: DANGER,
            on_primary: ON_PRIMARY,
            backdrop: BACKDROP,
            traffic_dim: 0x4d4c57,
            priority_low_fg: 0xbfbcc8,
            priority_low_bg: 0x85839233,
            priority_medium_fg: ZEST,
            priority_medium_bg: 0xe8fe9629,
            priority_high_fg: 0xff577d,
            priority_high_bg: 0xeb426829,
            remain_overdue_fg: 0xff577d,
            remain_overdue_bg: 0xeb426824,
            dark: true,
        }
    }

    pub fn from_appearance(appearance: gpui::WindowAppearance) -> Self {
        match appearance {
            gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => Self::dark(),
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight => Self::light(),
        }
    }

    pub fn for_window(window: &gpui::Window) -> Self {
        Self::from_appearance(window.appearance())
    }

    pub fn bg(&self) -> Rgba {
        rgb(self.bg)
    }
    pub fn ink(&self) -> Rgba {
        rgb(self.ink)
    }
    pub fn ink_soft(&self) -> Rgba {
        rgb(self.ink_soft)
    }
    pub fn ink_faint(&self) -> Rgba {
        rgb(self.ink_faint)
    }
    pub fn line(&self) -> Rgba {
        rgba(self.line)
    }
    pub fn line_strong(&self) -> Rgba {
        rgba(self.line_strong)
    }
    pub fn fill(&self) -> Rgba {
        rgba(self.fill)
    }
    pub fn card(&self) -> Rgba {
        rgb(self.card)
    }
    pub fn gold(&self) -> Rgba {
        rgb(self.gold)
    }
    pub fn gold_soft(&self) -> Rgba {
        rgba(self.gold_soft)
    }
    pub fn gold_hover(&self) -> Rgba {
        rgb(self.gold_hover)
    }
    pub fn danger(&self) -> Rgba {
        rgb(self.danger)
    }
    pub fn on_primary(&self) -> Rgba {
        rgb(self.on_primary)
    }
    pub fn backdrop(&self) -> Rgba {
        rgba(self.backdrop)
    }
    pub fn clear(&self) -> Rgba {
        rgba(0)
    }
    pub fn pin_fg(&self, pinned: bool) -> Rgba {
        if pinned {
            if self.dark {
                rgb(self.gold_hover)
            } else {
                rgb(self.gold)
            }
        } else {
            rgb(self.ink_soft)
        }
    }
    pub fn pin_bg(&self, pinned: bool) -> Rgba {
        if pinned {
            self.gold_soft()
        } else {
            self.bg()
        }
    }
    pub fn pin_hover(&self, pinned: bool) -> Rgba {
        if pinned {
            self.gold_soft()
        } else {
            self.fill()
        }
    }

    pub fn priority_tag(&self, priority: Priority) -> (Rgba, Rgba) {
        match priority {
            Priority::Low => (rgb(self.priority_low_fg), rgba(self.priority_low_bg)),
            Priority::Medium => (rgb(self.priority_medium_fg), rgba(self.priority_medium_bg)),
            Priority::High => (rgb(self.priority_high_fg), rgba(self.priority_high_bg)),
        }
    }

    pub fn remain_tag(&self, overdue: bool) -> (Rgba, Rgba) {
        if overdue {
            (rgb(self.remain_overdue_fg), rgba(self.remain_overdue_bg))
        } else {
            (self.ink_soft(), self.fill())
        }
    }

    pub fn deadline_tag(&self) -> (Rgba, Rgba) {
        (self.ink_soft(), self.fill())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanban_core::Priority;

    fn round_alpha(channel: f32) -> u32 {
        (channel * 255.0).round() as u32
    }

    fn pack_rgba(r: u32, g: u32, b: u32, a: f32) -> u32 {
        (r << 24) | (g << 16) | (b << 8) | round_alpha(a)
    }

    #[test]
    fn light_tokens_match_tauri_crush_pantera() {
        assert_eq!(BG, 0xfffaf1);
        assert_eq!(INK, 0x201f26);
        assert_eq!(INK_SOFT, 0x605f6b);
        assert_eq!(INK_FAINT, 0x858392);
        assert_eq!(CARD, 0xf7f6fb);
        assert_eq!(GOLD, 0x6b50ff);
        assert_eq!(GOLD_HOVER, 0x8b75ff);
        assert_eq!(DOLLY, 0xff60ff);
        assert_eq!(ZEST, 0xe8fe96);
        assert_eq!(DANGER, 0xeb4268);
        assert_eq!(ON_PRIMARY, 0xfffaf1);
        assert_eq!(TRAFFIC_DIM, 0xa2a0ad);
        assert_eq!(LINE, pack_rgba(32, 31, 38, 0.1));
        assert_eq!(LINE_STRONG, pack_rgba(107, 80, 255, 0.22));
        assert_eq!(FILL, pack_rgba(107, 80, 255, 0.07));
        assert_eq!(GOLD_SOFT, pack_rgba(107, 80, 255, 0.16));
        assert_eq!(BACKDROP, pack_rgba(32, 31, 38, 0.48));
    }

    #[test]
    fn card_mark_colors_match_tauri() {
        assert_eq!(PRIORITY_LOW_FG, 0x605f6b);
        assert_eq!(PRIORITY_LOW_BG, pack_rgba(96, 95, 107, 0.14));
        assert_eq!(PRIORITY_MEDIUM_FG, INK);
        assert_eq!(PRIORITY_MEDIUM_BG, pack_rgba(0xe8, 0xfe, 0x96, 0.62));
        assert_eq!(PRIORITY_HIGH_FG, DANGER);
        assert_eq!(PRIORITY_HIGH_BG, pack_rgba(235, 66, 104, 0.14));
        assert_eq!(REMAIN_OVERDUE_FG, DANGER);
        assert_eq!(REMAIN_OVERDUE_BG, pack_rgba(235, 66, 104, 0.12));
        let (low_fg, _) = Theme::light().priority_tag(Priority::Low);
        assert_eq!(low_fg, rgb(PRIORITY_LOW_FG));
        let (remain_fg, _) = Theme::light().remain_tag(true);
        assert_eq!(remain_fg, rgb(DANGER));
    }

    #[test]
    fn dark_tokens_match_tauri_crush_pantera() {
        let t = Theme::dark();
        assert_eq!(t.bg, 0x201f26);
        assert_eq!(t.ink, 0xecebf0);
        assert_eq!(t.ink_soft, 0xbfbcc8);
        assert_eq!(t.ink_faint, 0x858392);
        assert_eq!(t.card, 0x2d2c36);
        assert_eq!(t.gold, GOLD);
        assert_eq!(t.gold_hover, GOLD_HOVER);
        assert_eq!(t.dolly, DOLLY);
        assert_eq!(t.zest, ZEST);
        assert_eq!(t.danger, DANGER);
        assert_eq!(t.on_primary, ON_PRIMARY);
        assert_eq!(t.traffic_dim, 0x4d4c57);
        assert_eq!(t.line, pack_rgba(191, 188, 200, 0.12));
        assert_eq!(t.line_strong, pack_rgba(107, 80, 255, 0.38));
        assert_eq!(t.fill, pack_rgba(107, 80, 255, 0.12));
        assert_eq!(t.gold_soft, pack_rgba(107, 80, 255, 0.22));
        assert_eq!(t.backdrop, BACKDROP);
        assert!(t.dark);
    }

    #[test]
    fn dark_card_marks_match_tauri() {
        let t = Theme::dark();
        assert_eq!(t.priority_low_fg, 0xbfbcc8);
        assert_eq!(t.priority_low_bg, pack_rgba(133, 131, 146, 0.2));
        assert_eq!(t.priority_medium_fg, ZEST);
        assert_eq!(t.priority_medium_bg, pack_rgba(0xe8, 0xfe, 0x96, 0.16));
        assert_eq!(t.priority_high_fg, 0xff577d);
        assert_eq!(t.priority_high_bg, pack_rgba(235, 66, 104, 0.16));
        assert_eq!(t.remain_overdue_fg, 0xff577d);
        assert_eq!(t.remain_overdue_bg, pack_rgba(235, 66, 104, 0.14));
        assert_eq!(t.pin_fg(true), rgb(GOLD_HOVER));
        assert_eq!(t.pin_fg(false), rgb(t.ink_soft));
    }

    #[test]
    fn appearance_selects_tauri_palette() {
        assert_eq!(
            Theme::from_appearance(gpui::WindowAppearance::Light).bg,
            0xfffaf1
        );
        assert_eq!(
            Theme::from_appearance(gpui::WindowAppearance::VibrantLight).bg,
            0xfffaf1
        );
        assert_eq!(
            Theme::from_appearance(gpui::WindowAppearance::Dark).bg,
            0x201f26
        );
        assert_eq!(
            Theme::from_appearance(gpui::WindowAppearance::VibrantDark).bg,
            0x201f26
        );
    }
}
