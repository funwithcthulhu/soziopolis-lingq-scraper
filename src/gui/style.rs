use iced::theme::Palette;
use iced::{Color, color};

pub(super) const BG_DARK: Color = color!(0x0F1219);
pub(super) const BG_SIDEBAR: Color = color!(0x181C25);
pub(super) const BG_CARD: Color = color!(0x181C25);
pub(super) const BG_TAG: Color = color!(0x212734);
pub(super) const BORDER_SUBTLE: Color = color!(0x2E384A);
pub(super) const TEXT_PRIMARY: Color = color!(0xE1E5EE);
pub(super) const TEXT_SECONDARY: Color = color!(0xA0A8BA);
pub(super) const TEXT_DIM: Color = color!(0x969DAD);
pub(super) const ACCENT_BLUE: Color = color!(0x5C87FF);
pub(super) const ACCENT_GREEN: Color = color!(0x5ED682);
pub(super) const ACCENT_RED: Color = color!(0xEE6464);
pub(super) const LINK_BLUE: Color = color!(0x81A5FF);

pub(super) fn soziopolis_palette() -> Palette {
    Palette {
        background: BG_DARK,
        text: TEXT_PRIMARY,
        primary: ACCENT_BLUE,
        success: ACCENT_GREEN,
        danger: ACCENT_RED,
    }
}

pub(super) fn notice_color(kind: super::NoticeKind) -> Color {
    match kind {
        super::NoticeKind::Info => ACCENT_BLUE,
        super::NoticeKind::Success => ACCENT_GREEN,
        super::NoticeKind::Error => ACCENT_RED,
    }
}
