//! Chat panel state: one independent message history per `K` key,
//! plus scroll, auto-scroll, and a two-level render cache.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use ratatui::text::Line;

use super::paste::PasteEntry;
use super::ChatMessage;

/// Monotonic version counter. Bumped on every mutation that could
/// change the rendered output.
pub type LinesCache = Option<(u64, Vec<Line<'static>>)>;

/// `(message_version, inner_width_u16, post_wrap_visual_row_count)`.
pub type WrapCache = Option<(u64, u16, usize)>;

/// Chat panel state, generic over the key type the host uses to
/// distinguish independent conversation histories.
#[derive(Debug, Clone)]
pub struct ChatPanelState<K: Hash + Eq + Clone + Debug> {
    histories: HashMap<K, Vec<ChatMessage>>,
    active_key: K,
    message_version: u64,
    cached_lines: LinesCache,
    cached_wrap: WrapCache,
    scroll: u16,
    auto_scroll: bool,
    input: String,
    input_active: bool,
    input_version: u64,
    pastes: Vec<PasteEntry>,
}

impl<K: Hash + Eq + Clone + Debug> ChatPanelState<K> {
    pub fn new(active_key: K) -> Self {
        Self {
            histories: HashMap::new(),
            active_key,
            message_version: 0,
            cached_lines: None,
            cached_wrap: None,
            scroll: 0,
            auto_scroll: true,
            input: String::new(),
            input_active: false,
            input_version: 0,
            pastes: Vec::new(),
        }
    }

    pub fn insert_history(&mut self, key: K, initial: Vec<ChatMessage>) {
        self.histories.insert(key, initial);
    }

    pub fn set_active(&mut self, key: K) {
        if self.active_key != key {
            self.active_key = key;
            self.bump_version();
        }
    }

    pub fn active_key(&self) -> &K {
        &self.active_key
    }

    pub fn current_messages(&self) -> &[ChatMessage] {
        self.histories
            .get(&self.active_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn current_messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        self.histories
            .entry(self.active_key.clone())
            .or_insert_with(Vec::new)
    }

    pub fn push_message(&mut self, msg: ChatMessage) {
        self.current_messages_mut().push(msg);
        self.bump_version();
    }

    pub fn message_version(&self) -> u64 {
        self.message_version
    }

    pub fn bump_version(&mut self) {
        self.message_version = self.message_version.wrapping_add(1);
    }

    pub fn cached_lines(&self) -> &LinesCache {
        &self.cached_lines
    }

    pub fn set_cached_lines(&mut self, cache: LinesCache) {
        self.cached_lines = cache;
    }

    pub fn cached_wrap(&self) -> &WrapCache {
        &self.cached_wrap
    }

    pub fn set_cached_wrap(&mut self, cache: WrapCache) {
        self.cached_wrap = cache;
    }

    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    pub fn set_scroll(&mut self, val: u16) {
        self.scroll = val;
    }

    pub fn auto_scroll(&self) -> bool {
        self.auto_scroll
    }

    pub fn set_auto_scroll(&mut self, val: bool) {
        self.auto_scroll = val;
    }

    pub fn enable_auto_scroll(&mut self) {
        self.auto_scroll = true;
        self.scroll = 0;
    }

    pub fn disable_auto_scroll(&mut self) {
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_add(amount);
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll = 0;
    }

    pub fn resolve_scroll(&self, max_scroll: usize) -> usize {
        if self.auto_scroll {
            max_scroll
        } else {
            (self.scroll as usize).min(max_scroll)
        }
    }

    // ---- Input state accessors ----

    pub fn input_text(&self) -> &str {
        &self.input
    }

    pub fn input_text_mut(&mut self) -> &mut String {
        &mut self.input
    }

    pub fn input_active(&self) -> bool {
        self.input_active
    }

    pub fn set_input_active(&mut self, v: bool) {
        if self.input_active != v {
            self.input_active = v;
            self.bump_input_version();
        }
    }

    pub fn take_input_text(&mut self) -> String {
        if self.input.is_empty() && self.pastes.is_empty() {
            return String::new();
        }
        let out = std::mem::take(&mut self.input);
        self.pastes.clear();
        self.bump_input_version();
        out
    }

    pub fn clear_input_text(&mut self) {
        if !self.input.is_empty() {
            self.input.clear();
        }
        if !self.pastes.is_empty() {
            self.pastes.clear();
        }
        self.bump_input_version();
    }

    pub fn push_input_char(&mut self, c: char) {
        self.input.push(c);
        self.bump_input_version();
    }

    pub fn pop_input_char(&mut self) {
        if self.input.pop().is_some() {
            self.bump_input_version();
        }
    }

    pub fn push_input_str(&mut self, s: &str) {
        if !s.is_empty() {
            self.input.push_str(s);
            self.bump_input_version();
        }
    }

    pub fn push_paste(&mut self, content: &str) {
        if content.is_empty() {
            return;
        }
        let entry = PasteEntry::from_content(content);
        if entry.placeholder != entry.content {
            self.pastes.push(entry.clone());
        }
        self.input.push_str(&entry.placeholder);
        self.bump_input_version();
    }

    pub fn take_full_input_text(&mut self) -> String {
        let display = std::mem::take(&mut self.input);
        let pastes = std::mem::take(&mut self.pastes);
        self.bump_input_version();
        if pastes.is_empty() {
            return display;
        }
        let mut out = display;
        for entry in &pastes {
            if out.contains(&entry.placeholder) {
                out = out.replacen(&entry.placeholder, &entry.content, 1);
            }
        }
        out
    }

    pub fn pastes(&self) -> &[PasteEntry] {
        &self.pastes
    }

    pub fn clear_pastes(&mut self) {
        if !self.pastes.is_empty() {
            self.pastes.clear();
        }
    }

    pub fn input_version(&self) -> u64 {
        self.input_version
    }

    fn bump_input_version(&mut self) {
        self.input_version = self.input_version.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::paste::PASTE_SUMMARY_LEN_THRESHOLD;

    #[derive(Hash, Eq, PartialEq, Clone, Debug)]
    enum Tab {
        A,
        B,
    }

    fn new_state() -> ChatPanelState<Tab> {
        let mut s = ChatPanelState::new(Tab::A);
        s.insert_history(Tab::A, vec![ChatMessage::Divider]);
        s.insert_history(Tab::B, vec![ChatMessage::Divider]);
        s
    }

    #[test]
    fn new_state_has_auto_scroll_enabled() {
        let s = ChatPanelState::new(Tab::A);
        assert!(s.auto_scroll());
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn push_message_bumps_version() {
        let mut s = new_state();
        let v0 = s.message_version();
        s.push_message(ChatMessage::User { text: "hi".into() });
        assert_ne!(s.message_version(), v0);
    }

    #[test]
    fn set_active_bumps_version() {
        let mut s = new_state();
        let v0 = s.message_version();
        s.set_active(Tab::B);
        assert_ne!(s.message_version(), v0);
    }

    #[test]
    fn set_active_to_same_key_does_not_bump() {
        let mut s = new_state();
        let v0 = s.message_version();
        s.set_active(Tab::A);
        assert_eq!(s.message_version(), v0);
    }

    #[test]
    fn enable_auto_scroll_resets_scroll() {
        let mut s = new_state();
        s.set_scroll(42);
        s.enable_auto_scroll();
        assert!(s.auto_scroll());
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn scroll_down_disables_auto_scroll() {
        let mut s = new_state();
        s.scroll_down(5);
        assert!(!s.auto_scroll());
        assert_eq!(s.scroll(), 5);
    }

    #[test]
    fn scroll_up_clamps_to_zero() {
        let mut s = new_state();
        s.scroll_up(3);
        assert_eq!(s.scroll(), 0);
    }

    #[test]
    fn resolve_scroll_pins_to_max_when_auto() {
        let mut s = new_state();
        s.enable_auto_scroll();
        assert_eq!(s.resolve_scroll(100), 100);
    }

    #[test]
    fn resolve_scroll_clamps_manual() {
        let mut s = new_state();
        s.disable_auto_scroll();
        s.set_scroll(50);
        assert_eq!(s.resolve_scroll(10), 10);
    }

    #[test]
    fn resolve_scroll_preserves_manual_within_bounds() {
        let mut s = new_state();
        s.disable_auto_scroll();
        s.set_scroll(7);
        assert_eq!(s.resolve_scroll(100), 7);
    }

    #[test]
    fn histories_are_isolated_per_key() {
        let mut s = new_state();
        s.push_message(ChatMessage::User { text: "from a".into() });
        s.set_active(Tab::B);
        assert_eq!(s.current_messages().len(), 1);
        assert!(matches!(s.current_messages()[0], ChatMessage::Divider));
    }

    #[test]
    fn push_paste_short_paste_inserts_verbatim() {
        let mut s = new_state();
        s.push_paste("hello world");
        assert_eq!(s.input_text(), "hello world");
        assert!(s.pastes().is_empty());
    }

    #[test]
    fn push_paste_long_paste_records_entry_and_placeholder() {
        let mut s = new_state();
        let content = "line one\nline two\nline three\nline four";
        s.push_paste(content);
        assert_eq!(s.input_text(), "[Pasted ~4 lines]");
        assert_eq!(s.pastes().len(), 1);
        assert_eq!(s.pastes()[0].content, content);
        assert_eq!(s.pastes()[0].placeholder, "[Pasted ~4 lines]");
    }

    #[test]
    fn push_paste_long_single_line_uses_placeholder() {
        let mut s = new_state();
        let long = "a".repeat(PASTE_SUMMARY_LEN_THRESHOLD + 1);
        s.push_paste(&long);
        assert_eq!(s.input_text(), "[Pasted ~1 lines]");
        assert_eq!(s.pastes().len(), 1);
        assert_eq!(s.pastes()[0].content, long);
    }

    #[test]
    fn push_paste_empty_is_no_op() {
        let mut s = new_state();
        let v0 = s.input_version();
        s.push_paste("");
        assert_eq!(s.input_text(), "");
        assert!(s.pastes().is_empty());
        assert_eq!(s.input_version(), v0);
    }

    #[test]
    fn push_paste_bumps_input_version() {
        let mut s = new_state();
        let v0 = s.input_version();
        s.push_paste("anything");
        assert_ne!(s.input_version(), v0);
    }

    #[test]
    fn take_full_input_text_returns_verbatim_when_no_pastes() {
        let mut s = new_state();
        s.push_input_str("hello world");
        let out = s.take_full_input_text();
        assert_eq!(out, "hello world");
        assert_eq!(s.input_text(), "");
        assert!(s.pastes().is_empty());
    }

    #[test]
    fn take_full_input_text_expands_long_paste() {
        let mut s = new_state();
        let content = "line one\nline two\nline three\nline four";
        s.push_paste(content);
        assert_eq!(s.input_text(), "[Pasted ~4 lines]");
        assert_eq!(s.take_full_input_text(), content);
        assert_eq!(s.input_text(), "");
        assert!(s.pastes().is_empty());
    }

    #[test]
    fn take_full_input_text_mixes_typed_and_pasted_segments() {
        let mut s = new_state();
        s.push_input_char('h');
        s.push_input_char('i');
        s.push_input_char(' ');
        let pasted = "alpha\nbeta\ngamma\ndelta";
        s.push_paste(pasted);
        s.push_input_str(" bye");
        assert_eq!(s.input_text(), "hi [Pasted ~4 lines] bye");
        assert_eq!(s.take_full_input_text(), format!("hi {pasted} bye"));
    }

    #[test]
    fn take_full_input_text_handles_multiple_pastes() {
        let mut s = new_state();
        let a = "x\ny\nz\nw";
        let b = "1\n2\n3\n4\n5";
        s.push_paste(a);
        s.push_paste(b);
        assert_eq!(s.input_text(), "[Pasted ~4 lines][Pasted ~5 lines]");
        assert_eq!(s.take_full_input_text(), format!("{a}{b}"));
    }

    #[test]
    fn take_input_text_does_not_expand_placeholders() {
        let mut s = new_state();
        s.push_paste("a\nb\nc\nd");
        let display = s.take_input_text();
        assert_eq!(display, "[Pasted ~4 lines]");
        assert!(s.pastes().is_empty());
        assert_eq!(s.input_text(), "");
    }

    #[test]
    fn clear_input_text_drains_pastes() {
        let mut s = new_state();
        s.push_paste("a\nb\nc\nd");
        s.push_input_str(" trailing");
        s.clear_input_text();
        assert_eq!(s.input_text(), "");
        assert!(s.pastes().is_empty());
    }

    #[test]
    fn clear_pastes_does_not_touch_display() {
        let mut s = new_state();
        s.push_paste("a\nb\nc\nd");
        s.clear_pastes();
        assert_eq!(s.input_text(), "[Pasted ~4 lines]");
        assert!(s.pastes().is_empty());
    }
}
